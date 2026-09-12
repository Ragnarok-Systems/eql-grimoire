# Building it — Discord and web

**Status:** plan. Follows [PLAN.md](PLAN.md), which covered the engine; this covers the app.
**Decided:** build all 17 parked modules ourselves rather than link out. User-picked themes.
**Checked:** Discord Activities + Components V2 docs, Cloudflare Pages free tier, and what the
EQL client actually has on disk (August 2026).

---

## 0. The one sentence that has to land first

A Discord Activity **is a web page in an iframe** — so the website and the Discord app are not
two builds, they are one static bundle behind two doors, and the only thing that genuinely has
to be written twice is the order loop, because that has to work on a phone without opening
anything.

---

## 1. Three surfaces, one codebase

| Surface | What it is | What goes there |
|---|---|---|
| **The site** | `grimoire.<domain>` — the SPA, straight from a CDN | everything |
| **The Activity** | the same bundle, in Discord's iframe, desktop/mobile/web | everything |
| **Messages** | Components V2 in a channel or thread | the order loop, and only that |

The mockup already knows this. Its nav has a group called **In thread** — Offer, Materials,
Delivered, Standing — sitting apart from the other seven groups. That group is the message
surface. The rest is the SPA.

### What the Activity costs us

Activities are sandboxed. External requests are blocked by CSP unless declared as a **URL
mapping**, where a prefix like `/corpus` is proxied to a target host. So the corpus is fetched
through `/corpus/...` inside Discord and directly on the site — one line of config, one
`baseUrl` constant.

**To verify before committing:** whether HTTP `Range` requests survive Discord's proxy. The
whole artifact design in `grimoire-corpus` assumes they do. If they don't, the fallback is to
shard the corpus by domain and fetch whole shards, which the format already allows — but it is
worth ten minutes with a test mapping before it becomes an assumption.

### What the message surface can and cannot do

Components V2 gives Containers, TextDisplay (4,000 chars, markdown), Sections with a button or
image accessory, Separators, Media galleries, and the old action rows. **No tables, no
columns.** 40 top-level components per message, 10 per container.

That is enough for an order — a Section per line with an accept button on the right, a
Separator, a total. It is not enough for the catalogue, the ternary weighting control, or the
skill-band table. Which is the right split anyway: **the message is the notification and the
handshake; the Activity is the workbench.**

---

## 2. The critical path is data, not code

Building all 17 modules means the corpus has to answer everything the nav asks. Here is where
each piece actually comes from, checked rather than assumed.

### Already on every player's disk — free, no hosting, no scraping

| File | What it holds | Feeds |
|---|---|---|
| `spells_us.txt` | **73,963 spells**, 173 caret-delimited fields each | Spell checker, trio builder, AA planner |
| `dbstr_us.txt` | AA names under type 1 (`107^1^Natural Durability`), race and skill names | AA planner, trio builder |
| `racedata.txt` | race definitions and starting attributes | Trio builder |
| `eqlog_*.txt` | combines, kills, XP, damage | Parses, At the forge, Farmer John, drop rates |
| `*-Inventory.txt` | held items **with ids**, plus the keyring | Checklists, gear upgrades, quoting |

These files are already on every player's disk, which is why anything built on them needs no backend.
`dbstr_us.txt` holds **no item names** — items come from the server — which is exactly the line
between what the client can give and what the wiki must.

**One find worth calling out:** the tradeskill Mastery AAs are in there, and they are
**per-skill** — `Alchemy Mastery`, `Blacksmithing Mastery`, `Baking Mastery`, `Brewing Mastery`,
`Fishing Mastery`, `Fletching Mastery`, plus a general `Crafting Mastery`. `Mastery::bonus()`
currently returns zero and is marked unvalidated; it is not one AA to measure, it is seven.

### Must come from eqlwiki

Items, item stats, recipes, drop tables, mobs, zones, quests, factions. The MediaWiki API works
(`action=parse&prop=wikitext`), and 294 recipes are already ingested this way.

**This is the whole job.** Two tradeskills took a working parser and hand-saved pages. Six more
tradeskills, plus every item, mob and zone, is a different scale of work and needs an
automated, scheduled ingest rather than saved files.

### Must be measured

Drop rates, per-server. The 44% next to a timber wolf is a placeholder; pooled kill logs make
it real. Nothing else can produce this — not the wiki, not the client.

### Honest sizing

| Module group | Data it needs | State |
|---|---|---|
| Tradesman + Adventurer (8 screens) | recipes, items, prices | **built for 2 of 8 tradeskills** |
| Parses (5) | nothing but your own logs | parser exists; UI does not |
| Character (5) | spells, AAs, races — all on disk | ingest not written |
| The hunt (3) | mobs, zones, drops — wiki + logs | not started |
| Raid (3) | shared state + Discord API | not started |
| Collections (1) | items + your inventory | not started |

Five of the six groups are **a corpus ingest and a view**, not new science. The exception is
Raid, which is the only group that needs Discord to do something rather than show something.

---

## 3. Themes

A theme is a block of CSS custom properties and nothing else. The mockup already routes every
colour through variables (`--ink`, `--gold`, `--rust`, `--page`, `--panel`, `--line`), so this
is a `data-theme` attribute on `<html>` and one block per theme.

Four to start:

- **Grimoire** — what exists now. Default.
- **Parchment** — the same layout, light, ink on cream.
- **Amber** — EQ-classic, amber on black, the old UI's palette.
- **High contrast** — accessibility, not decoration.

Stored in the profile, remembered locally, and **applied before first paint** (an inline script
in `<head>` reading the stored value) so the page never flashes the wrong theme.

The con colours and the regard ladder are semantic, not decorative — red must stay legible as
"you will fail this" in every theme. Each theme declares its own con ramp, and a contrast check
runs in the test suite rather than by eye.

---

## 4. Build order

Each phase ships something a person would use on its own. Nothing here is a rewrite of what
exists — the engine, parser, corpus format and quote maths are done and tested.

**Phase 1 — the broker, finished.** Wire the designed mockup to the wasm engine, exactly as
`web/bench.html` already does. Kill the mockup's own JS maths. Add themes. Ingest the remaining
six tradeskills. *Ships: the app the mockup was drawn for, on the web, with real prices.*

**Phase 2 — the Activity.** Same bundle, Discord app, URL mapping, Embedded App SDK for
identity. *Ships: the same thing, inside Discord, on desktop and phone.*

**Phase 3 — the order loop.** The Worker: interactions endpoint, D1 for orders/workshops/regard,
Components V2 messages for Offer → Delivered. Ed25519 verification, no polling.
*Ships: two people can actually trade. This is the first thing no other EQL tool does.*

**Phase 4 — Parses.** Your parses, Fights, Compare, Benchmarks, At the forge. All client-side
over your own log; only anonymised buckets ever leave. *Ships: the flywheel — At the forge feeds
the combine model that Phase 1 prices with.*

**Phase 5 — Character.** Ingest `spells_us.txt`, `dbstr_us.txt`, `racedata.txt` into the corpus.
Trio builder, AA planner, levelling, gear upgrades, spell checker. *Ships: the five tools that
share one input, built once against one schema instead of five half-schemas.*

**Phase 6 — The hunt and Collections.** Automated wiki ingest for mobs, zones and drops.
Farmer John, kill tracker, atlas, checklists. Drop rates start as wiki numbers and get replaced
by measured ones as logs accumulate.

**Phase 7 — Raid.** Planner, spawn timers, LFG. Last because it needs presence and scheduling
that nothing else needs, and it fails badly in a small guild.

---

## 5. What it costs

Still nothing, and the numbers do not change much at 17 modules, because the modules are views.

| | Free tier | What we use |
|---|---|---|
| Cloudflare Pages | unlimited bandwidth, 500 builds/month | the whole SPA and every corpus artifact |
| Workers | 100k req/day, **10 ms CPU** | interactions only — no thinking |
| D1 | 5M reads / 100k writes / 5 GB | orders, workshops, regard, timers, LFG |
| Discord | — | identity, presence, voice, notifications, delivery |

The three ways this breaks are unchanged from `docs/HOSTING.md`: serving the corpus from a
database, thinking on the server, or polling. Adding modules does not threaten it. **Storing
inventories would**, and with checklists and gear upgrades in scope the temptation arrives in
Phase 5 — parse locally, keep locally, upload nothing.

The one new cost is the **wiki ingest**, which needs to run somewhere on a schedule. A Worker
cron is free and well inside 10 ms per tick if it fetches a few pages per run rather than
crawling.

---

## 6. What could still go wrong

- **The wiki is the bottleneck and it is young.** Every module past Phase 3 inherits its gaps.
  Two tradeskills took hand-saved pages; the rest needs a scheduled crawler, and the tables are
  written in two dialects by different authors with inconsistent columns.
- **Seventeen modules is a product.** The plan sequences them so each phase stands alone, but
  the honest risk is Phase 1 shipping and Phases 5–7 never doing so. That is fine if Phase 1 is
  genuinely good; it is not fine if the nav promises them.
- **`Range` through Discord's proxy is unverified**, and the corpus design leans on it.
- **Mastery is seven AAs, not one.** Whatever measures it has to bucket by tradeskill, and no
  single player's logs will cover more than one or two.
- **Parse comparison is socially loaded** (`docs/PARSES.md` §5). Decide the guardrails before
  Phase 4 ships, not after the first argument.
