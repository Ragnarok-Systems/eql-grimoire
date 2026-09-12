# The gear derivation format

**Status:** decided, and landed with no scorer attached to it.
**Code:** `crates/grimoire-core/src/derive.rs`. **Tests:** `crates/grimoire-core/tests/derivation.rs`.

Every number the gear tooling puts on a screen is a number a player will argue with, and the answer
to "why is that 135 and that 41" is the product. This document records the shape that answer takes,
what was rejected, and why each rejected shape fails.

It is decided first, before the scorer, because the shape is what every consumer is written against:
the scorer's weights, the item breakdown, the valet's per-step pricing, spare's per-niche verdict and
the exaltation audit's refusal reasons all return it. Deferring it would mean rewriting every one of
those consumers once the shape changed. The line it sits on: the engine owns every number someone
could argue with (a price, a count, a rank, a "you already have this", a "this is safe to destroy"),
and the screen owns everything that decides how that number looks.

## The chosen shape

A derivation is **typed data**, not a rendered sentence:

- `Derivation` — a `Subject`, a list of `Term`s, a `Unit`, and a `total` the terms add up to.
- `Term` — a `label`, the raw `count` it was computed over, the `Rate` applied, the resulting
  `value`, and an optional `TermNote`.
- `Rate` — a per-point `value`, its `Unit`, and a `Basis`. There is no way to build one without a
  basis.
- `Basis` — where the number came from, carrying the evidence rather than a sentence about it: a
  documented rule and its page, a measurement and its sample size and date, an ancestral-model value
  and what it was taken from, a judgement call and what the judgement is, or an unsettled value
  carrying the competing figure it is in tension with.
- `TermNote` — an enum, never free text: past the softcap, past the stat cap, halved past a
  breakpoint, below a threshold (carrying the threshold), no race data, zero weight.
- `Verdict<T>` — answered or refused, and `Refusal` names which input was missing.
- `ScoreKey` — a total order over scores: value descending, ties broken on the item's corpus key
  ascending. A non-finite score is refused at construction and never stored.

Three properties are load-bearing and each has a test behind it:

1. **The total cannot be supplied.** `Derivation::from_terms` is the only constructor and it sums the
   terms itself. Deserialisation goes through the same constructor, so a JSON payload carrying a
   total that disagrees with its terms has that total discarded rather than believed.
2. **A missing input is never a zero.** `Refusal` is a value, not an absence. Without it, a scorer
   whose softcap table failed to load would fall back to a softcap of zero and price every point of
   AC against it, repricing every item that carries AC with no error anywhere.
   `crates/grimoire-core/src/derive.rs` states that case in its module documentation so a reader of
   the code does not have to go and find it.
3. **A ranked list is reproducible.** Two runs over the same inputs produce the same order, because
   the order is total and the tie-break is deterministic. No `partial_cmp(...).unwrap()` exists in
   the module; the comparator uses `f64::total_cmp` over a value that is finite by construction.

### A worked example

One item, priced for one slot at one tier. Three terms shown; the full six-term fixture the tests
serialise is in `crates/grimoire-core/tests/derivation.rs` and measures 1,432 bytes on one line.

```json
{
  "subject": {
    "ItemScore": {
      "item": "crown_of_narandi",
      "slot": "Head",
      "tier": 3
    }
  },
  "terms": [
    {
      "label": "Stamina",
      "count": 30.0,
      "rate": {
        "value": 1.5,
        "unit": "HpEquivalent",
        "basis": {
          "Ancestral": {
            "taken_from": "the per-class hit point table"
          }
        }
      },
      "value": 45.0,
      "note": "PastSoftcap"
    },
    {
      "label": "Armour Class",
      "count": 41.0,
      "rate": {
        "value": 3.0,
        "unit": "HpEquivalent",
        "basis": {
          "Unsettled": {
            "judgement": "a point of AC is worth three hit points",
            "competing": {
              "value": 10.0,
              "unit": "HpEquivalent",
              "source": "an unsourced corpus post",
              "evidence": {
                "Judgement": {
                  "judgement": "a point of AC is worth about ten hit points"
                }
              }
            }
          }
        }
      },
      "value": 123.0,
      "note": "PastStatCap"
    },
    {
      "label": "Agility",
      "count": 4.0,
      "rate": {
        "value": 0.1,
        "unit": "HpEquivalent",
        "basis": {
          "Measured": {
            "sample": "one level 60 warrior",
            "sample_size": 120,
            "taken_on": "2026-08-29"
          }
        }
      },
      "value": 0.4,
      "note": {
        "BelowThreshold": {
          "threshold": 10.0
        }
      }
    }
  ],
  "total": 168.4,
  "unit": "HpEquivalent"
}
```

Everything a hover card needs is readable off that without parsing prose. The biggest contributor is
Armour Class at 123; it was 41 points at 3 HP-equivalents each; that rate is a judgement call with a
competing figure roughly three times larger in circulation, and the card can say so in whatever words
it likes. The term that crossed a cap says `PastStatCap` as an enum value, so a filter for "which
terms hit a cap" is an equality check rather than a substring search.

A refusal is the other thing this can be, and it is not mistakable for a score of zero:

```json
{ "Refused": "MissingSoftcapTable" }
```

There is no `total` field on that object at all, so a reader who does not know the schema still
cannot read it as a number.

## The three rejected alternatives

### 1. Markup strings

Return `<h5>AC</h5><p>…</p>` per weight, with the rates already interpolated into English, and let
the consumer mount it. It is the shortest path to a hover card, and it is the shape this decision
exists to refuse.

**Why it fails.** It puts the screen inside the engine. The sentence, the rounding, the pluralisation
and the ordering of the explanation all end up inside the wasm module, where changing a comma costs a
rebuild and a redeploy of the binary. It also makes the data unusable for anything but display: a
consumer that wants to sort by the largest contributing term, or filter to the terms that crossed a
cap, has to parse English out of HTML to find out. And it is a one-way door — once a screen depends
on the exact markup, the engine cannot change the wording without breaking it.

### 2. One `why` string per weight

Keep the numbers typed, but hang a single human-readable justification string off each weight.

**Why it fails.** It is unparseable, which sounds mild and is not. A `why` string is where the
per-term structure goes to die: the count, the rate, the basis and the cap flag are all in there, but
only as words. A consumer that wants to sort by the largest contributing term cannot, because there
are no terms — there is one string. Every question a player actually asks ("which stat is doing the
work here?", "is that rate measured or guessed?") becomes a string-matching exercise against text the
engine is free to reword. It is markup's failure again with the angle brackets removed.

### 3. A parallel map keyed by stat, alongside the weights

Return the weights as one structure and the explanations as a second structure keyed by stat name,
and let the consumer join them.

**Why it fails.** Two structures that can disagree. That is the same defect class as the missing
softcap table under property 2 above: a stat present in the weights and absent from the
explanations renders as a number with no justification, a stat present in the explanations and absent
from the weights renders as a justification for a number nobody computed, and neither case errors.
Nothing checks the join, so the two copies drift and the drift is silent. A `Term` carries its own
rate and its own basis, so the number and its explanation cannot be separated by construction — the
same reason `Derivation` computes its own total instead of accepting one.

## Considered and rejected: a version number on the shape

No version field, and no compatibility negotiation. There is one consumer tree and it ships as one
bundle — engine, corpus and page are deployed together — so a format change is a change to one
artifact, not a protocol break between two parties who upgrade separately. Adding negotiation
machinery to a format with a single first-party consumer is cost with no buyer. This is recorded so
that the next person does not re-open it by accident; if a second, independently-deployed consumer
ever appears, that is the event that makes it worth revisiting, and nothing else is.

## Budgets and rules the tests hold

- **Size.** A fully populated single-item derivation serialises to under 2,048 bytes. Measured:
  1,432. The rank views price thousands of records per dump, so a derivation that cost 20 KB apiece
  could not ride along with a row set at all.
- **No markup, ever.** A test serialises the fully populated fixture and asserts the JSON contains no
  `<`. Free text is confined to fields whose job is to name a source, and to the note inside a
  judgement. The fixture is the tests' own, so a future source name that legitimately contains a `<`
  is a deliberate edit to the fixture rather than a hole in the guard.
- **Determinism.** Ordered collections only; no hash-map iteration order reaches the wire. Two runs
  over the same inputs serialise to the same bytes.
- **Purity.** `grimoire-core` does no I/O, so a date in a `Measured` basis is a string the caller
  supplies — this crate owns no clock.
- **`serde` stays optional.** Every public type carries the conditional derive and the crate builds
  and tests with `--no-default-features`.

## What a consumer may and may not do

Render it, sort by term value, show the basis. Never recompute the total — that is the engine's
number and a consumer that re-adds the terms has become a second implementation of the model. Never
re-derive a rate. Never assume a term is present: a stat the model does not value produces no term,
and a stat it values at nothing produces a term carrying `ZeroWeight`, which is a different
statement. A computation that refuses part-way through returns the refusal and discards the terms it
had accumulated, so a half-built derivation never reaches a screen.
