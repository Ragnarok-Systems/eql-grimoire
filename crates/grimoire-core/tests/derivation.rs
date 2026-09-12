//! The derivation format, exercised the way a consumer exercises it.
//!
//! These live outside the module on purpose: they see `grimoire-core` through its public surface
//! and nothing else, so a field that is private stays private and a constructor that is the only
//! way in stays the only way in.
//!
//! The tests named `ac_*` are the ones an acceptance criterion in this story is discharged by. The
//! rest are edge cases the story names without giving them a criterion of their own.

use grimoire_core::derive::{
    Basis, Competing, Derivation, Rate, Refusal, ScoreKey, SettledBasis, Subject, Term, TermNote,
    Unit, Verdict,
};

// ---------------------------------------------------------------------------------------------
// Fixtures. The story owns these strings, which is what makes the "no `<`" guard in AC-001 able to
// fail: a genuine source name containing a `<` would be a deliberate edit here, not a silent hole.
// ---------------------------------------------------------------------------------------------

fn ancestral(value: f64) -> Rate {
    Rate::new(
        value,
        Unit::HpEquivalent,
        Basis::Ancestral {
            taken_from: "the per-class hit point table".into(),
        },
    )
}

fn wiki(value: f64) -> Rate {
    Rate::new(
        value,
        Unit::HpEquivalent,
        Basis::Wiki {
            page: "Upgrade tiers".into(),
        },
    )
}

fn measured(value: f64) -> Rate {
    Rate::new(
        value,
        Unit::HpEquivalent,
        Basis::Measured {
            sample: "one level 60 warrior".into(),
            sample_size: 120,
            taken_on: "2026-08-29".into(),
        },
    )
}

fn judgement(value: f64, what: &str) -> Rate {
    Rate::new(
        value,
        Unit::HpEquivalent,
        Basis::Judgement {
            judgement: what.into(),
        },
    )
}

/// The AC rate, which is the reason `Basis::Unsettled` exists.
fn unsettled_ac(value: f64) -> Rate {
    Rate::new(
        value,
        Unit::HpEquivalent,
        Basis::Unsettled {
            judgement: "a point of AC is worth three hit points".into(),
            competing: Competing {
                value: 10.0,
                unit: Unit::HpEquivalent,
                source: "an unsourced corpus post".into(),
                evidence: SettledBasis::Judgement {
                    judgement: "a point of AC is worth about ten hit points".into(),
                },
            },
        },
    )
}

/// The fully populated single-item derivation AC-001, AC-006 and AC-013 all run over.
///
/// Six terms, one per `TermNote` variant, between them covering every `Basis` variant.
fn populated_item_derivation() -> Derivation {
    Derivation::from_terms(
        Subject::ItemScore {
            item: "crown_of_narandi".into(),
            slot: "Head".into(),
            tier: 3,
        },
        Unit::HpEquivalent,
        vec![
            Term::new("Stamina", 30.0, ancestral(1.5), Some(TermNote::PastSoftcap)),
            Term::new(
                "Armour Class",
                41.0,
                unsettled_ac(3.0),
                Some(TermNote::PastStatCap),
            ),
            Term::new(
                "Strength",
                55.0,
                wiki(0.4),
                Some(TermNote::HalvedPastBreakpoint),
            ),
            Term::new(
                "Agility",
                4.0,
                measured(0.1),
                Some(TermNote::BelowThreshold { threshold: 10.0 }),
            ),
            Term::new(
                "Resist Magic",
                25.0,
                judgement(0.5, "a resist point is worth half a hit point"),
                Some(TermNote::NoRaceData),
            ),
            Term::new(
                "Charisma",
                12.0,
                judgement(0.0, "the model puts no value on charisma"),
                Some(TermNote::ZeroWeight),
            ),
        ],
    )
}

/// The other subject variant, so AC-001 covers both.
fn populated_stat_weight_derivation() -> Derivation {
    Derivation::from_terms(
        Subject::StatWeight {
            stat: "Stamina".into(),
        },
        Unit::HpEquivalent,
        vec![
            Term::new("Base conversion", 1.0, ancestral(1.5), None),
            Term::new(
                "Past the softcap",
                1.0,
                wiki(-0.75),
                Some(TermNote::HalvedPastBreakpoint),
            ),
        ],
    )
}

// ---------------------------------------------------------------------------------------------
// Variant coverage. Each `*_tag` function is exhaustive on purpose: adding a variant makes it fail
// to compile, which is the reminder to add the variant to the list beside it. Deleting one from a
// list fails AC-005's count assertion.
// ---------------------------------------------------------------------------------------------

fn basis_tag(basis: &Basis) -> &'static str {
    match basis {
        Basis::Wiki { .. } => "Wiki",
        Basis::Measured { .. } => "Measured",
        Basis::Ancestral { .. } => "Ancestral",
        Basis::Judgement { .. } => "Judgement",
        Basis::Unsettled { .. } => "Unsettled",
    }
}

fn every_basis() -> Vec<Basis> {
    vec![
        wiki(1.0).basis,
        measured(1.0).basis,
        ancestral(1.0).basis,
        judgement(1.0, "a judgement").basis,
        unsettled_ac(3.0).basis,
    ]
}

fn note_tag(note: &TermNote) -> &'static str {
    match note {
        TermNote::PastSoftcap => "PastSoftcap",
        TermNote::PastStatCap => "PastStatCap",
        TermNote::HalvedPastBreakpoint => "HalvedPastBreakpoint",
        TermNote::BelowThreshold { .. } => "BelowThreshold",
        TermNote::NoRaceData => "NoRaceData",
        TermNote::ZeroWeight => "ZeroWeight",
    }
}

fn every_term_note() -> Vec<TermNote> {
    vec![
        TermNote::PastSoftcap,
        TermNote::PastStatCap,
        TermNote::HalvedPastBreakpoint,
        TermNote::BelowThreshold { threshold: 10.0 },
        TermNote::NoRaceData,
        TermNote::ZeroWeight,
    ]
}

fn refusal_tag(refusal: &Refusal) -> &'static str {
    match refusal {
        Refusal::MissingSoftcapTable => "MissingSoftcapTable",
        Refusal::MissingItem { .. } => "MissingItem",
        Refusal::NamespaceNotCaptured { .. } => "NamespaceNotCaptured",
        Refusal::MissingRaceData => "MissingRaceData",
        Refusal::NoClassCanWear { .. } => "NoClassCanWear",
        Refusal::UnknownEffect { .. } => "UnknownEffect",
        Refusal::NonFiniteScore { .. } => "NonFiniteScore",
    }
}

fn every_refusal() -> Vec<Refusal> {
    vec![
        Refusal::MissingSoftcapTable,
        Refusal::MissingItem {
            key: "crown_of_narandi".into(),
        },
        Refusal::NamespaceNotCaptured {
            namespace: "inventory".into(),
        },
        Refusal::MissingRaceData,
        Refusal::NoClassCanWear {
            item: "a_rusty_bell".into(),
        },
        Refusal::UnknownEffect {
            effect: "Unfathomable Dread".into(),
        },
        Refusal::NonFiniteScore {
            item: "crown_of_narandi".into(),
        },
    ]
}

fn subject_tag(subject: &Subject) -> &'static str {
    match subject {
        Subject::StatWeight { .. } => "StatWeight",
        Subject::ItemScore { .. } => "ItemScore",
    }
}

fn every_subject() -> Vec<Subject> {
    vec![
        Subject::StatWeight {
            stat: "Stamina".into(),
        },
        Subject::ItemScore {
            item: "crown_of_narandi".into(),
            slot: "Head".into(),
            tier: 3,
        },
    ]
}

// ---------------------------------------------------------------------------------------------
// Acceptance criteria.
// ---------------------------------------------------------------------------------------------

/// AC-001. A fully populated derivation carries no markup and no rendered sentence.
///
/// Both subject variants, every `Basis` variant, every `TermNote` variant. Put an `<h5>` in any
/// field and this fails; if it does not fail, it is not guarding anything.
#[cfg(feature = "serde")]
#[test]
fn ac_001_a_populated_derivation_carries_no_markup() {
    let item = serde_json::to_string(&populated_item_derivation()).unwrap();
    let stat = serde_json::to_string(&populated_stat_weight_derivation()).unwrap();

    for tag in every_basis().iter().map(basis_tag) {
        assert!(
            item.contains(tag),
            "AC-001 fixture is missing the {tag} basis: {item}"
        );
    }
    for tag in every_term_note().iter().map(note_tag) {
        assert!(
            item.contains(tag),
            "AC-001 fixture is missing the {tag} note: {item}"
        );
    }
    assert!(item.contains("ItemScore"), "missing the item subject");
    assert!(stat.contains("StatWeight"), "missing the stat subject");

    for (which, json) in [("item", &item), ("stat weight", &stat)] {
        assert!(
            !json.contains('<'),
            "the {which} derivation serialised markup, which is the whole thing this format \
             exists to prevent: {json}"
        );
    }
}

/// AC-002. The reported total is the sum of the terms, and no public path supplies one.
///
/// `Derivation::from_terms` is the only constructor; it takes terms and computes the total itself.
#[test]
fn ac_002_the_total_is_the_sum_of_its_terms() {
    let terms = vec![
        Term::new("Stamina", 30.0, ancestral(1.5), None),
        Term::new("Strength", 55.0, wiki(0.4), None),
        Term::new("Charisma", -12.0, judgement(0.25, "a judgement"), None),
    ];
    let expected: f64 = terms.iter().map(|t| t.value).sum();

    let d = Derivation::from_terms(
        Subject::StatWeight {
            stat: "Stamina".into(),
        },
        Unit::HpEquivalent,
        terms,
    );

    assert!(
        (d.total() - expected).abs() < 1e-9,
        "total {} is not the sum of the terms {}",
        d.total(),
        expected
    );
    assert!((d.total() - 64.0).abs() < 1e-9, "total was {}", d.total());
}

/// AC-003. A non-finite score is refused at construction: no panic, no clamp, no stored NaN.
#[test]
fn ac_003_a_non_finite_score_is_refused() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let refused = ScoreKey::new(value, "crown_of_narandi");
        assert_eq!(
            refused,
            Err(Refusal::NonFiniteScore {
                item: "crown_of_narandi".into()
            }),
            "a score of {value} was not refused"
        );
    }
    assert!(ScoreKey::new(0.0, "crown_of_narandi").is_ok());
}

/// AC-004. A ranked list is reproducible: ten scores, two exact ties, two different input orders,
/// one output order. Remove the tie-break in `Ord for ScoreKey` and this fails.
#[test]
fn ac_004_a_ranked_list_is_reproducible_from_two_input_orders() {
    let scores = [
        (12.5, "gauntlets_of_dark_embers"),
        (202.9, "crown_of_narandi"),
        (41.0, "belt_of_iron"),
        (41.0, "amulet_of_the_drowned"), // the tie
        (7.25, "a_rusty_bell"),
        (98.0, "shield_of_the_immaculate"),
        (0.0, "a_bent_spoon"),
        (-3.5, "cursed_band"),
        (63.0, "cloak_of_flames"),
        (98.0, "ring_of_dain"), // the other tie
    ];

    let build = |rows: &[(f64, &str)]| -> Vec<ScoreKey> {
        let mut keys: Vec<ScoreKey> = rows
            .iter()
            .map(|(v, k)| ScoreKey::new(*v, *k).unwrap())
            .collect();
        keys.sort();
        keys
    };

    // Two different arrival orders over the same rows. No shuffling primitive is needed and none
    // would be honest here: the point is that the output does not depend on the input order.
    let forwards = build(&scores);
    let mut other_order: Vec<(f64, &str)> = scores.to_vec();
    other_order.reverse();
    other_order.rotate_left(3);
    let backwards = build(&other_order);

    assert_eq!(
        forwards, backwards,
        "the rank order depended on input order"
    );

    let order: Vec<&str> = forwards.iter().map(|k| k.item()).collect();
    assert_eq!(order[0], "crown_of_narandi", "highest score is not first");
    assert_eq!(
        (order[1], order[2]),
        ("ring_of_dain", "shield_of_the_immaculate"),
        "the 98.0 tie was not broken on the corpus key ascending"
    );
    assert_eq!(
        order[3], "cloak_of_flames",
        "63.0 does not sit between the 98.0 pair and the 41.0 pair"
    );
    assert_eq!(
        (order[4], order[5]),
        ("amulet_of_the_drowned", "belt_of_iron"),
        "the 41.0 tie was not broken on the corpus key ascending"
    );
    assert_eq!(order[9], "cursed_band", "the negative score is not last");
}

/// AC-005. Every variant of `Basis`, `TermNote`, `Refusal` and `Subject` survives a round trip.
///
/// Delete a variant from one of the `every_*` lists and the count assertion fails; add one to the
/// enum without listing it and the matching `*_tag` function fails to compile.
#[cfg(feature = "serde")]
#[test]
fn ac_005_every_variant_round_trips() {
    // A macro rather than a generic function: `serde` is an optional dependency of the crate and
    // is not a dev-dependency, so a test may not name `serde::Serialize` in a bound.
    macro_rules! round_trip {
        ($value:expr) => {
            serde_json::from_str(&serde_json::to_string($value).unwrap()).unwrap()
        };
    }

    let bases = every_basis();
    assert_eq!(bases.len(), 5, "Basis coverage is not the whole enum");
    assert_eq!(
        bases
            .iter()
            .map(basis_tag)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        bases.len(),
        "two entries in every_basis() are the same variant"
    );
    for basis in &bases {
        let back: Basis = round_trip!(basis);
        assert_eq!(&back, basis, "{} lost data", basis_tag(basis));
    }

    let notes = every_term_note();
    assert_eq!(notes.len(), 6, "TermNote coverage is not the whole enum");
    assert_eq!(
        notes
            .iter()
            .map(note_tag)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        notes.len(),
        "two entries in every_term_note() are the same variant"
    );
    for note in &notes {
        let back: TermNote = round_trip!(note);
        assert_eq!(&back, note, "{} lost data", note_tag(note));
    }

    let refusals = every_refusal();
    assert_eq!(refusals.len(), 7, "Refusal coverage is not the whole enum");
    assert_eq!(
        refusals
            .iter()
            .map(refusal_tag)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        refusals.len(),
        "two entries in every_refusal() are the same variant"
    );
    for refusal in &refusals {
        let back: Refusal = round_trip!(refusal);
        assert_eq!(&back, refusal, "{} lost data", refusal_tag(refusal));
    }

    let subjects = every_subject();
    assert_eq!(subjects.len(), 2, "Subject coverage is not the whole enum");
    assert_eq!(
        subjects
            .iter()
            .map(subject_tag)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        subjects.len(),
        "two entries in every_subject() are the same variant"
    );
    for subject in &subjects {
        let back: Subject = round_trip!(subject);
        assert_eq!(&back, subject, "{} lost data", subject_tag(subject));
    }

    // And the whole thing, both subjects, terms and all.
    for d in [
        populated_item_derivation(),
        populated_stat_weight_derivation(),
    ] {
        let back: Derivation = round_trip!(&d);
        assert_eq!(back, d, "a populated derivation did not round trip");
        assert_eq!(
            serde_json::to_string(&back).unwrap(),
            serde_json::to_string(&d).unwrap(),
            "the round trip was not byte-identical"
        );
    }
}

/// AC-006. A fully populated single-item derivation fits the render budget, and the measured byte
/// count goes on the record rather than being assumed.
#[cfg(feature = "serde")]
#[test]
fn ac_006_a_populated_item_derivation_fits_the_size_budget() {
    let json = serde_json::to_string(&populated_item_derivation()).unwrap();
    println!(
        "AC-006: one fully populated item derivation is {} bytes",
        json.len()
    );
    assert!(
        json.len() < 2048,
        "a single item derivation costs {} bytes, over the 2,048 byte budget",
        json.len()
    );
}

/// AC-007. Zero terms is a legal derivation totalling zero, and it is not a refusal.
#[cfg(feature = "serde")]
#[test]
fn ac_007_a_zero_term_derivation_is_not_a_refusal() {
    let empty = Derivation::from_terms(
        Subject::ItemScore {
            item: "a_bent_spoon".into(),
            slot: "Primary".into(),
            tier: 0,
        },
        Unit::HpEquivalent,
        Vec::new(),
    );
    assert_eq!(empty.total(), 0.0);
    assert!(empty.terms().is_empty());

    let empty_json = serde_json::to_string(&empty).unwrap();
    assert!(
        empty_json.contains("\"total\":0.0"),
        "a total of zero must serialise as zero and not as a signed zero: {empty_json}"
    );

    let back: Derivation = serde_json::from_str(&empty_json).unwrap();
    assert_eq!(back, empty, "the zero-term derivation did not round trip");
    assert_eq!(back.total(), 0.0);

    let scored: Verdict<Derivation> = Verdict::Answered(empty);
    let refused: Verdict<Derivation> = Verdict::Refused(Refusal::MissingSoftcapTable);
    let scored_json = serde_json::to_string(&scored).unwrap();
    let refused_json = serde_json::to_string(&refused).unwrap();

    assert_ne!(scored_json, refused_json);
    assert!(scored_json.contains("Answered"), "{scored_json}");
    assert!(refused_json.contains("Refused"), "{refused_json}");
    assert!(
        refused_json.contains("MissingSoftcapTable"),
        "a refusal must name the input that was missing: {refused_json}"
    );
    assert!(
        !refused_json.contains("total"),
        "a refusal must not carry a total anyone could read as a score: {refused_json}"
    );
    assert!(matches!(scored, Verdict::Answered(_)));
    assert!(matches!(
        refused,
        Verdict::Refused(Refusal::MissingSoftcapTable)
    ));
}

/// AC-013. The fixture AC-001 and AC-006 run over is actually populated: one term per `TermNote`
/// variant, every term carrying `label`, `count`, `rate.value`, `rate.unit`, `rate.basis`, `value`.
///
/// Run it with `--nocapture` and count `"basis"` in the printed JSON with the shell. The two
/// numbers are counted by two different tools over two different things and must agree.
#[cfg(feature = "serde")]
#[test]
fn ac_013_term_population() {
    let d = populated_item_derivation();
    let json = serde_json::to_string(&d).unwrap();

    let notes = every_term_note();
    assert!(
        d.terms().len() >= notes.len(),
        "the fixture has {} terms and TermNote has {} variants",
        d.terms().len(),
        notes.len()
    );
    assert!(d.terms().len() >= 6, "the fixture is not populated");

    let carried: std::collections::BTreeSet<&'static str> = d
        .terms()
        .iter()
        .filter_map(|t| t.note.as_ref())
        .map(note_tag)
        .collect();
    for tag in notes.iter().map(note_tag) {
        assert!(carried.contains(tag), "no term carries the {tag} note");
    }

    for term in d.terms() {
        assert!(!term.label.is_empty(), "a term has no label");
        assert!(term.count.is_finite(), "{} has no count", term.label);
        assert!(
            term.rate.value.is_finite(),
            "{} has no rate value",
            term.label
        );
        assert_eq!(term.rate.unit, Unit::HpEquivalent);
        assert!(term.value.is_finite(), "{} has no value", term.label);
        assert!(
            (term.value - term.count * term.rate.value).abs() < 1e-9,
            "{} is not its count times its rate",
            term.label
        );
    }

    println!("{json}");
    println!("AC-013: terms asserted = {}", d.terms().len());
}

// ---------------------------------------------------------------------------------------------
// Edge cases the story names without a criterion of their own.
// ---------------------------------------------------------------------------------------------

/// EDGE-002. A zero rate keeps its term, because "on the item and worth nothing" is not "absent".
#[test]
fn a_zero_rate_keeps_its_term() {
    let d = populated_item_derivation();
    let zero = d
        .terms()
        .iter()
        .find(|t| t.note == Some(TermNote::ZeroWeight))
        .expect("the fixture carries a zero-weight term");
    assert_eq!(zero.value, 0.0);
    assert_eq!(zero.count, 12.0, "the count survives a zero rate");
}

/// EDGE-003. Negative terms are kept, the total is signed, and nothing is clamped.
#[test]
fn negative_terms_are_kept_and_the_total_is_signed() {
    let d = Derivation::from_terms(
        Subject::ItemScore {
            item: "cursed_band".into(),
            slot: "Finger".into(),
            tier: 0,
        },
        Unit::HpEquivalent,
        vec![
            Term::new("Wisdom", -20.0, ancestral(0.3), None),
            Term::new("Stamina", 2.0, ancestral(1.5), None),
        ],
    );
    assert!((d.total() + 3.0).abs() < 1e-9, "total was {}", d.total());
    assert!(d.total() < 0.0, "total was {}", d.total());
    assert_eq!(d.terms()[0].value, -6.0);
}

/// REQ-007 over the JSON boundary: deserialisation is not a way to supply a total either. A wire
/// total that disagrees with the terms is discarded and recomputed.
#[cfg(feature = "serde")]
#[test]
fn a_wire_total_that_lies_is_recomputed_from_the_terms() {
    let json = r#"{
        "subject": {"ItemScore": {"item": "crown_of_narandi", "slot": "Head", "tier": 3}},
        "terms": [
            {"label": "Stamina", "count": 30.0,
             "rate": {"value": 1.5, "unit": "HpEquivalent",
                      "basis": {"Ancestral": {"taken_from": "the per-class hit point table"}}},
             "value": 45.0, "note": "PastSoftcap"}
        ],
        "total": 99999.0,
        "unit": "HpEquivalent"
    }"#;
    let d: Derivation = serde_json::from_str(json).unwrap();
    assert!(
        (d.total() - 45.0).abs() < 1e-9,
        "a caller supplied a total through JSON and it stuck: {}",
        d.total()
    );
}

/// The tie-break is the item's corpus key, and it is the *only* thing that separates equal scores.
#[test]
fn equal_scores_are_separated_only_by_the_corpus_key() {
    let a = ScoreKey::new(41.0, "amulet_of_the_drowned").unwrap();
    let b = ScoreKey::new(41.0, "belt_of_iron").unwrap();
    assert!(a < b, "the tie-break is not ascending on the corpus key");
    assert_eq!(a.value(), b.value());
    assert_ne!(a, b);
}
