# Parses

**Status:** planning. Nav group added; nothing built.
**Supersedes:** the single "Log parser" line in the roadmap, which undersold it.

---

## 0. The one sentence that has to land first

The log parser is not a tool in the hunt group — **it is the only thing in the whole
project that creates data instead of spending it**, and once parses can be compared
across a guild it stops being a personal readout and becomes the reason every other
number in the Grimoire is true rather than guessed.

---

## 1. Why comparison is nearly free in EverQuest

One player's log already contains **everyone else's melee and spell damage in range.**
That is how every EQ parser since GamParse has worked. So "looking at other people's
parses" mostly does not require other people to do anything — a single raid log
reconstructs the whole raid.

That matters for adoption. Every other social tool needs everyone to opt in before it
is useful to anyone. This one is useful from the first person who uploads.

---

## 2. What is in the group

| Screen | What it is |
|---|---|
| **Your parses** | your own history — damage, healing, uptime, deaths, mana, per session and over time |
| **Fights** | a single encounter broken out by participant, pulled from whoever's log covered it |
| **Compare** | you against your own past, your class at your level, or the guild |
| **Benchmarks** | anonymised percentiles by class, trio and level — the "am I pulling my weight" answer |
| **At the forge** | your crafting log: combines attempted, success rate by trivial, materials burned |

---

## 3. The part that closes a loop

**At the forge is the important one**, and it is the only entry here that feeds a thing
we have already built.

The combine formula in the spec came from 1,740 logged combines and 600 controlled
trials — one player, one skill band, hand-collected. Every quote the broker gives rests
on it, and the Mastery AA term is still marked UNVALIDATED because nobody has run the
experiment.

If crafting logs feed back automatically:

- the trivial/skill curve becomes **measured on this server**, not fitted from one
  person's data
- **Mastery stops being unvalidated** — thousands of combines at known AA ranks answer
  it in a week without anyone running a deliberate trial
- expected material burn becomes an observed number, so "he would burn 1.7× your
  materials" is a fact rather than an inference
- the same applies to **drop rates in Sources** — the 44% next to a timber wolf is a
  placeholder today; pooled kill logs make it real, and per-server

That is a flywheel no other EQ tool has, because no other EQ tool is also the thing
quoting you a price.

---

## 4. What crosses the wire, and what does not

Parsing happens **in the browser, on your own file.** A 159 MB eqlog never leaves your
machine.

What goes up is a **summary** — a few hundred bytes per session: fight rollups, combine
counts by (skill, trivial, AA rank, outcome), kill counts by (mob, zone, item). Small
rows, low write volume, comfortably inside D1's free tier.

Benchmarks are recomputed on artifact rebuild and shipped **inside the static corpus**,
so reading them costs nothing at request time.

---

## 5. The thing to be careful about

Parse comparison is socially loaded. Every guild that has ever run a parser has had the
argument about it. Some guardrails worth deciding **before** it ships, not after:

- benchmarks anonymised and bucketed; percentiles, never a leaderboard by name
- a player's own parse is theirs — visible to them by default and shared only on purpose
- no automatic posting of anyone's numbers into a channel
- if a fight view shows other named players, it shows what their own client would have
  shown them, and nothing derived that they did not consent to

The reputation system in this app deliberately avoided ranking people by volume because
it goes lopsided fast. Parses have exactly the same failure mode, with feelings attached.

---

## 6. Where it sits in the build order

Unchanged, and now with a better reason: **first.** Everything else on the roadmap is a
view over data, and this is the only thing that produces any. Built late, every tool
above it ships on my guesses — the potion numbers, the drop rates, the Mastery term.

---

## 7. Unresolved

1. Browser-only on an uploaded file, or a small resident agent watching the log folder?
   The agent is much better — live parsing, no upload step, combines feeding back as
   they happen — and it is the one thing on the whole roadmap that wants software
   installed on the player's machine.
2. Do combine and kill summaries need consent per upload, or is it opt-in once in the
   profile with a plain statement of what leaves?
3. Do we keep raw session summaries, or fold them into running aggregates and discard?
   Folding is cheaper and answers the privacy question, but it makes "your parses over
   time" thinner.
4. Cross-server pooling: is a combine on Qeynos evidence about a combine anywhere else,
   or does each server get its own curve?
