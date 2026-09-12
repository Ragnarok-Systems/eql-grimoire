# EQL Grimoire

Tradeskill broker for EverQuest Legends. Reads your logs and your inventory on your own
machine, prices a job honestly, and remembers who did what.

**Read [PLAN.md](PLAN.md) first.** It explains why the core of this repo is four pieces (a
crafting parser, one crate of maths, a static corpus and a thin hosted worker): crafting is the
part of EverQuest Legends the log records and no tool measures, and those four pieces are what it
takes to measure it.

---

## What's here

| Crate | What it is |
|---|---|
| `grimoire-core` | The domain and the maths. Combine odds, quoting, regard, order lifecycle. No I/O, no Discord, no HTTP. |
| `grimoire-parse` | Reads `eqlog_*.txt` and `*-Inventory.txt`. The only EQL parser that reads **crafting**. |
| `grimoire-corpus` | Content-addressed, range-readable static artifact. Akashic RFC 42 shaped. |
| `grimoire-wasm` | The engine for the browser: JSON in, JSON out, over a plain C ABI. No wasm-bindgen, no npm. |
| `grimoire-forge` | `grimoire` — the command line that ties them together. |
| `web/` | `app.html` — the designed UI on the real engine — plus `grimoire.js` and `bench.html`. |

```
cargo test --workspace     # 139 tests
cargo build --release
```

**Run it.** Double-click `run.cmd` on Windows, or `./run.sh` anywhere else. Then open:

### → http://127.0.0.1:8787/app.html

No wasm build needed for this path. Or by hand:

```
cargo run --release -p grimoire-forge -- serve
```

`grimoire serve` answers on `POST /engine` with the same `grimoire_wasm::dispatch` the wasm
module wraps — the same code behind a different door, not a second implementation. It exists
because `wasm32-unknown-unknown` cannot be installed everywhere, and a UI nobody can click is
a UI nobody has tested.

**Ship it.** The browser build has no server at all:

```
rustup target add wasm32-unknown-unknown
cargo build --release -p grimoire-wasm --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/grimoire_wasm.wasm web/
```

`web/app.test.js` and `web/bench.test.js` drive those pages in a real browser against real
engine output — piped through `grimoire dispatch` instead of wasm — so the UI is tested even
where the wasm target will not install:

```
node web/app.test.js --shots
node web/bench.test.js
```

Four themes ship: **Grimoire**, **Guild hall**, **Stone**, **System**. They were already
written in the mockup's CSS and never applied; the switcher is the swatch row under the nav.

---

## The command line

```
grimoire combines <eqlog.txt>...      read crafting out of a log
grimoire inventory <dump.txt>         read an /outputfile inventory dump
grimoire wiki <page.wikitext>...      turn a wiki recipe table into recipes
grimoire corpus <out.grim>            cut a corpus artifact
grimoire check <corpus.grim>          open an artifact and verify it
grimoire quote <corpus.grim> <item>   price a job
grimoire quotes <corpus.grim> <item>  the same job across a band of skills
grimoire dispatch                     the browser's engine API on stdin/stdout
```

Cutting a corpus from scratch:

```sh
grimoire combines "…/Logs/eqlog_Reviir_qeynos.txt" --csv data/trivials.csv
grimoire wiki data/wiki/Jewelcrafting.crafters.wikitext \
    --metals data/wiki/Jewelcrafting.metals.wikitext \
    --trivials data/trivials-measured.csv --out data/recipes-jewelcrafting.json
grimoire wiki data/wiki/Alchemy.recipes.wikitext --skill Alchemy \
    --prices data/wiki/Alchemy.reagents.wikitext --out data/recipes-alchemy.json
grimoire corpus web/corpus.grim --from data --trivials data/trivials-measured.csv
```

**294 recipes today** — 141 jewelcrafting, 153 alchemy — of which 195 are fully priced.

Logs live in `<EverQuest Legends>\Logs\eqlog_<char>_<server>.txt`. Inventory dumps land beside
the client as `<Char>_<server>-Inventory.txt` after `/outputfile inventory`.

Against a real 160 MB log, in half a second:

```
  1840 combine attempts      67 items     9 tradeskills

  trivials this log pins exactly
    Electrum Malachite Bracelet              74     76 attempts     47% landed
    Potion of Accuracy                       83    106 attempts     39% landed
    Gold Malachite Bracelet                 146     98 attempts     79% landed
    …

  the combine model against this log  (343 attempts at pinned trivials)
    skill−trivial      n   observed   model
      -99…-40        40      0.12    0.21
      -40…-25        68      0.50    0.45
      -25…-15        56      0.59    0.58
      -15…-5         67      0.69    0.70
       -5…99        112      0.72    0.76
    overall             343      0.58    0.59   model holds
```

---

## What a quote looks like

```
Gold Malachite Bracelet x10
  Jewelry Making · trivial 146 · a hand of skill 146 cons it grey and lands 88% of the time
  10 combines wanted, 11.4 attempts expected

  materials
      12 x Gold Bar                          139p 9s
      12 x Malachite                           6g 9s

  materials            139p 7g 8s
  his work            1p 1g 3s 6c
  risk                         7c
  ------------------------------
  subtotal          140p 9g 2s 3c
  guild courtesy    −21p 1g 3s 8c   −15%
  total             119p 7g 8s 5c
```

Twelve bars for ten bracelets is the whole argument: a hand who fails buys the difference, and
`grimoire quotes` shows what a worse one costs you.

```
Greater Potion of Accuracy x20 — trivial 150
  skill   con          lands   attempts        total
     90   yellow         29%     69.0    2617p 6g 7s
    110   white          49%     40.8   1555p 3g 5s 3c
    130   blue           69%     29.0   1100p 1g 2s 2c
    150   grey           89%     22.5   872p 3g 8s 3c
    170   grey           95%     21.1   834p 3g 5s 9c
```

---

## Four things the code knows that no wiki does

**The trivial is in your log.** `You can no longer advance your skill from making this item.`
fires exactly when skill reaches trivial, and the neighbouring skill-up line stamps the
number. Craft something from under trivial to over it and the log has told you its trivial
exactly. Eleven of them fell out of one character's logs.

**The combine formula is the classic EverQuest one, and it is now tested rather than assumed.**
`skill − 0.75·trivial + 51.5`, clamped 5–95%. Log-likelihood −200.4 across 343 real attempts,
against −202.5 for a two-parameter curve fitted to that same data — a formula with no free
parameters beat one with two. `grimoire combines` re-runs that check against any log and says
**MODEL HAS DRIFTED** if it stops holding.

**The inventory dump has a second table.** After the inventory it emits a three-column keyring
of collected augmentations, clickies and equipment. It is not stock — you can't hand a crafter
an augmentation you've merely collected — but it is exactly what a collection checklist wants.

**Gem prices are derivable, and nowhere written down.** The wiki publishes a per-recipe `Cost*`
and a per-bar metal price, never a gem price. A piece is one bar plus one gem, so the gem is
the difference — and every recipe using that gem agrees on it. 28 gem prices fall out, taking
jewelcrafting from 0 fully-priced recipes to 140. Derived, not transcribed, and marked so.

---

## Still assumed, and marked as such in the code

- **Mastery does nothing.** `Mastery::bonus()` returns zero on purpose. One crafter's logs mean
  AA rank never varied, so the term cannot be measured yet. A guess here would sit inside every
  price the app quotes.
- **The 95% ceiling.** No observation in the dataset is above trivial.
- **Failed combines destroy components.** `Disposition::PerAttempt` assumes classic behaviour.
  The log prints no component-loss line either way, so this needs an inventory diff across a
  known failure. If EQL returns materials on failure, every quote here is too high.
- **Grey does not mean safe.** At skill exactly equal to trivial the classic formula gives
  `0.25·trivial + 51.5` percent, so a maxed hand on a trivial-83 potion still fails better than
  one time in four. Counter-intuitive, measured, and pinned by a test so nobody "fixes" it.
- **`Source::Unknown` is treated as un-buyable.** A component with no known vendor price gets
  handed to the buyer to find rather than silently billed. Ten of eleven measured trivials
  matched the wiki exactly, so the wiki is trustworthy — but silence in it is not evidence.

---

## Nothing uploads

Logs and inventory dumps are read where they sit. What a crafting summary would send is
`(trivial, skill, attempts, successes)` — a few hundred bytes, no item names, no character
name. There is a test asserting the bucket type cannot carry an item name.

---

## Licence

GNU Affero General Public Licence v3 (`AGPL-3.0-only`), held by James McMenamin. Full text:
[`LICENSE`](LICENSE). Contributions are covered by [`CLA.md`](CLA.md); see
[`CONTRIBUTING.md`](CONTRIBUTING.md). Third-party material in the tree
(the bundled fonts, and the game data the desktop app loads at runtime):
[`docs/LICENSING.md`](docs/LICENSING.md).
