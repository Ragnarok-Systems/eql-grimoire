# What actually costs money

**Status:** decision note. Written after the roadmap, because the roadmap looked like
it broke the free-hosting constraint and mostly it doesn't.
**Checked:** Cloudflare Workers pricing, August 2026.

---

## 0. The one sentence that has to land first

You already said the answer — **send the client the database** — and the reason that
works for almost everything on the roadmap is that every tool on it is a *pure function
of (game data + your character)*. Neither of those needs a server. What needs a server
is the handful of things where two players have to agree on something, and that part is
small enough to live inside a free tier.

---

## 1. Free tier, as it actually stands

| | Free plan | Notes |
|---|---|---|
| Workers requests | 100,000 / day | |
| Workers CPU | **10 ms per invocation** | the real constraint, not the request count |
| D1 | 5M rows read/day · 100k rows written/day · 5 GB | the right store for us |
| Workers KV | 100k reads/day · **1,000 writes/day** · 1 GB | writes far too tight — do not use for order state |
| Durable Objects | 100k requests/day · 13k GB-s/day | now on the free plan, which it wasn't |
| R2 | 10 GB stored · 1M Class A · 10M Class B | where the corpus lives |
| Paid, if we ever cross | $5/month minimum | 10M requests, 30M CPU-ms |

Two things follow immediately. **D1, not KV** — KV's 1,000 writes a day would be
exhausted by one busy guild night. And **10ms CPU** means anything that thinks hard has
to think on the client or be precomputed into the artifact.

---

## 2. The split

### Ships as a static artifact — effectively free
Items, recipes, components, drop tables, vendors, spells, AAs, zone data, checklist
definitions. Content-addressed, so a rebuild is a new key and the CDN never needs
invalidating; **range-readable**, so a phone opening Farmer John fetches tens of
kilobytes out of a 40 MB corpus rather than the corpus.

This is exactly phase 1 of Akashic's verifiable-snapshot-publish RFC (RFC 42, which is not in
this repository), and the Grimoire is the first customer named in it. It also gets provenance for free: the
artifact carries the chain root it was cut from, so anyone can prove the item data
they're reading is the audited Codex at a stated moment, not a lookalike.

### Computes on the client — free, and the only way to survive 10 ms
Trio cross-analysis · gear upgrade filtering · Farmer John routing · levelling guide ·
spell checker · AA planner · checklist ticking · **log parsing**.

All of these are functions of the static corpus and your own character. None of them
should ever cross the wire. The log parser especially: a 159 MB eqlog is not going
anywhere near a Worker with a 10 ms budget, and it doesn't need to — it's your file, on
your machine, and the browser can read it.

Your inventory belongs here too. It's your dump, parsed locally, kept locally. We never
store it, which is a cost saving and a privacy answer in the same move.

### Needs shared state — small, and D1 handles it
Orders and their lifecycle · workshops (open, terms, hours) · regard · spawn timers ·
LFG posts · raid rosters.

These are narrow rows written a few hundred times a day for a guild. Comfortably inside
5M reads / 100k writes.

### Discord provides, and we do not host
**Voice.** Identity. Presence. Notifications. Message delivery. The client itself.

This is the bit I got wrong in the roadmap. A raid planner needs voice — but it needs to
*orchestrate Discord voice channels*, which is API calls. Discord carries the audio. We
never touch a media server.

---

## 3. So what actually breaks it

Not the raid planner. These:

1. **Serving the corpus from a database instead of shipping it.** One query per item
   lookup, at guild scale, is how 100k requests/day evaporates. The artifact is the
   whole trick.
2. **Any server-side thinking.** Trio cross-analysis is genuinely expensive. On the
   client it's free and instant; on a Worker it exceeds 10 ms and costs money per call.
3. **Storing inventories.** 5 GB is a lot until it's every member's full dump with
   history. Don't. Parse locally, keep locally.
4. **Images.** If the item system ends up serving item icons and zone maps, R2's 10 GB
   fills faster than the data does. Ship icons inside the artifact as a sprite sheet.
5. **Polling.** Any "check every 30 seconds" loop multiplies request count by the guild
   size. Discord will push us events; we should never poll.

---

## 4. When it does cost money

The honest line: **$5 a month**, and only if the guild gets big enough or someone builds
one of the traps above. That buys 10M requests and 30M CPU-ms, which for a guild-scale
app is not a ceiling you can see from here.

The thing that would genuinely change the model is going multi-guild and public. At that
point the corpus is still free — it's a CDN object — but the interactions endpoint and
D1 start to matter, and it becomes a real hosting decision rather than a free-tier trick.

---

## 5. Unresolved

1. Corpus size. A full EQL item + spell corpus is a guess right now. If it's 40 MB the
   range-read design is mandatory; if it's 8 MB we can just send it and cache it.
2. Does the artifact shard by domain (items / spells / zones) or ship as one file? One
   file is simpler to verify; shards mean the phone never fetches spell data to look up
   a bracer.
3. Where does the checklist tick state live — derived from inventory every time, or
   stored? Derived is free and self-healing; stored survives you not uploading a dump.
4. Log parser: browser-only on an uploaded file, or a small resident agent watching the
   folder? The second is much better and is the one thing on the roadmap that wants
   software on the player's machine.

---

**Sources:** [Cloudflare Workers pricing](https://developers.cloudflare.com/workers/platform/pricing/)
