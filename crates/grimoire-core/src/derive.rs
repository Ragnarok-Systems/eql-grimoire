//! The shape of an explanation, decided before the first weight is computed.
//!
//! Every number the gear tooling puts on a screen is a number a player will argue with, and the
//! answer to "why is that 135 and that 41" is the product. This module is that answer's type: a
//! [`Derivation`] is a subject, a list of [`Term`]s, a [`Unit`] and a total the terms add up to.
//! Nothing here computes a gear weight — the first weight lands with the scorer — and nothing here
//! renders one. The engine owns every number someone could argue with (a price, a count, a rank);
//! the screen owns everything that decides how that number looks. Every consumer of a weight is
//! written against this shape, so it is decided before the scorer is written. This module is that
//! decision.
//!
//! # What a consumer does with a derivation, and what it must never do
//!
//! Render it: read [`Derivation::terms`], print each [`Term`]'s `label`, `count`, `rate.value` and
//! `value` in whatever words and rounding the screen wants, and show the [`Basis`] beside the rate
//! so a reader can see how strong the evidence is. Sort by [`Term`]'s `value` if the biggest
//! contributor is the interesting one; sort a list of scores with [`ScoreKey`]. Never recompute the
//! total — [`Derivation::total`] is the number the engine stands behind, and a consumer that adds
//! the terms up itself has quietly become a second implementation. Never re-derive a rate: the
//! engine's rate is the model's rate, and a screen that multiplies it by something has changed the
//! model. Never assume a term is present — a stat the model does not value produces no term at all,
//! and a stat it values at nothing produces a term carrying [`TermNote::ZeroWeight`], which is a
//! different statement.
//!
//! # The unit
//!
//! Every weight in the gear model is quoted in **HP-equivalents**: a +1 HP stat on an item is worth
//! exactly 1, and the whole table scales together if that unit ever changes. Because it is a
//! property of every number in the model rather than of the table, it rides on every [`Rate`] and
//! every [`Derivation`] as a [`Unit`], so a consumer cannot put two numbers in different units side
//! by side without the type telling it.
//!
//! # Why a basis is part of a rate
//!
//! The rates in this model are not equally well founded. Some come from a documented rule, some
//! from a measurement against a live character, some from an ancestral per-class table whose own
//! constants are unpublished, and at least one is a judgement call with a known competing figure —
//! the HP-equivalent value of a point of AC, where an unsourced figure roughly ten times the one
//! the model uses would move the AC-to-INT ratio by a factor of three. That AC weight is why
//! [`Basis::Unsettled`] exists. A model that renders all four kinds in the same voice is lying
//! about three of them, so a [`Rate`] cannot be built without a [`Basis`], and the basis carries
//! the evidence — a page name, a sample size, a source — rather than a sentence about it.
//!
//! # Why [`Refusal`] exists, and why a missing input is never a zero
//!
//! Suppose the softcap table fails to load. Nothing errors. A scorer that reads a missing table as
//! an empty one falls back to a softcap of zero, and from then on every point of AC is priced as
//! if it were past the cap: every item that carries AC is repriced, and no screen says why. The
//! defect would not be the arithmetic; it would be that "I do not have the table" and "the table
//! says nothing" were the same value. [`Verdict`] makes them different values, [`Refusal`] names
//! which input was missing, and a refusal is never an empty answer and never a zero. A computation
//! that has already accumulated terms and then hits a missing input throws the terms away and
//! returns the refusal: a half-built derivation would let a screen render a plausible total that no
//! rule produced.
//!
//! # Ordering
//!
//! A score is an `f64` that gets sorted, so the ordering rule is part of the format rather than a
//! detail of whoever writes the first `sort_by`. [`ScoreKey`] is a **total** order — value
//! descending, ties broken on the item's corpus key ascending — so a ranked list is reproducible:
//! two runs over the same inputs produce the same order, and the same bytes when serialised. A
//! non-finite score never reaches the comparator, because [`ScoreKey::new`] refuses it.
//!
//! # Two things this module deliberately does not do
//!
//! No field here is rendered markup or a rendered sentence. Free text is confined to fields whose
//! job is to name a source — a page title, a description of a sample — and to the note carried
//! inside a judgement. The guard is a test that serialises a fixture and asserts the JSON contains
//! no `<`; it runs over a fixture this crate's own tests own, so the day a genuine source name
//! contains a `<` that is a deliberate edit to the fixture and not a silent hole in the guard.
//!
//! And a derivation is small on purpose. One item's derivation carries at most one term per scored
//! stat, and the test suite holds a fully populated single-item derivation under 2 KB of JSON,
//! because the rank views price thousands of records per dump and a derivation that cost 20 KB
//! apiece could not ride along with a row set at all.

use core::cmp::Ordering;

/// The unit every weight in the gear model is quoted in.
///
/// An enum rather than a comment, so that it travels with the number. There is one variant today,
/// and adding a second is a decision about the whole table rather than about one rate.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Unit {
    /// Hit points. A +1 HP stat on an item is worth exactly 1.
    HpEquivalent,
}

/// Where a number came from, when it cannot itself be the tail of an argument.
///
/// The four settled kinds. [`Basis::Unsettled`] carries one of these as the figure it is in
/// tension with, and that is the one level of nesting the model allows.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SettledBasis {
    /// A documented rule, naming the page that documents it.
    Wiki { page: String },
    /// A value measured against a live character, naming the sample and when it was taken.
    Measured {
        sample: String,
        sample_size: u32,
        /// The day the sample was taken, as `YYYY-MM-DD`. A string, because this crate does no I/O
        /// and owns no clock.
        taken_on: String,
    },
    /// A value carried over from the ancestral per-class model, naming what it was taken from.
    Ancestral { taken_from: String },
    /// Somebody decided, naming what the decision was.
    Judgement { judgement: String },
}

/// A figure a rate is in tension with.
///
/// Its own evidence is a [`SettledBasis`] and not another [`Basis`], which is how the type says
/// that nesting stops here: an unsettled rate may name the figure it disagrees with, and that
/// figure may not in turn be unsettled. A second level is a chain of disagreements with no measured
/// number at the end of it, which is a modelling smell rather than evidence.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Competing {
    /// The competing per-point value.
    pub value: f64,
    pub unit: Unit,
    /// Where the competing figure was seen. A name, never a sentence about it.
    pub source: String,
    pub evidence: SettledBasis,
}

/// How strong the ground under a [`Rate`] is.
///
/// Each variant carries the evidence rather than a sentence about it, so a screen can render a
/// documented rule and a judgement call in different voices without parsing anything.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Basis {
    /// A documented rule, naming the page that documents it.
    Wiki { page: String },
    /// Measured against a live character, naming the sample size and the date.
    Measured {
        sample: String,
        sample_size: u32,
        taken_on: String,
    },
    /// Taken from the ancestral per-class model, naming what it was taken from.
    Ancestral { taken_from: String },
    /// A judgement call, naming what the judgement is.
    Judgement { judgement: String },
    /// A judgement call with a known competing figure.
    ///
    /// The AC weight is why this variant exists: the model's HP-equivalent value for a point of AC
    /// is a judgement, and a competing figure roughly ten times larger is in circulation. A rate on
    /// this footing is still usable — it is the model's number — but a consumer that renders it
    /// without saying so is presenting a guess as a rule.
    Unsettled {
        judgement: String,
        competing: Competing,
    },
}

/// A per-point value, its unit, and the ground it stands on.
///
/// There is no constructor and no field default that omits the basis: a rate without one is
/// unrepresentable, because an unattributed number is the thing this module exists to prevent.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rate {
    /// What one point of the thing being rated is worth, in `unit`.
    pub value: f64,
    pub unit: Unit,
    pub basis: Basis,
}

impl Rate {
    /// Build a rate. Every argument is required; that is the point.
    pub fn new(value: f64, unit: Unit, basis: Basis) -> Rate {
        Rate { value, unit, basis }
    }
}

/// A fact about a term that a consumer would otherwise have to read out of prose.
///
/// An enum, never free text. A consumer that wants a sentence writes the sentence; the engine
/// supplies the fact, so a screen can filter on "which terms crossed a cap" without matching
/// strings.
#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TermNote {
    /// The stat is past its softcap and is being valued at the post-cap rate.
    PastSoftcap,
    /// The stat is past the hard cap and contributes nothing further.
    PastStatCap,
    /// The stat is past a breakpoint beyond which each point is worth half as much.
    HalvedPastBreakpoint,
    /// The count is below a threshold under which the model does not value the stat.
    BelowThreshold { threshold: f64 },
    /// The character's race is unknown, so a race-dependent part of the term was not applied.
    NoRaceData,
    /// The model values this stat at nothing. The term is kept anyway: "this stat is on the item
    /// and it is worth zero" is a different statement from the stat being absent, and a player who
    /// has overridden a weight to zero needs to see that they did.
    ZeroWeight,
}

/// One stat's contribution to a derivation.
///
/// A term composes: a label, the raw count it was computed over, the [`Rate`] applied, the value
/// that came out, and optionally one [`TermNote`]. Several separate facts about one number, each
/// one separately checkable. Flattened into a sentence, none of them is.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Term {
    /// What was counted. A name — "Stamina", "Armour Class" — never a sentence.
    pub label: String,
    /// The raw count the term was computed over. Signed: negative stats are on real items.
    pub count: f64,
    pub rate: Rate,
    /// `count × rate.value`, in `rate.unit`. Signed, and never clamped.
    pub value: f64,
    pub note: Option<TermNote>,
}

impl Term {
    /// Build a term. The value is the product of the count and the rate; there is no argument
    /// about it, which is why it is not a parameter.
    ///
    /// One constructor rather than a noted and an un-noted one: the note is `Option` at the call
    /// site, so a term built without one was built without one on purpose.
    pub fn new(label: impl Into<String>, count: f64, rate: Rate, note: Option<TermNote>) -> Term {
        Term {
            label: label.into(),
            count,
            value: count * rate.value,
            rate,
            note,
        }
    }
}

/// What a [`Derivation`] explains.
///
/// The item variant carries the slot and the tier because the same item in two slots at two tiers
/// is two different numbers, and a derivation that cannot say which of them it is is not evidence
/// of anything.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Subject {
    /// The weight the model puts on one stat.
    StatWeight { stat: String },
    /// One item, priced for one slot at one upgrade tier.
    ItemScore {
        /// The item's corpus key.
        item: String,
        slot: String,
        tier: u8,
    },
}

/// A number, and everything it was made of.
///
/// The total is not an input. [`Derivation::from_terms`] is the only way to build one and it sums
/// the terms itself, so a caller that wants to report a total the terms do not add up to has no way
/// to express it. The terms are not publicly mutable once a derivation exists — [`Derivation::terms`]
/// hands out a shared slice — so the total cannot drift away from them afterwards either:
///
/// ```compile_fail
/// use grimoire_core::derive::{Basis, Derivation, Rate, Subject, Term, Unit};
///
/// let rate = Rate::new(1.0, Unit::HpEquivalent, Basis::Wiki { page: "Stats".into() });
/// let d = Derivation::from_terms(
///     Subject::StatWeight { stat: "Stamina".into() },
///     Unit::HpEquivalent,
///     vec![Term::new("Stamina", 10.0, rate, None)],
/// );
/// d.terms()[0].value = 999.0; // terms() is a shared slice: this does not compile.
/// ```
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "DerivationWire"))]
pub struct Derivation {
    subject: Subject,
    terms: Vec<Term>,
    /// The sum of the term values. Computed here, never supplied.
    total: f64,
    unit: Unit,
}

impl Derivation {
    /// Build a derivation from its terms and compute its total.
    ///
    /// The only constructor. Zero terms is legal and totals zero: an item with no scored stats is a
    /// real row on a real dump, and "scored at zero" has to be tellable apart from "not scored",
    /// which is what [`Verdict`] is for.
    pub fn from_terms(subject: Subject, unit: Unit, terms: Vec<Term>) -> Derivation {
        let summed = terms.iter().map(|t| t.value).sum::<f64>();
        // `f64`'s `Sum` identity is `-0.0`, so an empty term list would otherwise report — and
        // serialise — a total of `-0.0`. The sign of a zero is not a fact about the item.
        let total = if summed == 0.0 { 0.0 } else { summed };
        let derivation = Derivation {
            subject,
            terms,
            total,
            unit,
        };
        debug_assert!(
            derivation.total_matches_terms(1e-9),
            "a derivation's total must be the sum of its terms",
        );
        derivation
    }

    pub fn subject(&self) -> &Subject {
        &self.subject
    }

    /// The terms, in the order they were given. A shared slice: see the type's own documentation.
    pub fn terms(&self) -> &[Term] {
        &self.terms
    }

    /// The sum of the term values. The number the engine stands behind; do not recompute it.
    pub fn total(&self) -> f64 {
        self.total
    }

    pub fn unit(&self) -> Unit {
        self.unit
    }

    /// Recompute the sum and compare it with the stored total.
    ///
    /// Private, and deliberately so: a consumer that checks the engine's arithmetic has become a
    /// second implementation of it. This exists for the debug assertion in
    /// [`Derivation::from_terms`], which is what stops a later edit from setting the total any
    /// other way.
    fn total_matches_terms(&self, tolerance: f64) -> bool {
        (self.terms.iter().map(|t| t.value).sum::<f64>() - self.total).abs() <= tolerance
    }
}

/// The wire form of a [`Derivation`], which exists only so that deserialising one recomputes the
/// total instead of trusting it.
///
/// Without it `serde` would be a public constructor that takes a total, and the guarantee above
/// would hold for Rust callers and not for JSON ones.
#[cfg(feature = "serde")]
#[derive(serde::Deserialize)]
struct DerivationWire {
    subject: Subject,
    terms: Vec<Term>,
    /// Accepted and discarded. The terms are the truth.
    #[serde(default)]
    #[allow(dead_code)]
    total: Option<f64>,
    unit: Unit,
}

#[cfg(feature = "serde")]
impl From<DerivationWire> for Derivation {
    fn from(wire: DerivationWire) -> Derivation {
        Derivation::from_terms(wire.subject, wire.unit, wire.terms)
    }
}

/// Why a question could not be answered.
///
/// Never an empty answer and never a zero — this module's documentation carries the postmortem that
/// is the reason this type exists at all. Each variant names the input that was missing, so a
/// screen can say which one rather than "no data".
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Refusal {
    /// The softcap table is not loaded. The one that started all this.
    MissingSoftcapTable,
    /// No item in the corpus under this key.
    MissingItem { key: String },
    /// A namespace the answer depends on was never captured from the dump.
    NamespaceNotCaptured { namespace: String },
    /// The character's race is unknown and the answer depends on it.
    MissingRaceData,
    /// No class can wear the item, so there is no slot to price it for.
    NoClassCanWear { item: String },
    /// An effect the model has no rule for.
    UnknownEffect { effect: String },
    /// A score that is not a finite number. Rejected at construction rather than sorted.
    NonFiniteScore { item: String },
}

/// Answered, or refused. There is no third case and no empty answer.
///
/// The return type of every gear op. A caller destructures it; it cannot accidentally read a
/// refusal as a zero.
///
/// Deliberately bare. There are no `is_answered`/`unwrap_or_default` conveniences, because every
/// one of them is a way to reach a number without having looked at whether there is one, and that
/// is the failure the whole type exists to make impossible. A caller writes the `match`.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Verdict<T> {
    Answered(T),
    Refused(Refusal),
}

impl<T> From<Result<T, Refusal>> for Verdict<T> {
    fn from(result: Result<T, Refusal>) -> Verdict<T> {
        match result {
            Ok(value) => Verdict::Answered(value),
            Err(refusal) => Verdict::Refused(refusal),
        }
    }
}

/// A score, and the key a ranked list is sorted by.
///
/// The order is total: value descending, then the item's corpus key ascending. The tie-break is not
/// decoration — two items with exactly equal scores are common, and without it the order of a rank
/// list depends on the order the rows arrived in, which is how "the rank list changed between two
/// runs over the same dump" happens.
///
/// The value is finite by construction. [`ScoreKey::new`] refuses a non-finite score rather than
/// storing it, so no comparator in this crate has to decide what a NaN sorts as.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScoreKey {
    value: f64,
    item: String,
}

impl ScoreKey {
    /// Build a score key, or refuse.
    ///
    /// A non-finite value — a NaN or an infinity — comes back as [`Refusal::NonFiniteScore`]. It is
    /// not clamped, not stored and not zeroed: a score nobody can order is a missing answer.
    pub fn new(value: f64, item: impl Into<String>) -> Result<ScoreKey, Refusal> {
        let item = item.into();
        if !value.is_finite() {
            return Err(Refusal::NonFiniteScore { item });
        }
        Ok(ScoreKey { value, item })
    }

    /// The score. Finite, always.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The item's corpus key, which is also the tie-break.
    pub fn item(&self) -> &str {
        &self.item
    }
}

// `value` is finite by construction, so `PartialEq` here is reflexive and `Eq` is honest.
impl Eq for ScoreKey {}

impl Ord for ScoreKey {
    fn cmp(&self, other: &ScoreKey) -> Ordering {
        // `total_cmp`, not `partial_cmp`: there is no `Option` to unwrap and no NaN to reach it.
        other
            .value
            .total_cmp(&self.value)
            .then_with(|| self.item.cmp(&other.item))
    }
}

impl PartialOrd for ScoreKey {
    fn partial_cmp(&self, other: &ScoreKey) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wiki_rate(value: f64) -> Rate {
        Rate::new(
            value,
            Unit::HpEquivalent,
            Basis::Wiki {
                page: "Upgrade tiers".into(),
            },
        )
    }

    #[test]
    fn a_term_value_is_the_product_of_its_count_and_its_rate() {
        let term = Term::new("Stamina", 30.0, wiki_rate(1.5), None);
        assert_eq!(term.value, 45.0);
        assert_eq!(term.note, None);
    }

    #[test]
    fn a_negative_count_gives_a_negative_term_and_is_not_clamped() {
        let term = Term::new("Charisma", -12.0, wiki_rate(0.5), None);
        assert_eq!(term.value, -6.0);
    }

    #[test]
    fn a_derivation_totals_its_own_terms() {
        let d = Derivation::from_terms(
            Subject::StatWeight {
                stat: "Stamina".into(),
            },
            Unit::HpEquivalent,
            vec![
                Term::new("Stamina", 30.0, wiki_rate(1.5), None),
                Term::new("Charisma", -12.0, wiki_rate(0.5), None),
            ],
        );
        assert!((d.total() - 39.0).abs() < 1e-9);
        assert_eq!(d.terms().len(), 2);
        assert_eq!(d.unit(), Unit::HpEquivalent);
        assert_eq!(
            d.subject(),
            &Subject::StatWeight {
                stat: "Stamina".into()
            }
        );
    }

    #[test]
    fn a_zero_rate_keeps_its_term() {
        let term = Term::new(
            "Armour Class",
            41.0,
            wiki_rate(0.0),
            Some(TermNote::ZeroWeight),
        );
        assert_eq!(term.value, 0.0);
        assert_eq!(term.note, Some(TermNote::ZeroWeight));
        assert_eq!(term.count, 41.0);
    }

    #[test]
    fn a_non_finite_score_is_refused() {
        assert_eq!(
            ScoreKey::new(f64::NAN, "belt_of_iron"),
            Err(Refusal::NonFiniteScore {
                item: "belt_of_iron".into()
            })
        );
        assert!(ScoreKey::new(f64::INFINITY, "x").is_err());
        assert!(ScoreKey::new(f64::NEG_INFINITY, "x").is_err());
        assert!(ScoreKey::new(-0.0, "x").is_ok());
    }

    #[test]
    fn scores_order_by_value_descending_then_by_key() {
        let mut keys = [
            ScoreKey::new(10.0, "b").unwrap(),
            ScoreKey::new(12.0, "z").unwrap(),
            ScoreKey::new(10.0, "a").unwrap(),
        ];
        keys.sort();
        let order: Vec<&str> = keys.iter().map(|k| k.item()).collect();
        assert_eq!(order, ["z", "a", "b"]);
    }

    #[test]
    fn a_refusal_is_not_an_answer() {
        let verdict: Verdict<ScoreKey> = ScoreKey::new(f64::NAN, "x").into();
        assert_eq!(
            verdict,
            Verdict::Refused(Refusal::NonFiniteScore { item: "x".into() })
        );

        let answered: Verdict<ScoreKey> = ScoreKey::new(1.0, "x").into();
        assert!(matches!(answered, Verdict::Answered(_)));
    }
}
