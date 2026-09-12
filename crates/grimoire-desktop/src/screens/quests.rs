//! Screen: quests. Decision D9: what a quest wants, what your bags hold, and what that leaves.
//!
//! WHAT EACH RULE READS:
//!   the three name keys       an item's identity, decorations stripped, for every lookup here
//!   held counts               what the inventory dump says is in your bags
//!   the step graph            each step of a quest, read off those counts
//!   a quest's components      its shopping list against the same counts
//!   part plans                one turn-in off a hub page, treated as its own quest
//!   the pooled list           every tracked quest's components merged into one list
//!   zone buckets              where an outstanding item comes from, grouped by zone
//!   the turn-in and browser rules  how those lists are sorted and filtered
//!
//! WHAT IS NOT BUILT, AND WHY IT SAYS SO ON SCREEN:
//!   the log ledger (trades that prove a hand-in, buys and combines since the dump, the log
//!   floor under the dump) belongs to the Sky lane's log stream. Without it the only witness is
//!   the inventory dump, so every held count here is the dump's word and the screen says "from
//!   the dump" rather than implying live truth. The geo BFS ordering (zones nearest to where you
//!   stand first) needs the current zone from the log stream, which the shared context does not
//!   carry yet; zone groups fall back to the tie-break order below. The manual "mark as held"
//!   checkbox needs persistence this lane does not own.
//!
//! THE DATA. quest-items.json (schema 6) is read HERE, from the snapshot's root, on a thread,
//! once. The D6 snapshot contract exposes `quests` but not the drops/npcs/geo/src tables this
//! screen is built on, so the file is parsed whole into `QuestBook` with `#[serde(flatten)] extra`
//! on every record. Positional arrays stay positional; each is commented with what the index was
//! observed to hold, and nothing invents a field name for them.

use crate::chrome::State;
use crate::screens::{Ask, Cx};
use crate::theme::*;
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

/* ------------------------------------------------------------- cross-screen jumps -- */

/// The find box asks this screen to open a quest by its title. The selection waits here until
/// this screen draws; the nav switch travels on `Cx.ask` (`Ask::ShowQuest`), as Items, Zones and
/// Drops do it. A whole quest's plan key is its title, so the title selects it directly.
static JUMP: Mutex<Option<String>> = Mutex::new(None);

pub fn jump_to(title: &str) {
    *JUMP.lock().unwrap_or_else(|p| p.into_inner()) = Some(title.to_owned());
}

fn take_jump() -> Option<String> {
    JUMP.lock().unwrap_or_else(|p| p.into_inner()).take()
}

/* ============================================================== the file, as it is on disk == */

/// quest-items.json, schema 6, as measured 2026-09-02 (924 quests, 1637 drop tables, 1149 NPCs,
/// 115 geo nodes, 3058 item sources).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct QuestBook {
    #[serde(default)]
    pub schema: u32,
    /// URL prefix every wiki path hangs off, "https://eqlwiki.com/index.php/".
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub meta: Map<String, Value>,
    #[serde(default)]
    pub quests: Vec<Quest>,
    /// item key -> list of [quest index, kind]. Positional: index 0 is the index into `quests`,
    /// index 1 is "c" (the quest consumes it) or "r" (the quest rewards it).
    #[serde(default)]
    pub items: HashMap<String, Vec<Vec<Value>>>,
    /// item key -> list of [mob name, mob wiki slug, zone name, zone wiki slug]. Positional, all
    /// four observed as strings on every row.
    #[serde(default)]
    pub drops: HashMap<String, Vec<Vec<Value>>>,
    #[serde(default)]
    pub npcs: HashMap<String, Npc>,
    #[serde(default)]
    pub geo: Option<Geo>,
    #[serde(default)]
    pub src: HashMap<String, ItemSource>,
    /// zone display name -> wiki path.
    #[serde(default)]
    pub zones: HashMap<String, String>,
    /// client item key -> wiki title ("bunker cell #1" -> "Bunker Cell No 1").
    #[serde(default)]
    pub alias: HashMap<String, String>,
    /// item key -> list of [client item id, label]. Positional: index 0 the numeric id the dump
    /// prints, index 1 what that id is ("Coin of the Tash - Azia", or "#18160" when unnamed).
    #[serde(default)]
    pub ids: HashMap<String, Vec<Vec<Value>>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Quest {
    #[serde(rename = "n")]
    pub name: String,
    /// The wiki path, and the quest's identity everywhere in this file.
    #[serde(default)]
    pub t: String,
    #[serde(default)]
    pub giver: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(default)]
    pub era: Option<String>,
    /// Out of era: the quest's zone is not in the game right now.
    #[serde(default)]
    pub oe: bool,
    #[serde(default)]
    pub lvl: Option<u32>,
    #[serde(default, rename = "lvlUse")]
    pub lvl_use: Option<u32>,
    #[serde(default)]
    pub classes: Vec<String>,
    #[serde(default)]
    pub items: Vec<String>,
    /// The unexpanded turn-in list, [name, qty]. Schema 6 fills this and leaves `need` empty.
    #[serde(default)]
    pub want: Vec<(String, u32)>,
    /// The older, recipe-expanded list. Empty on every quest in schema 6; kept as the fallback.
    #[serde(default)]
    pub need: Vec<(String, u32)>,
    #[serde(default)]
    pub rewards: Vec<String>,
    #[serde(default, rename = "relatedZones")]
    pub related_zones: Vec<String>,
    #[serde(default, rename = "relatedNpcs")]
    pub related_npcs: Vec<String>,
    #[serde(default)]
    pub parts: Vec<Part>,
    /// True when the page was split into named turn-ins (`parts` is then a real structure).
    #[serde(default)]
    pub split: bool,
    #[serde(default)]
    pub hub: bool,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// item name -> wiki path of the quest that pays it.
    #[serde(default)]
    pub from: HashMap<String, String>,
    /// item name -> "base" | "mid" | "reward" | "tool".
    #[serde(default)]
    pub roles: HashMap<String, String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Part {
    /// Components, [name, qty].
    #[serde(default)]
    pub c: Vec<(String, u32)>,
    /// Who takes them.
    #[serde(default)]
    pub g: Option<String>,
    /// The turn-in's name; "" is the page's leftovers, not a step.
    #[serde(default)]
    pub n: String,
    #[serde(default)]
    pub r: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Step {
    pub i: u32,
    /// give | kill | get | note | collect | combine | ground | say | forage | buy | fish.
    #[serde(default)]
    pub k: String,
    #[serde(default)]
    pub txt: String,
    /// Present (even empty) when the step hands something over. Presence matters to the bag
    /// rule below, which is why this is an Option and not a Vec.
    #[serde(default, rename = "in")]
    pub inputs: Option<Vec<(String, u32)>>,
    #[serde(default)]
    pub out: Vec<(String, u32)>,
    #[serde(default)]
    pub npc: Option<String>,
    #[serde(default)]
    pub npct: Option<String>,
    #[serde(default)]
    pub mob: Option<String>,
    #[serde(default)]
    pub pre: Vec<u32>,
    #[serde(default)]
    pub z: Option<String>,
    #[serde(default)]
    pub loc: Option<String>,
    #[serde(default)]
    pub fac: bool,
    #[serde(default)]
    pub gold: Option<u64>,
    /// A container the combine happens in. Presence matters, same as `inputs`.
    #[serde(default)]
    pub tool: Option<Vec<String>>,
    /// The client item id this step's product is pinned to, when several items share a name.
    #[serde(default)]
    pub iid: Option<u32>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Npc {
    #[serde(default)]
    pub loc: Option<String>,
    #[serde(default)]
    pub t: String,
    #[serde(default)]
    pub z: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Geo {
    #[serde(default)]
    pub alias: HashMap<String, String>,
    #[serde(default)]
    pub nodes: HashMap<String, GeoNode>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GeoNode {
    #[serde(default)]
    pub adj: Vec<String>,
    /// Atlas keys this node covers; more than one means a city page spanning several zones.
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub oe: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ItemSource {
    /// Kind per zone, parallel to `z`: drop | vendor | forage | ground | buy.
    #[serde(default)]
    pub k: Vec<String>,
    #[serde(default)]
    pub z: Vec<String>,
    #[serde(default)]
    pub r: Option<RecipeSrc>,
    /// zone -> mobs that drop it there.
    #[serde(default)]
    pub m: HashMap<String, Vec<String>>,
    /// zone -> list of [merchant, where]. Positional: index 0 the merchant, index 1 the loc.
    #[serde(default)]
    pub v: HashMap<String, Vec<Vec<String>>>,
    /// zone -> ground spawn coordinates.
    #[serde(default)]
    pub g: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub many: bool,
    #[serde(default)]
    pub isl: Option<String>,
    #[serde(default)]
    pub various: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RecipeSrc {
    /// Ingredients, [qty, name].
    #[serde(default)]
    pub c: Vec<(u32, String)>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/* ================================================================== the three name keys == */

/// Item IDENTITY key, everything the datasets are keyed by: backticks and apostrophes out,
/// lower case, whitespace collapsed. NOT article-stripped: "Sapphire" the merchant gem and "A
/// Sapphire" the quest drop are two items the wiki keeps apart on purpose.
pub fn item_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for ch in s.chars() {
        if ch == '`' || ch == '\'' {
            continue;
        }
        if ch.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        for l in ch.to_lowercase() {
            out.push(l);
        }
    }
    out
}

/// Peel the client's decorations, outermost first: trailing `*`, then `(Exaltation)`, then `+N`.
/// "Giant Snake Fang +4" and "Backpack*" find their base entries this way.
pub fn strip_decor(name: &str) -> String {
    let s = name.trim_end_matches('*');
    let s = match s.strip_suffix("(Exaltation)") {
        Some(rest) => rest.trim_end(),
        None => s,
    };
    let s = strip_tier(s);
    s.to_string()
}

fn strip_tier(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i == bytes.len() || i == 0 || bytes[i - 1] != b'+' {
        return s;
    }
    s[..i - 1].trim_end()
}

/// Matching key for a name as the LOG prints it: item_key with one leading article off.
/// Used for lookup fallbacks only, never as identity (see item_key).
pub fn norm_name(s: &str) -> String {
    let k = item_key(s);
    for art in ["a ", "an ", "the "] {
        if let Some(rest) = k.strip_prefix(art) {
            return rest.to_string();
        }
    }
    k
}

/// Mob key: the wiki disambiguates same-named mobs with a parenthetical the game never prints.
pub fn norm_mob(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    norm_name(&out)
}

/// An exaltation stone is named after the item it was rendered FROM, so the row is not one of
/// that item: nothing that answers "how many do I have" may count it.
pub fn is_exalt_stone(name: &str) -> bool {
    name.trim_end_matches('*').ends_with("(Exaltation)")
}

/* ================================================================ the book, indexed == */

/// The file plus the indexes the rules need, built once at load.
#[derive(Debug, Default)]
pub struct Book {
    pub data: QuestBook,
    pub path: PathBuf,
    /// article-stripped key -> src key. Consulted only when the exact key misses.
    src_loose: HashMap<String, String>,
    drops_loose: HashMap<String, String>,
    /// client item id -> what it is, flattened from `ids`.
    id_label: HashMap<u32, String>,
    /// quest path -> index into `quests`.
    by_key: HashMap<String, usize>,
    /// Sorted, de-duplicated, for the filter combos.
    pub classes: Vec<String>,
}

impl Book {
    pub fn load(path: &Path) -> Result<Book, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Book::from_json(&text, path)
    }

    pub fn from_json(text: &str, path: &Path) -> Result<Book, String> {
        let data: QuestBook =
            serde_json::from_str(text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Book::index(data, path.to_path_buf()))
    }

    fn index(data: QuestBook, path: PathBuf) -> Book {
        /* Loose alias per table: article-stripped key -> entry, an article-LESS title winning
         * any tie, so a game name whose article the title lacks is rescued without ever
         * overriding a real item. */
        let loose = |keys: &mut dyn Iterator<Item = &String>| {
            let mut out: HashMap<String, String> = HashMap::new();
            for k in keys {
                let nk = norm_name(k);
                let winner = match out.get(&nk) {
                    None => true,
                    Some(cur) => *cur != nk && *k == nk,
                };
                if winner {
                    out.insert(nk, k.clone());
                }
            }
            out
        };
        let src_loose = loose(&mut data.src.keys());
        let drops_loose = loose(&mut data.drops.keys());
        let mut id_label = HashMap::new();
        for list in data.ids.values() {
            for row in list {
                if let (Some(id), Some(label)) = (
                    row.first().and_then(Value::as_u64),
                    row.get(1).and_then(Value::as_str),
                ) {
                    id_label.insert(id as u32, label.to_string());
                }
            }
        }
        let by_key = data
            .quests
            .iter()
            .enumerate()
            .map(|(i, q)| (q.t.clone(), i))
            .collect();
        let mut classes: Vec<String> = data
            .quests
            .iter()
            .flat_map(|q| q.classes.iter())
            .filter(|c| !c.is_empty() && *c != "?" && *c != "Any")
            .cloned()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        classes.sort();
        Book {
            data,
            path,
            src_loose,
            drops_loose,
            id_label,
            by_key,
            classes,
        }
    }

    pub fn quest(&self, key: &str) -> Option<(usize, &Quest)> {
        self.by_key.get(key).map(|&i| (i, &self.data.quests[i]))
    }

    /// Every key a name answers to: raw, decoration-stripped, and the wiki title the client
    /// name aliases.
    pub fn name_keys(&self, name: &str) -> Vec<String> {
        let mut ks = vec![item_key(name)];
        let stripped = item_key(&strip_decor(name));
        if stripped != ks[0] {
            ks.push(stripped);
        }
        let mut extra = Vec::new();
        for k in &ks {
            if let Some(a) = self.data.alias.get(k) {
                let ak = item_key(a);
                if !ks.contains(&ak) && !extra.contains(&ak) {
                    extra.push(ak);
                }
            }
        }
        ks.extend(extra);
        ks
    }

    pub fn src_for(&self, name: &str) -> Option<&ItemSource> {
        let d = &self.data;
        d.src
            .get(&item_key(name))
            .or_else(|| d.src.get(&item_key(&strip_decor(name))))
            .or_else(|| {
                self.src_loose
                    .get(&norm_name(name))
                    .and_then(|k| d.src.get(k))
            })
            .or_else(|| {
                self.src_loose
                    .get(&norm_name(&strip_decor(name)))
                    .and_then(|k| d.src.get(k))
            })
    }

    pub fn drops_for(&self, name: &str) -> &[Vec<Value>] {
        let d = &self.data;
        d.drops
            .get(&item_key(name))
            .or_else(|| d.drops.get(&item_key(&strip_decor(name))))
            .or_else(|| {
                self.drops_loose
                    .get(&norm_name(name))
                    .and_then(|k| d.drops.get(k))
            })
            .or_else(|| {
                self.drops_loose
                    .get(&norm_name(&strip_decor(name)))
                    .and_then(|k| d.drops.get(k))
            })
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Does the file have the per-item source table at all? A file built before it existed must
    /// not be read as "the wiki lists no source" on every row.
    pub fn has_src(&self) -> bool {
        !self.data.src.is_empty()
    }

    /// A hub page ("Popular Quests by Level") indexes other quests' rewards as its components.
    /// The pipeline flags it `hub`; a file built before the flag is read by shape. Schema 6 fills
    /// `want` and leaves `need` empty, so the shape test reads whichever list the file filled.
    /// Reading `need` alone would never fire on this file.
    pub fn is_hub(q: &Quest) -> bool {
        let listed = if q.want.is_empty() {
            q.need.len()
        } else {
            q.want.len()
        };
        q.hub || (q.giver.is_none() && q.zone.is_none() && q.steps.is_empty() && listed >= 20)
    }

    /// An NPC's zone, from the file's NPC table.
    pub fn npc_zone(&self, npc: &str) -> Option<&str> {
        self.data.npcs.get(npc).and_then(|n| n.z.as_deref())
    }

    /// A geo node carrying more than one atlas key, whose own name is not a zone the log can
    /// report you standing in, is a wiki city page spanning several client zones. You cannot
    /// stand in it, so it is never a destination.
    pub fn is_umbrella(&self, zone: &str, known_zones: &HashSet<String>) -> bool {
        let Some(geo) = &self.data.geo else {
            return false;
        };
        geo.nodes.get(zone).is_some_and(|n| n.keys.len() > 1)
            && !known_zones.contains(&zone.to_lowercase())
    }
}

/* ========================================================== what the bag says you hold == */

/// One row of the dump as this screen needs it: name, client item id, count.
pub type DumpRow = (String, u32, u32);

/// The ONE place this file touches the ingest lane's `InventoryDump` fields: `rows()` walks every
/// row that is not an Empty slot, and each carries `name`, `item_id` (the client's id, as the
/// dump prints it) and `count`. The exaltation test is re-applied on the name below rather than trusting the
/// row's flag, so the rule this screen tests is the rule it runs on.
fn dump_rows(dump: &crate::ingest::InventoryDump) -> Vec<DumpRow> {
    dump.rows()
        .map(|r| (r.name.clone(), r.item_id as u32, r.count))
        .collect()
}

/// item_key -> count held, plus the ids the dump could name. Keyed by item_key and never by
/// norm_name: a dump prints the item's real name, so "Sapphire" and "A Sapphire" stay two rows.
#[derive(Debug, Default)]
pub struct Have {
    counts: HashMap<String, u32>,
    /// Insertion order, which decides ties in the loose lookup.
    order: Vec<String>,
    loose: std::cell::OnceCell<HashMap<String, (u32, usize)>>,
    ids: HashSet<u32>,
}

impl Have {
    pub fn from_rows(rows: &[DumpRow], book: &Book) -> Have {
        let mut h = Have::default();
        for (name, id, count) in rows {
            if *id != 0 && book.id_label.contains_key(id) {
                h.ids.insert(*id);
            }
            if is_exalt_stone(name) {
                continue;
            }
            h.add(name, *count, book);
        }
        h
    }

    pub fn add(&mut self, name: &str, n: u32, book: &Book) {
        for k in book.name_keys(name) {
            match self.counts.get_mut(&k) {
                Some(c) => *c += n,
                None => {
                    self.counts.insert(k.clone(), n);
                    self.order.push(k);
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// How many of a WIKI-named item you hold: exact key, then article-stripped, earliest
    /// insertion winning a tie between the two spellings.
    pub fn held(&self, name: &str) -> u32 {
        if let Some(c) = self.counts.get(&item_key(name)) {
            return *c;
        }
        if let Some(c) = self.counts.get(&item_key(&strip_decor(name))) {
            return *c;
        }
        let ix = self.loose.get_or_init(|| {
            let mut ix = HashMap::new();
            for (i, k) in self.order.iter().enumerate() {
                let nk = norm_name(k);
                ix.entry(nk).or_insert((self.counts[k], i));
            }
            ix
        });
        let a = ix.get(&norm_name(name));
        let b = ix.get(&norm_name(&strip_decor(name)));
        match (a, b) {
            (Some(a), Some(b)) => {
                if a.1 <= b.1 {
                    a.0
                } else {
                    b.0
                }
            }
            (Some(a), None) => a.0,
            (None, Some(b)) => b.0,
            (None, None) => 0,
        }
    }

    /// Copies of `name` in hand that the dump cannot put an id on: the number of pins we cannot
    /// honestly tick.
    pub fn unidentified(&self, name: &str, ids: &[u32]) -> u32 {
        let known = ids.iter().filter(|id| self.ids.contains(id)).count() as u32;
        self.held(name).saturating_sub(known)
    }

    pub fn has_id(&self, id: u32) -> bool {
        self.ids.contains(&id)
    }
}

/* ======================================================================= step state == */

#[derive(Debug, Clone)]
pub struct StepView {
    pub ix: usize,
    pub done: bool,
    pub can: bool,
    pub note: bool,
}

/// The step graph read off the bag: what is done, what you can do now, what is locked.
#[derive(Debug, Clone)]
pub struct StepState {
    pub states: Vec<StepView>,
    pub next: Option<usize>,
    /// The FINAL hand-in is satisfiable: 10 coins in the bag is step 13 of 17, not ready.
    pub handin_ready: bool,
    /// item_key -> copies a proven hand-in must have put in your bags. Always empty without the
    /// log ledger; carried so the ledger can plug in without changing this shape.
    pub gained: HashMap<String, u32>,
    pub unnamed: u32,
    pub done_n: usize,
    pub total: usize,
    pub finished: bool,
}

pub fn step_state(q: &Quest, have: &Have) -> Option<StepState> {
    let steps = &q.steps;
    if steps.is_empty() {
        return None;
    }
    let is_note = |st: &Step| st.k == "note";
    let held = |n: &str| have.held(n);
    let mut done: HashMap<u32, bool> = HashMap::new();
    let outs_held = |st: &Step| !st.out.is_empty() && st.out.iter().all(|(n, _)| held(n) >= 1);

    for st in steps.iter().rev() {
        if is_note(st) {
            continue;
        }
        let consumer_done = steps
            .iter()
            .any(|s2| s2.pre.contains(&st.i) && done.get(&s2.i).copied().unwrap_or(false));
        /* A combine whose product IS its container (fill the casket) cannot be read off the
         * bag: the empty casket is already there. Only the hand-in after it can tick it. */
        let made_new = st
            .out
            .iter()
            .any(|(n, _)| !st.tool.as_ref().is_some_and(|t| t.contains(n)));
        let from_bag = (st.inputs.is_some() || st.tool.is_some()) && made_new && outs_held(st);
        done.insert(st.i, consumer_done || from_bag);
    }

    /* Pins the data can name: a step whose item id the build pinned is done when THAT id is in
     * the dump, never when some same-named item is. Ten Old Silver Coin are ten items. */
    let mut pinned: HashSet<u32> = HashSet::new();
    let mut pinned_items: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for st in steps {
        let Some(iid) = st.iid else { continue };
        if is_note(st) {
            continue;
        }
        pinned.insert(st.i);
        if !done.get(&st.i).copied().unwrap_or(false) {
            done.insert(st.i, have.has_id(iid));
        }
        if let Some((n, _)) = st.out.first() {
            pinned_items.entry(n.clone()).or_default().push(iid);
        }
    }
    let mut unnamed = 0;
    for (n, ids) in &pinned_items {
        unnamed += have.unidentified(n, ids);
    }

    /* Pooled world pickups: the first `held` pins of an item count as done. */
    let mut pools: BTreeMap<String, Vec<&Step>> = BTreeMap::new();
    for st in steps {
        if st.inputs.is_some() || st.tool.is_some() || st.out.is_empty() {
            continue;
        }
        if done.get(&st.i).copied().unwrap_or(false) || pinned.contains(&st.i) {
            continue;
        }
        pools.entry(st.out[0].0.clone()).or_default().push(st);
    }
    for (n, pins) in &pools {
        let need_q = steps
            .iter()
            .filter_map(|s2| s2.inputs.as_ref())
            .flat_map(|ins| ins.iter().filter(|(m, _)| m == n).map(|(_, w)| *w))
            .max()
            .unwrap_or(0)
            .max(1);
        if held(n) >= need_q {
            for st in pins {
                done.insert(st.i, true);
            }
            continue;
        }
        let already = steps
            .iter()
            .filter(|s2| {
                s2.out.iter().any(|(m, _)| m == n) && done.get(&s2.i).copied().unwrap_or(false)
            })
            .count() as i64;
        let mut k = (held(n) as i64 - already).clamp(0, pins.len() as i64);
        for st in pins {
            if k <= 0 {
                break;
            }
            done.insert(st.i, true);
            k -= 1;
        }
    }

    let is_done = |i: u32| done.get(&i).copied().unwrap_or(false);
    let states: Vec<StepView> = steps
        .iter()
        .enumerate()
        .map(|(ix, st)| StepView {
            ix,
            done: is_done(st.i),
            can: !is_note(st) && !is_done(st.i) && st.pre.iter().all(|p| is_done(*p)),
            note: is_note(st),
        })
        .collect();
    let next = states
        .iter()
        .find(|s| s.can && matches!(steps[s.ix].k.as_str(), "give" | "combine"))
        .or_else(|| states.iter().find(|s| s.can))
        .map(|s| s.ix);
    let final_give = steps
        .iter()
        .rev()
        .find(|st| st.k == "give" && st.inputs.as_ref().is_some_and(|i| !i.is_empty()));
    let handin_ready = final_give.is_some_and(|fg| {
        !is_done(fg.i)
            && fg
                .inputs
                .as_ref()
                .unwrap()
                .iter()
                .all(|(n, w)| held(n) >= *w)
            && fg.pre.iter().all(|p| is_done(*p))
    });
    let real: Vec<&StepView> = states.iter().filter(|s| !s.note).collect();
    let done_n = real.iter().filter(|s| s.done).count();
    let total = real.len();
    Some(StepState {
        next,
        handin_ready,
        gained: HashMap::new(),
        unnamed,
        done_n,
        total,
        finished: total > 0 && done_n == total,
        states,
    })
}

/* ================================================================= the shopping list == */

/// Where a chain-made item is obtained: the step's NPC and what you do there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSrc {
    pub npc: String,
    pub kind: String,
    pub zone: String,
}

#[derive(Debug, Clone, Default)]
pub struct Comp {
    pub name: String,
    pub want: u32,
    pub have: u32,
    /// An ingredient standing in for something you make: what it is for.
    pub via: Option<String>,
    /// Wiki path of the quest that pays this item, when it is another quest's reward.
    pub from: Option<String>,
    pub step: Option<StepSrc>,
    /// Every producing step is locked behind unmet prereqs: not something to go get yet.
    pub locked: bool,
    /// Steps pinned to item ids: only the zones whose own copy is still missing.
    pub zones_pinned: Option<Vec<String>>,
    /// Set by merge_plans: which quests want it.
    pub tag: Option<String>,
    pub tip: Option<String>,
}

impl Comp {
    pub fn done(&self) -> bool {
        self.have >= self.want
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    /// `q.t`, or `q.t::partIndex` for one turn-in off a hub page.
    pub key: String,
    pub quest_ix: usize,
    pub part: Option<usize>,
    pub name: String,
    /// Short name for tags: a tracked turn-in is "Paladin Test of Sacrifice", never the hub.
    pub short: String,
    pub giver: Option<String>,
    pub rewards: Vec<String>,
    pub comps: Vec<Comp>,
    pub got: usize,
    pub need: usize,
    pub done: bool,
    pub steps: Option<StepState>,
}

impl Plan {
    /// A chain quest is READY when the FINAL hand-in is satisfiable, not when the base
    /// materials are pocketed.
    pub fn ready(&self) -> bool {
        match &self.steps {
            Some(ss) => ss.handin_ready,
            None => self.done,
        }
    }
    pub fn base_key(&self) -> &str {
        self.key.split("::").next().unwrap_or(&self.key)
    }
}

/// An item you can only make (no world source) expands into ingredients for the SHORTFALL only.
fn recipe_for<'a>(book: &'a Book, name: &str) -> Option<&'a RecipeSrc> {
    let s = book.src_for(name)?;
    let r = s.r.as_ref()?;
    if r.c.is_empty() {
        return None;
    }
    if !s.z.is_empty() || s.many || s.various {
        return None;
    }
    Some(r)
}

pub fn comps_for(q: &Quest, quest_ix: usize, have: &Have, book: &Book) -> Plan {
    let ss = step_state(q, have);
    let held = |n: &str| {
        have.held(n)
            + ss.as_ref()
                .and_then(|s| s.gained.get(&item_key(n)))
                .copied()
                .unwrap_or(0)
    };

    let mut made_by: HashMap<String, Vec<usize>> = HashMap::new();
    let mut consumed: HashSet<String> = HashSet::new();
    for (ix, st) in q.steps.iter().enumerate() {
        for (n, _) in &st.out {
            made_by.entry(item_key(n)).or_default().push(ix);
        }
        for (n, _) in st.inputs.iter().flatten() {
            consumed.insert(item_key(n));
        }
    }
    let state_of = |ix: usize| ss.as_ref().and_then(|s| s.states.get(ix));

    let top: Vec<(String, u32)> = if !q.want.is_empty() {
        q.want.clone()
    } else if !q.need.is_empty() {
        q.need.clone()
    } else {
        q.items.iter().map(|n| (n.clone(), 1)).collect()
    };
    let mut wants: Vec<(String, u32)> = Vec::new();
    let mut want = |n: &str, w: u32| {
        let k = item_key(n);
        match wants.iter_mut().find(|(m, _)| item_key(m) == k) {
            Some(cur) => cur.1 += w,
            None => wants.push((n.to_string(), w)),
        }
    };
    match &ss {
        Some(s) => {
            for sv in &s.states {
                if sv.done {
                    continue;
                }
                for (n, w) in q.steps[sv.ix].inputs.iter().flatten() {
                    want(n, *w);
                }
            }
            for (n, w) in &top {
                let k = item_key(n);
                if !consumed.contains(&k) && !made_by.contains_key(&k) {
                    want(n, *w);
                }
            }
        }
        None => {
            for (n, w) in &top {
                want(n, *w);
            }
        }
    }

    let mut rows: Vec<Comp> = Vec::new();
    let put = |rows: &mut Vec<Comp>, n: &str, w: u32, via: Option<&str>| {
        let k = item_key(n);
        if let Some(cur) = rows.iter_mut().find(|c| item_key(&c.name) == k) {
            cur.want += w;
            if via.is_some() && cur.via.as_deref() != via {
                cur.via = None;
            }
            return;
        }
        let makers: Vec<usize> = made_by.get(&k).cloned().unwrap_or_default();
        let giver = makers
            .iter()
            .map(|&ix| &q.steps[ix])
            .find(|st| st.npc.is_some() && matches!(st.k.as_str(), "give" | "get" | "buy"));
        let step = giver.map(|st| StepSrc {
            npc: st.npc.clone().unwrap_or_default(),
            kind: st.k.clone(),
            zone: st
                .z
                .clone()
                .or_else(|| {
                    st.npc
                        .as_deref()
                        .and_then(|p| book.npc_zone(p))
                        .map(str::to_string)
                })
                .unwrap_or_default(),
        });
        let states: Vec<&StepView> = makers.iter().filter_map(|&ix| state_of(ix)).collect();
        let locked = !states.is_empty() && states.iter().all(|s| !s.done && !s.can);
        let pins: Vec<usize> = makers
            .iter()
            .copied()
            .filter(|&ix| q.steps[ix].iid.is_some() && q.steps[ix].z.is_some())
            .collect();
        let zones_pinned = if pins.len() > 1 && pins.len() == makers.len() {
            let mut zs: Vec<String> = Vec::new();
            for ix in pins {
                if state_of(ix).is_some_and(|s| s.done) {
                    continue;
                }
                let z = q.steps[ix].z.clone().unwrap_or_default();
                if !zs.contains(&z) {
                    zs.push(z);
                }
            }
            if zs.is_empty() {
                None
            } else {
                Some(zs)
            }
        } else {
            None
        };
        rows.push(Comp {
            name: n.to_string(),
            want: w,
            have: held(n),
            via: via.map(str::to_string),
            from: q.from.get(n).cloned(),
            step,
            locked,
            zones_pinned,
            tag: None,
            tip: None,
        });
    };
    /* The recipe roll-up, against the bag. A step makes it, or another quest pays it: the chain
     * says how, no roll-up. Otherwise held copies cover their share and only the shortfall costs
     * ingredients, three levels deep at most. */
    type PutFn<'a> = dyn Fn(&mut Vec<Comp>, &str, u32, Option<&str>) + 'a;
    struct Expander<'a> {
        q: &'a Quest,
        book: &'a Book,
        made_by: &'a HashMap<String, Vec<usize>>,
        held: &'a (dyn Fn(&str) -> u32 + 'a),
        put: &'a PutFn<'a>,
    }
    impl Expander<'_> {
        fn expand(&self, rows: &mut Vec<Comp>, n: &str, w: u32, depth: u32, via: Option<&str>) {
            let k = item_key(n);
            if self.made_by.contains_key(&k) || self.q.from.contains_key(n) || depth >= 3 {
                return (self.put)(rows, n, w, via);
            }
            let short = w as i64 - (self.held)(n) as i64;
            let r = if short > 0 {
                recipe_for(self.book, n)
            } else {
                None
            };
            let Some(r) = r else {
                return (self.put)(rows, n, w, via);
            };
            for (qn, ing) in &r.c {
                self.expand(rows, ing, qn * short as u32, depth + 1, Some(n));
            }
        }
    }
    let ex = Expander {
        q,
        book,
        made_by: &made_by,
        held: &held,
        put: &put,
    };
    for (n, w) in &wants {
        ex.expand(&mut rows, n, *w, 0, None);
    }
    let got = rows.iter().filter(|c| c.done()).count();
    let need = rows.len();
    Plan {
        key: q.t.clone(),
        quest_ix,
        part: None,
        name: q.name.clone(),
        short: q.name.clone(),
        giver: q.giver.clone(),
        rewards: q.rewards.clone(),
        comps: rows,
        got,
        need,
        done: need > 0 && got == need,
        steps: ss,
    }
}

/// One turn-in off a hub page as a quest of its own: that part's items only. The Plane of Sky
/// test pages are five quests wearing one URL, and tracking one must not drag the other four.
pub fn part_plan(q: &Quest, quest_ix: usize, pi: usize, have: &Have) -> Option<Plan> {
    let p = q.parts.get(pi)?;
    let comps: Vec<Comp> =
        p.c.iter()
            .map(|(n, w)| Comp {
                name: n.clone(),
                want: *w,
                have: have.held(n),
                from: q.from.get(n).cloned(),
                ..Default::default()
            })
            .collect();
    let got = comps.iter().filter(|c| c.done()).count();
    let need = comps.len();
    let short = if p.n.is_empty() {
        format!("Turn-in {}", pi + 1)
    } else {
        p.n.clone()
    };
    Some(Plan {
        key: format!("{}::{pi}", q.t),
        quest_ix,
        part: Some(pi),
        name: format!("{short} ({})", q.name),
        short,
        giver: p.g.clone().or_else(|| q.giver.clone()),
        rewards: p.r.clone(),
        comps,
        got,
        need,
        done: need > 0 && got == need,
        steps: None,
    })
}

/// Pool tracked plans into one shopping list: an item two quests want is one thing to farm, at
/// the larger requirement. The tag names the quest by its SHORT name.
pub fn merge_plans(plans: &[&Plan]) -> Vec<Comp> {
    struct M {
        comp: Comp,
        quests: Vec<(String, String)>,
    }
    let mut merged: Vec<(String, M)> = Vec::new();
    for p in plans {
        let name = p.short.clone();
        let rew = p
            .rewards
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        for c in &p.comps {
            let k = item_key(&c.name);
            if let Some((_, cur)) = merged.iter_mut().find(|(mk, _)| *mk == k) {
                cur.comp.want = cur.comp.want.max(c.want);
                if !cur.quests.iter().any(|(n, _)| *n == name) {
                    cur.quests.push((name.clone(), rew.clone()));
                }
                /* One quest chain-makes it, another farms it: the pooled row cannot claim the
                 * chain, and it hides only when EVERY quest that wants it says locked. */
                if cur.comp.step.is_some() != c.step.is_some() {
                    cur.comp.step = None;
                }
                match (&mut cur.comp.zones_pinned, &c.zones_pinned) {
                    (Some(a), Some(b)) => {
                        for z in b {
                            if !a.contains(z) {
                                a.push(z.clone());
                            }
                        }
                    }
                    (a, _) => *a = None,
                }
                cur.comp.locked = cur.comp.locked && c.locked;
            } else {
                merged.push((
                    k,
                    M {
                        comp: Comp {
                            via: None,
                            from: None,
                            tag: None,
                            tip: None,
                            ..c.clone()
                        },
                        quests: vec![(name.clone(), rew.clone())],
                    },
                ));
            }
        }
    }
    merged
        .into_iter()
        .map(|(_, m)| {
            let names: Vec<&str> = m.quests.iter().map(|(n, _)| n.as_str()).collect();
            let tip = m
                .quests
                .iter()
                .map(|(n, r)| {
                    if r.is_empty() {
                        n.clone()
                    } else {
                        format!("{n}: {r}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let mut c = m.comp;
            c.tag = Some(if names.len() > 1 {
                format!("{} quests", names.len())
            } else {
                names[0].to_string()
            });
            c.tip = if tip.is_empty() { None } else { Some(tip) };
            c
        })
        .collect()
}

/* ============================================================ where an item comes from == */

/// "::" cannot collide with a zone name.
pub const MANY: &str = "::many";
pub const NOSRC: &str = "::nosrc";

pub fn bucket_label(z: &str) -> Option<&'static str> {
    match z {
        MANY => Some("Anywhere, many zones"),
        NOSRC => Some("Source not listed on the wiki"),
        _ => None,
    }
}

/// Every item lands in exactly one bucket per zone it names. Items the wiki puts in five or more
/// zones get one bucket of their own; items with no stated source get another, because
/// "everywhere" and "the wiki does not say" are different answers.
pub fn buckets_for(book: &Book, name: &str) -> Vec<String> {
    let Some(s) = book.src_for(name) else {
        return vec![NOSRC.to_string()];
    };
    if s.many {
        return vec![MANY.to_string()];
    }
    if !s.z.is_empty() {
        return s.z.clone();
    }
    vec![NOSRC.to_string()]
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceNote {
    pub mobs: Vec<String>,
    pub isle: Option<String>,
    pub note: Option<String>,
}

/// How you get this item IN this zone: the mobs to kill, or, when the zone is on the item's
/// sold-by or foraged list, what to do instead. A ground spawn gets its coordinates.
pub fn source_in(book: &Book, name: &str, zone: &str) -> SourceNote {
    let Some(s) = book.src_for(name) else {
        return SourceNote::default();
    };
    let isle = s.isl.as_ref().map(|i| format!("Isle {i}"));
    if let Some(mobs) = s.m.get(zone).filter(|m| !m.is_empty()) {
        return SourceNote {
            mobs: mobs.clone(),
            isle,
            note: None,
        };
    }
    if let Some(locs) = s.g.get(zone).filter(|l| !l.is_empty()) {
        let at: Vec<&str> = locs.iter().take(2).map(String::as_str).collect();
        return SourceNote {
            mobs: vec![],
            isle,
            note: Some(format!("ground spawn, {}", at.join(" / "))),
        };
    }
    if let Some(v) = s.v.get(zone).and_then(|v| v.first()) {
        let npc = v.first().cloned().unwrap_or_default();
        let at = v.get(1).cloned().unwrap_or_default();
        let note = if at.is_empty() {
            format!("sold by {npc}")
        } else {
            format!("sold by {npc}, {at}")
        };
        return SourceNote {
            mobs: vec![],
            isle,
            note: Some(note),
        };
    }
    let kind =
        s.z.iter()
            .position(|z| z == zone)
            .and_then(|i| s.k.get(i))
            .map(String::as_str);
    let note = match kind {
        Some("vendor") => Some("from a merchant".to_string()),
        Some("forage") => Some("forage".to_string()),
        Some("buy") => Some("buy it".to_string()),
        _ => None,
    };
    SourceNote {
        mobs: vec![],
        isle,
        note,
    }
}

/// How a chain-made row is obtained.
pub fn step_note(st: &StepSrc) -> String {
    match st.kind.as_str() {
        "get" => format!("from {}", st.npc),
        "buy" => format!("buy from {}", st.npc),
        _ => format!("turn-in at {}", st.npc),
    }
}

#[derive(Debug, Clone)]
pub struct Bucket {
    pub zone: String,
    pub items: Vec<Comp>,
    pub umbrella: bool,
    pub left: usize,
}

/// Rows -> ordered zone buckets. Zones with something outstanding outrank cleared ones, the two
/// catch-all buckets sit at the bottom whatever they hold, and an unresolved city sinks below
/// the zones you can actually walk to. Ordering by distance from where you stand needs the
/// current zone, which the shared context does not carry yet, so ties fall to more-left-first,
/// then name.
pub fn zone_buckets(rows: &[Comp], book: &Book, known_zones: &HashSet<String>) -> Vec<Bucket> {
    let mut by: Vec<(String, Vec<Comp>)> = Vec::new();
    for r in rows {
        if r.locked {
            continue;
        }
        let zs: Vec<String> = match (&r.step, &r.zones_pinned) {
            (Some(st), _) if !st.zone.is_empty() => vec![st.zone.clone()],
            (_, Some(zs)) if !zs.is_empty() => zs.clone(),
            _ => buckets_for(book, &r.name),
        };
        for z in zs {
            match by.iter_mut().find(|(k, _)| *k == z) {
                Some((_, items)) => items.push(r.clone()),
                None => by.push((z, vec![r.clone()])),
            }
        }
    }
    let mut out: Vec<Bucket> = by
        .into_iter()
        .map(|(zone, mut items)| {
            items.sort_by(|a, b| {
                a.done()
                    .cmp(&b.done())
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            let left = items.iter().filter(|r| !r.done()).count();
            let umbrella = book.is_umbrella(&zone, known_zones);
            Bucket {
                zone,
                items,
                umbrella,
                left,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        let ca = bucket_label(&a.zone).is_some();
        let cb = bucket_label(&b.zone).is_some();
        ca.cmp(&cb)
            .then_with(|| a.umbrella.cmp(&b.umbrella))
            .then_with(|| (b.left > 0).cmp(&(a.left > 0)))
            .then_with(|| b.left.cmp(&a.left))
            .then_with(|| a.zone.cmp(&b.zone))
    });
    out
}

/* ====================================================================== filters, sorts == */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Era {
    #[default]
    Any,
    In,
    Out,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    pub q: String,
    pub cls: String,
    pub zone: String,
    pub era: Era,
    pub lvl_min: u32,
    pub lvl_max: u32,
}

fn has_needle(fields: &[&str], needle: &str) -> bool {
    fields.iter().any(|s| s.to_lowercase().contains(needle))
}

/// The non-search filters, shared by quest rows and their part rows.
pub fn qb_filters(q: &Quest, f: &Filter) -> bool {
    // a quest with no class list is open to everyone: it passes any class filter
    if !f.cls.is_empty()
        && !q.classes.is_empty()
        && !q.classes.iter().any(|c| *c == f.cls || c == "Any")
    {
        return false;
    }
    if !f.zone.is_empty() && q.zone.as_deref() != Some(f.zone.as_str()) {
        return false;
    }
    if f.era == Era::In && q.oe {
        return false;
    }
    if f.era == Era::Out && !q.oe {
        return false;
    }
    if (f.lvl_min > 0 || f.lvl_max > 0) && q.lvl.is_none() {
        return false;
    }
    if let Some(l) = q.lvl {
        if f.lvl_min > 0 && l < f.lvl_min {
            return false;
        }
        if f.lvl_max > 0 && l > f.lvl_max {
            return false;
        }
    }
    true
}

pub fn qb_match(q: &Quest, f: &Filter) -> bool {
    let needle = f.q.trim().to_lowercase();
    if !needle.is_empty() {
        let mut fields: Vec<&str> = vec![&q.name];
        fields.extend(q.giver.as_deref());
        fields.extend(q.zone.as_deref());
        fields.extend(q.era.as_deref());
        fields.extend(q.items.iter().map(String::as_str));
        fields.extend(q.rewards.iter().map(String::as_str));
        fields.extend(q.related_zones.iter().map(String::as_str));
        fields.extend(q.related_npcs.iter().map(String::as_str));
        fields.extend(q.classes.iter().map(String::as_str));
        if !has_needle(&fields, &needle) {
            return false;
        }
    }
    qb_filters(q, f)
}

/// A hub page's turn-in matches on ITS OWN name, giver, items and reward: "aldryn" must surface
/// "Paladin Test of Sacrifice", not just the hub.
pub fn qb_match_part(q: &Quest, p: &Part, f: &Filter) -> bool {
    let needle = f.q.trim().to_lowercase();
    if !needle.is_empty() {
        let mut fields: Vec<&str> = vec![&p.n, &q.name];
        fields.extend(p.g.as_deref());
        fields.extend(p.c.iter().map(|(n, _)| n.as_str()));
        fields.extend(p.r.iter().map(String::as_str));
        if !has_needle(&fields, &needle) {
            return false;
        }
    }
    qb_filters(q, f)
}

/// Turn-ins: what you hold something for, ready first, in-era before out-of-era, then by how
/// complete, then by size (a 4-of-4 outranks a 1-of-1), then name.
pub fn sort_turnins(plans: &mut [Plan], book: &Book) {
    plans.sort_by(|a, b| {
        let qa = &book.data.quests[a.quest_ix];
        let qb = &book.data.quests[b.quest_ix];
        b.done
            .cmp(&a.done)
            .then_with(|| qa.oe.cmp(&qb.oe))
            .then_with(|| {
                let ra = a.got as f64 / a.need.max(1) as f64;
                let rb = b.got as f64 / b.need.max(1) as f64;
                rb.partial_cmp(&ra).unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| b.need.cmp(&a.need))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Name,
    Lvl,
    Zone,
    Era,
    Held,
}

pub fn sort_rows(rows: &mut [Plan], book: &Book, key: SortKey, ascending: bool) {
    rows.sort_by(|a, b| {
        let qa = &book.data.quests[a.quest_ix];
        let qb = &book.data.quests[b.quest_ix];
        let ord = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Lvl => qa.lvl.unwrap_or(999).cmp(&qb.lvl.unwrap_or(999)),
            SortKey::Zone => {
                let za = qa
                    .zone
                    .as_deref()
                    .map(str::to_lowercase)
                    .unwrap_or_else(|| "\u{ffff}".into());
                let zb = qb
                    .zone
                    .as_deref()
                    .map(str::to_lowercase)
                    .unwrap_or_else(|| "\u{ffff}".into());
                za.cmp(&zb)
            }
            SortKey::Era => {
                let ea = format!(
                    "{}{}",
                    if qa.oe { "z" } else { "a" },
                    qa.era.as_deref().unwrap_or("")
                );
                let eb = format!(
                    "{}{}",
                    if qb.oe { "z" } else { "a" },
                    qb.era.as_deref().unwrap_or("")
                );
                ea.cmp(&eb)
            }
            SortKey::Held => {
                let h = |p: &Plan| {
                    if p.need > 0 {
                        -(p.got as f64 / p.need as f64 + p.need as f64 / 1000.0)
                    } else {
                        1.0
                    }
                };
                h(a).partial_cmp(&h(b)).unwrap_or(std::cmp::Ordering::Equal)
            }
        };
        let ord = if ascending { ord } else { ord.reverse() };
        ord.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

pub fn era_short(e: &str) -> String {
    let t = e.trim();
    let t = t
        .strip_suffix("Era")
        .or_else(|| t.strip_suffix("era"))
        .unwrap_or(t);
    t.trim_end().to_string()
}

/// The facts line: out of era leads, because it changes what the whole row means.
pub fn quest_facts(q: &Quest, chain: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if q.oe {
        let era = q.era.as_deref().map(era_short).unwrap_or_default();
        parts.push(format!(
            "out of era, {}",
            if era.is_empty() { "?".into() } else { era }
        ));
    }
    if let Some(l) = q.lvl {
        match q.lvl_use {
            Some(u) => parts.push(format!("lvl {l} (use {u})")),
            None => parts.push(format!("lvl {l}")),
        }
    }
    let cls: Vec<&str> = q
        .classes
        .iter()
        .filter(|c| !c.is_empty() && *c != "?")
        .map(String::as_str)
        .collect();
    if !cls.is_empty() {
        parts.push(cls.join("/"));
    }
    if let Some(z) = &q.zone {
        parts.push(z.clone());
    }
    if let Some(g) = &q.giver {
        parts.push(format!("to {g}"));
    }
    if chain {
        parts.push("multi-step chain, no single turn-in".into());
    }
    parts.join(" · ")
}

/* ================================================================== plans, cached == */

struct PlanCache {
    sig: u64,
    /// Every quest's plan, by quest index.
    quests: Vec<Plan>,
    /// Every named, non-empty part of a split page, by key.
    parts: HashMap<String, Plan>,
}

fn rows_sig(rows: &[DumpRow]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rows.len().hash(&mut h);
    for (n, id, c) in rows {
        n.hash(&mut h);
        id.hash(&mut h);
        c.hash(&mut h);
    }
    h.finish()
}

fn build_cache(book: &Book, have: &Have, sig: u64) -> PlanCache {
    let quests: Vec<Plan> = book
        .data
        .quests
        .iter()
        .enumerate()
        .map(|(i, q)| comps_for(q, i, have, book))
        .collect();
    let mut parts = HashMap::new();
    for (i, q) in book.data.quests.iter().enumerate() {
        if !q.split {
            continue;
        }
        for (pi, p) in q.parts.iter().enumerate() {
            if p.n.is_empty() || p.c.is_empty() {
                continue;
            }
            if let Some(plan) = part_plan(q, i, pi, have) {
                parts.insert(plan.key.clone(), plan);
            }
        }
    }
    PlanCache { sig, quests, parts }
}

/* ======================================================================= the screen == */

enum Load {
    None,
    Loading {
        root: PathBuf,
        rx: Receiver<Result<Book, String>>,
    },
    Ready {
        root: PathBuf,
    },
    Failed {
        root: PathBuf,
        why: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum View {
    #[default]
    Turnins,
    ByZone,
    ByGiver,
    All,
}

pub struct QuestsScreen {
    load: Load,
    book: Option<Arc<Book>>,
    cache: Option<Arc<PlanCache>>,
    view: View,
    filter: Filter,
    sort: SortKey,
    ascending: bool,
    ready_only: bool,
    selected: Option<String>,
    group: Option<String>,
    /// Plan keys, in the order the player tracked them. This run only: saving them needs a
    /// settings field this lane does not own, and the screen says so.
    tracked: Vec<String>,
    /// Quests whose locked steps are unfolded.
    open_locked: HashSet<String>,
}

impl Default for QuestsScreen {
    fn default() -> Self {
        Self {
            load: Load::None,
            book: None,
            cache: None,
            view: View::Turnins,
            filter: Filter::default(),
            sort: SortKey::Name,
            ascending: true,
            ready_only: false,
            selected: None,
            group: None,
            tracked: Vec::new(),
            open_locked: HashSet::new(),
        }
    }
}

const ROW_H: f32 = 36.0;

impl QuestsScreen {
    fn ensure_loaded(&mut self, ctx: &egui::Context, root: &Path) {
        let same = match &self.load {
            Load::None => false,
            Load::Loading { root: r, .. }
            | Load::Ready { root: r }
            | Load::Failed { root: r, .. } => r == root,
        };
        if !same {
            let path = root.join("quest-items.json");
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(Book::load(&path));
            });
            self.load = Load::Loading {
                root: root.to_path_buf(),
                rx,
            };
            self.book = None;
            self.cache = None;
        }
        if let Load::Loading { root, rx } = &self.load {
            match rx.try_recv() {
                Ok(Ok(book)) => {
                    self.book = Some(Arc::new(book));
                    self.load = Load::Ready { root: root.clone() };
                }
                Ok(Err(why)) => {
                    self.load = Load::Failed {
                        root: root.clone(),
                        why,
                    };
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.load = Load::Failed {
                        root: root.clone(),
                        why: format!(
                            "{}: the reader thread ended without an answer",
                            root.join("quest-items.json").display()
                        ),
                    };
                }
            }
        }
    }

    fn plans_for(&mut self, book: &Arc<Book>, rows: &[DumpRow]) -> Arc<PlanCache> {
        let sig = rows_sig(rows);
        if let Some(c) = &self.cache {
            if c.sig == sig {
                return c.clone();
            }
        }
        let have = Have::from_rows(rows, book);
        let c = Arc::new(build_cache(book, &have, sig));
        self.cache = Some(c.clone());
        c
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let Some(data) = cx.data else {
            problem(ui, cx.data_err.unwrap_or("the snapshot is not loaded"));
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "Quests need quest-items.json from the data snapshot. Put data/ beside the \
                 executable; the Settings screen lists every folder the loader tries, in order.",
                )
                .color(TEXT_2),
            );
            return;
        };
        let root = data.report().root;
        self.ensure_loaded(ui.ctx(), &root);
        let book = match (&self.load, &self.book) {
            (Load::Ready { .. }, Some(b)) => b.clone(),
            (Load::Loading { root, .. }, _) => {
                ui.horizontal(|ui| {
                    state_square(
                        ui.painter(),
                        ui.cursor().left_top() + Vec2::new(5.0, 9.0),
                        State::Working,
                    );
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "reading {}",
                            root.join("quest-items.json").display()
                        ))
                        .font(FontId::monospace(11.5))
                        .color(TEXT_2),
                    );
                });
                return;
            }
            (Load::Failed { why, .. }, _) => {
                problem(ui, why);
                return;
            }
            _ => return,
        };

        let (rows, bag_from): (Vec<DumpRow>, Option<String>) = match cx.ingest.inventory() {
            Some(d) => {
                let who = d
                    .character
                    .as_deref()
                    .map(|c| format!(" ({c})"))
                    .unwrap_or_default();
                let when = d
                    .modified
                    .map(|m| format!(", dump written {}", m.format("%Y-%m-%d %H:%M")))
                    .unwrap_or_default();
                (
                    dump_rows(d),
                    Some(format!("{}{who}{when}", d.path.display())),
                )
            }
            None => (Vec::new(), None),
        };
        let have_rows = !rows.is_empty();
        let plans = self.plans_for(&book, &rows);
        let have = Have::from_rows(&rows, &book);
        if let Some(title) = take_jump() {
            /* A jump lands on the whole quest, whichever view is up; the filters are left alone
             * because the detail pane shows the selection regardless of them. */
            if plans.quests.iter().any(|p| p.key == title) {
                self.selected = Some(title);
            } else {
                self.selected = None;
            }
        }
        let known_zones: HashSet<String> =
            data.zones.iter().map(|z| z.name.to_lowercase()).collect();

        /* The strip: views, search, filters. */
        ui.horizontal(|ui| {
            for (v, label) in [
                (View::Turnins, "Turn-ins"),
                (View::ByZone, "By zone"),
                (View::ByGiver, "By giver"),
                (View::All, "All quests"),
            ] {
                let on = self.view == v;
                let txt = egui::RichText::new(label).color(if on { FLARE } else { TEXT_2 });
                if ui
                    .add(
                        egui::Button::new(txt)
                            .fill(if on {
                                PANEL_2
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(Stroke::NONE),
                    )
                    .clicked()
                {
                    self.view = v;
                    self.group = None;
                }
            }
            ui.add_space(12.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.filter.q)
                    .hint_text("search quests, givers, items, rewards")
                    .desired_width(260.0)
                    .font(FontId::monospace(12.0)),
            );
            if self.view == View::Turnins {
                ui.checkbox(&mut self.ready_only, "ready only");
            }
        });
        ui.horizontal(|ui| {
            /* every combo takes the rail's bar as its open marker (`chrome::combo_icon`), never
             * egui's triangle, which would be a chevron: a third shape in a two-shape vocabulary */
            egui::ComboBox::from_id_salt("quests_era")
                .icon(crate::chrome::combo_icon)
                .selected_text(match self.filter.era {
                    Era::Any => "any era",
                    Era::In => "in era",
                    Era::Out => "out of era",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter.era, Era::Any, "any era");
                    ui.selectable_value(&mut self.filter.era, Era::In, "in era");
                    ui.selectable_value(&mut self.filter.era, Era::Out, "out of era");
                });
            egui::ComboBox::from_id_salt("quests_cls")
                .icon(crate::chrome::combo_icon)
                .selected_text(if self.filter.cls.is_empty() {
                    "any class"
                } else {
                    self.filter.cls.as_str()
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter.cls, String::new(), "any class");
                    for c in &book.classes {
                        ui.selectable_value(&mut self.filter.cls, c.clone(), c);
                    }
                });
            if self.view != View::ByZone {
                /* The zone list follows the era filter: "in era" never names a locked zone. */
                let mut zones: Vec<&str> = book
                    .data
                    .quests
                    .iter()
                    .filter(|q| match self.filter.era {
                        Era::Any => true,
                        Era::In => !q.oe,
                        Era::Out => q.oe,
                    })
                    .filter_map(|q| q.zone.as_deref())
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                zones.sort();
                egui::ComboBox::from_id_salt("quests_zone")
                    .icon(crate::chrome::combo_icon)
                    .selected_text(if self.filter.zone.is_empty() {
                        "any zone"
                    } else {
                        self.filter.zone.as_str()
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.filter.zone, String::new(), "any zone");
                        for z in zones {
                            ui.selectable_value(&mut self.filter.zone, z.to_string(), z);
                        }
                    });
            }
            ui.label(egui::RichText::new("lvl").color(TEXT_3));
            ui.add(egui::DragValue::new(&mut self.filter.lvl_min).range(0..=70));
            ui.label(egui::RichText::new("to").color(TEXT_3));
            ui.add(egui::DragValue::new(&mut self.filter.lvl_max).range(0..=70));
            if self.view == View::All {
                ui.add_space(12.0);
                ui.label(egui::RichText::new("sort").color(TEXT_3));
                for (k, label) in [
                    (SortKey::Name, "name"),
                    (SortKey::Lvl, "lvl"),
                    (SortKey::Zone, "zone"),
                    (SortKey::Era, "era"),
                    (SortKey::Held, "items"),
                ] {
                    let on = self.sort == k;
                    let txt = format!(
                        "{label}{}",
                        if on {
                            if self.ascending {
                                " ^"
                            } else {
                                " v"
                            }
                        } else {
                            ""
                        }
                    );
                    if ui.selectable_label(on, txt).clicked() {
                        if on {
                            self.ascending = !self.ascending;
                        } else {
                            self.sort = k;
                            self.ascending = true;
                        }
                    }
                }
            }
        });

        /* The source line: which file, how much of it, and whose bag. */
        let d = &book.data;
        let src_line = format!(
            "{} · schema {} · {} quests · {} drop tables · {} NPCs · {} item sources · {} {}",
            book.path.display(),
            d.schema,
            d.quests.len(),
            d.drops.len(),
            d.npcs.len(),
            d.src.len(),
            d.meta.get("license").and_then(Value::as_str).unwrap_or(""),
            d.meta.get("source").and_then(Value::as_str).unwrap_or(""),
        );
        ui.label(
            egui::RichText::new(src_line)
                .font(FontId::monospace(10.5))
                .color(TEXT_3),
        );
        let bag_line = if have_rows {
            format!(
                "holding: {} rows from {}; exaltation stones not counted; every count is the dump's word as of that write, the log is not read here yet",
                rows.len(),
                bag_from.as_deref().unwrap_or("the inventory dump")
            )
        } else {
            /* The ingest's own reason: with no Logs folder it names Settings, with one it names
             * the slash command. Either way every count below is 0 until a dump is read. */
            format!(
                "holding: no inventory dump read yet. {} Until then every count below is 0.",
                cx.ingest.inventory_problem().unwrap_or(
                    "In game type /outputfile inventory, then press Re-read under SOURCES on the Settings screen."
                )
            )
        };
        ui.label(
            egui::RichText::new(bag_line)
                .font(FontId::monospace(10.5))
                .color(if have_rows { TEXT_3 } else { TEXT_2 }),
        );
        ui.add_space(6.0);

        match self.view {
            View::Turnins => self.turnins(ui, cx, &book, &plans, &have, &known_zones),
            View::All => self.all(ui, cx, &book, &plans, &have, &known_zones),
            View::ByZone => self.grouped(ui, cx, &book, &plans, &have, &known_zones, true),
            View::ByGiver => self.grouped(ui, cx, &book, &plans, &have, &known_zones, false),
        }
    }

    /// Rows for the browser views: every quest matching the filter, plus a split page's named
    /// turn-ins as rows of their own.
    fn browser_rows(&self, book: &Book, plans: &PlanCache) -> Vec<Plan> {
        let mut rows: Vec<Plan> = Vec::new();
        for (i, q) in book.data.quests.iter().enumerate() {
            if qb_match(q, &self.filter) {
                rows.push(plans.quests[i].clone());
            }
            if q.split {
                for (pi, p) in q.parts.iter().enumerate() {
                    if !p.n.is_empty() && !p.c.is_empty() && qb_match_part(q, p, &self.filter) {
                        if let Some(plan) = plans.parts.get(&format!("{}::{pi}", q.t)) {
                            rows.push(plan.clone());
                        }
                    }
                }
            }
        }
        sort_rows(&mut rows, book, self.sort, self.ascending);
        rows
    }

    fn turnins(
        &mut self,
        ui: &mut Ui,
        cx: &mut Cx,
        book: &Arc<Book>,
        plans: &PlanCache,
        have: &Have,
        known: &HashSet<String>,
    ) {
        let mut held: Vec<Plan> = Vec::new();
        for (i, q) in book.data.quests.iter().enumerate() {
            if q.items.is_empty() || Book::is_hub(q) {
                continue;
            }
            let p = &plans.quests[i];
            if p.got > 0 {
                held.push(p.clone());
            }
        }
        sort_turnins(&mut held, book);
        let needle = self.filter.q.trim().to_lowercase();
        let matches = |p: &Plan| {
            let q = &book.data.quests[p.quest_ix];
            if !qb_filters(q, &self.filter) {
                return false;
            }
            if needle.is_empty() {
                return true;
            }
            let mut fields: Vec<&str> = vec![&p.name];
            fields.extend(q.giver.as_deref());
            fields.extend(q.zone.as_deref());
            fields.extend(q.items.iter().map(String::as_str));
            fields.extend(q.rewards.iter().map(String::as_str));
            has_needle(&fields, &needle)
        };
        let ready: Vec<Plan> = held
            .iter()
            .filter(|p| p.ready())
            .filter(|p| matches(p))
            .cloned()
            .collect();
        let partial: Vec<Plan> = if self.ready_only {
            vec![]
        } else {
            held.iter()
                .filter(|p| !p.ready())
                .filter(|p| matches(p))
                .cloned()
                .collect()
        };
        let n_ready = held.iter().filter(|p| p.ready()).count();
        let n_partial = held.len() - n_ready;

        egui::Panel::left("quests_list").default_size(400.0).resizable(true).frame(egui::Frame::NONE.fill(INK)).show(ui, |ui| {
            ui.label(egui::RichText::new(format!("{n_ready} ready · {n_partial} partly collected")).font(FontId::monospace(11.0)).color(TEXT_2));
            egui::ScrollArea::vertical().id_salt("turnins_scroll").auto_shrink([false, false]).show(ui, |ui| {
                if ready.is_empty() && partial.is_empty() {
                    let why = if self.ready_only && !held.is_empty() {
                        "Nothing ready. Untick ready only to see partly collected quests."
                    } else if !needle.is_empty() {
                        "Nothing you hold items for matches that search."
                    } else if have.is_empty() {
                        "No inventory dump has been read, so nothing can be matched. The line above says where the ingest looked and what to do."
                    } else {
                        "The dump holds nothing any quest page lists."
                    };
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(why).color(TEXT_2));
                }
                if !ready.is_empty() {
                    caps(ui, &format!("READY TO HAND IN ({})", ready.len()));
                    for p in &ready {
                        self.row(ui, book, p);
                    }
                }
                if !partial.is_empty() {
                    caps(ui, &format!("PARTLY COLLECTED ({})", partial.len()));
                    for p in &partial {
                        self.row(ui, book, p);
                    }
                }
                if book.has_src() && (!ready.is_empty() || !partial.is_empty()) {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(
                        "Counts come from the quest page where it states one and are assumed to be one where it does not, so ready can still be short on a page that never said how many. Items you can only make are counted through their recipe.",
                    ).font(FontId::proportional(10.5)).color(TEXT_3));
                }
            });
        });
        self.detail_pane(ui, cx, book, plans, have, known);
    }

    fn all(
        &mut self,
        ui: &mut Ui,
        cx: &mut Cx,
        book: &Arc<Book>,
        plans: &PlanCache,
        have: &Have,
        known: &HashSet<String>,
    ) {
        let rows = self.browser_rows(book, plans);
        let pages: HashSet<usize> = rows.iter().map(|p| p.quest_ix).collect();
        let meta = if pages.len() == rows.len() {
            format!("{} of {} quests", rows.len(), book.data.quests.len())
        } else {
            format!(
                "{} turn-ins across {} of {} quests",
                rows.len(),
                pages.len(),
                book.data.quests.len()
            )
        };
        egui::Panel::left("quests_list")
            .default_size(400.0)
            .resizable(true)
            .frame(egui::Frame::NONE.fill(INK))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(meta)
                        .font(FontId::monospace(11.0))
                        .color(TEXT_2),
                );
                egui::ScrollArea::vertical()
                    .id_salt("all_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if rows.is_empty() {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Nothing matches those filters.").color(TEXT_2),
                            );
                        }
                        for p in &rows {
                            self.row(ui, book, p);
                        }
                    });
            });
        self.detail_pane(ui, cx, book, plans, have, known);
    }

    /// By zone or by giver: a group list on the left, the group's quests in the middle, and the
    /// selected quest's detail under them.
    #[allow(clippy::too_many_arguments)]
    fn grouped(
        &mut self,
        ui: &mut Ui,
        cx: &mut Cx,
        book: &Arc<Book>,
        plans: &PlanCache,
        have: &Have,
        known: &HashSet<String>,
        by_zone: bool,
    ) {
        let rows = self.browser_rows(book, plans);
        let group_of = |p: &Plan| -> String {
            let q = &book.data.quests[p.quest_ix];
            if by_zone {
                q.zone
                    .clone()
                    .unwrap_or_else(|| "(no zone on the page)".into())
            } else {
                p.giver
                    .clone()
                    .unwrap_or_else(|| "(no giver on the page)".into())
            }
        };
        let mut groups: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for p in &rows {
            let e = groups.entry(group_of(p)).or_insert((0, 0));
            e.0 += 1;
            if p.ready() {
                e.1 += 1;
            }
        }
        let kind = if by_zone { "zones" } else { "givers" };
        egui::Panel::left("quests_groups")
            .default_size(280.0)
            .resizable(true)
            .frame(egui::Frame::NONE.fill(INK))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!("{} {kind} · {} quests", groups.len(), rows.len()))
                        .font(FontId::monospace(11.0))
                        .color(TEXT_2),
                );
                egui::ScrollArea::vertical()
                    .id_salt("groups_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (g, (n, ready)) in &groups {
                            let sel = self.group.as_deref() == Some(g.as_str());
                            let (rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 24.0),
                                Sense::click(),
                            );
                            let p = ui.painter();
                            if sel {
                                p.rect_filled(rect, 0.0, PANEL_2);
                                p.rect_filled(
                                    Rect::from_min_size(
                                        rect.left_top(),
                                        Vec2::new(2.0, rect.height()),
                                    ),
                                    0.0,
                                    GOLD,
                                );
                            } else if resp.hovered() {
                                p.rect_filled(rect, 0.0, PANEL);
                            }
                            let col = if sel {
                                FLARE
                            } else if resp.hovered() {
                                GOLD_HI
                            } else {
                                TEXT
                            };
                            p.text(
                                Pos2::new(rect.left() + 12.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                g,
                                FontId::proportional(12.5),
                                col,
                            );
                            let count = if *ready > 0 {
                                format!("{ready} ready / {n}")
                            } else {
                                n.to_string()
                            };
                            p.text(
                                Pos2::new(rect.right() - 8.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                count,
                                FontId::monospace(10.5),
                                if *ready > 0 { TEXT } else { TEXT_3 },
                            );
                            if resp.clicked() {
                                self.group = Some(g.clone());
                                self.selected = None;
                            }
                        }
                    });
            });

        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(INK).inner_margin(egui::Margin::symmetric(12, 0))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("group_body").auto_shrink([false, false]).show(ui, |ui| {
                if by_zone && !self.tracked.is_empty() {
                    let tracked: Vec<&Plan> = self.tracked.iter().filter_map(|k| find_plan(plans, k)).collect();
                    let pooled = merge_plans(&tracked);
                    let left = pooled.iter().filter(|c| !c.done()).count();
                    caps(ui, "TRACKED, POOLED BY ZONE");
                    ui.label(egui::RichText::new(format!("{} items across {} quests · {left} still to get · tracked for this run only", pooled.len(), tracked.len())).font(FontId::monospace(10.5)).color(TEXT_3));
                    let buckets = zone_buckets(&pooled, book, known);
                    self.buckets(ui, cx, book, &buckets, true);
                    ui.add_space(10.0);
                }
                let Some(g) = self.group.clone() else {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("Pick one of the {kind} on the left.")).color(TEXT_2));
                    return;
                };
                if by_zone {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&g).font(crate::fonts::display(15.0)).color(GOLD_HI));
                        if g.starts_with('(') {
                            return;
                        }
                        if ui.add(egui::Label::new(egui::RichText::new("open zone").color(TEXT_2)).sense(Sense::click())).clicked() {
                            cx.ask = Ask::ShowZone(g.clone());
                        }
                    });
                } else {
                    let zone = book.npc_zone(&g).map(str::to_string);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&g).font(crate::fonts::display(15.0)).color(GOLD_HI));
                        if let Some(z) = &zone {
                            let loc = book.data.npcs.get(&g).and_then(|n| n.loc.clone());
                            let where_ = match loc { Some(l) => format!("{z} ({l})"), None => z.clone() };
                            ui.label(egui::RichText::new(where_).font(FontId::monospace(11.0)).color(TEXT_2));
                        }
                    });
                }
                ui.add_space(4.0);
                let in_group: Vec<&Plan> = rows.iter().filter(|p| group_of(p) == g).collect();
                for p in &in_group {
                    self.row(ui, book, p);
                }
                if let Some(sel) = self.selected.clone() {
                    if in_group.iter().any(|p| p.key == sel) {
                        ui.add_space(8.0);
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                        ui.painter().rect_filled(rect, 0.0, RULE);
                        if let Some(plan) = find_plan(plans, &sel).cloned() {
                            self.detail(ui, cx, book, &plan, have, known);
                        }
                    }
                }
            });
        });
    }

    fn detail_pane(
        &mut self,
        ui: &mut Ui,
        cx: &mut Cx,
        book: &Arc<Book>,
        plans: &PlanCache,
        have: &Have,
        known: &HashSet<String>,
    ) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(INK)
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("detail_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let plan = self
                            .selected
                            .as_ref()
                            .and_then(|k| find_plan(plans, k))
                            .cloned();
                        match plan {
                            Some(p) => self.detail(ui, cx, book, &p, have, known),
                            None => {
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("Pick a quest on the left.").color(TEXT_2),
                                );
                            }
                        }
                    });
            });
    }

    /// One list row: a leading state square, the name, got/need, the facts.
    fn row(&mut self, ui: &mut Ui, book: &Book, p: &Plan) {
        let q = &book.data.quests[p.quest_ix];
        let sel = self.selected.as_deref() == Some(p.key.as_str());
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
        let painter = ui.painter();
        if sel {
            painter.rect_filled(rect, 0.0, PANEL_2);
            painter.rect_filled(
                Rect::from_min_size(rect.left_top(), Vec2::new(2.0, ROW_H)),
                0.0,
                GOLD,
            );
        } else if resp.hovered() {
            painter.rect_filled(rect, 0.0, PANEL);
        }
        /* Ready is settled; short of ready is idle with the count beside it. WORKING is a thread
         * of ours running, never "some of the pieces are in hand". */
        let state = if p.ready() {
            State::Settled
        } else {
            State::Idle
        };
        state_square(
            painter,
            Pos2::new(rect.left() + 10.0, rect.top() + 12.0),
            state,
        );
        let name_col = if sel {
            FLARE
        } else if resp.hovered() {
            GOLD_HI
        } else {
            TEXT
        };
        painter.text(
            Pos2::new(rect.left() + 22.0, rect.top() + 12.0),
            Align2::LEFT_CENTER,
            &p.name,
            FontId::proportional(12.5),
            name_col,
        );
        let count = if p.need > 0 {
            format!("{}/{}", p.got, p.need)
        } else {
            String::new()
        };
        painter.text(
            Pos2::new(rect.right() - 8.0, rect.top() + 12.0),
            Align2::RIGHT_CENTER,
            count,
            FontId::monospace(11.0),
            if p.done { TEXT } else { TEXT_2 },
        );
        let chain =
            p.part.is_none() && !q.items.is_empty() && q.giver.is_none() && q.zone.is_none();
        let mut facts = quest_facts(q, chain);
        if p.part.is_none() && q.split {
            facts = format!("{} turn-ins · {facts}", q.parts.len());
        }
        if Book::is_hub(q) {
            facts = format!("index page, not a turn-in · {facts}");
        }
        if self.tracked.contains(&p.key) {
            facts = format!("tracked · {facts}");
        }
        painter.text(
            Pos2::new(rect.left() + 22.0, rect.top() + 27.0),
            Align2::LEFT_CENTER,
            facts,
            FontId::proportional(10.5),
            TEXT_3,
        );
        if resp.clicked() {
            self.selected = if sel { None } else { Some(p.key.clone()) };
        }
    }

    /// The quest, opened: steps, zone groups, turn-ins on the page, rewards, related pages.
    fn detail(
        &mut self,
        ui: &mut Ui,
        cx: &mut Cx,
        book: &Book,
        p: &Plan,
        have: &Have,
        known: &HashSet<String>,
    ) {
        let q = &book.data.quests[p.quest_ix];
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&p.name)
                    .font(crate::fonts::display(16.0))
                    .color(GOLD_HI),
            );
            let tracked = self.tracked.contains(&p.key);
            if ui
                .small_button(if tracked { "Untrack" } else { "Track" })
                .clicked()
            {
                if tracked {
                    self.tracked.retain(|k| *k != p.key);
                } else {
                    self.tracked.push(p.key.clone());
                }
            }
            if !book.data.base.is_empty() && ui.small_button("wiki").clicked() {
                let url = format!("{}{}", book.data.base, p.base_key());
                if let Err(e) = open::that(&url) {
                    log::warn!("open {url}: {e}");
                }
            }
        });
        let chain =
            p.part.is_none() && !q.items.is_empty() && q.giver.is_none() && q.zone.is_none();
        let mut facts = quest_facts(q, chain);
        if let Some(g) = &p.giver {
            if p.part.is_some() {
                facts = format!("to {g} · {facts}");
            }
        }
        ui.label(
            egui::RichText::new(facts)
                .font(FontId::proportional(11.5))
                .color(TEXT_2),
        );
        let count = format!("{} of {} items in hand", p.got, p.need);
        ui.label(
            egui::RichText::new(count)
                .font(FontId::monospace(11.0))
                .color(if p.done { TEXT } else { TEXT_2 }),
        );

        if let Some(ss) = &p.steps {
            self.steps(ui, q, p, ss);
        }

        if p.comps.is_empty() {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("The wiki page lists no items for this quest.").color(TEXT_2),
            );
        } else if book.has_src() {
            let buckets = zone_buckets(&p.comps, book, known);
            let locked = p.comps.iter().filter(|c| c.locked).count();
            self.buckets(ui, cx, book, &buckets, false);
            if locked > 0 {
                ui.label(egui::RichText::new(format!("{locked} more item{} wait on steps that are locked until earlier hand-ins are done", if locked == 1 { "" } else { "s" })).font(FontId::proportional(10.5)).color(TEXT_3));
            }
        } else {
            /* A file older than the source table: the flat list, with the lowest droppers. A
             * dropper the tailed log has seen die this run is marked; the wiki's name and the
             * game's differ by a parenthetical the game never prints, which `norm_mob` drops. */
            caps(ui, "ITEMS");
            let killed: HashSet<String> = cx
                .ingest
                .kills()
                .iter()
                .map(|k| norm_mob(&k.name))
                .collect();
            for c in &p.comps {
                let d = if c.done() {
                    vec![]
                } else {
                    book.drops_for(&c.name)
                        .iter()
                        .take(2)
                        .cloned()
                        .collect::<Vec<_>>()
                };
                let src: Vec<String> = d
                    .iter()
                    .map(|row| {
                        let mob = row.first().and_then(Value::as_str).unwrap_or("");
                        let zone = row.get(2).and_then(Value::as_str).unwrap_or("");
                        let seen = if killed.contains(&norm_mob(mob)) {
                            " (killed this run)"
                        } else {
                            ""
                        };
                        if zone.is_empty() {
                            format!("{mob}{seen}")
                        } else {
                            format!("{mob} · {zone}{seen}")
                        }
                    })
                    .collect();
                item_row(ui, cx, c, &src.join(", "));
            }
        }

        if p.part.is_none() && q.split && q.parts.len() >= 2 {
            caps(ui, &format!("{} TURN-INS ON THIS PAGE", q.parts.len()));
            for (pi, part) in q.parts.iter().enumerate() {
                let got = part.c.iter().filter(|(n, w)| have.held(n) >= *w).count();
                let title = if part.n.is_empty() {
                    "Also listed on this page".to_string()
                } else {
                    part.n.clone()
                };
                ui.horizontal(|ui| {
                    state_square(
                        ui.painter(),
                        ui.cursor().left_top() + Vec2::new(5.0, 9.0),
                        if got == part.c.len() && !part.c.is_empty() {
                            State::Settled
                        } else {
                            State::Idle
                        },
                    );
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new(title).color(TEXT));
                    if let Some(g) = &part.g {
                        ui.label(egui::RichText::new(format!("to {g}")).color(TEXT_2));
                    }
                    if !part.r.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("reward: {}", part.r.join(", ")))
                                .color(TEXT_2),
                        );
                    }
                    ui.label(
                        egui::RichText::new(format!("{got}/{}", part.c.len()))
                            .font(FontId::monospace(11.0))
                            .color(TEXT_2),
                    );
                    let key = format!("{}::{pi}", q.t);
                    let tracked = self.tracked.contains(&key);
                    if ui
                        .small_button(if tracked { "Untrack" } else { "Track" })
                        .clicked()
                    {
                        if tracked {
                            self.tracked.retain(|k| *k != key);
                        } else {
                            self.tracked.push(key);
                        }
                    }
                });
                for (n, w) in &part.c {
                    let c = Comp {
                        name: n.clone(),
                        want: *w,
                        have: have.held(n),
                        ..Default::default()
                    };
                    item_row(ui, cx, &c, "");
                }
            }
        }

        if !p.rewards.is_empty() {
            ui.add_space(6.0);
            let shown: Vec<&str> = p.rewards.iter().take(10).map(String::as_str).collect();
            let more = p.rewards.len().saturating_sub(10);
            let mut line = format!("reward: {}", shown.join(", "));
            if more > 0 {
                line.push_str(&format!(" +{more} more"));
            }
            ui.label(egui::RichText::new(line).color(TEXT_2));
        }
        /* FACTION. factions.json names the quests that raise and lower each faction, so a quest
         * page can say which factions its hand-in moves. 446 of the 924 quests are named by at
         * least one faction; the other 478 draw nothing at all rather than an empty heading,
         * because a section that is blank on half the screens is furniture.
         *
         * NO LINK ON THE FACTION NAME. There is no Factions screen for it to open, and the owner
         * has been deleting rows rather than adding them. The wiki's own description of the
         * faction is the hover, which is the whole of what factions.json has to say about it. */
        let moves: Vec<(String, &'static str, String)> = match cx.data {
            Some(data) => data
                .factions_of_quest(&q.name)
                .into_iter()
                .map(|m| {
                    (
                        m.faction.name.clone(),
                        m.way.label(),
                        m.faction.desc.clone().unwrap_or_else(|| {
                            "factions.json carries no description for this faction".to_owned()
                        }),
                    )
                })
                .collect(),
            None => Vec::new(),
        };
        if !moves.is_empty() {
            caps(ui, "FACTION");
            for (name, way, desc) in &moves {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(*way)
                            .font(FontId::monospace(11.0))
                            .color(TEXT_2),
                    );
                    ui.label(egui::RichText::new(name).color(TEXT))
                        .on_hover_text(desc);
                });
            }
            ui.label(
                egui::RichText::new(
                    "from factions.json qr and ql. It says which way, never by how much: the file \
                     carries no number.",
                )
                .font(FontId::proportional(10.5))
                .color(TEXT_3),
            );
        }

        /* The wiki's top table fills empty cells with a literal "None". */
        let rz: Vec<&str> = q
            .related_zones
            .iter()
            .filter(|z| !z.is_empty() && *z != "None")
            .map(String::as_str)
            .collect();
        let rn: Vec<&str> = q
            .related_npcs
            .iter()
            .filter(|n| !n.is_empty() && *n != "None")
            .map(String::as_str)
            .collect();
        if !rz.is_empty() || !rn.is_empty() {
            let mut rel: Vec<String> = Vec::new();
            if !rz.is_empty() {
                rel.push(format!("zones: {}", rz.join(", ")));
            }
            if !rn.is_empty() {
                let more = rn.len().saturating_sub(8);
                let mut s = format!(
                    "NPCs: {}",
                    rn.iter().take(8).cloned().collect::<Vec<_>>().join(", ")
                );
                if more > 0 {
                    s.push_str(&format!(" +{more}"));
                }
                rel.push(s);
            }
            ui.label(
                egui::RichText::new(rel.join(" · "))
                    .font(FontId::proportional(10.5))
                    .color(TEXT_3),
            );
        }
    }

    fn steps(&mut self, ui: &mut Ui, q: &Quest, p: &Plan, ss: &StepState) {
        caps(ui, "STEPS");
        let head = if ss.finished {
            format!("all {} steps done, read off the bag", ss.total)
        } else if ss.done_n > 0 {
            format!("{} of {} steps done, read off the bag", ss.done_n, ss.total)
        } else {
            format!("{} steps, none provable from the bag yet", ss.total)
        };
        ui.label(
            egui::RichText::new(head)
                .font(FontId::monospace(10.5))
                .color(TEXT_3),
        );
        if ss.unnamed > 0 {
            ui.label(egui::RichText::new(format!(
                "{} more in hand than the dump can name: several items here share one name and only the export carries the id that tells them apart. Run /outputfile inventory to place {}.",
                ss.unnamed, if ss.unnamed == 1 { "it" } else { "them" }
            )).font(FontId::proportional(10.5)).color(TEXT_2));
        }
        let doable: Vec<&StepView> = ss.states.iter().filter(|s| s.can).collect();
        let locked: Vec<&StepView> = ss
            .states
            .iter()
            .filter(|s| !s.note && !s.done && !s.can)
            .collect();
        let notes: Vec<&StepView> = ss.states.iter().filter(|s| s.note).collect();
        for s in &doable {
            step_line(ui, &q.steps[s.ix], State::You);
        }
        if !locked.is_empty() {
            let open = self.open_locked.contains(&p.key);
            let label = format!(
                "{} {} later step{}, locked until the ones above are done",
                if open { "v" } else { ">" },
                locked.len(),
                if locked.len() == 1 { "" } else { "s" }
            );
            if ui
                .add(
                    egui::Label::new(egui::RichText::new(label).color(TEXT_2))
                        .sense(Sense::click()),
                )
                .clicked()
            {
                if open {
                    self.open_locked.remove(&p.key);
                } else {
                    self.open_locked.insert(p.key.clone());
                }
            }
            if open {
                for s in &locked {
                    step_line(ui, &q.steps[s.ix], State::Idle);
                }
            }
        }
        for s in &notes {
            ui.label(
                egui::RichText::new(&q.steps[s.ix].txt)
                    .font(FontId::proportional(10.5))
                    .color(TEXT_3),
            );
        }
    }

    fn buckets(&mut self, ui: &mut Ui, cx: &mut Cx, book: &Book, buckets: &[Bucket], pooled: bool) {
        if buckets.is_empty() {
            return;
        }
        for b in buckets {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                match bucket_label(&b.zone) {
                    Some(l) => {
                        ui.label(egui::RichText::new(l).color(TEXT_2));
                    }
                    None => {
                        let r = ui.add(
                            egui::Label::new(egui::RichText::new(&b.zone).color(GOLD))
                                .sense(Sense::click()),
                        );
                        if r.clicked() {
                            cx.ask = Ask::ShowZone(b.zone.clone());
                        }
                        if b.umbrella {
                            ui.label(
                                egui::RichText::new("the wiki does not say which zone")
                                    .color(TEXT_3),
                            );
                        }
                    }
                }
                let n = if b.left > 0 {
                    format!("{} to get", b.left)
                } else {
                    "all held".to_string()
                };
                ui.label(
                    egui::RichText::new(n)
                        .font(FontId::monospace(10.5))
                        .color(if b.left > 0 { TEXT_2 } else { TEXT }),
                );
            });
            for c in &b.items {
                let src = if c.done() {
                    String::new()
                } else if let Some(st) = &c.step {
                    step_note(st)
                } else {
                    let s = source_in(book, &c.name, &b.zone);
                    let mobs: Vec<&str> = s.mobs.iter().take(3).map(String::as_str).collect();
                    let who = if mobs.is_empty() {
                        s.note.clone().unwrap_or_default()
                    } else {
                        mobs.join(", ")
                    };
                    match s.isle {
                        Some(i) if !who.is_empty() => format!("{i} · {who}"),
                        Some(i) => i,
                        None => who,
                    }
                };
                let mut note = src;
                if !c.done() {
                    if let Some(v) = &c.via {
                        note = if note.is_empty() {
                            format!("for {v}")
                        } else {
                            format!("for {v} · {note}")
                        };
                    }
                    if let Some(fr) = &c.from {
                        if let Some((_, fq)) = book.quest(fr) {
                            let s = format!("reward of {}", fq.name);
                            note = if note.is_empty() {
                                s
                            } else {
                                format!("{note} · {s}")
                            };
                        }
                    }
                }
                if pooled {
                    if let Some(t) = &c.tag {
                        note = if note.is_empty() {
                            t.clone()
                        } else {
                            format!("{t} · {note}")
                        };
                    }
                }
                item_row(ui, cx, c, &note);
            }
        }
    }
}

fn find_plan<'a>(plans: &'a PlanCache, key: &str) -> Option<&'a Plan> {
    if let Some(p) = plans.parts.get(key) {
        return Some(p);
    }
    plans.quests.iter().find(|p| p.key == key)
}

/* ============================================================== small painted pieces == */

fn state_square(p: &egui::Painter, at: Pos2, s: State) {
    let sq = Rect::from_center_size(at, Vec2::splat(6.0));
    match s {
        State::Idle => {
            p.rect_stroke(sq, 0.0, Stroke::new(1.0, IDLE), egui::StrokeKind::Middle);
        }
        _ => {
            p.rect_filled(sq, 0.0, s.color());
        }
    }
}

fn caps(ui: &mut Ui, s: &str) {
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 14.0), Sense::hover());
    crate::chrome::tracked(ui, rect.left_top(), s, 9.5, 1.8, GOLD_DIM);
    ui.add_space(2.0);
}

/// A refusal is a result: the WRONG state colour and the words, never an empty list.
///
/// A WRAPPED LABEL, NOT A PAINTED LINE. A parse failure here carries the file's path and serde's
/// reason, and a painted single line ran that off the right edge, so the screen said "quest-items
/// .json:" and lost the part a person could act on. The frame grows to the text and the WRONG bar
/// is painted down its full height afterwards.
fn problem(ui: &mut Ui, msg: &str) {
    let full = ui.available_width();
    let out = egui::Frame::NONE
        .fill(SUNK)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(full - 24.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(msg)
                        .font(FontId::proportional(12.5))
                        .color(TEXT),
                )
                .wrap(),
            );
        });
    let r = out.response.rect;
    ui.painter().rect_filled(
        Rect::from_min_size(r.left_top(), Vec2::new(3.0, r.height())),
        0.0,
        WRONG,
    );
}

/// One item row: state square, the name (click asks the Items screen), have/want, the note.
fn item_row(ui: &mut Ui, cx: &mut Cx, c: &Comp, note: &str) {
    ui.horizontal(|ui| {
        let at = ui.cursor().left_top() + Vec2::new(5.0, 9.0);
        state_square(
            ui.painter(),
            at,
            if c.done() {
                State::Settled
            } else {
                State::Idle
            },
        );
        ui.add_space(14.0);
        let r = ui.add(
            egui::Label::new(egui::RichText::new(&c.name).color(if c.done() {
                TEXT_2
            } else {
                TEXT
            }))
            .sense(Sense::click()),
        );
        if r.clicked() {
            cx.ask = Ask::ShowItem(c.name.clone());
        }
        if c.want > 1 || c.have > 0 {
            ui.label(
                egui::RichText::new(format!("{}/{}", c.have, c.want))
                    .font(FontId::monospace(11.0))
                    .color(if c.done() { TEXT } else { TEXT_2 }),
            );
        }
        if !note.is_empty() {
            ui.label(
                egui::RichText::new(note)
                    .font(FontId::proportional(10.5))
                    .color(TEXT_3),
            );
        }
    });
}

fn step_line(ui: &mut Ui, st: &Step, state: State) {
    ui.horizontal(|ui| {
        let at = ui.cursor().left_top() + Vec2::new(5.0, 9.0);
        state_square(ui.painter(), at, state);
        ui.add_space(14.0);
        let mut where_: Vec<String> = Vec::new();
        if let Some(z) = &st.z {
            where_.push(z.clone());
        }
        if let Some(l) = &st.loc {
            where_.push(format!("({l})"));
        }
        let txt = if where_.is_empty() {
            st.txt.clone()
        } else {
            format!("{} · {}", st.txt, where_.join(" "))
        };
        ui.add(
            egui::Label::new(egui::RichText::new(txt).color(if state == State::Idle {
                TEXT_3
            } else {
                TEXT
            }))
            .wrap(),
        );
    });
}

/* ============================================================================ tests == */

#[cfg(test)]
mod tests {
    use super::*;

    fn book_from(json: &str) -> Book {
        Book::from_json(json, Path::new("test.json")).expect("test json parses")
    }

    fn have_of(book: &Book, rows: &[(&str, u32, u32)]) -> Have {
        let rows: Vec<DumpRow> = rows
            .iter()
            .map(|(n, id, c)| (n.to_string(), *id, *c))
            .collect();
        Have::from_rows(&rows, book)
    }

    #[test]
    fn item_key_keeps_articles_and_drops_apostrophes() {
        assert_eq!(
            item_key("Ambassador D`Vinn's  Ring "),
            "ambassador dvinns ring"
        );
        assert_ne!(
            item_key("A Sapphire"),
            item_key("Sapphire"),
            "identity must keep the article"
        );
        assert_eq!(
            norm_name("A Sapphire"),
            norm_name("Sapphire"),
            "the loose key drops it"
        );
        assert_eq!(norm_mob("a shadowknight (Ogre)"), "shadowknight");
    }

    #[test]
    fn strip_decor_peels_the_client_decorations_outermost_first() {
        assert_eq!(strip_decor("Giant Snake Fang +4"), "Giant Snake Fang");
        assert_eq!(strip_decor("Backpack*"), "Backpack");
        assert_eq!(strip_decor("Earthshaker (Exaltation)*"), "Earthshaker");
        assert_eq!(strip_decor("Earthshaker +3 (Exaltation)"), "Earthshaker");
        assert_eq!(
            strip_decor("Sword +"),
            "Sword +",
            "a bare plus is not a tier"
        );
        assert!(is_exalt_stone("Earthshaker (Exaltation)*"));
        assert!(!is_exalt_stone("Earthshaker"));
    }

    const TINY: &str = r#"{
      "schema": 6, "base": "https://eqlwiki.com/index.php/",
      "quests": [
        {"n": "Bone Chips", "t": "Bone_Chips", "giver": "Vaal", "zone": "Kaladim", "items": ["Bone Chips"],
         "want": [["Bone Chips", 4]], "classes": ["Cleric"], "lvl": 5, "rewards": ["Ale"]},
        {"n": "A Sapphire Ring", "t": "Sapphire_Ring", "giver": "Iksar", "zone": "Cabilis", "items": ["A Sapphire"],
         "want": [["A Sapphire", 1]], "classes": [], "lvl": 30, "era": "Kunark Era", "oe": true}
      ],
      "src": {
        "bone chips": {"k": ["drop"], "z": ["Najena"], "m": {"Najena": ["a skeleton"]}},
        "a bar of ronium": {"k": [], "z": [], "r": {"c": [[1, "Melatite"], [2, "Enchanted Platinum Bar"]]}},
        "edible goo": {"k": [], "z": [], "r": {"c": [[1, "Rat Ears"]]}},
        "fire opal": {"k": ["drop"], "z": ["A", "B", "C", "D", "E"], "many": true}
      },
      "alias": {"bunker cell #1": "Bunker Cell No 1"},
      "ids": {"old silver coin": [[1, "Coin - Azia"], [2, "Coin - Beza"], [3, "Coin - Caza"]]},
      "npcs": {"Vaal": {"t": "Vaal", "z": "Kaladim", "loc": "1, 2"}},
      "geo": {"alias": {}, "nodes": {"Freeport": {"adj": [], "keys": ["efp", "wfp"]}, "Najena": {"adj": [], "keys": ["najena"]}}}
    }"#;

    #[test]
    fn a_dump_row_with_a_decoration_answers_the_bare_wiki_name() {
        let b = book_from(TINY);
        let h = have_of(&b, &[("Giant Snake Fang +4", 0, 2)]);
        assert_eq!(h.held("Giant Snake Fang"), 2);
    }

    #[test]
    fn an_exaltation_stone_never_counts_as_the_item_it_was_rendered_from() {
        let b = book_from(TINY);
        let h = have_of(&b, &[("Earthshaker (Exaltation)*", 0, 1)]);
        assert_eq!(h.held("Earthshaker"), 0);
    }

    #[test]
    fn the_alias_table_answers_the_wiki_title_for_a_client_name() {
        let b = book_from(TINY);
        let h = have_of(&b, &[("Bunker Cell #1", 0, 1)]);
        assert_eq!(h.held("Bunker Cell No 1"), 1);
    }

    #[test]
    fn loose_lookup_rescues_a_stray_article_but_exact_wins() {
        let b = book_from(TINY);
        let h = have_of(&b, &[("Sapphire", 0, 3), ("A Sapphire", 0, 1)]);
        assert_eq!(h.held("A Sapphire"), 1, "exact key first");
        assert_eq!(h.held("Sapphire"), 3);
        let h2 = have_of(&b, &[("A Ruby", 0, 2)]);
        assert_eq!(
            h2.held("Ruby"),
            2,
            "the loose index drops the dump's article"
        );
    }

    #[test]
    fn holding_one_of_four_bone_chips_is_not_holding_bone_chips() {
        let b = book_from(TINY);
        let h = have_of(&b, &[("Bone Chips", 0, 1)]);
        let p = comps_for(&b.data.quests[0], 0, &h, &b);
        assert_eq!(p.got, 0);
        assert!(!p.done);
        let h4 = have_of(&b, &[("Bone Chips", 0, 4)]);
        let p4 = comps_for(&b.data.quests[0], 0, &h4, &b);
        assert!(p4.done);
    }

    #[test]
    fn a_recipe_expands_the_shortfall_only_and_a_world_source_outranks_it() {
        let b = book_from(TINY);
        let q: Quest = serde_json::from_str(
            r#"{"n": "Goo", "t": "Goo", "items": ["Edible Goo"], "want": [["Edible Goo", 4]]}"#,
        )
        .unwrap();
        let h = have_of(&b, &[("Edible Goo", 0, 4)]);
        let p = comps_for(&q, 0, &h, &b);
        assert_eq!(
            p.comps.len(),
            1,
            "four in the bag never turn into Rat Ears 0/1"
        );
        assert_eq!(p.comps[0].name, "Edible Goo");
        let h1 = have_of(&b, &[("Edible Goo", 0, 1)]);
        let p1 = comps_for(&q, 0, &h1, &b);
        let ears = p1
            .comps
            .iter()
            .find(|c| c.name == "Rat Ears")
            .expect("shortfall expands");
        assert_eq!(ears.want, 3);
        assert_eq!(ears.via.as_deref(), Some("Edible Goo"));
        // an item with a world source is farmed, not rolled up
        assert!(recipe_for(&b, "Bone Chips").is_none());
        assert!(recipe_for(&b, "a bar of ronium").is_some());
    }

    const COINS: &str = r#"{"n": "Coins", "t": "Coins", "items": ["Old Silver Coin", "Gleaming Coin"],
      "want": [["Old Silver Coin", 3]],
      "steps": [
        {"i": 1, "k": "ground", "iid": 1, "z": "Ak'Anon", "out": [["Old Silver Coin", 1]], "txt": "Azia"},
        {"i": 2, "k": "ground", "iid": 2, "z": "Grobb", "out": [["Old Silver Coin", 1]], "txt": "Beza"},
        {"i": 3, "k": "ground", "iid": 3, "z": "Halas", "out": [["Old Silver Coin", 1]], "txt": "Caza"},
        {"i": 4, "k": "give", "npc": "Vaal", "pre": [1, 2, 3], "in": [["Old Silver Coin", 3]], "out": [["Gleaming Coin", 1]], "txt": "Hand Vaal the three coins"},
        {"i": 5, "k": "give", "npc": "Vaal", "pre": [4], "in": [["Gleaming Coin", 1]], "out": [["Ring", 1]], "txt": "Hand Vaal the Gleaming Coin"}
      ]}"#;

    #[test]
    fn a_chain_is_ready_only_when_the_final_hand_in_is_satisfiable() {
        let b = book_from(TINY);
        let q: Quest = serde_json::from_str(COINS).unwrap();
        let h = have_of(
            &b,
            &[
                ("Old Silver Coin", 1, 1),
                ("Old Silver Coin", 2, 1),
                ("Old Silver Coin", 3, 1),
            ],
        );
        let p = comps_for(&q, 0, &h, &b);
        let ss = p.steps.as_ref().unwrap();
        assert!(
            !ss.handin_ready,
            "three coins in the bag is step 4 of 5, not ready"
        );
        assert!(!p.ready());
        let hg = have_of(&b, &[("Gleaming Coin", 0, 1)]);
        let pg = comps_for(&q, 0, &hg, &b);
        assert!(pg.steps.as_ref().unwrap().handin_ready);
        assert!(pg.ready());
    }

    #[test]
    fn pinned_steps_tick_by_client_id_not_by_name() {
        let b = book_from(TINY);
        let q: Quest = serde_json::from_str(COINS).unwrap();
        // Azia and Caza in the bank, by id; Beza missing
        let h = have_of(&b, &[("Old Silver Coin", 1, 1), ("Old Silver Coin", 3, 1)]);
        let ss = step_state(&q, &h).unwrap();
        assert!(ss.states[0].done, "Azia");
        assert!(!ss.states[1].done, "Beza is the one still missing");
        assert!(ss.states[2].done, "Caza");
        assert_eq!(ss.unnamed, 0);
        // the shopping list sends you only to the city whose coin is missing
        let p = comps_for(&q, 0, &h, &b);
        let coin = p
            .comps
            .iter()
            .find(|c| c.name == "Old Silver Coin")
            .unwrap();
        assert_eq!(
            coin.zones_pinned.as_deref(),
            Some(&["Grobb".to_string()][..])
        );
        // a coin looted since the export has no id: counted, not guessed at
        let h2 = have_of(&b, &[("Old Silver Coin", 1, 1), ("Old Silver Coin", 0, 1)]);
        assert_eq!(step_state(&q, &h2).unwrap().unnamed, 1);
    }

    #[test]
    fn pooled_pickups_mark_the_first_held_pins_done() {
        let b = book_from(TINY);
        let q: Quest = serde_json::from_str(r#"{"n": "Hearts", "t": "Hearts", "items": ["Wooden Heart"],
          "steps": [
            {"i": 1, "k": "kill", "out": [["Wooden Heart", 1]], "txt": "Loot from a treant"},
            {"i": 2, "k": "get", "npc": "Jyle", "out": [["Wooden Heart", 1]], "txt": "from Jyle"},
            {"i": 3, "k": "give", "npc": "Vaal", "pre": [1], "in": [["Wooden Heart", 1]], "out": [["Prize", 1]], "txt": "hand it in"}
          ]}"#).unwrap();
        let h = have_of(&b, &[("Wooden Heart", 0, 1)]);
        let ss = step_state(&q, &h).unwrap();
        assert!(
            ss.states[0].done && ss.states[1].done,
            "one heart satisfies every way of getting it"
        );
        assert!(ss.handin_ready);
        let h0 = have_of(&b, &[]);
        let ss0 = step_state(&q, &h0).unwrap();
        assert!(!ss0.states[0].done && !ss0.states[1].done);
    }

    #[test]
    fn a_locked_row_is_kept_off_the_zone_list_until_the_chain_reaches_it() {
        let b = book_from(TINY);
        let q: Quest = serde_json::from_str(r#"{"n": "Token", "t": "Token", "items": ["Prize"],
          "steps": [
            {"i": 1, "k": "give", "npc": "Vaal", "in": [["Bone Chips", 1]], "out": [["Token of Truth", 1]], "txt": "chips for a token"},
            {"i": 2, "k": "give", "npc": "Vaal", "pre": [1], "in": [["Token of Truth", 1]], "out": [["Testimony", 1]], "txt": "token for testimony"},
            {"i": 3, "k": "give", "npc": "Vaal", "pre": [2], "in": [["Testimony", 1]], "out": [["Prize", 1]], "txt": "testimony for the prize"}
          ]}"#).unwrap();
        let h = have_of(&b, &[]);
        let p = comps_for(&q, 0, &h, &b);
        let token = p
            .comps
            .iter()
            .find(|c| c.name == "Token of Truth")
            .expect("undone step input wanted");
        assert!(
            !token.locked,
            "step 1 is doable, so its product is not locked"
        );
        assert_eq!(
            token.step.as_ref().map(|s| s.npc.as_str()),
            Some("Vaal"),
            "a chain-made row is sourced from its step's NPC"
        );
        let testimony = p
            .comps
            .iter()
            .find(|c| c.name == "Testimony")
            .expect("the final hand-in's input is wanted");
        assert!(
            testimony.locked,
            "its only producing step waits on step 1, so it is not something to go get yet"
        );
        let chips = p.comps.iter().find(|c| c.name == "Bone Chips").unwrap();
        assert!(!chips.locked);
        assert!(
            p.comps.iter().all(|c| c.name != "Prize"),
            "the page item a step makes is not a top-level want"
        );
        let known = HashSet::new();
        let buckets = zone_buckets(&p.comps, &b, &known);
        let listed: Vec<&str> = buckets
            .iter()
            .flat_map(|bk| bk.items.iter().map(|c| c.name.as_str()))
            .collect();
        assert!(listed.contains(&"Token of Truth") && listed.contains(&"Bone Chips"));
        assert!(
            !listed.contains(&"Testimony"),
            "locked rows stay off the zone list until the chain reaches them"
        );
        assert_eq!(p.got, 0);
        assert_eq!(
            p.need, 3,
            "locked rows stay in the counts and the checklist"
        );
    }

    #[test]
    fn zone_buckets_put_catch_alls_last_and_outstanding_first() {
        let b = book_from(TINY);
        let comps = vec![
            Comp {
                name: "Fire Opal".into(),
                want: 1,
                have: 0,
                ..Default::default()
            },
            Comp {
                name: "Nothing Known".into(),
                want: 1,
                have: 0,
                ..Default::default()
            },
            Comp {
                name: "Bone Chips".into(),
                want: 1,
                have: 0,
                ..Default::default()
            },
        ];
        let known = HashSet::new();
        let zs: Vec<String> = zone_buckets(&comps, &b, &known)
            .into_iter()
            .map(|k| k.zone)
            .collect();
        assert_eq!(zs, vec!["Najena", MANY, NOSRC]);
        assert_eq!(bucket_label(MANY), Some("Anywhere, many zones"));
        let held = vec![
            Comp {
                name: "Bone Chips".into(),
                want: 1,
                have: 1,
                ..Default::default()
            },
            Comp {
                name: "Fire Opal".into(),
                want: 1,
                have: 0,
                ..Default::default()
            },
        ];
        let zs2: Vec<String> = zone_buckets(&held, &b, &known)
            .into_iter()
            .map(|k| k.zone)
            .collect();
        assert_eq!(
            zs2,
            vec!["Najena", MANY],
            "a catch-all sits at the bottom whatever it holds, even under a cleared zone"
        );
        let two = vec![
            Comp {
                name: "Bone Chips".into(),
                want: 1,
                have: 1,
                ..Default::default()
            },
            Comp {
                name: "Gem".into(),
                want: 1,
                have: 0,
                ..Default::default()
            },
        ];
        let b2 = book_from(
            r#"{"quests": [], "src": {"bone chips": {"k": ["drop"], "z": ["Najena"]}, "gem": {"k": ["drop"], "z": ["Befallen"]}}}"#,
        );
        let zs3: Vec<String> = zone_buckets(&two, &b2, &known)
            .into_iter()
            .map(|k| k.zone)
            .collect();
        assert_eq!(
            zs3,
            vec!["Befallen", "Najena"],
            "a cleared zone sinks below one with something to get"
        );
    }

    #[test]
    fn an_umbrella_city_page_is_named_as_unresolved() {
        let b = book_from(TINY);
        let known: HashSet<String> = ["najena".to_string()].into_iter().collect();
        assert!(b.is_umbrella("Freeport", &known));
        assert!(!b.is_umbrella("Najena", &known));
    }

    #[test]
    fn source_in_says_what_to_do_in_that_zone() {
        let b = book_from(
            r#"{"quests": [], "src": {
          "gem": {"k": ["vendor", "drop"], "z": ["Freeport", "Najena"], "m": {"Najena": ["a skeleton"]}, "v": {"Freeport": [["Merchant Bob", "(1, 2)"]]}},
          "crystal": {"k": ["ground"], "z": ["The Great Divide"], "g": {"The Great Divide": ["(-4948, -699)", "-5000,-650", "third"]}},
          "sky ring": {"k": ["drop"], "z": ["The Plane of Sky"], "isl": "6", "m": {"The Plane of Sky": ["Bazzt Zzzt"]}},
          "herb": {"k": ["forage"], "z": ["Misty"]}
        }}"#,
        );
        assert_eq!(source_in(&b, "Gem", "Najena").mobs, vec!["a skeleton"]);
        assert_eq!(
            source_in(&b, "Gem", "Freeport").note.as_deref(),
            Some("sold by Merchant Bob, (1, 2)")
        );
        assert_eq!(
            source_in(&b, "Crystal", "The Great Divide").note.as_deref(),
            Some("ground spawn, (-4948, -699) / -5000,-650")
        );
        let sky = source_in(&b, "Sky Ring", "The Plane of Sky");
        assert_eq!(sky.isle.as_deref(), Some("Isle 6"));
        assert_eq!(
            source_in(&b, "Herb", "Misty").note.as_deref(),
            Some("forage")
        );
        assert_eq!(source_in(&b, "Unknown", "Misty"), SourceNote::default());
    }

    #[test]
    fn merge_plans_takes_the_larger_want_and_names_every_quest() {
        let mk = |name: &str, comps: Vec<Comp>| Plan {
            key: name.into(),
            quest_ix: 0,
            part: None,
            name: name.into(),
            short: name.into(),
            giver: None,
            rewards: vec!["Prize".into()],
            got: 0,
            need: comps.len(),
            done: false,
            steps: None,
            comps,
        };
        let a = mk(
            "Alpha",
            vec![Comp {
                name: "Bone Chips".into(),
                want: 2,
                have: 0,
                ..Default::default()
            }],
        );
        let c = mk(
            "Charlie",
            vec![
                Comp {
                    name: "Bone Chips".into(),
                    want: 5,
                    have: 0,
                    ..Default::default()
                },
                Comp {
                    name: "Ale".into(),
                    want: 1,
                    have: 0,
                    ..Default::default()
                },
            ],
        );
        let pooled = merge_plans(&[&a, &c]);
        assert_eq!(pooled.len(), 2);
        let chips = pooled.iter().find(|c| c.name == "Bone Chips").unwrap();
        assert_eq!(chips.want, 5);
        assert_eq!(chips.tag.as_deref(), Some("2 quests"));
        assert!(chips.tip.as_deref().unwrap().contains("Alpha: Prize"));
        let ale = pooled.iter().find(|c| c.name == "Ale").unwrap();
        assert_eq!(ale.tag.as_deref(), Some("Charlie"));
    }

    #[test]
    fn a_part_plan_carries_only_that_turn_ins_items() {
        let b = book_from(TINY);
        let q: Quest = serde_json::from_str(r#"{"n": "Sky Tests", "t": "Sky_Tests", "split": true, "items": ["A", "B"],
          "parts": [{"n": "Test of Love", "g": "Aldryn", "c": [["A", 1]], "r": ["Ring"]}, {"n": "Test of War", "c": [["B", 2]]}]}"#).unwrap();
        let h = have_of(&b, &[("A", 0, 1)]);
        let p = part_plan(&q, 0, 0, &h).unwrap();
        assert_eq!(p.key, "Sky_Tests::0");
        assert_eq!(p.short, "Test of Love");
        assert_eq!(p.comps.len(), 1);
        assert!(p.done);
        assert_eq!(p.giver.as_deref(), Some("Aldryn"));
        assert_eq!(p.base_key(), "Sky_Tests");
        let f = Filter {
            q: "aldryn".into(),
            ..Default::default()
        };
        assert!(
            qb_match_part(&q, &q.parts[0], &f),
            "a part matches on its own giver"
        );
        assert!(!qb_match_part(&q, &q.parts[1], &f));
    }

    #[test]
    fn hub_pages_are_read_by_shape_off_want() {
        let items: Vec<String> = (0..20).map(|i| format!("Item {i}")).collect();
        let want: Vec<(String, u32)> = items.iter().map(|n| (n.clone(), 1)).collect();
        let hub = Quest {
            name: "Popular Quests by Level".into(),
            items,
            want,
            ..Default::default()
        };
        assert!(Book::is_hub(&hub));
        let real = Quest {
            name: "Bone Chips".into(),
            giver: Some("Vaal".into()),
            ..hub.clone()
        };
        assert!(!Book::is_hub(&real));
    }

    #[test]
    fn filters_follow_the_old_browser_rules() {
        let b = book_from(TINY);
        let q0 = &b.data.quests[0];
        let q1 = &b.data.quests[1];
        assert!(qb_filters(
            q0,
            &Filter {
                cls: "Cleric".into(),
                ..Default::default()
            }
        ));
        assert!(!qb_filters(
            q0,
            &Filter {
                cls: "Warrior".into(),
                ..Default::default()
            }
        ));
        assert!(
            qb_filters(
                q1,
                &Filter {
                    cls: "Warrior".into(),
                    ..Default::default()
                }
            ),
            "no class list is open to everyone"
        );
        assert!(!qb_filters(
            q1,
            &Filter {
                era: Era::In,
                ..Default::default()
            }
        ));
        assert!(qb_filters(
            q1,
            &Filter {
                era: Era::Out,
                ..Default::default()
            }
        ));
        assert!(!qb_filters(
            q0,
            &Filter {
                lvl_min: 10,
                ..Default::default()
            }
        ));
        assert!(qb_filters(
            q0,
            &Filter {
                lvl_max: 10,
                ..Default::default()
            }
        ));
        assert!(
            qb_match(
                q0,
                &Filter {
                    q: "ALE".into(),
                    ..Default::default()
                }
            ),
            "search reads rewards, case folded"
        );
        assert!(!qb_match(
            q0,
            &Filter {
                q: "sapphire".into(),
                ..Default::default()
            }
        ));
        assert_eq!(
            quest_facts(q1, false),
            "out of era, Kunark · lvl 30 · Cabilis · to Iksar"
        );
        assert_eq!(era_short("Velious Era"), "Velious");
    }

    #[test]
    fn turnins_sort_ready_first_then_in_era_then_completeness_then_size() {
        let b = book_from(TINY);
        let mk = |name: &str, ix: usize, got: usize, need: usize| Plan {
            key: name.into(),
            quest_ix: ix,
            part: None,
            name: name.into(),
            short: name.into(),
            giver: None,
            rewards: vec![],
            comps: vec![],
            got,
            need,
            done: need > 0 && got == need,
            steps: None,
        };
        let mut v = vec![
            mk("half", 0, 1, 2),
            mk("one of one", 0, 1, 1),
            mk("four of four", 0, 4, 4),
            mk("oe done", 1, 1, 1),
            mk("third", 0, 1, 3),
        ];
        sort_turnins(&mut v, &b);
        let names: Vec<&str> = v.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["four of four", "one of one", "oe done", "half", "third"]
        );
    }

    #[test]
    fn browser_sorts_are_stable_by_name() {
        let b = book_from(TINY);
        let mk = |name: &str, ix: usize| Plan {
            key: name.into(),
            quest_ix: ix,
            part: None,
            name: name.into(),
            short: name.into(),
            giver: None,
            rewards: vec![],
            comps: vec![],
            got: 0,
            need: 1,
            done: false,
            steps: None,
        };
        let mut v = vec![mk("zed", 1), mk("alpha", 0), mk("beta", 0)];
        sort_rows(&mut v, &b, SortKey::Lvl, true);
        assert_eq!(
            v.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "beta", "zed"]
        );
        sort_rows(&mut v, &b, SortKey::Lvl, false);
        assert_eq!(v[0].name, "zed");
    }

    #[test]
    fn a_bad_file_is_an_error_with_the_path_not_a_panic() {
        let e =
            Book::from_json("{ not json", Path::new("C:/somewhere/quest-items.json")).unwrap_err();
        assert!(e.starts_with("C:/somewhere/quest-items.json:"), "{e}");
        let e2 = Book::load(Path::new("C:/definitely/not/here/quest-items.json")).unwrap_err();
        assert!(e2.contains("quest-items.json"), "{e2}");
    }

    /* The real file. Fails loudly when absent, unless GRIMOIRE_NO_DATA=1 says so on purpose. */
    fn real_book() -> Option<Book> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("quest-items.json");
        if !path.exists() {
            if std::env::var("GRIMOIRE_NO_DATA").as_deref() == Ok("1") {
                crate::data::testdata::say_skipped(&format!(
                    "{} is absent and GRIMOIRE_NO_DATA=1",
                    path.display()
                ));
                return None;
            }
            panic!("{} is absent. Copy the snapshot into crates/grimoire-desktop/data/, or set GRIMOIRE_NO_DATA=1 to skip on purpose.", path.display());
        }
        Some(Book::load(&path).expect("the real quest-items.json parses"))
    }

    #[test]
    fn the_real_file_parses_whole_and_keeps_its_counts() {
        let Some(b) = real_book() else { return };
        let d = &b.data;
        assert_eq!(d.schema, 6);
        assert!(d.quests.len() >= 900, "{} quests", d.quests.len());
        assert!(d.drops.len() >= 1600, "{} drop tables", d.drops.len());
        assert!(d.npcs.len() >= 1100, "{} npcs", d.npcs.len());
        assert!(d.src.len() >= 3000, "{} sources", d.src.len());
        assert!(d.geo.as_ref().map(|g| g.nodes.len()).unwrap_or(0) >= 100);
        assert!(d.items.len() >= 4000);
        assert!(d.ids.len() >= 190);
        assert_eq!(
            d.meta.get("license").and_then(Value::as_str),
            Some("CC BY-SA 4.0")
        );
        // positional rows are kept as they were observed: four strings per drop row
        let row = d
            .drops
            .values()
            .next()
            .and_then(|v| v.first())
            .expect("a drop row");
        assert_eq!(row.len(), 4);
        assert!(row.iter().all(Value::is_string));
        // every quest has a key and nothing invented a hub flag
        assert!(d.quests.iter().all(|q| !q.t.is_empty()));
        assert_eq!(
            d.quests.iter().filter(|q| Book::is_hub(q)).count(),
            3,
            "the three index pages"
        );
    }

    #[test]
    fn the_real_coldain_ring_chain_reads_off_the_bag() {
        let Some(b) = real_book() else { return };
        let (ix, q) = b
            .quest("10th_Coldain_Ring_Quest")
            .expect("the ring quest is in the file");
        assert!(!q.steps.is_empty());
        let h = have_of(&b, &[]);
        let p = comps_for(q, ix, &h, &b);
        assert!(!p.ready());
        assert!(p.steps.as_ref().unwrap().done_n == 0);
        let with = have_of(&b, &[("Dirk of the Dain", 0, 1)]);
        let p2 = comps_for(q, ix, &with, &b);
        assert!(p2.got >= 1);
        assert!(!p2.ready(), "the dirk is step one, not the final hand-in");
    }

    #[test]
    fn the_real_file_finds_a_job_for_nanrum_by_zone_and_giver() {
        let Some(b) = real_book() else { return };
        let (ix, q) = b.quest("A_Job_for_Nanrum").unwrap();
        assert_eq!(q.giver.as_deref(), Some("Basher Nanrum"));
        assert_eq!(q.zone.as_deref(), Some("Grobb"));
        let h = have_of(&b, &[("Fire Beetle Eye", 0, 3)]);
        let p = comps_for(q, ix, &h, &b);
        assert!(p.done, "three eyes is the whole turn-in");
        let known = HashSet::new();
        let buckets = zone_buckets(&p.comps, &b, &known);
        assert!(!buckets.is_empty());
    }
}
