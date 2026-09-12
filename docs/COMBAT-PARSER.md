# Combat parsing in `eql-grimoire`

**Status:** spec, for James. Written against the tree as it stood on 2026-08-29, and against 308 MB / 3,673,067 lines of James's own EverQuest Legends logs.
**Reading order:** §1 is the position. §2 is the ceiling everything else lives under. §7 is the hazard register. §11 is what James has to settle himself.

---

## 1. Position

### 1.1 Working discipline

Implementers work from this spec, from real logs, and from the MIT-licensed reference in §9.6. The grammar is the EQL log's own line forms, measured in §2; every constant is derived from the census corpus and published, never taken on trust.

### 1.2 The op shape

The engine is batch-with-lookahead, so the op takes a window and returns a report on a throttle, never a stream. §8.3 proves that exact rather than merely cheap.

### 1.3 Why build it, given `PLAN.md:182`

Three reasons, in order of weight.

**1. `PARSES.md:6-9` already makes the argument, and it is the strongest one in the project.** "The log parser is not a tool in the hunt group, it is the only thing in the whole project that creates data instead of spending it." `PARSES.md` §3 makes the crafting quote engine depend on it: the combine formula rests on 1,740 logged combines from one player, the Mastery AA term is still marked UNVALIDATED, and the 44% drop rate next to a timber wolf is a placeholder. The combat half and the crafting half of a log parse are the same tailer, the same cursor, the same event store and the same coverage meter. Building the tailer for crafting and refusing to read the 50.3% of lines that are combat is not a smaller project, it is the same project with the differentiator removed.

**2. "The fourth site doing the same thing" is not what this is.** Measured against James's logs, four capabilities are available in EQL that no shipping EQ parser implements, because every one of them assumes live-EQ or classic-EQ line forms:

| Capability | Evidence | Who ships it |
|---|---|---|
| Composed inline modifier flags (28 observed combinations) | §2.3 | nobody: a whitespace splitter breaks on multi-word flags |
| Strikethrough-corrected melee accuracy | `(Strikethrough)` 504 + composites | nobody: live EQ does not log strikethrough reliably |
| Pet ownership read directly off the name (`` <Owner>`s warder ``) | measured; `My leader is` = 0 occurrences | nobody |
| Level, race and trio class for every player in range, from `/who` | `[40 CLR/MNK/BRD] Hrolf (Iksar)` | nobody: `/who` lists every player in range, not only you |

Plus persistence: grimoire keeps the events it parses, so a question that spans sessions ("is this weapon better" across two zone visits) is answered from stored data rather than from whatever text is still inside a rolling window.

**3. Accuracy is a real differentiator here, not a slogan.** A line that carries both a number and a damage word and produces no event is a silent loss, and an unparsed-line alarm keyed on the parser's own phrases cannot see it (§7 H1). That is beatable with a type change and a counter.

---

## 2. What the log can and cannot support

Hard limits first, because every metric in §3 is defined inside them.

### 2.1 The timestamp ceiling, quantified

Every line carries `[Www Mmm DD HH:MM:SS YYYY] `, bracket content exactly 24 bytes, whole seconds, no fractional part, no timezone. Verified on all 2,359,160 lines of the 190 MB log: zero unstamped lines, zero multi-line records, day always zero-padded.

A printed second `t` means the true event time is uniform on `[t, t+1)`. For a fight measured as span `S = t_last - t_first`, the true duration is `S + e` where `e = u_last - u_first` is **triangular on (-1, +1)**, mean 0, variance 1/6, **sigma 0.408 s**. For a triangular distribution `P(|e| > x) = (1-x)^2`, so the 95% bound is `|e| <= 0.776 s`. Relative error on any rate is `-e/S`:

| Measured span | Absolute bound | 95% band | 1 sigma |
|---|---|---|---|
| 60 s | ±1.7% | ±1.3% | ±0.7% |
| 30 s | ±3.3% | ±2.6% | ±1.4% |
| **16 s** | **±6.3%** | **±4.9%** | ±2.6% |
| 10 s | ±10.0% | ±7.8% | ±4.1% |
| 6 s | ±16.7% | ±12.9% | ±6.8% |
| **5 s (measured median fight)** | **±20.0%** | **±15.5%** | ±8.2% |
| **4 s** | ±25.0% | **±19.4%** | ±10.2% |
| 1 s | ±100% | ±77.6% | ±40.8% |
| 0 s | undefined | undefined | undefined |

This is not a corner case. On a real 4,392-line EQL fixture, **33 of 119 fights (28%) are zero-length, 45 (38%) are 3 seconds or shorter, and the median is 5 seconds.** For a typical EQL fight the DPS figure is uncertain by roughly a sixth from the clock alone, before any parsing question arises.

**Derived publishability floors, and they are policy, not taste:**

| Span | Output |
|---|---|
| `S >= 16 s` | point estimate `D/S` (95% band at or under 5%) |
| `4 s <= S < 16 s` | interval `[D/(S+0.776), D/(S-0.776)]`, never the midpoint |
| `S < 4 s` | `Refused(BelowResolutionFloor)` |

Two further quantisations sit on top of the clock. **DoT ticks land on a ~6 second server tick**, so any window under 6 seconds contains either zero DoT damage or a whole tick: the DoT component of a short-window rate is a coin flip, not a rate. That sets the **minimum honest burst window at 10 seconds**. And **the stamp is client receive time, not server event time**, so a lag spike compresses several seconds of combat into one printed second: peak DPS is inflated by latency, and the worse the connection the higher the reported peak. Burst is a lower-confidence metric than sustained and must be labelled as one.

The inclusive `+1` convention EQLogParser adopts is worth stating precisely, because it is not a rounding fix. `S` is an **unbiased** estimator of true duration. `S + 1` has error `1 - e` on `(0, 2)` with **mean exactly +1.0 second**, so it is a deliberate one-second inflation of the denominator, adopted to avoid dividing by zero. Cost: **-3.2% on a 30 s fight, -16.7% on a 5 s fight**, and a 3x overstatement on a single-second fight whose expected true duration is 1/3 s. Use `S`, refuse at `S = 0`, and keep `S + 1` only as an explicitly named `dps_gamparse_compat` field.

### 2.2 What the log does not carry at all

| Absent | Consequence |
|---|---|
| Pre-mitigation damage (measured: **zero** damage lines carry a `(potential)` parenthetical) | PDPS, potential damage, mitigation %, AC value: **not computable** |
| Absorb amounts (`magical skin absorbs the blow`, no number) | Absorbed points invisible to both the attacker's total and the tank's damage taken |
| Target HP, max HP, encounter fight % | No health curve in a death recap, no wipe-progress bar. HP is only ever a bound over completed kills (§3, T3.5) |
| Entity IDs | Two mobs named `a decaying skeleton` are one name, forever (§7 H5) |
| Ownership fields (`My leader is`: **0 occurrences** in the corpus) | Pet and charm attribution is inference, always (§5) |
| Buff landing, duration or fade for other actors | Uptime for anyone but the logging character: not computable |
| Per-hit buff snapshots | rDPS / nDPS / aDPS / cDPS: **not computable**, at any effort |
| Resource values (only `Insufficient Mana`, 273 occurrences, no number) | Mana curve: not computable. It is a cast-reliability counter, nothing more |
| Double / triple attack markers | Extra swings appear as more damage lines. Only the flagged subset is measurable (§3, T3.1) |
| Partial resist markers | Any resist count is a **floor**, never a rate |
| Chat filter state | A filtered category and a genuinely quiet fight are the same data (§7 H7) |
| Sub-second ordering | Line order is client **write** order, not causal order: the XP burst prints **before** the death line it belongs to |

Two limits are about perspective rather than fields, and they compose with the above: the log records only what the client was in range for, and heals are logged **BY you and TO you only**. A "raid meter" from one log is structurally a different quantity from raid DPS, and a healer ranking from a DPS's log is not a weak measurement, it is a false statement.

### 2.3 What the log carries that the surveys assumed it did not

These are measured, and four of them change the taxonomy.

**Inline modifier flags exist in full, and they compose.** Census over the whole corpus, damage and miss lines only:

```
41787 (Critical)             296 (Riposte Critical)          7 (Critical Flurry)
24489 (Riposte)              223 (Riposte Strikethrough)     6 (Strikethrough Finishing Blow)
 1269 (Crippling Blow)       196 (Critical Double Bow Shot)  5 (Riposte Slay Undead)
 1187 (Finishing Blow)        88 (Rampage)                   4 (Strikethrough Crippling Blow)
 1164 (Double Bow Shot)       69 (Strikethrough Critical)    3 (Wild Rampage)
  713 (Slay Undead)           31 (Riposte Strikethrough Critical)
  504 (Strikethrough)         13 (Riposte Crippling Blow)    + 8 further compositions
  386 (Flurry)                 7 (Riposte Finishing Blow)
```

Vocabulary of ten members, five of them multi-word, space-joined, and every one of the 28 observed combinations obeys one slot order:

> `[Riposte] [Strikethrough] [Critical | Crippling Blow | Finishing Blow | Slay Undead] [Double Bow Shot | Flurry | Rampage | Wild Rampage]`

Three consequences. Splitting on whitespace or at capital-letter boundaries turns `(Slay Undead)` into two flags and `(Riposte Crippling Blow)` into three: **tokenise by longest match against the vocabulary, left to right, and count the residue.** `Riposte` and `Strikethrough` are defensive-interaction flags, not crit flags. And the flag parser must be scoped to damage and heal lines, because `(Blocked by Swift Like the Wind.)` and `(Lvl: 52)` share the shape.

**Zero occurrences, corpus-wide:** Deadly Strike, Twincast, Lucky, Assassinate, Headshot. These are Absent, not zero (§6, arm 4).

**Overheal is fully computable, and the orientation is settled.** `healed X for A (B) hit points`: in freeport, **6,991 parenthetical lines, zero inversions, zero equal cases**; `A` is always strictly less than `B`. On the 190 MB log, 30,110 of 143,249 heal lines carry the parenthetical and 2,000 sampled cases are all strictly less, including many `for 0 (12)`. The parenthetical appears to be printed **only when overheal is nonzero** (19,588 plain-form lines in freeport, zero equal cases in 2,000 sampled). So: `A` is applied, `B` is potential, `overheal = B - A`, and a plain line means overheal zero. This was the largest flagged uncertainty and it resolves in grimoire's favour.

**Other confirmed format facts:**

| Fact | Measured |
|---|---|
| Damage elements, closed set of 9 | non-melee 202,317, magic 27,175, fire 8,280, cold 5,855, poison 3,312, disease 2,369, unresistable 371, prismatic 168, chromatic 7 |
| Attack verbs, from the unambiguous `tries to <verb>` slot | 20 verbs from slash 234,713 down to smash 179, then a **cliff to 18** (chat, "tries to beg"). `gore` and `slam` do not occur |
| Damage-shield participles | only `pierced` 59,889, `burned` 24,043, `tormented` 1,811 |
| Terminator varies by perspective | `points of non-melee damage!` when the target is YOU (7,181) vs `.` otherwise (36,147). A rule anchored on `.` drops all incoming DS damage |
| Singular at N=1 | `point of damage.` 25,778 vs `points of damage.` 167,116 (freeport) |
| DoT tick shapes | `from your <Spell>` 1,243; `from <Spell> by <Caster>` 16,011; **anonymous** `by <Spell>` 953 |
| Zone lines | `You have entered Befallen 4 (Refined).` Instance number and tier tag are part of the line |
| `/who` | `[40 CLR/MNK/BRD] Hrolf (Iksar)  ZONE: Befallen 6 (befallen)` |
| Deaths | `You have slain X!` 2,142, `X has been slain by Y!` 3,916, `You have been slain by X!` 84, `X died.` 26 |
| Combat share | 220,250 damage + 159,427 miss = **50.3%** of freeport's 753,988 lines |
| Quantified lines | ~40.7% of freeport lines carry a space-delimited digit run after the stamp |
| Unmodelled today | `You hurt yourself for N points.` (4,247 freeport, 5,855 corpus), `You were hit by non-melee for N damage.` (106), `You avoid the stunning blow.`, `Insufficient Mana` (273) |

**Time is near-monotonic in the file, but design for the opposite anyway.** Measured: **one backward step in 753,988 lines, worst case 2 seconds.** That does not license assuming monotonicity: a DST fall-back replays an hour, and the 190 MB log spans Jul and Aug, so **lexical comparison of the stamp string reorders the file** ("Aug" < "Jul"). Parse to an integer second, never sort the string, count inversions as a data-quality figure, clamp negative deltas to zero.

---

## 3. The metric taxonomy

A metric is not specified until three things are written down: the **numerator**, the **denominator**, and the **population of events included**. Every entry below carries all three by definition plus its accuracy hazard.

Six foundations sit underneath all of it, and they are where accuracy actually lives.

| # | Foundation |
|---|---|
| F1 | **Three-arm line outcome**, never `Option`. `Recognised` / `Ignored(RuleId)` / `Unknown`, where `Ignored` requires a positive match on a catalogued ignore rule and is never a fallthrough. |
| F2 | **Canonical order is file order.** Sort key is `(second, line_ordinal)` everywhere, and the ordinal is a field on the event, not a `Vec` index. Never sort by timestamp alone. |
| F3 | **A stamp is a 1-second interval, not an instant.** Every rate carries the §2.1 band. |
| F4 | **Spell identity is `(base_name, Option<rank>)`.** Rank is stripped only when it validates as a Roman numeral. |
| F5 | **Every event carries an attribution grade,** not a boolean: `SelfEvident` / `NamedInLine` / `OwnerEncodedInName` / `ClaimedByTell` / `ClaimedByCastLock` / `Unattributed`. |
| F6 | **No field is ever named `dps`.** The denominator basis is part of the value. |

### Tier 1: table stakes

Without these it is not a parser.

| Metric | Definition | Accuracy hazard |
|---|---|---|
| `damage_total` | Sum of landed damage attributed to one actor inside one encounter, plus a parallel `damage_unattributed` channel | Anonymous DoT ticks (953) and casterless damage shields have no source. They are real damage that came off the mob. Routing them to `Unattributed` rather than dropping them is what makes `attributed + unattributed = total` reconcilable |
| `dps_active` | damage / that actor's own union of engagement stretches, gap tolerance in the column header | Not additive across actors. This is what GamParse means and what raiders are calibrated on |
| `dps_encounter` | damage / the encounter's full span | The only denominator under which contributors sum to the group. **Never share one denominator across actors:** a raider who joined 10 minutes late is understated by exactly the fraction missed |
| Fight and pull segmentation | §4 | Fight start must be the first **blow**, not the first touch |
| `damage_share_pct` | actor damage / all included actors in scope | The most robust comparative number available, because it is denominator-free. But the "all actors" population is one filtered, range-limited client view. Label the scope |
| Damage by source and ability | per `(actor, ability)`: hits, total, share, avg, max, flag columns | **Verb families collapse.** Kick, Round Kick and Flying Kick all print `kick`; Tiger Claw prints `strike`. A verb row is a family, not a skill |
| `damage_taken` total and by source/ability | | Populations must match: a total including spells and DoTs divided by a count of melee and ranged hits is two different populations (§7 H20). `You were hit by non-melee` has neither source nor ability and is its own category, not a DoT |
| Deaths and kill credit | one predicate, with the **basis** recorded per kill | More than one predicate in one app lets two screens disagree on the same session (§7 H21). Basis is `OwnBlow / ClaimedActor / ServerReward / Witnessed / Inferred` |
| Attempts, landed, avoided, `to_hit_pct` | attempts = landed + avoided | Weapon swings and combat skills tallied **separately**, not blended into one headline |
| `healing_effective`, `healing_potential`, `overheal` | effective = A, potential = B when present else A, overheal = potential - effective | The passive `has been healed` form names **no healer** and must not count as yours. Reflexive targets resolve to the actor the line already named. Mend prints no number and can only ever be a use-counter |
| Coverage residue | `lines_seen` / `recognised` / `ignored` / `unknown` + top-N unknown shapes | **The most important item in the taxonomy.** §9.5 |
| Side and ownership | §5, as intervals with F5 grades | Do not infer enemy-ness from **name shape**. Positive evidence only, plus an explicit `Unclassified` bucket |

### Tier 2: expected

An EQ raider will ask where these are.

| Metric | Note |
|---|---|
| Crit rate by family | Four crit-family flags (§2.3). Compute it for DoT and damage-shield rows too, not only melee |
| Avoidance rates, **both directions** | Keep the outgoing `but <how>` clause. Thrown away, it leaves you able to see that you missed but never why: exactly the information separating an accuracy problem from a level-difference problem |
| `full_resists_observed` | Three-way attempts derivation: your cast lines where they exist, else landed + resisted for procs, else **blank**, never resists/resists = 100%. Named a floor, not a rate |
| Cast reliability | casts, fizzles, interrupts, out-of-mana (273 measured, currently parsed and never surfaced) |
| Burst: fixed 10s / 30s / 60s rolling maximum | Not a variable-width sparkline peak, whose bucket widens with the slice, so the same fight reports different peaks at different zoom levels |
| Damage composition by category | melee / skill / ranged / cast / proc / DoT / DS / pet / charm, exclusive and exhaustive, with a visible residual |
| HPS, healing by target, healing received | Single-perspective. Default to the received view, which is always complete for the logging character, and label whose log it came from |
| Per-mob aggregate | Their DPS denominated on the mob's **own offensive window**, not the fight span |
| Death recap | Incoming sequence only. **No health curve is possible** |
| The posted parse string | `1. Name = Total@DPS in Ns`. The at-sign slot is active-time DPS by convention |
| Raid table with **per-actor** active seconds | Emit `dps_encounter` (shared, additive) and `dps_active` (per-actor) side by side |
| Zone and session segmentation | With the instance/tier decomposition of `Befallen 4 (Refined)` |
| Active time and activity % | The diagnostic that explains a bad DPS: low output with high activity is a rotation problem, low with low is an attention problem |
| Session economy | XP with same-second post-ding rollover excluded from per-kill averages; coin by source; **observed** drop rate labelled a floor (the log has no line for opening an empty corpse) |

### Tier 3: advanced

The differentiators. T3.1, T3.2, T3.3 and T3.6 are available because of §2.3's measurements and are shipped by no EQ parser.

| Metric | Definition and why it beats the field |
|---|---|
| **T3.1 Extra-swing accounting** | Flurry rate, double-bow-shot rate, rampage rate, from per-swing flags. In a game where multi-attack was assumed unmeasurable, this is a real measurable subset. **Caveat:** Double and Triple Attack exist as EQL mechanics but are **not flagged**, so coverage is partial and must be labelled so |
| **T3.2 Strikethrough-aware accuracy** | `hits / (attempts - parries - dodges - blocks - absorbs)`. rumstil documents that live EQ does not report defences other than riposte on a strikethrough, which biases the denominator permanently. **EQL logs `(Strikethrough)` explicitly**, so grimoire can compute the corrected denominator live-EQ parsers cannot |
| **T3.3 Graded pet and charm attribution** | Four evidence grades (§5). Grade (a), owner encoded in the name, is free ground truth nobody exploits |
| **T3.4 Same-name contamination**, propagated everywhere | Three detectors (§7 H5). Detecting it is half the job: the flag must reach **every** per-entity figure, not one |
| **T3.5 Mob HP bounds** | `hp_max = damage taken - heals received`, `hp_min = hp_max - last_blow + 1`, multiple clean kills **intersect** rather than average, and an empty intersection is a finding (the name covers more than one variant), not noise |
| **T3.6 Roster-aware raid view from `/who`** | Level, race and trio class for every player in range. Enables per-class share, level-normalised comparison, and correct pet grouping. Stamp it and decay it: it is a snapshot, not a fact for the session |
| **T3.7 Stance / invocation A/B** | EQL-specific, no analogue in classic parsers. Ship it **with a sample-size gate**, or a 30-second combo outranks a 30-minute one |
| **T3.8 Swing interval with a Poisson band** | An **observed** interval, never a weapon delay: haste and unflagged multi-attack fold in. Only same-actor ratios are meaningful |
| **T3.9 Rates as intervals** | §2.1's floors, made visible. The direct answer to "accuracy of that parse is very important" |
| **T3.10 Attribution reconciliation, surfaced** | `total == sum(rows) + unattributed`, shown as a data-quality signal. Necessary and **not sufficient**, see §7 H28 |
| **T3.11 Cross-session persistence** | Stored events make a question across sessions answerable. This is grimoire's decisive structural advantage, and a parser built on a rolling text window cannot retrofit it cheaply |
| **T3.12 Single-target vs multi-target split** | The available anti-padding measure. EQ names the target on every line, so it is free |
| **T3.13 Buff-block telemetry** | `Your <Spell> spell did not take hold. (Blocked by <Existing>.)` Tells a buffer exactly which casts were wasted. Nobody parses it |

### Tier 4: out of scope, and why

Every entry here will be asked for. Each has a reason, and most are measurements rather than assumptions.

| Metric | Why not |
|---|---|
| rDPS / nDPS / aDPS / cDPS | Requires per-hit buff snapshots. EQL logs no buff application, duration or multiplier for other actors, so the counterfactual is unrecoverable. **Do not ship a field named rDPS** |
| PDPS / potential damage | **Measured: zero damage lines carry a potential parenthetical.** Omit, never silently equal to DPS |
| Mitigation %, AC value | Only post-mitigation damage is logged. Any such number is fabricated |
| Absorbed damage totals | The absorb line carries no number. Count absorb **events** as a defence category |
| Boss HP %, wipe-progress bars | No HP field anywhere |
| Twincast / Deadly Strike / Lucky / Assassinate / Headshot | **Measured zero.** Rendering `0%` asserts the mechanic exists and the player has none, which is a false statement about the game |
| Double / triple attack rate | Mechanics exist, extra swings unflagged |
| Buff uptime for other actors | No landing, duration or fade event |
| Percentiles, brackets, all-stars | No population, and manufacturing one means uploading named third-party performance data, colliding with `PARSES.md` §5. An invented percentile is worse than none: authoritative-looking and unfalsifiable |
| TMI / KRSI / tank rankings | Need a health pool, sub-second resolution, and hand-authored per-boss tanking windows |
| GCD / APM / weaving | 1 Hz timestamps |
| Threat / aggro | Nothing in the log |
| True raid-wide DPS | One filtered, range-limited client view. Name the scope, never call it raid DPS |

**What actually transfers from the Warcraft Logs / FFLogs lineage** is the *discipline*, not the metrics: publish more than one denominator and put the denominator in the field name; active time as a first-class diagnostic; death recap; boss-only damage as the anti-padding measure; drill-down from every aggregate to the raw lines that produced it; refusing to score input you cannot score honestly; and visible honesty about known breakage. The one place credit reassignment is both meaningful and provable in EQ is **pets and charms** (§5): an enchanter's contribution *is* their charmed pet's damage, and unlike a buff, that damage is directly attributable.

---

## 4. Encounter detection

This is where parsers disagree, and it is entirely a matter of undeclared convention rather than of reading the log wrong. Three shipping implementations pick three different rules:

| Tool | Fight start | Fight end | Fight timeout | Encounter grouping |
|---|---|---|---|---|
| GamParse | first damage in either direction | slain line, else **last damage** | 30 s | none published |
| rumstil | first hit **or miss** | mob death | | none |
| EQLogParser | | | 30 s with damage blocks, 60 s without | 120 s gap |

### 4.1 Three windows per fight, never one

| Window | Definition | Used for |
|---|---|---|
| **Presence** | first to last of *any* event naming the entity, including mez, spell-fade, con | boundary bookkeeping only, **never a denominator** |
| **Engagement** | first to last damage-or-miss event **in either direction** | the fight's real bounds, time-to-kill, per-fight rates |
| **Actor-active** | union of one actor's own damage/miss stretches, gaps over `G` excised | per-actor DPS, never shared |

Using presence as a denominator is the mez bug: a mez landing on a parked add 30 seconds before you engage starts the clock and deflates per-fight DPS by 30 seconds. The mob's own output obviously needs the engagement window (a mob that stood there 20 seconds before it noticed you was not dealing damage for those 20 seconds), and yours needs exactly the same rule. That symmetry is the whole argument.

### 4.2 The rules, stated exactly

1. **Start.** The first damage event in either direction involving the mob. Debuff, root, snare and mez do **not** start a fight. Record the first-miss timestamp separately so a pull-phase view is possible without moving the denominator. (rumstil's "first hit or miss" is a defensible alternative; it must be a named option, not a silent choice.)
2. **End.** The death line where one exists. Otherwise **the timestamp of the last damage**, never the timeout expiry. Ending at expiry adds the whole timeout (30 to 45 seconds) of dead air to every unfinished fight and can halve trash DPS.
3. **Two timeouts.** `fight_idle` closes one mob. `pull_lull` cuts a group of fights into an encounter on **total** quiet. One value forces a choice between splitting a boss at a lull and welding trash into the boss.
4. **Both constants are named, configurable, and printed in the report header,** so a disputed parse is reproducible. They are chosen bounds, not measured ones, and the spec should say so rather than dress them as findings.
5. **Union time segments, never sum durations.** GamParse fixed exactly this and noted it "lifts DPS numbers to better reflect what they should be."
6. **Group encounters from the per-second ledger, not from overlapping fight windows.** Cut when the gap exceeds `pull_lull` **or** when no single fight's activity span covers both sides of the gap ("nobody left standing"). Without the second test, one long-lived add welds two unrelated pulls together: the real observed regression is a fire giant warrior fusing two named bosses 25 seconds apart into a single four-minute encounter.
7. **A mob alive across a cut belongs to both pulls**, with its damage split at the boundary via the ledger.
8. **Fight identity is `(start_second, mob_name)`,** computed at construction, never an array index. Positional ids renumber under a sliding window: measured, **28% of simulated live polls swapped which mob an index named**.

### 4.3 The per-second ledger is the primitive

`beats: Vec<{ second, you, pet, charm, taken }>`, appended only by damage and miss events (a mez refresh is bookkeeping, not combat). This is the single design decision that makes arbitrary re-slicing (pulls, windows, focus, burst maxima, charts, share-of-total) **exact rather than approximate**. It must be keyed rather than blindly appended, because the "second being written is always the last bucket" assumption is false under any inversion.

---

## 5. Attribution rules

**The fundamental fact: the log contains no ownership field.** Live EQ has `/pet leader` returning `My leader is <Name>.` as ground truth; that line has **zero occurrences** in this corpus. So ownership is inference, always, and the only defensible posture is a graded ladder with an explicit refusal arm.

### 5.1 The confidence ladder

| Grade | Meaning |
|---|---|
| `Proven` | A log line states it: your own first-person lines, your own killing blow, a past-tense pet tell addressed to you, a charm broadcast locked to your own cast, an owner encoded in the name |
| `Inferred(rule_id, window)` | A correlation within a stated window: kill credit from an XP line within ±2 s, special-attack lane from a `You will now use X` span |
| `Unattributed` | The log names no source, or names one that cannot be resolved |

**Every aggregate must be filterable to `Proven` only.** That is the strict mode. `Inferred` records which rule and which window produced it, so a disagreement is diagnosable rather than arguable.

### 5.2 Pets

Two claims, in descending strength, and no others.

1. **Owner encoded in the name.** Beastlord pets print as `` <Owner>`s warder `` (measured: `` Bate`s warder ``, `` Beavis`s warder ``). This is direct, requires no inference, and eliminates the problem entirely for the most common pet class. `Proven`.
2. **A past-tense second-person tell** addressing you as Master (`<Name> told you, '...Master...'`, 1,878 occurrences). The client shows you nobody's pet tells but your own, and a live player cannot produce the past-tense form. `Proven`.

Ambient `/say` claims are **rejected**. A say has a radius, and at a two-mage camp it claims the other mage's pet.

The cost of the surefire rule is real and must be **surfaced as a figure, not as silence**: a pet that never receives an attack-family order never tells, so 100% of its damage is unattributed, and a charmed pet can kill several named mobs with the parse showing nothing. Ship an **"unattributed team-adjacent damage"** figure (damage to mobs you were fighting, from actors that are neither you, nor a claimed actor, nor an actor that ever damaged you) as a bounded uncertainty next to the total, and tell the user that one `/pet attack` fixes it.

### 5.3 Charms

A casterless `<N> has been charmed.` broadcast (420 occurrences) is yours if and only if **all three** hold:

1. your own `You begin casting <S>.` is the most recent cast within `CHARM_WINDOW` before it;
2. no interrupt, fizzle or resist of that spell's **base name** (F4) landed between;
3. **exactly one** broadcast falls in the window.

Two broadcasts means neither is claimed. Refusing is correct, and the refusal's cost goes in the unattributed bucket rather than into a guess.

### 5.4 Claims are intervals, with an asymmetric boundary rule

A claim is `[t0, t1)` per name, because summoned-pet names come from a shared generator and charm hands mobs back. `t0` backdates to the last boundary so swings between summon and your first order still count.

| Boundary | Second |
|---|---|
| the actor's own death line | **inclusive** (`ts + 1`): a killing blow prints in the same second as the death line |
| you zoned or died | **inclusive** |
| your charm spell wearing off it | **inclusive** |
| it damaging **you** | **exclusive**: the triggering hit belongs to the broken charm |

**Proven charm-held spans outrank inference.** Between a proven grant and the first wear-off of that same base spell, the name is yours regardless of what else the log says, and the two ambiguous boundaries (it hit you, it died) are **suppressed** inside that span. The case this exists for: two mobs both named `` Innoruuk`s Chosen `` fought each other, the hostile twin hit the player at 12:57:55 and died at 12:58:29, ending the claim on the player's still-charmed pet, and a named kill at 13:00:13 was credited to nobody. The Allure did not wear off until 13:03:04.

### 5.5 DoTs

Three tick shapes, and the third is unattributable by construction:

| Shape | Count | Attribution |
|---|---|---|
| `has taken N damage from your <Spell>.` | 1,243 | `Proven`, yours |
| `has taken N damage from <Spell> by <Caster>.` | 16,011 | `NamedInLine` |
| `has taken N damage by <Spell>.` | 953 | **`Unattributed`** |

Credit **at application, not at tick**: a tick belongs to the applier regardless of what happens afterwards. The anonymous form occurs specifically when the caster died or zoned, which is exactly when a raid parse matters most. Never infer the caster from the spell name alone.

### 5.6 Ambiguous names

| Case | Rule |
|---|---|
| Article capitalisation (`A training dummy` / `a training dummy`) | Canonicalise a leading `A `, `An `, `The ` to lowercase, unconditionally. **Not** on within-window evidence, which makes identity depend on window size (§7 H17). Never lowercase unconditionally: that merges the player `Hadden` with a mob `a hadden` |
| Reflexive pronouns (`healed himself`) | Resolve to the actor the same line already named. Load-bearing for HP bounds: a lifetapping mob's self-heal must come back out of its damage total. Assert in a test that **no emitted actor name is a pronoun** |
| Player vs NPC vs pet | `Targeted (Player): <name>` **proves** PC-ness. `(NPC)` proves nothing, because pets type as NPC. Build the proven-player set from the positive line only |
| Pet possessives | Backtick, not apostrophe (`` Teir`Dal ``, `` V`Zher ``, `` Ravlin`s warder ``) |
| Another player's pet | Its own untagged row, never folded into a player. A text log cannot tie the two together |
| Two same-named mobs | Not resolvable. Detect and exclude (§7 H5), never average |

### 5.7 Mercenaries

**No evidence EverQuest Legends has them.** It is pre-Kunark classic content, and mercs are a much later live-EQ system. Do not build for a system that may not exist (§11.8). If they exist, they are the pet problem with a different proof line and the same claim model.

---

## 6. The error model

Stated once, applied everywhere.

### 6.1 The argument

Grimoire already has this policy in three places, and combat needs the same shape plus one arm.

`Source::Unknown` (`grimoire-core/src/recipe.rs:120-122`): *"Not known yet. Treated as un-buyable, because promising to source something the app cannot price is worse than asking."* The structure worth copying is the part usually missed: **the unknown is neither blanked nor guessed, it is given a different disposition that changes what the system does downstream.** It routes to the buyer. It is an honest answer with an action attached, not an absence.

`pinned_trivial()` (`grimoire-parse/src/combines.rs:53-62`) returns a number only when the bracket has closed to one or less: *"Anything looser is a range, and a range must not be reported as a number."*

`inventory.unreadable` (`grimoire-parse/src/inventory.rs:84-85`) counts rows it could not read rather than dropping them: *"Non-zero means the format moved."* The combine path notably does not have this, and that gap is exactly what §9.5 closes.

### 6.2 The policy

**Four display arms, and one figure that accompanies all four.**

| Arm | When | Output |
|---|---|---|
| **Exact** | The log stated it and nothing was inferred | The number, plain |
| **Bounded** | The log constrains it to an interval | **The interval. Never the midpoint, never an average of the ends** |
| **Refused** | The log cannot constrain it usefully | A refusal token **with a one-line reason**. Not a blank, not a zero |
| **Absent** | The metric does not exist in this game | **The column does not render at all** |
| *Coverage* | always | The residue that fed the figure: `lines_seen / recognised / ignored / unknown`, the unattributed bucket, the taint count |

Examples: damage from named sources, hit and miss counts, coin in copper are **Exact**. Mob HP, any DPS over a 4-16 second span, any total with unattributed residue (`[attributed, attributed + unattributed]`), mob level from cons are **Bounded**. Sub-4-second DPS, a resist rate with no attempts denominator, mitigation %, absorbed amounts, other-player buff uptime, percentile rank are **Refused**. Twincast, Flurry-when-unobserved, rDPS are **Absent**.

**Why refusal beats a caveated number.** A caveated number gets pasted into Discord, compared against a friend's, and screenshotted. **The caveat does not travel with the number. The refusal does, because it is the number.** This is precisely why `Source::Unknown` is not "Vendor with a warning attached". A blank is worse than either, because a blank reads as zero or as not-loaded-yet, both of which are false statements.

**The rule that keeps the policy usable:** refuse only when there is no honest bracket. If it can be bracketed, bracket it. Most things here can be. Refusal is the last arm, not the first.

### 6.3 The shape, and its invariants

```rust
pub enum Measured<T> {
    Exact(T),
    Bounded { lo: T, hi: T },   // invariant: lo <= hi, both stated-derived
    Refused(Reason),            // carries why, always
}

pub struct Confidence {
    attribution: Attribution,   // Proven | Inferred(RuleId, Window) | Unattributed
    coverage: Coverage,
    taint: bool,
    perspective: Perspective,   // observer + Complete | PartialByRange | PartialByFilter
}
```

Non-negotiable, each with a test:

1. `lo <= hi` on every `Bounded`.
2. `Refused` always carries a `Reason`. There is no `Refused(())`.
3. **No unknown ever defaults to a known value.** A null-source DoT does not default to the log owner. An unknown level does not default to 50. A missing heal potential does not default to the applied amount, which makes overheal read 0% and a wasteful healer look perfect. An unrecognised line does not become `Ignored`.
4. Every rate names its basis **in the type**, not in a tooltip.
5. The parse is **deterministic and idempotent**: same file, same numbers, every run, in any chunking. Without this none of the above means anything, because the user cannot reproduce the figure they are disputing.
6. `total == sum(attributed) + unattributed`, exactly, on integers.

---

## 7. Accuracy hazards, ranked

Ranked by **how badly and how silently a hazard corrupts a number**: silence (does the wrong number look normal), magnitude (unbounded / large / moderate / small), frequency. Three bands fall out. Band A is silent and unbounded: these destroy trust retroactively, because when one is found every past number becomes suspect.

### Band A: silent and unbounded

| # | Hazard | Wrong number the user sees | Mitigation | How tested |
|---|---|---|---|---|
| **H1** | An unrecognised line is indistinguishable from an ignored one, and the drift alarm is blind by construction | Totals quietly low by an unbounded amount under a banner reading `0.0% unparsed`, because a tripwire keyed on the parser's own phrases cannot see a line none of those phrases match. A reworded melee sentence loses every melee hit and moves no banner | F1 trichotomy; coverage ratio with `lines_seen` as denominator; bounded residue sample; alarm keyed on a **broader, independent** signal (any line carrying an integer no rule consumed); grammar versioning | The mutation catalogue (§9.5), asserting the **alarm fires**, not that totals hold |
| **H2** | A closed enumeration drifts out from under a sentence that still matches | A damage-shield total 2.1% low with no structural change to complain about. **Measured: `tormented` is 1,811 lines of the corpus's damage-shield participles, so a participle set that omits it drops all of them while the byte-identical sentence with a listed participle parses** | Loose-extractor census versus strict set over the whole corpus, as a test **and** a `grimoire census` CLI command. A set rejection routes to `Unknown`, not to silence | `enumeration_closure_over_corpus`; plus a test that a fabricated verb increments `unknown` |
| **H3** | Encounter boundaries unspecified | The same fight at 45K in one tool and 28K in another. Ending at timeout expiry adds 30-45 s of dead air to every unfinished fight | §4 in full | **The boundary matrix**: one fixture, N configurations, asserting **damage total is invariant** and only rates move. This separates boundary policy from attribution completely |
| **H4** | The DPS denominator is undeclared | Four numbers all labelled DPS. **Measured in one session: combat seconds 1,145, active 3,611, wall 8,666. A 7.6x spread on the same damage** | Basis in the type (F6); `dps_encounter` and `dps_active` side by side, gap tolerance in the header; never share a denominator across actors | `changing_the_basis_changes_the_number`; `no_report_mixes_bases_in_one_comparison` |
| **H5** | One mob name is many creatures | Per-fight damage, DPS, avg length, XP/kill and their-DPS are a blend with no marker. **Measured: 5 of 20 name-groups needed interval clustering; 25 of 119 fights taint-flagged (21%)** | Three detectors: post-death activity on a dead name; a new fight on that name within the bound of a kill; **dotStack** (the same `(second, spell, caster)` triple twice on one name, impossible for one caster on one entity). **Propagate taint to every per-entity figure**, not just the HP bound | Synthetic two-same-named-mobs scenario: assert taint fires, the fight is absent from per-mob HP, its damage is present in the session total |
| **H6** | One client's range-limited view presented as a raid parse | Out-of-range players at zero, their misses invisible. A healer leaderboard from a DPS's log ranks the DPS first and every healer at zero | `Perspective` as a field, not a tooltip; name the metric by scope (`damage_seen_from(observer)`); prefer share-of-observed; never rank anyone the observer's client cannot fully see | A fixture where an actor appears in a kill line with no damage lines: assert `PartialByRange`, not `damage: 0` |
| **H7** | Chat filters delete categories, and no line records filter state | A group-mate at 0 DPS; a session with four necromancers and zero DoT ticks. Indistinguishable from "nothing happened" | Three named heuristics: took damage with zero incoming melee from that mob; casters present with zero DoT ticks; an actor in kill lines with zero damage lines. Fire a **diagnostic**, not a zero | Delete all `dot` lines from a real fixture; assert the diagnostic fires and the column renders `unknown` |
| **H8** | No ownership field | Two opposite failures: a pet's entire damage missing with no figure quantifying it, or another player's pet claimed as yours | §5 in full, plus the unattributed-team-adjacent figure | Adversarial pet fixture with computable ground truth: two casters, overlapping charms, a pet that never tells |
| **H9** | Null-source damage | The mob's HP bracket and the sum of meter rows for the same fight cannot be reconciled, and nothing explains the gap | Carry null source explicitly; aggregate into a visible `Unattributed` row **that appears in totals**; never default to the owner | `total_reconciles`; `no_null_source_defaults_to_owner` as a property |
| **H10** | Chat-quoted combat lines become real damage | Damage that never happened, injected by another human. **grimoire's existing defence does not transfer:** crafting rules anchor at byte 0 and no chat form begins `You have fashioned`; combat lines begin with an **arbitrary actor name**, so offset-0 anchoring is structurally impossible | Chat prefilter **before any combat rule**, covering multi-word speakers and the outbound `You tell/say/shout` forms; match combat as a **total grammar** with no leading slop, anchored to end of line | An adversarial corpus embedding **every** recognised combat sentence inside **every** chat form: zero events, totals byte-identical. Written by someone trying to break it |

### Band B: silent but bounded

| # | Hazard | Wrong number | Mitigation | How tested |
|---|---|---|---|---|
| **H11** | Whole-second resolution | Two-decimal DPS whose real uncertainty is a sixth of its value. Median fight is 5 s | §2.1's floors: point at ≥16 s, interval 4-16 s, refuse below 4 s | `duration_is_never_negative`; `no_nan_or_infinity`; golden assertion that sub-4 s fights emit `Refused` |
| **H12** | The inclusive `+1` convention | Every rate low by a computable amount: -3.2% at 30 s, -16.7% at 5 s | Use `S`; refuse at `S = 0`; keep `S+1` only as `dps_gamparse_compat` | `single_second_fight_refuses`; `compat_and_measured_differ_by_the_documented_factor` |
| **H13** | Non-monotonic time; a stamp string that is not sortable | A negative denominator, then after a `max(1,…)` floor, **the raid's total damage printed as its DPS**. Separately, lexical stamp order interleaves Apr and Aug | Parse to a **naive civil** timestamp once, never sort the string, never round-trip a zone-aware epoch; clamp negative deltas to zero and **count** them | A two-month fixture; a DST fall-back fixture; a concatenated-sessions fixture; `non_monotonic_events_are_counted_not_absorbed` |
| **H14** | Sorting by timestamp destroys intra-second order | Mob HP bounds change between two parses of the same file, because `hp_min` depends on which blow was last | Sort key `(second, ordinal)` everywhere, ordinal a field not an index. Ordinal is a **tiebreaker, not causality**: the XP burst prints before its death line | `two_parses_of_the_same_file_are_byte_identical`; a 12-events-in-one-second fixture |
| **H15** | Overkill counted as damage | A killing blow for 5,000 on 800 remaining HP inflates the total and the max-hit tile by 4,200. **EQ has no overkill field, so it cannot be removed** | Document as a known upward bias on killing blows; **exclude the killing blow from max-hit statistics** | A synthetic kill whose final blow exceeds scripted HP |
| **H16** | One line matched by two rules | A damage category exactly doubled | Ordered, first-match-wins dispatch as a type-level guarantee, so one line yields at most one event | `sum_of_category_totals_equals_total` exactly, on integers; `every_line_produces_at_most_one_event` |
| **H17** | Whole-window inference | The same fight attributes differently by window size, so the overlay and the report disagree. Three such rules are tempting: evidence-gated name folding, kill-gated enemy classification, cast-gated proc classification | Replace all three with window-independent rules (§8.3) | `identity_is_window_invariant`: parse whole, then at three window sizes, assert an identical canonical actor set |
| **H18** | Enemy classification by name shape | Zero damage to a boss you fled, or whose death line fell outside the window. A single-word NPC produces **no fight and no row** | Positive evidence only, plus an explicit `Unclassified` bucket the UI surfaces | A fixture with a single-word NPC damaged and never killed: assert its damage never vanishes |
| **H19** | Spell identity keyed on the raw string | Every ranked nuke filed as a proc; the resist table splits one spell into a casts-only row and a landed-only row, both rates wrong. **Measured: `Venom of the Snake I` 1,209 and `Venom of the Snake` 237 in one file, one caster, one line form** | F4: base name everywhere, rank a separate field, Roman-numeral validated | `ranked_and_unranked_forms_of_one_spell_are_one_row` |
| **H20** | Population mismatch inside one statistic | An avg incoming hit inflated by every nuke in the window (total over all categories, count over melee and ranged only). Same class of defect: melee headline blending weapons with skills; a focused fight counting an unrelated fight's casts | A derived statistic names both populations and a test asserts they are the same set. Separate weapons from skills at the **category** level | `numerator_and_denominator_populations_match`; two-simultaneous-fights fixture |
| **H21** | More than one kill-credit predicate | Two kill counts on screen at once. Silent-kill inference also invents kills from a group-mate's XP | One predicate, **basis recorded**, strict mode excluding `Inferred`, inferred kills marked | `all_surfaces_agree_on_kill_count`; a nearby-group XP fixture |
| **H22** | Live tail: rotation, truncation, short reads, encoding | Totals that jump or a session that vanishes. Padding a short read injects NULs **and advances the cursor past bytes never read**. Whole-file UTF-8 decode kills a 160 MB log on one bad byte; the CLI's Latin-1 fallback and the browser's `file.text()` produce **different entity names from the same file**, and names are the join key | Read bytes; decode **lossily per line**, counting; same rule both doors. Return only bytes read; `Vec<u8>` remainder; shrink detection by size regression; replacement by file identity; **tail every matching file independently** (the busiest-file heuristic flips every poll for a boxer) | Chunk-invariance property; suffix property; a rotation simulation; `same_log_same_names_on_both_surfaces` with a Latin-1 high byte |

### Band C: visible, small, or narrow

| # | Hazard | Wrong number | Mitigation | How tested |
|---|---|---|---|---|
| **H23** | Non-content-derived identity and non-determinism | The user clicks a fight and gets a different one (28% of polls). Rust randomises `HashMap` order per process | `(start_second, mob_name)`; total order with explicit tiebreakers before serialisation, as `combines.rs:158-165` already does | `two_parses_serialise_byte_identically`; `fight_ids_are_stable_across_window_sizes` |
| **H24** | Metrics the game may not have, rendered as zero | `Twincast 0%` asserts the mechanic exists and the player has none | Capability profile generated by the census; Absent means the column does not render | `no_column_renders_without_an_observation` |
| **H25** | Unmeasurable quantities presented as measurements | A mitigation %, an absorb total, a resist *rate*, a percentile | Name the field for what it is: `full_resists_observed`, `absorb_events`. Where nothing honest can be said, refuse | Type-level: no field named `*_rate` is constructible without a denominator |
| **H26** | Name canonicalisation splits one actor | Two rows, each with half the damage and split HP bounds | Article lowering only; never unconditional lowercasing | Both capitalisations of one mob assert one row; a player and mob differing only by case assert two |
| **H27** | Reflexive pronouns and self-damage | An actor named `himself`; 5,855 self-damage lines producing nothing. **Measured: 96 of the fixture's events have source == target, 5.6% of points** | Resolve reflexives to the named actor; give self-damage an explicit rule and an explicit home; **decide deliberately whether it enters the damage total and write the decision into the test name** | `no_emitted_actor_name_is_a_pronoun`; `self_damage_has_a_named_home` |
| **H28** | Conservation invariants pass while the number is wrong | All of the above, protected by a green suite. **`total == sum(rows)` and `sum(fights) == total` both hold on a parse that drops lines,** because a line never read is missing from both sides | Never ship conservation without coverage beside it, and **say so in the test names**. Add a **rule-coverage gate**: every grammar rule must be exercised by at least one fixture line, or CI fails | The gate is the test |

**Adjacent, not ranked.** `PARSES.md` §1 makes "one player's log already contains everyone else's damage in range" the adoption argument, and §5 requires a fight view show "nothing derived that they did not consent to". A combat parser processes named non-consenting third parties by construction; the crafting parser never did. This is not an accuracy hazard, but it lands on the **test corpus**, which is the easier-to-miss exfiltration route. See §9.7.

---

## 8. Implementation shape

### 8.1 Crates and modules

A combat parser is three separable things and only one of them is stateful.

| Stage | Shape | Stateful | Home |
|---|---|---|---|
| line to raw event | `&str -> Outcome<'_>` | no | `grimoire-parse::combat` |
| raw event to interned event | `(&mut Names, Raw<'_>) -> Event` | append-only | `grimoire-core::combat::Ledger` |
| events to report | `(&[Event], &Options) -> Report` | no, a pure fold | `grimoire-core::combat` |
| bytes to lines to events | `Cursor { offset, remainder }` | **yes, and only here** | `grimoire-parse::cursor` + `grimoire-agent` |

```
crates/grimoire-core/src/combat/
    mod.rs      Event, Side, ActorId, Stamp, Names (interner)
    ledger.rs   append-only Vec<Event> + per-second beats
    fight.rs    segmentation, encounter grouping, taint detection
    claim.rs    pet/charm claim intervals and boundaries
    rate.rs     Rate { num, den, basis } and every derived metric
    report.rs   Report + Options, the pure fold
crates/grimoire-parse/src/combat/
    mod.rs      Outcome, the anchored dispatcher
    grammar.rs  the recognisers
    tables.rs   generated const tables (verbs, participles, flags)
    flags.rs    longest-match flag tokeniser
crates/grimoire-parse/src/cursor.rs
```

`grimoire-parse` already depends on `grimoire-core` and already produces core domain types (`line.rs` produces `Skill` through `Skill::from_log`), so this preserves the existing purity gradient unchanged: core is I/O-free, parse is `&str` in and values out, neither touches a file.

The biggest payoff is not tidiness, it is testability. **`report()` is testable with a hand-built `Vec<Event>` and no log text at all**, which is what separates "the grammar is wrong" from "the aggregation is wrong". A design that fuses parse, claims, fights and analysis into one pass over raw text structurally cannot do this.

**The interner is mandatory and cheap.** `Event` carries `ActorId(u32)`, not `&'a str`, because the ledger outlives the buffer it was parsed from. Measured: **1,694 distinct names across 199.9 MB**, so a `Vec<Box<str>>` plus a `HashMap` is a few hundred KB; the ledger at 12 bytes per event is ~11.6 MB for 969,511 events. Keeping the borrowed step separate preserves what makes `line.rs`'s tests good: fixtures are plain string literals with no setup.

### 8.2 The stateful-parser-behind-a-pure-op problem

`dispatch.rs:7-8` requires ops be pure functions of their arguments: no handle, no session, no registry. A combat parser needs forward lookahead and per-actor accumulation, both of which look like state.

**Rejected: a stateful handle.** It needs a registry, a lifetime contract across the C ABI, a leak story when a page navigates away, and it makes every op's result depend on hidden state, destroying the golden-file and idempotence properties in §9.4. It buys avoiding a re-parse that measures **8 ms on an 8 MB window**. Write the rejection into the doc comment so nobody re-litigates it.

**Rejected: caller-carried fold state.** Pure, but the state *is* the ledger, so serialising it per tick costs strictly more than re-parsing the text it came from.

### 8.3 The resolution: batch-with-window, made exact

A windowed batch op is cheap, but cost is the weaker case. The strong case is that a bounded window is **provably equivalent to a whole-file parse**, which is what makes the live meter and the report unable to disagree.

Every lookahead a combat parser needs is bounded: kill-reward clustering ~2 s, charm grant ~12 s, encounter lull ~20 s, fight idle ~45 s, same-name taint ~10 s. So:

```rust
pub const FINALITY_SECS: u32 = 60;
```

> **Finality theorem.** Every fact about second `t` is final once a line stamped `t + FINALITY_SECS` has been seen. A window whose first line is at least `FINALITY_SECS` before the first second reported produces byte-identical output to a whole-file parse over that range.

The theorem holds only if the **whole-window inferences are eliminated** (H17). Three replacements, each window-independent:

| Naive rule | Replacement |
|---|---|
| Sentence-case fold gated on seeing the lowercase form in the window | A leading article is always lowered. No evidence gate |
| `mobSet` membership gated on a kill in the window | Positive evidence only, plus an explicit `Unclassified` bucket the report prints |
| Proc-vs-cast gated on a cast line in the window | Key on the rank-stripped base (F4); mark `Unknown` rather than `Proc` when the window head is within `FINALITY_SECS` of the event |

With those fixed, the window is exact, and `tail()` finally gets its first caller.

**The op:**

```json
{"op":"combat","window":"<text>","view":"report"|"meter"|"events"|"coverage",
 "opts":{"basis":"active","gap_secs":30,"idle_secs":45,"lull_secs":20,
         "owner":"Reviir","include":["ds","proc"],"since":null}}
```

- `window` is **capped at 8 MB**; over that, return an error rather than trying. The caller does the tailing via `tail(log, 8<<20)`.
- `view:"meter"` returns a small pre-resolved payload, because the overlay must not pay to serialise a full report at 1 Hz. The overlay holds no engine.
- `view:"coverage"` returns only the drift numbers. Cheapest call, and the one the UI polls to decide whether to show a banner.
- `opts` is **echoed back inside the report, verbatim.** A stored or shared parse that does not carry its options is not reproducible, and EQLogParser users on the same log already get different totals because inclusion is user-configured.

### 8.4 Grammar growth

**`Line` does not grow, it splits, and it stops returning `Option`.** Adding forty variants to the six-variant enum is wrong twice: `combines.rs:133`'s `_ => continue` would swallow every one silently, and every consumer becomes non-exhaustive. `Line::Entered` and `Line::RecipeSearch` are already inert variants nothing reads, which is the shape of that failure.

```rust
pub enum Outcome<'a> {
    Craft(line::Line<'a>),      // unchanged
    Combat(combat::Raw<'a>),
    Session(session::Raw<'a>),  // zone, who, level, login, stance, invocation
    Ignored(Ignore),            // POSITIVE match on a named rule
    Unknown,
}
```

Three hard rules: never `Option`; `Ignored` is never a fallthrough; **every consumer matches exhaustively** (delete `_ => continue`, so adding a variant is a compile error at every consumer, which puts the reachability gate in the type system rather than in someone's memory).

Fix `Line::Entered` in the same change. It is currently **unreachable**: the `ENTERED` constant begins with `Y` and the check sits inside `if !body.starts_with('Y')` (`line.rs:71-83`, confirmed by a scratch test returning `None`). Zone is the session boundary the whole combat model hangs off, and it does not work today. Guard the known false positive (`You have entered an area where levitation does not function.`) and use `strip_suffix('.')` for consistency with `trim_sentence`.

**Dispatch is anchored, not a pattern list, and never regex.** The crafting parser rejects on one byte because 99% of lines are not crafting. Combat inverts that: **68.5% of lines match a combat-shaped anchor**, and there is no discriminating first byte because the actor name leads the sentence. Measured on 199.9 MB:

| Strategy | Time | Throughput |
|---|---|---|
| stamp split only | 52.2 ms | |
| crafting first-byte gate | 60.2 ms | |
| **anchored dispatch** | **153.7 ms** | **1.30 GB/s** |
| naive 24-pattern `contains` scan | 272.8 ms | 0.73 GB/s |

The shape: split stamp, reject on last byte not `.` or `!`, one `find(" for ")`, digit run, classify on the following token (`" point"` / `" hit points"` / `" damage"`), else `" has taken "`, else `", but "`, else prefix families. One scan buys the family; the family narrows to a handful of byte comparisons. **Do not build an ordered-regex rule list.** At the measured naive cost it does not fit §8.6's budget, and load-bearing rule ordering is a fragility the anchored shape does not have.

**Closed enumerations are generated `const` tables, not runtime data files.** `tables.rs` is emitted by `grimoire census --emit-tables` from the checked-in census, with a test asserting regeneration reproduces the committed table byte for byte. That gets data-drivenness (derived, regenerable, diffable in review) without a loader. A table you must remember to load is a table that ships unloaded. A `const` cannot be.

**Flags get a longest-match tokeniser** over the vocabulary sorted by descending length, single space between matches, and **any leftover token is an `UnknownFlag` that is counted and named in the coverage report**. That gives the flag vocabulary its own drift alarm for free, which matters because flags are exactly what a patch adds to.

**Timestamps** parse at fixed byte offsets (measured free) into `Stamp { secs: i64, ord: u32 }`. Keep the raw `&str` for display only. No date library: the workspace has two dependencies and this needs neither.

**Encoding:** read bytes, split on `b'\n'`, strip trailing `b'\r'`, decode **per line** with `from_utf8_lossy`, count the lines that needed it. One rule, both doors, one test.

### 8.5 The native door

```
grimoire-agent
   Cursor { offset: u64, remainder: Vec<u8> }   <- grimoire-parse, pure, tested
   -> lines (lossy per-line decode, counted)
   -> grimoire_parse::combat::parse             <- pure
   -> Ledger::push                              <- append-only
   -> report(&ledger.events[range], &opts)      <- pure
```

The remainder must be `Vec<u8>`, not `String`, and a read returns only the bytes it read: a padded short read injects NULs into the stream and moves the cursor past bytes never read.

### 8.6 Performance target

> **A whole-file combat parse of a 200 MB EQL log completes in under 400 ms on the native path, single threaded, cold parser state, warm page cache, while producing the coverage numbers. CI gate at 600 ms.**
>
> **Live path: an 8 MB window costs 35 ms or less end to end**, so a 1 Hz tick is ~3% of one core.

Calibration: `grimoire combines` on the same 199.9 MB file measures **178.9 / 172.2 / 173.1 ms** (~1.15 GB/s, 13.6 M lines/s; `README.md:96`'s "half a second" is conservative by 3x). A realistic combat pass doing stamp split, structural reject, anchor scan, integer parse, actor/target extraction, interning, per-actor accumulation and ledger append measures **208 ms** (960 MB/s, 969,511 events). So 400 ms has headroom for the real grammar, and 600 ms catches a 3x regression without flaking.

Five constraints follow, all measured:

1. **No regex, no ordered pattern list.** 1.8x before any regex engine exists (§8.4).
2. **No per-line allocation.** ~1 M events, two owned `String`s each would dominate the cost. Intern.
3. **Split the release profile.** `opt-level = "z"` costs **+41% on the anchored path and +86% on the stamp split**, measured on identical source. Correct for the wasm artifact, wrong for the CLI. Add `[profile.release.package.grimoire-forge] opt-level = 3`. This is a live defect today, independent of combat.
4. **The whole-log op is not viable; the window op is.** Marshalling the 199.9 MB log through a JSON op costs **470.5 ms** (328.4 encode + 142.1 decode) on a 204,623,487-byte payload, native Rust to Rust, before one line is parsed. The browser path is strictly worse: ~800 MB across both heaps. This is the same class of defect §2.4 already calls critical for the corpus. 8 MB costs 20.0 ms; 1 MB costs 2.3 ms. (Note `Request::Harvest { log: String }` has the identical problem today for a whole-log crafting harvest; fixing combat this way fixes the pattern.)
5. **Stream on the native path.** Peak resident drops from 200 MB of text plus 11.6 MB of ledger to ~12 MB plus a 64 KB buffer. `grimoire-forge/src/main.rs:106-114` currently reads and decodes the whole file, and its whole-file Latin-1 fallback is a correctness bug as well as a memory one.

**Do not parallelise.** The grammar is stateless per line and the log is byte-range splittable with `tail()`'s resync rule, so it parallelises trivially. Do not. 208 ms single threaded is inside budget, the fold is order dependent, and multi-threading would compromise §9.4's determinism guarantees for no user-visible gain. Write this down with the measurement so it is not re-litigated.

---

## 9. The validation harness

The discipline to mirror is `grimoire-core/tests/calibration.rs` plus `pinned_combines.csv`: a real-data fixture in `tests/`, `include_str!`d, pinned against a model with a stated tolerance and a likelihood floor, with a CLI line that prints "MODEL HAS DRIFTED" past a threshold. Combat gets the same structure, adapted honestly.

**Precondition, and it is met.** `eql-grimoire` is under git and carries workflows in `.github`, so every mitigation in §7 is a regression test that a bisect can locate and every number that moves has a commit behind it. That is what makes the rest of this section worth writing: a regression test without version control is a one-shot assertion, and the day one stopped being green would be unfindable.

`proptest` and `insta` enter as **dev-dependencies only**, which do not reach the wasm or release build and are compatible with the two-dependency posture.

### 9.1 Layout

```
crates/grimoire-parse/tests/
    grammar.rs                     coverage, census, adversarial, mutation
    fixtures/
        eql_session.log            real, scrubbed, checked in
        eql_session.census.tsv     shape inventory, checked in, diffed
        hostile_chat.log           adversarial, hand-written
        synthetic_*.log            generated, with truth files
crates/grimoire-core/tests/
    combat_report.rs               invariants, golden, drift model
    fixtures/
        eql_session.events.jsonl   the fixture's events, checked in
        eql_session.report.json    the golden report, checked in
        synthetic_*.truth.json     computable ground truth
```

### 9.2 Four fixture formats, because they test four different things

| Format | Tests |
|---|---|
| `*.log` | the grammar, and nothing else |
| `*.events.jsonl` | **the separator between a grammar bug and an aggregation bug.** A report test consuming this never touches a matcher |
| `*.report.json` | the golden. Keys sorted; rows with an explicit total order including a name tiebreaker (`dispatch.rs:158-161` already does this for `hands`); **every rate as `{"num":N,"den":D,"basis":"active"}`, never a float**, which eliminates float-diff noise and structurally prevents NaN and infinity; `opts` echoed in |
| `*.census.tsv` | normalised shape inventory with counts, regenerated in CI and diffed |

### 9.3 Synthetic ground truth

`grimoire simulate --scenario s.json --out log.txt --truth truth.json`. Because the log is emitted from a known model, the expected report is **arithmetic, not a snapshot**.

The trap to design around: a generator emitting sentences from the parser's rule list is a round trip that tests nothing. So the templates are derived from the checked-in census, and `grammar.rs` asserts every template the generator can emit appears in `eql_session.census.tsv`. If the game changes a sentence, the census changes, the templates go stale, and the test says so.

Scenarios that must exist because no captured log reliably contains them:

| Scenario | Pins |
|---|---|
| two same-named mobs alive simultaneously | taint fires; excluded from HP and TTK, included in damage |
| a charmed pet changing sides mid-fight | boundary inclusivity (§5.4) |
| a DoT ticking after its target dies | ticks land in the dead mob's fight, not the next same-named one |
| a fight entirely inside one logged second | refusal, not a divide by zero |
| a heal with no named healer | never counted as the owner's |
| a damage shield firing on a mob you are not fighting | does not open a fight |
| a stamp going backwards by 3600 s | counted, clamped, no negative duration |
| a window starting mid-fight | boundary fights flagged, excluded from rate aggregates |
| a raid second with 60 lines | intra-second order stable across re-parse |
| a `/who` mid-session changing a roster entry | roster snapshots stamped and decayed |

### 9.4 Invariants

Asserted on every fixture and as `proptest` properties.

| Invariant | Why |
|---|---|
| `lines_seen == recognised + ignored + unknown` | the coverage denominator is real |
| `total == sum(source rows)` | attribution lost or duplicated nothing |
| `total == sum(per-fight own-side) + orphan`, and **orphan is reported** | orphaned events have no other symptom. Compute the two sides by independent paths or it is a tautology |
| each damage event contributes to exactly one `(source, target)` and at most one fight | catches zero-damage events routed twice or not at all |
| `duration = end - start >= 0`, refuse at 0 | fencepost and divide-by-zero |
| `hp_lo <= hp_hi`; an empty intersection is a **finding** | the bracket discipline `combines.rs:51-62` already states |
| no rate emitted as a bare float | structural, from §9.2 |
| `serialize(report) == serialize(report)` across two process starts | `HashMap` order is randomised per process |
| `parse(tail(log, n))` is a suffix of `parse(log)` for every `n` | generalises `lib.rs:98-111` |
| chunked feed produces an identical event stream to a whole feed, including chunks splitting a line, a CRLF and a multibyte char | the overlay and the report cannot disagree |
| a window starting `FINALITY_SECS` early reports identically to a whole-file parse over the same range | §8.3's theorem, as a test |
| never panics on arbitrary bytes, truncation at every offset, 40-digit integers, lone surrogates | `panic = "abort"` is set, so a panic in a browser tab kills the module for the session |
| no actor name is a pronoun | §5.6 |

One correction to inherit: `lib.rs:75-80` asserts every surviving line of a tail parses successfully, which is only satisfiable because that fixture contains no chat and cannot survive a real log. Split it into "the slice is empty or begins with a fully-stamped line" and "`split_stamp` succeeds on the first line", and assert nothing about later lines.

### 9.5 The unparsed-line alarm and its threshold

**The signal.** A stamped line, matched by no rule and by no named ignore rule, carrying an ASCII digit run of length at least 1 immediately preceded by a space. Deliberately **broader than every combat rule and independent of every damage phrase**, so it catches `dealing 47 damage`, `You hurt yourself for 1 points.`, and `tormented … for 9 points of non-melee damage`. Measured denominator: ~40.7% of lines carry a quantity, so the signal is not swamped by chat.

A tripwire keyed on the parser's own damage phrases fails on both counts. A phrase pattern that allows one word between `points of` and `damage`, with a word class that excludes the hyphen, **cannot match `points of non-melee damage`**, so the entire 202,317-line non-melee family would be invisible to it by construction.

**Three numbers, not one.**

```
unknown_share  = unknown_lines / stamped_lines
damage_at_risk = sum over unknown quantified lines of the largest integer on the line
at_risk_share  = damage_at_risk / (attributed_damage + damage_at_risk)
```

`at_risk_share` is the one that escalates. A line count weighs an unknown line carrying a large melee hit the same as one carrying a one-point tick, so a line-count banner can read small while most of the damage is gone.

**Thresholds.**

| Condition | Action |
|---|---|
| `at_risk_share <= 0.5%` | publish normally |
| `0.5% < at_risk_share <= 5%` | publish with a banner naming the top unknown shapes and the estimated damage at risk |
| `at_risk_share > 5%` | **refuse to publish damage totals and DPS.** Publish coverage only |
| any quantified shape absent from the checked-in census | **CI failure regardless of share.** The first sighting of a new format is the event; its volume is not |
| `at_risk_share > 0` on the checked-in real fixture | **CI failure.** Every quantified line in the fixture must be recognised or covered by a named ignore rule |

**Normalisation must fold entity names as well as digits**, or the census is unreviewable: a digits-only fold gives **93,028 distinct shapes** on one 753,988-line log versus 722 on a 4,392-line fixture. Fold digits to `N`, article-led lowercase runs to `<mob>`, a bare capitalised word in an actor slot to `<name>`, the parenthetical to `(FLAGS)`. The fold rules get their own tests: under-folding is noise, over-folding hides a new format inside an old shape.

**Mutation-test the alarm itself.** `tests/drift.rs` applies a catalogue to the golden log and asserts the alarm fires at a named minimum severity on each. A drift detector nothing exercises is a light bulb nobody has switched on.

| # | Mutation | Observed drift class |
|---|---|---|
| 1 | reword the melee sentence (`for N points of damage.` to `dealing N damage.`) | patch reword |
| 2 | introduce an unknown melee verb | new ability |
| 3 | introduce an unknown DS participle | **the `tormented` case, which actually happened** |
| 4 | introduce an unknown token inside the flag parenthetical | new modifier |
| 5 | change the terminator (`.` to `!`) | the incoming-DS perspective case |
| 6 | insert a clause before the amount | |
| 7 | change the stamp to 23 or 25 bytes | |
| 8 | introduce a new DoT tick form | |
| 9 | **negative case:** inject a benign non-quantified chat line | the alarm must **not** move. An alarm that cries wolf gets muted, and a muted alarm is worse than none |

### 9.6 Model pinning, and the differential oracle

`calibration.rs` pins a formula with **no free parameters** against 343 real attempts, band tolerance 0.12, log-likelihood floor -215.0. Combat has no published formula, so it gets weaker but genuinely external models. **Say that in the test names rather than implying parity.**

| Pin | Model | A break means |
|---|---|---|
| Swing cadence | Inter-swing intervals for one `(actor, verb)` inside one engaged stretch are near-constant. Pin the coefficient of variation for the fixture's highest-volume actor | Fights are mis-split, or swings are double counted. A real model, not self-consistency |
| Flag rate stability | Crit per swing is Bernoulli. Pin the observed rate inside a binomial interval | The flag tokeniser regressed (someone reintroduced a capital-letter split and `(Riposte Critical)` stopped counting) |
| HP bracket closure | Brackets across kills of one name should intersect. Pin the closure rate | Attribution broke: brackets stop intersecting **before** totals visibly move |

`grimoire combat` ends with `model holds` or `MODEL HAS DRIFTED, re-check before trusting a parse`, matching the crafting path verbatim in idiom.

**The oracle.** `github.com/DranakCorps-bot/EQBuddy` is **MIT**, C#, **EverQuest Legends specific**, runs 1,152 tests of which 1,107 are on the parser and stats, and ships `tests/fixtures/eqlog_Testchar_fixture.txt`: 301,318 bytes, 4,392 lines, a real EQL session (Befallen, West Commonlands) containing melee, misses, DoT ticks, damage shields, lifetap self-heals, kills, loot, coin, faction, XP, cons, and same-name mob-on-mob combat. MIT to MIT with attribution, so it can be checked in with its copyright notice and the visible credit its README asks for. It solves the hardest logistical problem in the plan (a shareable real log containing no third-party names to scrub) on day one, and it runs on every commit.

Its limits, stated plainly: one character, one class, one level band, three zones. No raid, no charm, no pet swap, no bard, no group split, no resist-heavy caster. A suite green on this file proves the grammar reads **this play**, not EQL, and the rule-coverage gate (H28) is what makes that gap visible rather than assumed.

**Differential rules.** Compare **only conserved quantities**: total damage by source, kill counts, per-mob kills, coin in copper, XP tick counts, loot counts. **Exclude every rate**, because the denominator divergence is documented (rumstil divides by fight duration, GamParse by player active duration) and definitional differences generate permanent false alarms that train the team to ignore the harness. Differences are a **triage list, not an auto-fail**: each resolves to grimoire bug, oracle bug, or a written-down definitional difference. And two parsers agreeing proves consistency, not correctness: neither derives from a published spec, and both could be missing `tormented`. **The census is the only check that asks what the game is writing that nobody has seen.**

### 9.7 Privacy, enforced by test

Two enforcement points, mirroring `Harvest::calibration_buckets()` (`combines.rs:83-85`) and the test that renders it and asserts no item name appears:

1. `CombatBuckets` carries bucketed integers only (class, level band, percentile, no names), is the **only** combat type with a `Serialize` path reachable from an upload op, and a test renders it and asserts no name appears.
2. A scrub test walks every checked-in log fixture, extracts every capitalised single-word actor name, and fails on anything outside an allow-list. The scrub must **preserve shape class** (a backticked name stays backticked, an article mob stays an article mob), and a scrubbed fixture is re-run through the invariant suite to prove the scrub did not change the numbers.

---

## 10. Phasing

Two orderings, and they are not the same.

### 10.1 Ship order

Each phase is usable on its own.

| Phase | Work | Exit |
|---|---|---|
| **C0. The gate** (half a day, no combat code) | `git init` + CI on `cargo test --workspace`. Fix `Line::Entered`. Replace `Option` with `Outcome` in the existing crafting grammar and add `unknown`/`ignored` counters to `Harvest`. Delete `_ => continue`. Split the `opt-level` profile. Ship `grimoire census` and **check in the census of James's corpus** | A red/green gate exists, and there is a written inventory of what the game actually writes. **The census is the first artifact, not the parser.** You cannot write a grammar for a format you have not inventoried |
| **C1. Grammar and coverage** (no analytics) | `grimoire_parse::combat` + `grimoire combat --coverage` | One number to a user: "I understand 99.x% of the quantified lines in your log, and here are the top shapes I do not." Useful on day one as a bug report anyone can run, and it gates everything after |
| **C2. Ledger and one honest number** | `core::combat` ledger, fights, beats, per-source table, DPS with the basis in the field name. One wasm op | The first thing a player calls a parser. Goldens and conservation invariants land here |
| **C3. Attribution** | §5 in full, plus the printed unattributed bucket | Nothing before C3 is trustworthy for a pet or charm class |
| **C4. Entity resolution** | Taint detection propagated to **every** per-fight number | Detecting the condition is not enough; a flag that reaches one consumer leaves every other per-fight number blended |
| **C5. Incoming and healing** | Damage taken by ability and source, avoidance rates (now computable), HPS, overheal (now confirmed), death recap | |
| **C6. Live window and overlay** | `view:"meter"`, the `Cursor`, `grimoire-agent`, chunk-invariance | |
| **C7. Comparative surfaces** | Stance/invocation A/B with gates, resist tables, HP brackets with clustering | |

### 10.2 Accuracy order

The ordering to reach for when there is a choice. It differs from the ship order.

1. **The coverage meter and the census.** Converts unknown-unknowns into a number. Everything else is guessing at an error bar this measures.
2. **The flag tokeniser and rank-stripped spell identity.** Two small rules that each recover a whole category. The flag split is measurably wrong on 1,462 lines of composed flags; the rank mismatch fires on the player's own DoT ticks.
3. **Per-actor active-time denominators with the basis in the field name.** A shared denominator is the most visible defect a parse can have, and the fix is arithmetic.
4. **Taint propagation.** One boolean already computed, plumbed to every consumer.
5. **Attribution.** Largest gain, most work.
6. **Presence versus engagement windows.** The same engagement rule for the mob's output and for yours.

---

## 11. Open questions

Ordered by how much they move the plan. Each is a place where guessing produces a confidently wrong number, which is what this document exists to prevent.

**11.1 Does EQL emit the crit-family flags in content beyond what Reviir has played?**
Settled in the affirmative for the corpus (§2.3), which removes the largest flagged uncertainty in the surveys. What is *not* settled: the census covers one character, mostly classic zones, roughly three months. `gore` and `slam` do not occur here but may exist elsewhere; other flags may. **This is exactly why the coverage meter ships in C1 rather than being backfilled.** No action needed beyond shipping it.

**11.2 Why does the spell rank appear inconsistently, and on which line forms?**
Two measurements that are not obviously reconcilable: direct spell-damage lines (`by <Spell>`) carry a rank on **exactly one line** in the corpus, while DoT tick lines (`from your <Spell>`) carry it on 1,209 occurrences of `Venom of the Snake I` against 237 without, and `Curse I` 217 against `Curse` 158, same caster, same file. The likely reading is that the two line families behave differently and that the tick form changed mid-log (a client change, or two ranks memorised). The rule (key on base, carry rank) is correct either way, but if the reason matters, it needs a timestamp-ordered look at when each form appears. **Do not build rank-dependent logic until this is understood.**

**11.3 Is the heal parenthetical really absent only when overheal is zero?**
Inferred from 2,000 samples with zero equal cases plus 6,991 freeport cases with zero equal cases. Strong, not proven. If a zero-overheal parenthetical exists anywhere, "absent means overheal 0" is wrong and overheal is computable only on the ~21% of lines that carry it. **One targeted grep before the overheal metric ships.**

**11.4 Does self-damage belong in a damage total?**
`You hurt yourself for N points.` occurs 5,855 times corpus-wide with no rule at all. It is unambiguously real HP loss, but whether it enters DPS is a product decision, not a parsing one. **It must have an explicit rule either way, and the decision goes in the test name.**

**11.5 What are `fight_idle` and `pull_lull` actually worth?**
Shipping tools disagree (30/60/120, 30), and none publishes a measurement behind its constants. Grimoire should **derive its own from the census corpus** and print them in the report header. Until then they are chosen bounds and the spec should say so.

**11.6 Is `FINALITY_SECS = 60` safe?**
Derived from the five lookahead windows above, four of which are themselves chosen constants. The bound is sound given those rules and unsound if a new rule needs longer lookahead. **Make it a named constant and assert in a test that no rule's window exceeds it.**

**11.7 Are EQL timestamps ever meaningfully non-monotonic?**
Measured: one inversion in 753,988 lines, worst case 2 seconds. DST has not been observed in this corpus at all. **Scan the full 3.67 M-line corpus for inversions and for a repeated hour before carrying any monotonicity assumption into Rust.**

**11.8 Does EverQuest Legends have mercenaries?**
No evidence either way in the corpus. Do not build for a system that may not exist.

**11.9 What is the file encoding?**
No authoritative statement exists for any EQ client. The safe engineering answer (read bytes, decode lossily per line) does not depend on knowing, but if the file is Windows-1252 rather than UTF-8, non-ASCII names need an explicit CP1252 decode to round-trip.

**11.10 Is the residue really unknown damage, or partly rules that emit no event?**
A recognition figure that counts only "emitted an event" conflates "matched no rule" with "matched a rule that emits nothing". The coverage meter has to keep those apart (F1's `Ignored` against `Unknown`), or the unknown-damage figure cannot be bounded above without classifying every residue shape by hand. C0's census closes this.

**11.11 The EQBuddy attribution wording.**
The licence is MIT and the README asks for a visible "based on EQBuddy" credit naming what was taken. Straightforward, but it is James's call, and it should be confirmed from the repo rather than from this document.

**11.12 Does `PLAN.md:181-182` get amended, or does this stand as a deliberate exception?**
`README.md`'s opening still describes the four-piece scope. Combat parsing is now the second do-not-build item to be built. **Decide whether the list is amended or whether the exceptions are enumerated,** and fix the front door either way.

---

## Appendix. Measurements this spec rests on

Taken 2026-08-29 against `C:/Users/Public/Daybreak Game Company/Installed Games/EverQuest Legends/Logs/` and a release build of `grimoire-forge`.

| | |
|---|---|
| Corpus | `eqlog_Reviir_freeport.txt` 753,988 lines / 59 MB; `_neriak` 395,117; `_qeynos` 164,802; `_qeynos 1` 2,359,160 / 199,903,528 bytes. **3,673,067 lines / ~308 MB** |
| Stamp | 24 bytes bracketed, on 2,359,160 of 2,359,160 lines. Day always zero-padded. Corpus spans Jul and Aug |
| Combat share | 220,250 damage + 159,427 miss = 50.3% of freeport |
| Quantified lines | ~40.7% of freeport carry a space-delimited digit run |
| Modifier flags | 10-member vocabulary, 28 observed compositions, 5 multi-word. Critical 41,787 down to Wild Rampage 3. Deadly Strike / Twincast / Lucky / Assassinate / Headshot: **0** |
| Elements | 9: non-melee 202,317 … chromatic 7 |
| Verbs | 20, slash 234,713 down to smash 179, then a cliff to 18 |
| DS participles | pierced 59,889, burned 24,043, tormented 1,811 |
| Heals | freeport 6,991 with parenthetical (0 inversions, 0 equal), 19,588 without. 190 MB log: 30,110 of 143,249 |
| Damage lines with a potential parenthetical | **0** |
| Cast lines with a Roman rank | 2,243 of 4,556 (49.2%) |
| DoT shapes | yours 1,243, named 16,011, **anonymous 953** |
| Pets | Master tells 1,878; casterless `has been charmed.` 420; `<name> pet` swings 6,030; **`My leader is` 0** |
| Deaths | 2,142 / 3,916 / 84 / 26 |
| Timestamp inversions | 1 in 753,988, worst 2 s |
| Unmodelled | `You hurt yourself` 4,247 freeport / 5,855 corpus; `hit by non-melee` 106; `Insufficient Mana` 273 |
| Fixture fight lengths | 33 of 119 zero-length, 45 at or under 3 s, median 5 s |
| Fixture same-name collisions | 5 of 20 name-groups needed clustering; 25 of 119 fights tainted; 96 events with source == target (5.6% of points) |
| Duration error | triangular on (-1,+1), sigma 0.408 s, 95% bound 0.776 s |
| `grimoire combines` on 199.9 MB | 178.9 / 172.2 / 173.1 ms (~1.15 GB/s) |
| Combat bench on 199.9 MB | 208 ms, 960 MB/s, 969,511 events, **1,694 distinct names** |
| Dispatch | stamp 52.2 ms; first-byte 60.2; **anchored 153.7 (1.30 GB/s); naive 24-pattern 272.8 (0.73 GB/s)** |
| `opt-level="z"` penalty | stamp 96.9 vs 52.2 (+86%); anchored 216.4 vs 153.7 (+41%) |
| JSON marshalling | whole log 204,623,487 bytes, 328.4 + 142.1 = **470.5 ms**; 8 MB = 20.0 ms; 1 MB = 2.3 ms |
| Ledger | 12 bytes/event, ~11.6 MB for 969,511 events |
| grimoire tests | **1,479 passing** across 32 targets (3 ignored, 0 failing), from `cargo test --workspace` |
| grimoire version control | git, with workflows under `.github` |
| `Line::Entered` | **unreachable**, confirmed by a scratch test returning `None` |
| `tail()` callers | **0** |
| EQBuddy fixture | 301,318 bytes / 4,392 lines, real EQL, **MIT** |