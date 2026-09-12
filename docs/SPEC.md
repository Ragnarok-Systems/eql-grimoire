# EQL Tradeskill Broker — Specification v1.0

A Discord application that brokers tradeskill work orders between players and crafters in
**EverQuest Legends**.

Status: **draft for review.** No code written yet.
Last updated: 2026-08-09

---

## 1. Constraints and architecture

| Constraint | Consequence |
|---|---|
| **No self-hosting.** Owner will not run a box. | Cloudflare's free tier. Serverless, no machine to babysit. |
| **Zero cost.** | Workers + Pages + Durable Objects free tiers. No paid Discord features. |
| **Rust** for the engine. | Worker compiles to WASM. UI is TypeScript (SDK requirement). |
| **Akashic**, not SQL, as the data source. | Akashic can't run serverless (below) — it stays offline and *exports*. |
| **Proper app UI**, not slash-command soup. | Discord Activity — a real web app in an iframe inside Discord. |
| Low volume, guild scale. | Everything above fits inside free-tier limits with room to spare. |

### 1.1 Why Akashic can't be the live store

Akashic is a 57-crate Rust workspace that runs embedded, as a server, or distributed, with a
BLAKE3-chained audit journal as a first-class feature. It is a good fit for the Codex and a bad
fit for a Worker: it is **file-backed** (`--data-dir`), **thread-parallel** (`rayon`, `crossbeam`),
and the tier-2 path uses **io_uring**. There is no `wasm32` target in the workspace outside the
Python bindings. Cloudflare Workers have no filesystem and no threads.

That's fine, because the workload splits cleanly along the same seam.

### 1.2 The split

```
   ┌─ YOUR MACHINE (offline, no uptime requirement) ──────────────┐
   │  _EQL_DataExport Codex                                       │
   │    peq_full.sqlite  +  eqlwiki snapshot  +  delta_recon       │
   │            │                                                  │
   │        Akashic  ── compress, index, BLAKE3 audit chain        │
   │            │                                                  │
   │            └──► catalogue.bin  (read-only, versioned export)  │
   └────────────────────────┬──────────────────────────────────────┘
                            │  publish on rebuild
   ┌────────────────────────▼──────────────────────────────────────┐
   │  CLOUDFLARE (free tier, zero maintenance)                     │
   │                                                               │
   │   R2 ─────────► catalogue.bin        (10 GB free)             │
   │   Workers ────► Rust/WASM: interactions, pricing, routing     │
   │   Durable Obj ► mutable state: orders, profiles, reputation   │
   │   Pages ──────► the Activity UI (TypeScript)                  │
   └───────────────────────────────────────────────────────────────┘
```

**Why this seam:** the catalogue is large and read-only, changing only when the Codex is rebuilt.
The live state — crafter profiles, open orders, reputation — is a few thousand small records at
guild volume. Akashic's strengths (compression, vector search, audit chaining) serve the first.
A Durable Object serves the second.

### 1.3 Free-tier budget

| Service | Free limit | Our expected use |
|---|---|---|
| Workers requests | 100,000 / day | a busy guild might see hundreds |
| Workers CPU | **10 ms / invocation** | ⚠️ see §6.4 — the one real risk |
| Durable Objects | 100,000 req/day, SQLite backend | KV-style API, no SQL written |
| R2 | 10 GB, 1M Class A ops | catalogue is single-digit MB |
| Pages | unlimited static requests | the Activity |

**No gateway connection is needed.** Every interaction in this design — slash commands, buttons,
modals, autocomplete, thread creation — is request/response over HTTPS. That is what makes
serverless viable at all here, and it's why Workers' ~0 ms cold start matters: Discord enforces a
**3-second** response deadline, and a sleeping container would fail the first command after any
quiet period.

**Bonus from this choice:** we are not using `serenity`, so **Components V2 is available**. Sending
interaction JSON directly sidesteps the fact that V2 never shipped in a stable serenity release.

---

## 2. Actors

| Actor | Identity | Role |
|---|---|---|
| **Player** | Discord user + EQL character(s) | Places orders, supplies materials, confirms receipt |
| **Crafter** | Discord user + skills per tradeskill | Accepts orders, works a queue, delivers |
| **Broker** (the bot) | — | Prices, routes, holds state, records reputation |

One user can be both. **Server (shard) is a hard matching constraint** — you cannot trade across
servers. Guild is soft, used only for the discount.

---

## 3. Game mechanics

### 3.1 Base success chance — validated against 1,740 real combines

```
trivial >= 68 :  p_linear = MIN(skill - 0.75 * trivial + 51.5, 95)
trivial <= 67 :  p_linear = MIN(skill - trivial + 66,          95)
always        :  floored at 5
```

This is empirically confirmed, not assumed. `eqlog_<char>_<server>.txt` records every outcome by
name and every skill-up with its exact value, so skill at each attempt is reconstructable:

```
[Sat Aug 08 05:38:33 2026] You lacked the skills to fashion Greater Potion of Heat.
[Sat Aug 08 05:38:51 2026] You have fashioned the items together to create something new: Greater Potion of Heat.
[Sat Aug 08 02:12:23 2026] You have become better at Alchemy! (237)
```

Calibration over 912 clean Alchemy combines (skill 2→237) plus 600 controlled trials:

| predicted band | n | predicted | observed |
|---|---:|---:|---:|
| 20–30% | 38 | 25.1% | **23.7%** |
| 30–40% | 57 | 35.0% | **36.8%** |
| 50–60% | 140 | 55.6% | **52.1%** |
| 60–70% | 204 | 65.3% | **66.2%** |
| 70–80% | 367 | 74.8% | **74.7%** |
| 80–90% | 409 | 84.9% | **85.1%** |

Letting the constants float free gains 1.9 log-likelihood units on 1,740 observations —
statistically nothing. **The published constants are already at the optimum.**

### 3.2 The 95% cap is soft

600 controlled combines (100 each) at Alchemy skill 237, Mastery 0/3:

| recipe | trivial | observed | linear (cap 95) | hybrid |
|---|---:|---:|---:|---:|
| Blood of the Wolf | 37 | **100/100** | 95.0 ✗ | 99.9 ✓ |
| Philter of the Wolf I | 75 | **99/100** | 95.0 ✗ | 99.1 ✓ |
| Philter of the Wolf II | 100 | **98/100** | 95.0 ✗ | 97.9 ✓ |
| Mist of the Wolf | 111 | **98/100** | 95.0 ✗ | 97.1 ✓ |
| Philter of the Wolf III | 135 | **97/100** | 95.0 ✗ | 95.0 ~ |
| Mist of the Wolf I | 170 | **95/100** | 95.0 ✓ | 95.0 ✓ |

587/600 = **97.83%** against a predicted 95% — **z = +3.18**; `P(100/100 | p=0.95) = 0.59%`; and
the results fall monotonically with trivial, which a flat cap cannot produce.

```
p_eff = MAX( p_linear(T,S) , 1 - 0.28 * (T/S)^3 )
```

**+9.8 LL** over the hard cap on 1,512 clean combines.

**Confidence — this section has been wrong twice:**

| Claim | Confidence |
|---|---|
| Linear form governs the low/mid regime | **High** — 912 combines, well calibrated |
| The hard 95% cap is wrong | **High** — z = +3.18; 100/100 observed |
| The tail is specifically cubic at k = 0.28 | **Low** — 2 params fit to 6 points at one skill |

⚠️ The tail was fit entirely at skill 237. Applying `(T/S)³` at skill 30 is extrapolation — it
predicts 90.4% where the linear form says 75%. The `max()` wins on aggregate likelihood, but the
low-skill regime is thin. §3.4 fixes this as data accumulates.

### 3.3 Mastery AAs — UNVALIDATED, price as rank 0

Each tradeskill has a 3-rank Mastery AA that *"reduces the chance of failing &lt;skill&gt; recipes."*
Only Jewel Craft Mastery is documented in full: **10 / 25 / 50%**. If it multiplies the failure
side it is not clamped by any cap: `p = 1 − (1 − p_eff)(1 − reduction)`.

**Controlled before/after test — Heat Awareness potions, 100 combines each arm:**

| | successes |
|---|---:|
| Alchemy Mastery **0/3** | 83/100 |
| Alchemy Mastery **1/3** | 83/100 |

Zero measured difference. Owner's read: it's behaving like the warrior AAs and granting 1% of the
stated value instead of 10%. **Decision: treat the AA as non-functional. Price `aa_rank = 0`.**

⚠️ **But this test cannot actually detect the claimed effect, and that matters.** At 83% success,
rank 1 would shift the rate by only **+1.7 points** (83 → 84.7). The SD of the observed difference
at n=100 per arm is **5.3 successes**, so the result sits **0.32 SD** from the claimed effect —
indistinguishable from it. The test is consistent with "broken" *and* with "working as documented."

| test condition | n per arm for 80% power |
|---|---:|
| 83% success, rank 1 ← what was run | **7,352** |
| 50% success, rank 1 | 1,562 |
| 20% success, rank 1 | 443 |
| 83% success, rank 3 | 238 |
| 20% success, rank 3 | **20** |

**The fix is experimental design, not more grinding.** The AA multiplies *failure*, so its absolute
effect scales with how often you fail:

| base success | rank 1 shifts | rank 3 shifts |
|---:|---:|---:|
| 83% | +1.7 pts | +8.5 pts |
| 50% | +5.0 pts | +25.0 pts |
| 20% | +8.0 pts | **+40.0 pts** |

**Twenty combines per arm on a recipe you fail 80% of the time settles rank 3 outright.** Testing at
83% success was the most expensive possible place to look.

Also unresolved: Tinkering has no Mastery AA listed, and Spell Research appears in recipe data but
isn't one of the nine skills.

### 3.4 Self-calibration from logs — the killer feature

**Never ask users to type the in-game UI percentage.** At skill 237 the UI showed 100/99/98/97
where the roll was capped at 95 — its semantics are unknown and it must not feed pricing.

Ingest the log instead. One crafter's log yielded **1,740 labelled combines across a 217-point
skill range**, for zero user effort.

- Upload in the Activity → Worker extracts every combine, reconstructs skill per attempt, stores
  `(skill, aa_rank, trivial, outcome)`.
- Refit by maximum likelihood; alarm when observed calibration drifts from predicted.
- **This is how the AA ranks get solved**, and how the low-skill tail (§3.2) gets nailed down.
- It also detects live balance patches — if Daybreak retunes a trivial, calibration drifts within
  a few hundred combines instead of silently mispricing every order.

⚠️ **Privacy.** Logs contain tells, guild chat and group chat. Parse only the three line shapes
above, retain nothing else, and say so at the upload prompt.

### 3.5 What a failed combine costs — material disposition

| Disposition | PEQ signal | eqlwiki marker | Units for N successes, A attempts |
|---|---|---|---|
| `CONSUMED` | `failcount = 0` | (default) | `qty × A` |
| `RETURNED_ON_FAIL` | `failcount = componentcount` | `On Failure Returns:` | `qty × N` |
| `TOOL` | `comp > 0, succ > 0, fail > 0` | `(returned)` | `qty × 1` |
| `CONTAINER` | `iscontainer = 1` | `In [[Forge]]:` | never consumed |

1. **Destroyed is the default** — 75,934 of 88,946 component rows (85.4%) return nothing.
2. **Returns are all-or-nothing** — 13,010 of 13,012 return the full count. `failcount` is a boolean.
3. **The rule is learnable: tools survive, consumables don't.** Most-returned: `Hammer`, `Burin`,
   `Needle`, `Filleting Knife`. Most-destroyed: `Inlay`, `Pattern`, `Flask`, `Acid`, `Resin`.

```
RECIPE: Fish Bones (trivial 171, nofail=0)
  Filleting Knife    comp=1 succ=1 fail=1  -> TOOL, survives everything
  Roots              comp=1 succ=0 fail=0  -> DESTROYED on fail
  Prepared Fish      comp=0 succ=1 fail=0  -> PRODUCT
  Tackle Box         comp=0 succ=0 fail=0  -> CONTAINER
```

**2,740 of 22,775 recipes carry `nofail = 1`** — they cannot fail. `A = N`, risk premium zero,
skill irrelevant to price. Charging a skill markup on those would be an obvious tell.

⚠️ PEQ is classic EQ — a strong prior for the ~88% matching classic, no data for the 12% new.
**Recipe names collide** (three distinct "Fish Bones"); always key on `recipe_id`, never name.

---

## 4. Data: the Codex → catalogue pipeline

### 4.1 Sources (all already on disk)

`_EQL_DataExport/` is substantial prior work. Build on it; do not re-scrape.

| Source | What it gives | Caveat |
|---|---|---|
| `data/peq_full.sqlite` (285 MB) | 72 tables, 2.6M rows. `items` 117,944 **with `price` and `stacksize`**; `tradeskill_recipe` 22,775; `tradeskill_recipe_entries` 177,291 with the disposition fields; `merchantlist` 64,260 | **Classic EQ, not EQL** |
| `data/eqlwiki/*.jsonl` | 13,108-page snapshot: quests, npcs, quest_items, factions, zones | **No item pages** — recipes not included |
| eqlwiki item pages | `\|playercrafted =` blocks: skill, trivial, container, components, disposition markers | needs a pull; see §4.3 |
| `delta_recon/` | EQL↔PEQ divergence **already measured: 88% classic / 12% new** | — |

**PEQ is the backbone, eqlwiki is the EQL overlay.** Every catalogue row carries provenance
(`peq` / `eqlwiki` / `observed` / `manual`) and a confidence score. Where they disagree, eqlwiki
wins and the conflict is logged for review.

### 4.2 What PEQ already solves

Two gaps an earlier draft called blocking are simply not gaps:

- **Vendor prices** — `items.price` across 117,944 rows, plus 64,260 merchantlist entries.
- **Stackability** — `items.stacksize` / `stackable`.

Residual: of 9,364 distinct recipe components, 61% have a nonzero price but only **18% are actually
merchant-stocked**. So the buy-vs-farm split still needs judgement, and a `/price` community-report
path stays in the design as a *refinement*, not a foundation.

### 4.3 The eqlwiki item-page pull

Recipes live in the `|playercrafted =` parameter of `{{Itempage}}` on the **product's** page.
There are no recipe pages. Full-text search is dead (no CirrusSearch — `insource:` and `incategory:`
return zero) and `Category:Player Crafted` is **incomplete** (verified). So:

1. Enumerate namespace 0 via `list=allpages` (~11k item pages).
2. Batch wikitext 50 at a time: `generator=allpages&prop=revisions&rvprop=content&rvslots=main&redirects=1`
3. **Recipe signal = a non-empty `|playercrafted =` block.** Not the category.
4. Keep raw wikitext so re-parsing never needs a re-crawl.

Parser must tolerate real variants: `(Trivial: 46)`, `(Trivial : 41)`, `(Trivial:21)`,
`(Trivial: 115/17)`, `(Trivial: ?)`; skill aliases (`[[Skill Pottery|Pottery]]`, `[[Jewelry Making]]`
— resolve with `&redirects=1`); and junk (`** many other recipes`).

No robots.txt is published and **no licence is declared** — attribute eqlwiki in `/about`, send a
descriptive User-Agent, use `maxlag=5`.

### 4.4 The catalogue export

Akashic ingests both sources, reconciles, and emits **`catalogue.bin`** — a versioned, read-only
artifact uploaded to R2 on each rebuild.

Contents: items (id, name, price, stacksize), recipes (product, skill, trivial, container, yield,
`nofail`), components with quantity and disposition, the recursive subcombine graph, vendors with
zone and coordinates, drop sources, and a **prebuilt name index for autocomplete** (§6.4).

Properties that matter: immutable and content-addressed, so the Worker caches aggressively and a
rebuild is just a new key; small (single-digit MB compressed); and independently verifiable via
Akashic's BLAKE3 chain.

**Publish policy: everything ships.** The three-tier "vaulted" note in `DATA_STORE.md` is retired —
drop rates included. The farmable-materials list shows zone, mob, and rate, because a crafter
deciding whether to farm or buy needs to know if it's a 40% drop or a 2% one.

---

## 5. Data model

Two stores with different shapes, because they have different jobs.

### 5.1 Read-only catalogue (R2, from Akashic)

```
item          { id, name, price_cp, stacksize, stackable, provenance, confidence }
recipe        { id, product_item_id, skill, trivial, container_item_id,
                yield_qty, nofail, provenance }
component     { recipe_id, item_id, qty, disposition }   -- CONSUMED|RETURNED_ON_FAIL|TOOL|CONTAINER
vendor        { id, name, zone, x, y }
item_vendor   { item_id, vendor_id }
drop_source   { item_id, zone, mob, drop_rate }            -- rate included; drives farm-vs-buy
name_index    { trigram -> [item_id] }                    -- prebuilt, for the 3s autocomplete budget
```

### 5.2 Mutable state (Durable Object, KV-style — no SQL written)

One DO per guild. Natural isolation, and it caps blast radius.

```
crafter:{discord_id}          { character, server, guild, guild_discount_pct,
                                min_tip_pp, min_player_rep, accepting, tools[],
                                pricing_overrides }
crafter:{discord_id}:skills   { <skill>: { value, mastery_aa_rank, mastery_unlocked } }
player:{discord_id}           { character, server, guild }

order:{id}                    { player_id, crafter_id, server, status, thread_id,
                                mats_mode, eta_committed_at, eta_hours,
                                quote_pp, tip_pp, quote_breakdown, timestamps{} }
order:{id}:lines              [ { recipe_id, qty } ]
order:{id}:materials          [ { item_id, qty, disposition, fulfilment, unit_cp, charged_pp } ]
queue:{crafter_id}            [ order_id ]                -- ordered; position is derived

rep:{discord_id}:{role}       { follow_through, accuracy, calibration, speed,
                                communication, effective_n, tags{}, updated_at }
rating:{order_id}:{rater}     { integrity, communication, tags[], comment, created_at }

event:{seq}                   { order_id, at, actor, type, payload, prev_hash, hash }
snapshot:{char}:{ts}          { namespaces{ name: CAPTURED|NOT_CAPTURED }, items[] }
calib:{seq}                   { skill_name, skill, aa_rank, trivial, outcome }
```

### 5.3 The event chain

Akashic has BLAKE3 chaining natively; the DO does not, so we do it by hand — it's four lines:

```
hash = BLAKE3( prev_hash ‖ seq ‖ at ‖ actor ‖ type ‖ payload )
```

Append-only. Any edit to any past event breaks every hash after it, verifiably by anyone with a
copy. **Then anchor it:** a daily cron Worker posts the current chain head — one 64-char hash —
into a public read-only Discord channel. Discord timestamps it and nobody, including the operator,
can retroactively rewrite history without the published root disagreeing.

That is tamper-evidence without a distributed ledger, at zero cost. Periodically the event log
exports back into Akashic on the owner's machine for long-term storage and its own audit chain.

---

## 6. Pricing engine

### 6.1 Algorithm

```
p      = 1.0 if recipe.nofail else p_eff(T, S)      -- §3.2; aa_rank forced to 0 (§3.3)
N      = ceil(qty_needed / yield_qty)
A      = N / p

for each component c:
    units(c) = qty × (1 if TOOL else N if RETURNED_ON_FAIL else A)
    cost(c)  = units(c) × price(c) × M_BUY
    if enchanted:            cost = cost × M_ENCH + ENCH_FEE
    if crafter_supplies:     cost = cost × M_SUPPLY
    if TOOL and crafter owns it:  cost = 0
    if CRAFTED:              recurse

materials = Σ cost(c)
labor     = A × labor_rate(T)
risk      = 0 if nofail else N × RISK_BASE × (1/p − 1)
subtotal  = materials + labor + risk
tip       = max(subtotal × TIP_PCT, crafter.min_tip_pp)
total     = (subtotal + tip) × (1 − guild_discount if same guild)
```

### 6.2 Multipliers

| Key | Default | Rationale |
|---|---|---|
| `M_BUY` | 2.5× | substantial markup on purchased mats; plat is abundant |
| `M_SUPPLY` | 2.0× | additional, multiplicative — stacks to **5× vendor** |
| `M_ENCH` | 1.5× + 25pp | enchanted components |
| `labor_rate` | 5 / 10 / 20 / 35 pp per attempt | by trivial band <100 / <150 / <200 / 200+ |
| `RISK_BASE` | 5 pp | scales with `1/p − 1`, vanishes at high skill |
| `TIP_PCT` | 20% | floored at the crafter's `min_tip_pp` |

⚠️ **These six numbers are the only part of this spec with no empirical basis** — and there is no
market data to ground them with. EQL has no auction house. The 159MB log contains **one** `auctions,`
line (someone selling plat) and 12 WTS mentions total. eqlwiki's `Special:AuctionTracker` extension
404s and is presumably vestigial. There is nothing to scrape.

**What guild chat does reveal is the economy's scale, and it says these defaults are far too low:**

| Observed | Implication |
|---|---|
| *"Paying 5649pp (all I have) for a tracker that can do the entire zone"* | a one-off **service** clears 5,000pp+ |
| *"around 500k plat to make a piece of +10 resist gear"* | high-end crafting runs to six figures |
| *"there's mounts that cost 100k"* | 100k is an ordinary purchase |

Against that, §6.3's 216–501pp quotes are a rounding error. Nine data points isn't a calibration,
but the direction is unambiguous: **raise the defaults, especially `TIP_PCT` and `min_tip_pp`.**

### 6.2.1 Price discovery from behaviour

Since no external source exists, do what §3.4 does for mechanics — **learn from outcomes**:

| Signal | Reading | Adjust |
|---|---|---|
| Player views the quote in the Activity, abandons cart | too expensive | ↓ multipliers |
| Player submits without hesitation, repeatedly | too cheap | ↑ multipliers |
| Crafter overrides the quote upward | engine is under-pricing | ↑ toward the override |
| Order sits unclaimed through several offer cohorts | crafter's cut too low | ↑ `TIP_PCT` |
| Instant accepts by the whole cohort | crafter's cut too high | ↓ `TIP_PCT` |

Tune **per server**, since economies diverge. Log every adjustment to the event chain so pricing
history is auditable, and never move a multiplier more than 10% per adjustment window.

**Cheapest immediate calibration:** post five sample quotes in guild chat and ask "too high, about
right, too low?" 253 guild lines mention tradeskills — the community is engaged and will answer.

### 6.3 Worked examples

**Fine Plate Bracer** (trivial 188) — 1× Water Flask, 1× Flask of Acid, 4× Small Brick of Ore.
Crafter `min_tip_pp` = 50.

| Scenario | p | attempts | materials | labor | risk | tip | **total** |
|---|---:|---:|---:|---:|---:|---:|---:|
| skill 150, player farms | 60.5% | 1.65 | 227.3 | 33.1 | 3.3 | 52.7 | **316 pp** |
| skill 188, player farms | 95.0% | 1.05 | 144.7 | 21.1 | 0.3 | 50.0 | **216 pp** |
| skill 150, crafter supplies, guild −15% | 60.5% | 1.65 | 454.5 | 33.1 | 3.3 | 98.2 | **501 pp** |

Under-skilled costs **1.46×**. Crafter-supplied plus discount is **1.58×**. Both fall out of the
formula — no crafter guesswork.

**Batwing Pie ×10** (trivial 142, skill 120) — the Pie Tin is a `TOOL` the crafter owns, charged
**0**, not 10 × 42pp. Total **448 pp**.

**Water Flask ×20** (trivial 21, skill 30) — 22.1 expected attempts but only **20 bottles**, since
the bottle survives a failed brew. Total **271 pp**, mostly labor: 22 combines of tedium.

### 6.4 The 10ms CPU budget

Workers' free tier caps CPU at **10 ms per invocation**. This is an engineering budget we design
to, not a risk we hope clears — two paths spend it:

- **Recursive recipe resolution.** A deep subcombine chain (Batwing Pie → Pie Tin → Ceramic Lining
  → …) walks many nodes.
- **Autocomplete.** 25 results from ~11k items inside a **3-second, non-deferrable** budget.

**How we stay inside it, by construction:**

1. **Precompute resolved trees at export time.** Akashic flattens every recipe's full subcombine
   graph offline, where CPU is free, and `catalogue.bin` ships the flattened form. The Worker walks
   a list, not a graph.
2. **Prebuilt trigram index** in the catalogue. Autocomplete is a lookup, never a scan.
3. **Cache quotes in the DO**, keyed on `(recipe, qty, skill, mats_mode)`. Repeat quotes are free.
4. **Benchmark in CI** against the deepest chain in the catalogue and fail the build over budget.

With the tree pre-flattened, the Worker's remaining work is arithmetic over a few dozen rows —
microseconds in WASM, not milliseconds.

---

## 7. Order lifecycle

```
DRAFT ─submit─► SOURCING ─offer(cohort of 3)─► OFFERED ─accept+ETA─► ACCEPTED
                   │                              │                      │
                   └────── decline / timeout ─────┘                      ▼
                                                            AWAITING_MATERIALS
                                                                         │ "parcels received"
                                                                         ▼
                                                                  IN_PROGRESS  ◄─ queue position
                                                                         │ "done, sent back"
                                                                         ▼
                                                                    DELIVERED
                                                                         │ "received"
                                                                         ▼
                                                              COMPLETE ─► rate both sides
```

Side exits: `CANCELLED` (free before ACCEPTED; counts against reputation after), `EXPIRED`,
`DISPUTED` (freezes the order; **no human arbiter** — resolves by reputation per §8.8).

**Accepting requires committing an ETA** — one select menu, pre-filled with the peer median for
that difficulty. Without it §8's calibration axis cannot exist.

⚠️ **The ETA clock starts at `mats_received`, not `accepted`** — otherwise a slow player tanks the
crafter's score. For `CRAFTER_SUPPLIES` orders it starts at accept.

**Queue position is derived**, never stored: the crafter's `AWAITING_MATERIALS` + `IN_PROGRESS`
orders by `accepted_at`.

Nothing here is verifiable by the bot. "Parcels received" and "delivered" are attestations — which
is what §8 exists for.

---

## 8. Reputation and routing

### 8.1 Speed and calibration are different metrics

The pizza problem: one shop gets your toppings wrong half the time and its ETA is noise — sometimes
wildly early, sometimes late. The other is dead on the time and the order is always right.

Note *why* the bad one is early: it pads. **Being early is an estimation defect, not a virtue.**
Reward speed alone and crafters sandbag to look heroic; reward calibration alone and someone
promises 30 days, delivers in 30, scores perfectly, and is useless. You need both, separately.

### 8.2 Metrics

**Objective** — computed from timestamps and state transitions, ungameable by ratings:
follow-through (completed ÷ accepted), post-accept cancels, ghost rate, accuracy, calibration,
speed, offer-response latency, quote stability, dispute rate, rework rate.

Decline rate is tracked but barely penalised — declining beats accepting and ghosting.

**Subjective** — counterparty-rated 1–5 after `COMPLETE` only: integrity, communication.

### 8.3 Formulas

**Calibration** — asymmetric; late hurts more than early, but padding is still a defect:

```
e = (actual − promised) / promised
penalty = min(1, e) if e > 0 else min(1, |e|/2)
score   = 100 × (1 − penalty)
```

Promised 12h → 1h scores **54** (sandbagged) · 12h scores **100** · 13h scores **92** ·
24h scores **0**. Max sandbagging costs 50; 2× late costs everything.

**Speed** — peer-relative, so a trivial-188 order isn't punished for legitimately taking longer:

```
ratio = actual / peer_median(difficulty_bucket)
score = 100 × clamp((2.0 − ratio) / 1.5, 0, 1)
```

**Headline** — weighted, with follow-through as a **cap, not just a term**:

```
raw      = 0.30·follow_through + 0.25·accuracy + 0.20·calibration + 0.15·speed + 0.10·communication
headline = min(raw, follow_through)
```

| Crafter | raw | capped |
|---|---:|---:|
| accurate, dead-on ETA, average speed | 92.1 | **92.1** (4.6★) |
| 50% accurate, wild ETA, very fast | 65.5 | **65.5** (3.3★) |
| fast, accurate, pleasant, ghosts 1 in 3 | 85.6 | **67.0** (3.4★) |

The last row is why the cap exists — weighted alone they'd show 4.3★.

**Shrinkage** `(n·observed + 5·prior)/(n + 5)` — one perfect order displays **79**, not 100, and one
bad review can't sink a veteran. **Recency** — 60-day half-life. **Difficulty weighting** —
each order contributes `log(1 + expected_attempts)`, so grinding trivial orders never outranks real work.

### 8.4 Anti-gaming

| Vector | Mitigation |
|---|---|
| Wash trading between two accounts | cap any counterparty pair at 15% of effective `n` |
| Cherry-picking trivial orders | difficulty weighting |
| Sandbagging ETAs | two-sided calibration penalty |
| Accept everything, ghost the hard ones | follow-through cap + ghost counter shown raw |
| Rating retaliation | blind two-sided; reveal after both submit or 72h |
| Rep-farming then coasting | recency decay + `INACTIVE` after 30d |

### 8.5 Routing — uses the score, never the volume

Routing on orders-completed produces a lopsided market fast. Note that **every headline component
is a rate**, never a count. That's deliberate.

**Hard filters** (unscored, and they run **both directions** — §8.6.1): same server · has the skill ·
`p_eff` above threshold · accepting · queue below cap · **player clears the crafter's `min_player_rep`**.

```
weight = (quality/100)²
       × 1/(1 + queue_depth)                    -- load balance
       × (1 + min(0.5, days_idle/K))            -- idle crafters surface
       × (1 + 0.8 × max(0, (10 − eff_n)/10))    -- exploration, decays by order 10
```

Selection is **weighted-random, not top-pick**, offered to a **cohort of 3 — first to accept wins**.
Explainable ("you were in the first offer group"), self-balancing, and it rewards responsiveness.
Hard cap: no crafter takes >35% of the last 60 orders in a skill.

Simulated, 8 crafters, 600 orders:

| Strategy | top share | Gini | quality↔volume corr | starved |
|---|---:|---:|---:|---:|
| Always pick highest | **100%** | 0.875 | +0.48 | **7 of 8** |
| Weighted + cohort + cap | **18.8%** | 0.159 | **+0.94** | **0 of 8** |

Top-pick collapses on the *first* order — whoever gets lucky takes everything and the rest never
generate a data point. The correlation column is the surprise: spreading work makes the ranking
**more accurate**, because you cannot rank crafters you never sample.

Excluded from routing entirely: orders completed, lifetime volume, tenure, plat earned, tier.

### 8.6 Feedback

Structured tags first — pick up to 2 from a button row: `⚡ Fast`, `🎯 Nailed the ETA`,
`💬 Great comms`, `📦 Perfect order`, `🐢 Slow to reply`. No typing, aggregates cleanly.
Optional 200-char comment, blind two-sided.

**Tags and comments are context and never move the number** — otherwise it's a brigading surface.
One public right-of-reply per comment lets an unfair review answer itself. Nobody adjudicates it;
readers weigh both and move on.

### 8.6.1 Disputes resolve by reputation, not by a moderator

**There is no moderator role and no arbiter.** The audience is grown adults playing a thirty-year-old
game; the design assumes good faith. Bad actors are handled by the market — problem players find it
harder to get a crafter, problem crafters find it harder to get work.

**Mechanics, deliberately simple:**

- Either party can mark an order `DISPUTED`. It freezes and **auto-closes after 7 days**.
- Both parties take an integrity hit. Both sides' **dispute count and dispute *rate*** are shown raw
  and never averaged into the headline.
- Dispute rate is the tell. Someone who disputes one order in three is visible as the problem
  without anyone having to rule on anything.

**Player reputation is a routing input**, not decoration. Crafters set **`min_player_rep`** and the
§8.5 hard filter runs in **both directions** — a crafter is only eligible if the player also clears
their floor.

⚠️ **Newcomer guard — the one piece of machinery worth keeping.** Shrinkage puts a brand-new player
at the population prior (~79), not zero. **Cap `min_player_rep` at that prior** so no crafter can
set a floor excluding every first-timer. Without it the market closes to newcomers and dies, and
that failure is silent.

Player-side components: parcel promptness, ghost rate, confirmation promptness, post-accept
cancels, dispute rate, and a rated **Clarity** axis. Same shrinkage, decay and counterparty caps
as the crafter side.

> *If griefing ever appears in practice*, the escalation is to weight a dispute by the disputer's
> own record — `(headline/100) × (1 − dispute_rate)` — so serial disputers decay toward zero
> influence. Not built. Written down so it isn't re-derived under pressure.

### 8.7 Display

```
Grimtooth  ·  Blacksmithing 188  ·  ⟨Ironforge⟩ Firiona Vie
★ 4.6   Reliable

  Follow-through  96%    48 of 50 accepted orders completed
  Accuracy        98%    1 rework in 48
  On-time         ±8%    says 12h, delivers 13h
  Speed          1.3x    faster than peers on comparable work
  Communication   4.8 ★     ⚡ Fast ×12 · 🎯 Nailed the ETA ×9

  2 post-accept cancels · 0 ghosts · 0 disputes · active 3h ago
```

`On-time ±8% — says 12h, delivers 13h` is the whole distinction, legible at a glance.
Raw flags never folded into the headline: `NEW` (<5 effective orders), `INACTIVE` (30d), ghost
count, and **dispute count alongside dispute *rate*** — the rate is what exposes a serial disputer.
Tiers (Apprentice → Master) are **cosmetic and feed nothing in §8.5**.

---

## 9. User interface

### 9.1 The Activity — a real app inside Discord

A web app in an iframe, on desktop, mobile and web, launched by `LAUNCH_ACTIVITY`
(interaction callback type 12) from a command, component, or modal submission. Static site →
**Cloudflare Pages, free**.

Carries everything with real input:

- **Order builder** — multi-tradeskill cart, item search, quantities, live price preview
- **Recipe browser** — the subcombine tree, buy-vs-farm split, vendor zones and coords
- **Crafter profile** — skills, AA ranks, guild discount, tip floor, tools owned, accepting toggle
- **Queue dashboard** — the crafter's work list, ETAs, mark-done
- **Inventory upload** — with the §10 capture warning front and centre

### 9.2 Components V2 in the thread

Available because we aren't on serenity. Containers with accent colours, sections with thumbnails,
separators, 40 components per message.

Carries everything that should stay a record: the offer card with the crafter's rep, accept/decline,
the materials checklist, parcels-received, mark-done, queue position, rating prompt.

### 9.3 The broker thread

Bots **cannot** create multi-party DMs — that API doesn't exist. A private thread is strictly
better and free for all servers since Nov 2022.

`POST /channels/{id}/threads` type **12** (`GUILD_PRIVATE_THREAD`), **`invitable: false`**, then
`PUT /channels/{id}/thread-members/{user_id}` for each party.

Permissions: `CREATE_PRIVATE_THREADS`, `SEND_MESSAGES_IN_THREADS`, `VIEW_CHANNEL`, `MANAGE_THREADS`.
⚠️ **`SEND_MESSAGES` has no effect in threads** — the classic silent-failure trap, and it's also
required to *add members*.

⚠️ **~1000 active threads per guild.** Undocumented but real; hitting it blocks creation,
unarchiving *and* member-adds, and auto-archive silently accelerates as you approach it. **Archive
on `COMPLETE`** — archived threads don't count — and alarm at 700.

### 9.4 Fallbacks and unknowns

⚠️ A Discord community thread reports Activities not launching in text channels. Current docs
imply command-launch works and never state a voice requirement, but never explicitly confirm text
either. **Verify with a hello-world Activity before building on it.** If it's voice-only, fall
back to OAuth2 link-out to the same Pages app — costs the in-Discord feel, nothing else.

Activities proxy network calls through Discord's CDN via a URL-mapping config. The Embedded App
SDK client is **TypeScript only** — the UI is TS, the engine stays Rust.

### 9.5 Rate limits

50 req/sec global; **interaction endpoints are exempt**, so all cart traffic is free. A work order
costs ~4 requests; the binding constraint is the per-channel bucket, ≈1 order/sec sustained. The
real hazard is the **10,000-invalid-requests-per-10-min** ban if you loop on failures after hitting
the thread ceiling — **circuit-break on repeated 4xx**.

---

## 10. Inventory ingestion

### 10.1 🚨 THE CAPTURE TRAP

> **`/outputfile inventory` only writes containers whose window is OPEN when it runs.**
> Close the Tradeskill Depot, Dragon Hoard, or Bank and those namespaces are **silently absent**.
> Not empty — absent. The file looks complete. Nothing errors.

The consequence isn't a crash, it's a **wrong quote with real plat attached** — telling a player to
farm 40 Malachite they have 229 of.

It is not hypothetical. Answering "how many Black Sapphires do I have," this spec's author bucketed
by `Bank`/`General`/`SharedBank`, saw nothing else, and reported **3**. The answer was **7** — 3 in
the backpack, 4 in `Personal-Depot16`. **Assume this bug gets written once; design so it can't survive.**

Prompt shown before every upload:

> Open your **Inventory**, **Bank**, **Tradeskill Depot** and **Dragon Hoard** — *all at the same
> time* — and only then type `/outputfile inventory`.

### 10.2 Format (confirmed on live dumps)

Tab-separated, CRLF, header `Location  Name  ID  Count  Slots`. Vacant slots written as literal
`Empty` with ID 0. Nested containers repeat the suffix — `Bank6-Slot8-Slot3`.

```
General 1-Slot2     Black Sapphire   10036   3   10
Personal-Depot16    Black Sapphire   10036   4   10
```

Namespaces: `General N`, `Bank N`, `SharedBank`, **`Personal-Depot N`**, **`Hoard N`**, `Any Slot`,
`KeyRing`, `Augmentation`, `Activated`, `Equipment`, `Held`, plus equipment slots.

**The `ID` column is an exact join key — prefer it over fuzzy name matching.**

### 10.3 Detection — absence vs emptiness

| Observation | Meaning | Treat as |
|---|---|---|
| rows present, some non-`Empty` | window open, has items | **known** |
| rows present, **all** `Empty` | window open, genuinely empty | **known, zero** |
| **no rows at all** | **window closed** | **NOT CAPTURED** |

Confirmed on two dumps: `Reviir_neriak` lists `Bank` 24 slots / 0 non-empty (open, empty) but has
**zero** `Personal-Depot` rows (closed). `Reviir_qeynos` has `Personal-Depot` 29/29, `Hoard` 9/10.

Rules: record per-namespace status; a missing namespace is `NOT_CAPTURED`, **never zero**. Any BOM
diff touching one is marked **PARTIAL**. **Never** advise buying or farming when a location is
uncaptured — say *"check your depot first, it wasn't in this snapshot."* Echo a capture summary on
every upload. Snapshots are point-in-time and stamped.

---

## 11. Open questions

| # | Question | Status |
|---|---|---|
| 1 | Activities in text channels? | ✅ **RESOLVED — yes.** Shipped [May 2024](https://discord.com/blog/discord-update-may-13-2024-changelog), GA Sept 2024, works in guild text + DM + GDM. No allowlist, no cost. Use `contexts:[0,1,2]`, `integration_types:[0,1]`, respond `{"type":12}`. The "voice-only" myth is the legacy 2021 invite mechanism. |
| 2 | 10ms CPU | ✅ **Engineering budget, not a risk** — §6.4. Pre-flatten trees at export, CI benchmark. |
| 3 | Mastery AA | ✅ **Treated as non-functional**, price rank 0. Controlled test showed 83/100 → 83/100, though it was underpowered (§3.3). Cheap confirmation: 20 combines/arm on a recipe you fail 80% of the time. |
| 4 | **The low-skill tail.** §3.2's cubic was fit entirely at skill 237; at skill 30 it predicts 90.4% where the linear form says 75%. Nothing validates that regime. | Self-resolves via §3.4 log ingestion as low-skill crafters upload. No action needed. |
| 5 | Market price source | ✅ **RESOLVED — none exists.** No auction house; 1 auction line in 159MB. Use §6.2.1 behavioural discovery. |
| 6 | Publish policy | ✅ **RESOLVED — publish everything**, drop rates included. |
| 7 | Akashic snapshot export | **Build it.** Akashic is a DB, not an ETL tool. See §12.1 — there's a case for making it a first-class Akashic feature. |
| 8 | Dispute moderation | ✅ **RESOLVED — nobody.** No arbiter; reputation-weighted resolution per §8.6.1. |

**No blockers.**

---

## 12. Build phases

| # | Phase | Why here |
|---|---|---|
| 1 | **Catalogue export.** Codex → Akashic → `catalogue.bin` → R2. Hand-review a sample. | Ships nothing user-facing, de-risks everything |
| 2 | **Pricing crate.** Pure Rust, no Discord, golden-file tests. | Formula has changed three times — pin it with tests |
| 3 | **Worker skeleton.** Interactions endpoint, signature verify, one autocomplete command. | Proves the 3s budget and the 10ms cap |
| 4 | **Activity shell.** Pages deploy, `LAUNCH_ACTIVITY`, resolves Q1. | Unblocks all UI work |
| 5 | **Profiles + catalogue browse.** | First real user value |
| 6 | **Order flow.** Cart → route → thread → state machine → event chain. | The core |
| 7 | **Reputation + routing.** | Needs order volume to mean anything |
| 8 | **Log calibration.** | Makes everything else self-correcting |

Phases 1 and 2 carry the real risk and neither needs Discord to build or test.

### 12.1 Should the snapshot export be an Akashic feature?

Phase 1 needs something Akashic doesn't have: **emit a content-addressed, immutable, verifiable
read-only snapshot to object storage.** We can hand-roll it for this project. The argument for
building it into Akashic instead:

**The problem is general, not ours.** Every embedded or server database hits the same wall — it
cannot run where the compute is. Workers, Lambda, Deno Deploy, Pages Functions, browser WASM: no
filesystem, no threads, no connection pool. Today the answers are all lossy: ship a SQLite file and
lose your engine, export Parquet and lose your indexes, or stand up an API and lose the point.

**Akashic already has the differentiator.** A BLAKE3-chained audit journal means a snapshot can
carry its chain root, so a consumer at the edge can *prove* the artifact derives from the audited
source at a stated point in time. That is not a feature Datasette, DuckDB-over-HTTP or
Parquet/Iceberg can offer — they give you data, not provenance.

**Where that's worth money:** regulated dataset distribution to partners (prove they got the exact
audited version); reproducible ML feature stores (pin a verifiable snapshot to a training run);
compliance archives that are queryable rather than tarballs; and read-replicas-without-replication
for edge apps — no pool, no failover, no ops.

**Shape:** `akashic publish --to s3://… --lens row,document` → immutable artifact + manifest with
the chain root, plus a thin reader crate (`no_std`-friendly, wasm32) that queries the artifact
without the engine. One-line verification: `akashic verify <artifact>`.

**The pitch:** *your database, readable from places a database can't go, with proof it's the real
thing.* This project is a genuine first customer — and the requirement was discovered, not invented.

---

## Sources

- [eqlwiki Tradeskills](https://eqlwiki.com/Tradeskills) · [eqlwiki API](https://eqlwiki.com/api.php)
- Local: `_EQL_DataExport/` Codex — `peq_full.sqlite`, eqlwiki snapshot, `delta_recon`
- Local: `eqlog_Reviir_qeynos.txt` — 1,740 labelled combines; plus 600 controlled trials
- [Discord — Interactions](https://docs.discord.com/developers/interactions/receiving-and-responding) ·
  [Components](https://docs.discord.com/developers/components/reference) ·
  [Threads](https://docs.discord.com/developers/topics/threads) ·
  [Rate limits](https://docs.discord.com/developers/topics/rate-limits) ·
  [Activities](https://docs.discord.com/developers/activities/overview) ·
  [Entry Point commands](https://docs.discord.com/developers/interactions/application-commands)
- [Cloudflare Workers pricing](https://developers.cloudflare.com/workers/platform/pricing/)
- Local: an embedded Akashic workspace, BLAKE3 audit chain
