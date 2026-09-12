# EQL Grimoire — the build plan

**Status:** plan, being executed.
**Supersedes:** ROADMAP.md's ordering. Keeps HOSTING.md and PARSES.md intact.
**Written after:** reading eqlegendstools.com and eqlegends.tools, and after
finding 254 MB of real EQL logs and two real inventory dumps on the machine.

---

## 0. The one sentence that has to land first

**Most of the fifteen tools on the roadmap already exist somewhere and are good**, so the plan is
not "build fifteen tools" any more. It is *build the four things nobody has, get them right, and
link out to the people who already did the rest.*

---

## 1. What is already built, by whom, and how well

| Roadmap entry | Already exists | Where |
|---|---|---|
| The item system | **yes, twice** — BiS gear/weapons, procs, focus, clickies, worn, comparison, exaltation planner | eqlegendstools.com |
| Collections / checklists | **yes** — Plane of Sky quest tracker | eqlegendstools.com `/posky` |
| Spells & skills browser | **yes** | eqlegends.tools |

Existing sites already do the two things I had listed as our unfair advantages: reading your
inventory dump, and knowing your trio. **Rebuilding any of this is dead work.**

### What nobody has built

| Gap | Evidence |
|---|---|
| **Tradeskills, at all** | eqlegends.tools lists "Crafting Recipe Finder" as *upcoming*. Only gnollguard.com has a recipe list, and it is a list. |
| **Crafting log parsing** | The log carries every combine, every skill-up and every component consumed. Grimoire's parser reads all three. |
| **Anything with two players in it** | The existing sites are single-player, client-side, stateless. No orders, no reputation, no shared state, by design. |
| **Anything inside Discord** | They are all websites. |

That is the whole product: **the broker, and the crafting data underneath it.** It is not a
subset of the roadmap — it is the part of the roadmap that was never anyone else's.

### What this changes about the third advantage

ROADMAP.md §1 claimed three things nobody else knows: your inventory, your trio, and who in
your guild can make what. The first two are now table stakes. **The third is still ours alone**,
and everything in this plan hangs off it.

---

## 2. The thing that landed on the desk

`C:\Users\Public\...\EverQuest Legends\Logs\` holds three logs — 254 MB — and parsing them for
combine lines gives:

```
1,840 combine attempts   1,320 success / 520 failure
8,602 skill-up lines, each stamping the exact skill value at that moment
   67 distinct crafted items
```

The combine grammar is fully determined, and it is richer than expected:

```
You have fashioned the items together to create something new: <Item>.   success
You lacked the skills to fashion <Item>.                                 failure
You can no longer advance your skill from making this item.              skill >= trivial
You have become better at <Skill>! (<n>)                                 skill is now exactly n
```

Because a skill-up carries the new value and fires on the same timestamp as the combine that
caused it, **every attempt can be labelled with its tradeskill and the crafter's exact skill
at the moment of the attempt.** 1,496 of the 1,840 attempts label cleanly.

And because "You can no longer advance your skill" fires precisely when skill reaches trivial,
**eleven items have their trivial pinned exactly** by the log:

```
Electrum Malachite Bracelet 74 · Electrum Lapis Lazuli Earring 76 · Potion of Accuracy 83
Electrum Bloodstone Necklace 87 · Electrum Onyx Pendant 90 · Electrum Jasper Earring 92
Electrum Amber Earring 100 · Jaded Electrum Bracelet 106 · Electrum Pearl Choker 108
Gold Malachite Bracelet 146 · Golden Hematite Choker 154
```

343 attempts sit at those known trivials, which is enough to **test the formula the spec has
been running on** rather than replace it. The result is the good kind of boring:

```
chance = skill − 0.75·trivial + 51.5      (trivial ≥ 68)
chance = skill − trivial + 66             (trivial < 68)      clamped to 5…95%
```

| skill − trivial | attempts | observed | classic formula | freely-fitted curve |
|---|---|---|---|---|
| −60 … −40 | 37 | 0.14 | 0.22 | 0.16 |
| −40 … −25 | 68 | 0.50 | 0.45 | 0.43 |
| −25 … −15 | 56 | 0.59 | 0.58 | 0.57 |
| −15 … −5 | 67 | 0.69 | 0.70 | 0.68 |
| −5 … +5 | 112 | 0.72 | 0.76 | 0.76 |

Log-likelihood −200.4 for the classic formula against −202.5 for a two-parameter curve fitted
to this very data. **A formula with no free parameters beat one with two.** EQL is running
classic EverQuest tradeskill maths, and the spec's model was already right.

**What this replaces:** the combine model was fitted from one player's hand-collected trials
and carried as an assumption. It is now tested against the game, and it survived.

**What it deletes:** the mockup's `max(p, 1 − 0.28·(trivial/skill)³)` floor changes the
likelihood by zero to one decimal place — it never binds on real data. Dead code; remove it.

**What it honestly does not answer.** One crafter, so AA rank never varied — **Mastery is still
unvalidated and this data cannot validate it.** No attempt in the set is above trivial, so the
0.95 ceiling is assumed, not observed. And the log never prints a component-loss line, so
whether a failed combine destroys materials still cannot be read out of a log — it needs either
an inventory diff across a failure or someone watching a stack count.

---

## 3. What gets built

Four things, in this order. Each is useful alone; each feeds the next.

### 3.1 `grimoire-parse` — the crafting parser

The one parser nobody has. Reads an eqlog and emits combines, skill trajectories, trivial
brackets and material burn. Also reads the `/out inventory` TSV, which carries **item IDs**, not
just names — the join key to everything else.

Pure Rust over `&str`. Native for the corpus builder, `wasm32` for the browser. The file never
leaves the machine (HOSTING.md §2), and it tail-reads, because 160 MB is a normal log size here,
not a worst case.

Reading a real dump turned up something no documentation mentions: **the inventory file has a
second table.** After the inventory, the game writes a three-column keyring —
`Augmentation` / `Activated` / `Equipment` — of things you have *collected*. Counting those as
stock would tell a crafter you can hand him an augmentation that is fused into a bracer. Not
counting them at all would throw away the best checklist input in the game. They get their own
list.

### 3.2 `grimoire-core` — the maths, once

Every number in the mockup currently lives in JavaScript: combine probability, expected
attempts, material burn, labour, risk, guild courtesy, the regard ladder, the order lifecycle.
That has to become **one Rust crate that the Worker, the browser and the corpus builder all
call**, or the quote a crafter sees and the quote a buyer sees will drift apart. Tested against
the measured curve in §2.

### 3.3 `grimoire-corpus` — the artifact

Items, recipes, components, sources, trivials. Content-addressed, range-readable, manifest
carrying provenance — RFC 42's shape, built standalone because **RFC 42 is proposed, not
accepted, and `akashic publish` does not exist yet.** The reader is a trait with one method, so
when publish mode lands the artifact swaps underneath without touching a caller. This is the
first-customer proof the RFC asks for.

Sources, in order of trust: the client's own files, then eqlwiki, then measured log data.
Both ends checked rather than assumed:

- `dbstr_us.txt` is `id^type^text^flag` and holds **no item names** — items come from the
  server, not the client. So the client gives spells, races and skills; it cannot give items,
  drops or recipes.
- **eqlwiki runs a live MediaWiki API.** `api.php?action=query&list=categorymembers` answers,
  and `Category:Tradeskills` returns `Skill Alchemy`, `Skill Baking`, `Skill Blacksmithing`,
  `Skill Brewing`, `Skill Fletching`, `Skill Jewelcrafting`, `Skill Pottery`, `Skill Tailoring`,
  `Skill Tinkering`, eleven `Cultural Tradeskills:` pages and `Tradeskill Equipment`. That is
  the ingestion path, and the recent-changes feed is how it stays current.

### 3.4 `grimoire-worker` — the only hosted part

Discord interactions endpoint on Cloudflare Workers. Orders, lifecycle, regard, workshops.
D1 not KV (HOSTING.md §1). Everything that thinks runs on the client; the Worker only agrees.

### Not built, deliberately

Trio builder, AA planner, levelling, gear upgrades, spell checker, zone atlas, kill tracker,
combat parsing. Being the fourth site to do trio analysis is how this project dies.

---

## 4. Order of work

1. **Parser + core, with the log as the test fixture.** Real data, so the tests are real.
2. **Corpus, seeded from the client files and the measured trivials.**
3. **Wire the mockup to the wasm.** The UI is already designed and argued over; it needs a
   real engine behind it.
4. **Worker last**, because it needs a Discord app and it is the only part that can cost money.

---

## 5. What could still go wrong

- **Recipe coverage.** 67 items observed is not a corpus. Without recipes, the broker can quote
  only what someone has already crafted in front of it.
- **One crafter's logs.** Everything measured here is Reviir's. It generalises only once other
  people's logs feed in — which is exactly what §3.1 is for.
- **Discord is the only part that can bite.** Everything else is a static file and a wasm blob.
