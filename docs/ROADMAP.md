# EQL Grimoire — what else goes in the book

**Status:** planning. Nothing here is built; the nav carries a parked entry for each
so the shape is visible while we decide order.
**Built today:** the broker half — Commission, Item lookup (parked), In flight,
Inventory, Work orders, Sources, Workshop.

---

## 0. The one sentence that has to land first

Every tool on this list is worth building on its own, but the reason to build them
**here** rather than as seven separate sites is that the Grimoire already knows three
things nobody else does: **your inventory**, **your trio and its levels**, and **who
in your guild can make what.** A checklist that knows what you already hold beats a
checklist. A farm route that knows your level beats a drop table.

---

## 1. What the app already has that the others don't

| Fact we hold | Where it came from | What it unlocks |
|---|---|---|
| Your full inventory across all four coffers | `/outputfile inventory` | checklists that tick themselves, "you already have 3 of 5" |
| Your trio and each class's level | profile | levelling routes, gear filtering, trio analysis |
| Who is open for business, and what they can make | workshops | LFG, raid slots, work orders |
| Your regard, and everyone else's | order history | who to trust without a moderator |

Anything on the roadmap that doesn't lean on at least one of those is a tool we're
building for no reason other than that it's missing.

---

## 2. The list, grouped as the nav groups them

### Character
| Tool | What it does | Leans on |
|---|---|---|
| **Trio builder** | proper cross-analysis of a trio — not "these three are popular" but how the kits actually overlap, where the dead weight is, and what it costs you in effective level | trio, levels, AAs, gear |
| **AA planner** | order of purchase against what you actually do | trio, levels |
| **Levelling guide** | takes your trio *and the level of each class in it* and says where to go — the lowest class is the one that needs the XP, which is the bit generic guides miss | trio, levels |
| **Gear upgrades** | what your next step is per slot, filtered to what your trio can wear and what a hand in your guild can make | inventory, trio, workshops |
| **Spell checker** | which of your spells you're missing, and where they come from | trio, levels, inventory |

### The hunt
| Tool | What it does | Leans on |
|---|---|---|
| **Farmer John** | where to go for a given item — an extension of item search, weighted by your level and how long the run takes | inventory, levels |
| **Gotta kill 'em all** | named/rare tracker | — |
| **Zone atlas** | maps, connections, level bands | levels |
| **Log parser** | reads your eqlog — combines, drops, deaths, XP rate — and feeds everything above | *feeds* the rest |

### Raid
| Tool | What it does | Leans on |
|---|---|---|
| **Raid planner** | full PUG raiding through Discord, including voice | workshops, trio |
| **Spawn timers** | shared raid timers | — |
| **Looking for group** | find a group for a farm, using who's actually online and open | workshops, levels |

### Collections
| Tool | What it does | Leans on |
|---|---|---|
| **Checklists** | the Plane of Sky pattern — but the boxes tick themselves from your inventory, and each unticked line links to Farmer John and to a hand who can make it | inventory |

### And the big one
| Tool | What it does |
|---|---|
| **The item system** | an eqitems-equivalent for EQL: every item, every source, every recipe, cross-linked. Everything above is a view onto it. |

---

## 3. Order of work, and why

**The log parser comes first**, even though it's the least exciting thing on the list.
It is the only entry on the roadmap that *produces* data rather than consuming it —
combines feed the success model, drops feed Farmer John, XP feeds the levelling guide.
Build it late and every tool above it is running on a guess.

**Then the item system**, because Farmer John, gear upgrades, checklists and spell
checker are all views onto one dataset. Building them separately means four
half-schemas that disagree.

**Then the character tools** — trio builder, levelling guide, AA planner — because
they share one input and one another's output.

**Raid last.** It needs voice, presence and scheduling, none of which the rest needs,
and it is the only group that fails badly if the guild is small.

---

## 4. Honest risks

- **Data, not code, is the whole job.** Every tool here is a thin view over a dataset
  we do not have yet. The trio builder is a weekend; a trustworthy corpus of EQL spell
  data is not.
- **Scope.** Fifteen tools is a product, not a Discord app. The four already built work
  because they do one loop end to end. The next one should too.
- **The free-hosting constraint still binds.** A raid planner with voice and presence is
  not a static artifact on a CDN. That's the first item on this list that breaks the
  Cloudflare-free-tier model, and it should be costed before it's promised.
- **eqlwiki is one source and it is young.** Anything that depends on complete item or
  spell coverage inherits its gaps.

---

## 5. Open questions

1. Does the log parser run in the browser on an uploaded file, or does it need a
   resident agent watching the folder? The second is far better and far harder.
2. Checklists: per-character, per-account, or per-guild?
3. Is the item system ours to build, or do we bind to eqlwiki and cache?
4. Raid planner — is the guild big enough to need one, or is LFG the real want?
