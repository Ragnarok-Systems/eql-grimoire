//! Screen: gear. Decision D9: what your worn set is worth, and what the catalogue would beat it
//! with.
//!
//! WHAT IS HERE, over one `/outputfile inventory` dump: the slot vocabulary and the tier math, the
//! stat weights and the scorer, the catalogue ranker (how strong an item is for a class, as its
//! standing in that class's own list), and the spare verdict ("which of the things I own does
//! nobody need"). Every rule carries a test below that fails without it. The dump parser and the
//! copies rule live next door in `inventory.rs`, because they are facts about the dump rather
//! than about the catalogue.
//!
//! WHY THE WEIGHTS ARE DERIVED AND NOT TYPED. A weight per stat is derived from the trio and the
//! level, and every weight carries a sentence saying where it came from. The player may override
//! any of them; the overrides are the "stat priorities" list at the bottom of this screen, kept in
//! settings under one key, so every surface that prices an item reads the same overrides from one
//! place. Overrides move the trio's numbers only. The per-class catalogue rank on the Inventory
//! screen keeps each class's own computed weights, so an override that says what AC is worth to
//! YOU never rewrites what it is worth to a Cleric.
//!
//! WHAT THE SCORER CANNOT SEE. The score prices stats, AC (through the class softcap), resists,
//! haste and weapon ratio. It has no term for a click, a proc, a worn effect or a bard's
//! instrument. Every surface that shows a score says so next to the number rather than letting an
//! empty cell read as a verdict.
//!
//! RACE IS NOT A SETTING. A race would let STA, INT and WIS be priced from the character's own
//! totals. The settings contract carries no race, so every derivation here runs the no-race path,
//! which assumes the 101 to 200 band. The
//! `cur` argument survives on the functions so a race can be wired later without a rewrite.

use crate::screens::Cx;
use crate::settings::Settings;
use crate::theme::*;
use egui::{FontId, RichText, Ui};
use serde_json::Value;
use std::collections::HashMap;

/* ------------------------------------------------------------------ classes -- */

/// The sixteen class codes, in the game's class order.
pub const CLASSES: [&str; 16] = [
    "WAR", "CLR", "PAL", "RNG", "SHD", "DRU", "MNK", "BRD", "ROG", "SHM", "NEC", "WIZ", "MAG",
    "ENC", "BST", "BER",
];
/// The full names, same order.
pub const CLASS_NAMES: [&str; 16] = [
    "Warrior",
    "Cleric",
    "Paladin",
    "Ranger",
    "Shadow Knight",
    "Druid",
    "Monk",
    "Bard",
    "Rogue",
    "Shaman",
    "Necromancer",
    "Wizard",
    "Magician",
    "Enchanter",
    "Beastlord",
    "Berserker",
];
pub const MAX_LEVEL: u32 = 50;
/// The Item Upgrade System cap: the highest tier a name can carry.
pub const MAX_TIER: u32 = 10;

pub fn class_name(code: &str) -> &str {
    CLASSES
        .iter()
        .position(|c| *c == code)
        .map(|i| CLASS_NAMES[i])
        .unwrap_or(code)
}

/// The canonical `&'static str` for a class code, or None when it is not one of the sixteen.
pub fn class_code(code: &str) -> Option<&'static str> {
    CLASSES.iter().find(|c| **c == code).copied()
}

/* -------------------------------------------------------------------- slots -- */

/// "Any Slot": worn, but never named by an item record.
pub const ANY: &str = "Any Slot";

/// The placement vocabulary: the slot names the wiki puts on an item.
pub const SLOTS: [&str; 20] = [
    "Charm",
    "Ear",
    "Head",
    "Face",
    "Neck",
    "Shoulders",
    "Arms",
    "Back",
    "Wrist",
    "Range",
    "Hands",
    "Primary",
    "Secondary",
    "Fingers",
    "Chest",
    "Legs",
    "Feet",
    "Waist",
    "Ammo",
    "Power Source",
];

/// What the DUMP can report you wearing: SLOTS plus Any Slot. Worn AC and worn stat totals are
/// summed over this, not over SLOTS: reading only SLOTS silently drops whatever is worn in Any
/// Slot, whose AC is on the character just the same. ONE table: the ingest reads the dump and
/// owns the list; this screen re-exports it rather than carrying a second copy that could drift
/// by one slot. A test below holds the last entry to `ANY`.
pub use crate::ingest::WORN_SLOTS;

/// How many positions a slot has: Ear, Wrist, Fingers and Any Slot hold two,
/// so second best is still worn.
pub fn paired(slot: &str) -> usize {
    match slot {
        "Ear" | "Wrist" | "Fingers" | ANY => 2,
        _ => 1,
    }
}

/// Is this location string a worn slot, and which one.
/// `Finger` and `Ring` are the client's older spellings of `Fingers` and are folded into it, the
/// way parseInventory does.
pub fn worn_slot(loc: &str) -> Option<&'static str> {
    match loc {
        "Finger" | "Ring" => Some("Fingers"),
        _ => WORN_SLOTS.iter().find(|s| **s == loc).copied(),
    }
}

/// Is the weapon two handed: the skill starts with `2H` or contains `Two`,
/// case folded. A two-hander takes the off hand with it, which is why it is its own catalogue in
/// the main hand and can never sit in the off hand.
pub fn two_h(skill: Option<&str>) -> bool {
    let Some(s) = skill else { return false };
    let up = s.to_ascii_uppercase();
    up.starts_with("2H") || up.contains("TWO")
}

/* ------------------------------------------------------------------- stats -- */

/// The fifteen weights, key and label, in the order the priorities panel lists them.
pub const STAT_DEFS: [(&str, &str); 15] = [
    ("ac", "AC"),
    ("hp", "HP"),
    ("mana", "Mana"),
    ("end", "Endurance"),
    ("str", "STR"),
    ("sta", "STA"),
    ("agi", "AGI"),
    ("dex", "DEX"),
    ("wis", "WIS"),
    ("int", "INT"),
    ("cha", "CHA"),
    ("atk", "Attack"),
    ("sv", "Resists (each; SV VOID unscored)"),
    ("haste", "Haste (per 1%)"),
    ("ratio", "Weapon ratio (dmg/dly x10)"),
];

/// The twelve an item's stat block can carry, in breakdown order.
pub const ITEM_STATS: [(&str, &str); 12] = [
    ("ac", "AC"),
    ("hp", "HP"),
    ("mana", "Mana"),
    ("end", "Endurance"),
    ("str", "STR"),
    ("sta", "STA"),
    ("agi", "AGI"),
    ("dex", "DEX"),
    ("wis", "WIS"),
    ("int", "INT"),
    ("cha", "CHA"),
    ("atk", "Attack"),
];

/// Resist keys as gear-data spells them, and what they mean.
pub fn sv_name(k: &str) -> &'static str {
    match k {
        "f" => "FIRE",
        "c" => "COLD",
        "m" => "MAGIC",
        "d" => "DISEASE",
        "p" => "POISON",
        "v" => "VOID",
        _ => "?",
    }
}

fn mana_stat(cls: &str) -> Option<&'static str> {
    match cls {
        "BRD" | "SHD" | "NEC" | "WIZ" | "MAG" | "ENC" => Some("int"),
        "PAL" | "RNG" | "CLR" | "DRU" | "SHM" | "BST" => Some("wis"),
        _ => None,
    }
}
const FULL_CASTER: [&str; 7] = ["CLR", "DRU", "SHM", "NEC", "WIZ", "MAG", "ENC"];
const PURE_MELEE: [&str; 4] = ["WAR", "MNK", "ROG", "BER"];
const TANKS: [&str; 3] = ["WAR", "PAL", "SHD"];

/// The ancestral per-class HP factor at 50.
fn lm(cls: &str) -> f64 {
    match cls {
        "WAR" => 270.0,
        "SHD" | "PAL" | "BER" => 230.0,
        "RNG" => 200.0,
        "MNK" | "BRD" | "ROG" | "BST" => 180.0,
        "CLR" | "DRU" | "SHM" => 150.0,
        "MAG" | "WIZ" | "NEC" | "ENC" => 120.0,
        _ => 0.0,
    }
}
const MANA_FAC_50: f64 = 4.5;
const STAT_CAP: f64 = 510.0;
const STA_DR: f64 = 255.0;

/* --------------------------------------------------------------- tier math -- */

/// Item Upgrade System tier math, the rule the wiki documents:
/// stat@N = max(floor(base x (1 + N/10)), stat@(N-1) + 1). Negative stats are left as they are,
/// because the wiki states the rule for increases only.
///
/// Integer arithmetic on purpose: `base * (1 + i/10)` in floating point hits IEEE-754 traps
/// (45 x 1.4 = 62.999...) and floors one short.
pub fn stat_at(base: i32, n: u32) -> i32 {
    if base <= 0 || n == 0 {
        return base;
    }
    let mut s = base;
    for i in 1..=n as i32 {
        s = std::cmp::max(base * (10 + i) / 10, s + 1);
    }
    s
}

/* ------------------------------------------------------------- the record -- */

/// gear-data's four class and race set shapes (`{"all":1}`, `{"all":1,"x":[..]}`, `{"c":[..]}`,
/// `{"none":1}`), plus the record that has no line at all, which is unrestricted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Set {
    /// No `cls` or `rc` field: the wiki page had no line, so nothing is excluded.
    Unstated,
    /// Everyone except the listed codes.
    All { except: Vec<String> },
    /// Exactly these.
    Only(Vec<String>),
    /// Nobody.
    NoOne,
}

impl Set {
    fn from_value(v: Option<&Value>) -> Set {
        let Some(v) = v else { return Set::Unstated };
        let Some(o) = v.as_object() else {
            return Set::Unstated;
        };
        if o.get("none").map(truthy).unwrap_or(false) {
            return Set::NoOne;
        }
        if o.get("all").map(truthy).unwrap_or(false) {
            return Set::All {
                except: str_list(o.get("x")),
            };
        }
        Set::Only(str_list(o.get("c")))
    }

    /// The set as a list of class codes, or None for "any class".
    pub fn classes(&self) -> Option<Vec<&'static str>> {
        match self {
            Set::Unstated => None,
            Set::NoOne => Some(Vec::new()),
            Set::All { except } => {
                if except.is_empty() {
                    None
                } else {
                    Some(
                        CLASSES
                            .iter()
                            .copied()
                            .filter(|c| !except.iter().any(|x| x == c))
                            .collect(),
                    )
                }
            }
            Set::Only(c) => Some(c.iter().filter_map(|k| class_code(k)).collect()),
        }
    }

    /// The class list as a column reads it.
    pub fn text(&self) -> String {
        match self {
            Set::Unstated => String::new(),
            Set::NoOne => "NONE".into(),
            Set::All { except } if except.is_empty() => "ALL".into(),
            Set::All { except } => format!("ALL except {}", except.join(" ")),
            Set::Only(c) => c.join(" "),
        }
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn int_of(v: Option<&Value>) -> Option<i32> {
    let v = v?;
    match v {
        Value::Number(n) => n.as_f64().map(|f| f as i32),
        Value::String(s) => s.trim().parse::<f64>().ok().map(|f| f as i32),
        _ => None,
    }
}

fn int_map(v: Option<&Value>) -> Vec<(String, i32)> {
    let mut out = Vec::new();
    if let Some(o) = v.and_then(Value::as_object) {
        for (k, x) in o {
            if let Some(n) = int_of(Some(x)) {
                out.push((k.clone(), n));
            }
        }
    }
    out
}

/// One gear-data item record, as the scorer reads it. Field names are gear-data.json's own
/// (measured 2026-09-02 over 6891 records): `n`, `t`, `cls`, `rc`, `sl`, `st`, `sv`, `dmg`, `dly`,
/// `skill`, `haste`, `req`, `deity`, `wt`, `era`, `oe`, `fl`, `eff`, `foc`, `inst`, `charges`,
/// `src`. Anything else on the record is not read here.
#[derive(Clone, Debug)]
pub struct GearRec {
    pub name: String,
    /// The wiki title (`t`), the identity every other dataset keys by. Falls back to the name.
    pub key: String,
    pub cls: Set,
    pub rc: Set,
    pub sl: Vec<String>,
    /// The stat block, keyed as gear-data keys it (`ac hp mana end str sta agi dex wis int cha atk`).
    pub st: Vec<(String, i32)>,
    /// Resists, keyed `f c m d p v`.
    pub sv: Vec<(String, i32)>,
    pub dmg: Option<i32>,
    pub dly: Option<i32>,
    pub skill: Option<String>,
    pub haste: i32,
    pub req: u32,
    pub deity: bool,
    pub wt: Option<f64>,
    pub era: Option<String>,
    /// Out of era: not in the catalogue, ranked by nothing.
    pub oe: bool,
    pub fl: Vec<String>,
    /// Effect names (`eff[].n`), for the Effect column and the "unscored" caveat.
    pub eff: Vec<String>,
    pub foc: bool,
    pub inst: bool,
    pub charges: u32,
    /// The zones the wiki lists it dropping in (`src.d[][0]`), in page order. `src.d` is a
    /// positional array: index 0 is the zone name, index 1 the list of `[mob, level, rarity]`.
    pub src_zones: Vec<String>,
    /// `src.q`: the wiki lists it as a quest reward.
    pub quest: bool,
}

impl GearRec {
    /// Read a record out of its JSON. None when it has no name: a nameless record cannot be looked
    /// up by anything and would only pollute the pools.
    pub fn from_value(v: &Value) -> Option<GearRec> {
        let o = v.as_object()?;
        /* `n` is gear-data's field; `name` is what the data lane's typed record serialises when
         * it did not rename the field back. Either is the same fact. */
        let name = o.get("n").or_else(|| o.get("name"))?.as_str()?.to_owned();
        if name.is_empty() {
            return None;
        }
        let key = o
            .get("t")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| name.clone());
        let mut src_zones = Vec::new();
        let mut quest = false;
        if let Some(src) = o.get("src").and_then(Value::as_object) {
            quest = src.get("q").map(truthy).unwrap_or(false);
            if let Some(d) = src.get("d").and_then(Value::as_array) {
                for entry in d {
                    /* positional: [zone, [[mob, lvl, rarity], ...]] */
                    if let Some(z) = entry
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(Value::as_str)
                    {
                        src_zones.push(z.to_owned());
                    }
                }
            }
        }
        Some(GearRec {
            name,
            key,
            cls: Set::from_value(o.get("cls")),
            rc: Set::from_value(o.get("rc")),
            sl: str_list(o.get("sl")),
            st: int_map(o.get("st")),
            sv: int_map(o.get("sv")),
            dmg: int_of(o.get("dmg")),
            dly: int_of(o.get("dly")),
            skill: o.get("skill").and_then(Value::as_str).map(str::to_owned),
            haste: int_of(o.get("haste")).unwrap_or(0),
            req: int_of(o.get("req")).map(|r| r.max(0) as u32).unwrap_or(0),
            deity: o.get("deity").map(truthy).unwrap_or(false),
            wt: o.get("wt").and_then(Value::as_f64),
            era: o.get("era").and_then(Value::as_str).map(str::to_owned),
            oe: o.get("oe").map(truthy).unwrap_or(false),
            fl: str_list(o.get("fl")),
            eff: o
                .get("eff")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e.get("n").and_then(Value::as_str).map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            foc: o.get("foc").map(truthy).unwrap_or(false),
            inst: o
                .get("inst")
                .and_then(Value::as_array)
                .map(|a| !a.is_empty())
                .unwrap_or(false),
            charges: int_of(o.get("charges"))
                .map(|c| c.max(0) as u32)
                .unwrap_or(0),
            src_zones,
            quest,
        })
    }

    pub fn stat(&self, k: &str) -> i32 {
        self.st
            .iter()
            .find(|(s, _)| s == k)
            .map(|(_, v)| *v)
            .unwrap_or(0)
    }

    /// Every stat at a tier.
    pub fn stats_at(&self, tier: u32) -> Vec<(&str, i32)> {
        self.st
            .iter()
            .map(|(k, v)| (k.as_str(), stat_at(*v, tier)))
            .collect()
    }

    /// Does the score read anything on it: a stat block, a resist, a
    /// haste line or a damage value. A record with only a click has nothing to rank.
    pub fn scorable(&self) -> bool {
        !self.st.is_empty() || !self.sv.is_empty() || self.haste != 0 || self.dmg.is_some()
    }

    /// The slots the record names that are real placements.
    pub fn real_slots(&self) -> Vec<&str> {
        self.sl
            .iter()
            .map(String::as_str)
            .filter(|s| SLOTS.contains(s))
            .collect()
    }

    /// The slots the record names that the dump can report worn.
    pub fn worn_slots(&self) -> Vec<&str> {
        self.sl
            .iter()
            .map(String::as_str)
            .filter(|s| WORN_SLOTS.contains(s))
            .collect()
    }

    /// The record's class list. None is any class.
    pub fn classes(&self) -> Option<Vec<&'static str>> {
        self.cls.classes()
    }

    pub fn is_two_h(&self) -> bool {
        two_h(self.skill.as_deref())
    }

    /// The trade state, read off the flags line.
    pub fn trade(&self) -> &'static str {
        if self.fl.iter().any(|f| f == "no_drop") {
            "no drop"
        } else if self.fl.iter().any(|f| f == "no_trade") {
            "no trade"
        } else if self.fl.iter().any(|f| f == "attunable") {
            "attunable"
        } else {
            "yes"
        }
    }

    /// The reasons a score under this item is not the whole story.
    pub fn unscored(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.inst {
            out.push("inst");
        }
        if !self.eff.is_empty() {
            out.push("effect");
        }
        if self.charges > 0 {
            out.push("charges");
        }
        out
    }
}

/* --------------------------------------------------------------- the names -- */

/// Item IDENTITY key, everything the datasets are keyed by: backticks and
/// apostrophes out, lower case, whitespace collapsed. NOT article stripping: `Sapphire` and
/// `A Sapphire` are two items the wiki keeps apart on purpose.
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

/// The loose lookup key: the identity key plus one leading article off. Only ever a
/// FALLBACK for a game name whose article the wiki title does not carry.
pub fn norm_name(s: &str) -> String {
    let k = item_key(s);
    for art in ["a ", "an ", "the "] {
        if let Some(rest) = k.strip_prefix(art) {
            return rest.to_owned();
        }
    }
    k
}

/// Peel the client's decorations off a dump name, outermost first:
/// trailing stars, ` (Exaltation)`, ` +N`.
pub fn strip_decor(name: &str) -> String {
    let s = name.trim_end_matches('*');
    let s = s
        .strip_suffix("(Exaltation)")
        .map(str::trim_end)
        .unwrap_or(s);
    crate::screens::inventory::base_name(s).0
}

/// The whole in-era and out-of-era catalogue, indexed by name. Built once per snapshot and kept.
pub struct Catalogue {
    pub recs: Vec<GearRec>,
    exact: HashMap<String, usize>,
    loose: HashMap<String, usize>,
    /// How many snapshot records were read, whether or not they became a GearRec.
    pub source_items: usize,
}

impl Catalogue {
    /// From item JSON values. This is the reader; the snapshot adapter and the data tests both go
    /// through it.
    pub fn from_values<'a>(items: impl Iterator<Item = &'a Value>) -> Catalogue {
        let mut recs = Vec::new();
        let mut source_items = 0;
        for v in items {
            source_items += 1;
            if let Some(r) = GearRec::from_value(v) {
                recs.push(r);
            }
        }
        let mut exact: HashMap<String, usize> = HashMap::new();
        for (i, r) in recs.iter().enumerate() {
            /* first wins, the way gear-data's names map took keys[0] */
            exact.entry(item_key(&r.name)).or_insert(i);
        }
        /* the article-stripped key, with an article-LESS title winning any tie */
        let mut loose: HashMap<String, usize> = HashMap::new();
        let mut keys: Vec<&String> = exact.keys().collect();
        keys.sort();
        for k in keys {
            let lk = norm_name(k);
            let i = exact[k];
            if &lk == k {
                loose.insert(lk, i);
            } else {
                loose.entry(lk).or_insert(i);
            }
        }
        Catalogue {
            recs,
            exact,
            loose,
            source_items,
        }
    }

    /// From the snapshot the data lane loaded.
    ///
    /// THE ONE PLACE THIS FILE TOUCHES THE DATA LANE'S ITEM TYPE. The record is round-tripped
    /// through serde so the reader sees gear-data's own field names whether the data lane typed a
    /// field or left it in `extra`. Assumed: `crate::data::Item: serde::Serialize` and every typed
    /// field keeps its gear-data name (a `rename` if the Rust field is called something else).
    pub fn from_snapshot(snap: &crate::data::Snapshot) -> Result<Catalogue, String> {
        let mut vals = Vec::with_capacity(snap.items.len());
        for it in &snap.items {
            vals.push(
                serde_json::to_value(it)
                    .map_err(|e| format!("item record would not serialise: {e}"))?,
            );
        }
        Ok(Catalogue::from_values(vals.iter()))
    }

    /// Resolve a name off a dump row: exact
    /// first, exact on the decoration-stripped name, then the article-stripped fallbacks.
    pub fn find(&self, name: &str) -> Option<usize> {
        let stripped = strip_decor(name);
        self.exact
            .get(&item_key(name))
            .or_else(|| self.exact.get(&item_key(&stripped)))
            .or_else(|| self.loose.get(&norm_name(name)))
            .or_else(|| self.loose.get(&norm_name(&stripped)))
            .copied()
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }
}

/* ------------------------------------------------------------- the softcaps -- */

/// AC softcap per class, index = level - 1 (1..=50), plus the multiplier that prices AC past the
/// cap.
///
/// WHERE THESE NUMBERS CAME FROM IS NOT RECORDED, AND THAT IS A DEFECT IN THIS TABLE RATHER THAN
/// A DETAIL OF IT. Nothing in this repository derives, regenerates or checks them: there is no
/// emitter, no checked-in source to diff against, and no test that would notice if a single row
/// were wrong. An earlier version of this comment said the table was derived from the client's
/// own files. That sentence is gone rather than reworded, because no such derivation exists here
/// and a sourcing claim a reader cannot follow is worse than an admitted gap.
///
/// The rule this table is owed: a closed enumeration belongs in a generated `const`, emitted from a
/// checked-in source, with a test asserting that regeneration reproduces the committed bytes. Until
/// that exists, treat every number below as unverified.
///
/// It is kept in its unverified state because deleting it fails in a silent direction: with no
/// table at all a missing cap reads as a cap of ZERO, so every point of AC on every item is
/// priced at the past-cap multiplier instead of in full. Every score carrying AC moves, and
/// nothing errors.
pub const SOFTCAPS: [(&str, f32, [u16; 50]); 16] = [
    (
        "WAR",
        0.35,
        [
            312, 314, 316, 318, 320, 322, 324, 326, 328, 330, 332, 334, 336, 338, 340, 342, 344,
            346, 348, 350, 352, 354, 356, 358, 360, 362, 364, 366, 368, 370, 372, 374, 376, 378,
            380, 382, 384, 386, 388, 390, 392, 394, 396, 398, 400, 402, 404, 406, 408, 410,
        ],
    ),
    (
        "CLR",
        0.3,
        [
            274, 276, 278, 278, 280, 282, 284, 286, 288, 290, 292, 292, 294, 296, 298, 300, 302,
            304, 306, 308, 308, 310, 312, 314, 316, 318, 320, 322, 322, 324, 326, 328, 330, 332,
            334, 336, 336, 338, 340, 342, 344, 346, 348, 350, 352, 352, 354, 356, 358, 360,
        ],
    ),
    (
        "PAL",
        0.33,
        [
            298, 300, 302, 304, 306, 308, 310, 312, 314, 316, 318, 320, 322, 324, 326, 328, 330,
            332, 334, 336, 336, 338, 340, 342, 344, 346, 348, 350, 352, 354, 356, 358, 360, 362,
            364, 366, 368, 370, 372, 374, 376, 378, 380, 382, 384, 384, 386, 388, 390, 392,
        ],
    ),
    (
        "RNG",
        0.315,
        [
            286, 288, 290, 292, 294, 296, 298, 298, 300, 302, 304, 306, 308, 310, 312, 314, 316,
            318, 320, 322, 322, 324, 326, 328, 330, 332, 334, 336, 338, 340, 342, 344, 344, 346,
            348, 350, 352, 354, 356, 358, 360, 362, 364, 366, 368, 368, 370, 372, 374, 376,
        ],
    ),
    (
        "SHD",
        0.33,
        [
            298, 300, 302, 304, 306, 308, 310, 312, 314, 316, 318, 320, 322, 324, 326, 328, 330,
            332, 334, 336, 336, 338, 340, 342, 344, 346, 348, 350, 352, 354, 356, 358, 360, 362,
            364, 366, 368, 370, 372, 374, 376, 378, 380, 382, 384, 384, 386, 388, 390, 392,
        ],
    ),
    (
        "DRU",
        0.265,
        [
            254, 256, 258, 260, 262, 264, 264, 266, 268, 270, 272, 272, 274, 276, 278, 280, 282,
            282, 284, 286, 288, 290, 290, 292, 294, 296, 298, 300, 300, 302, 304, 306, 308, 308,
            310, 312, 314, 316, 318, 318, 320, 322, 324, 326, 328, 328, 330, 332, 334, 336,
        ],
    ),
    (
        "MNK",
        0.3,
        [
            274, 276, 278, 278, 280, 282, 284, 286, 288, 290, 292, 292, 294, 296, 298, 300, 302,
            304, 306, 308, 308, 310, 312, 314, 316, 318, 320, 322, 322, 324, 326, 328, 330, 332,
            334, 336, 336, 338, 340, 342, 344, 346, 348, 350, 352, 352, 354, 356, 358, 360,
        ],
    ),
    (
        "BRD",
        0.3,
        [
            274, 276, 278, 278, 280, 282, 284, 286, 288, 290, 292, 292, 294, 296, 298, 300, 302,
            304, 306, 308, 308, 310, 312, 314, 316, 318, 320, 322, 322, 324, 326, 328, 330, 332,
            334, 336, 336, 338, 340, 342, 344, 346, 348, 350, 352, 352, 354, 356, 358, 360,
        ],
    ),
    (
        "ROG",
        0.28,
        [
            264, 266, 268, 270, 272, 272, 274, 276, 278, 280, 282, 282, 284, 286, 288, 290, 292,
            294, 294, 296, 298, 300, 302, 304, 306, 306, 308, 310, 312, 314, 316, 316, 318, 320,
            322, 324, 326, 328, 328, 330, 332, 334, 336, 338, 340, 340, 342, 344, 346, 348,
        ],
    ),
    (
        "SHM",
        0.28,
        [
            264, 266, 268, 270, 272, 272, 274, 276, 278, 280, 282, 282, 284, 286, 288, 290, 292,
            294, 294, 296, 298, 300, 302, 304, 306, 306, 308, 310, 312, 314, 316, 316, 318, 320,
            322, 324, 326, 328, 328, 330, 332, 334, 336, 338, 340, 340, 342, 344, 346, 348,
        ],
    ),
    (
        "NEC",
        0.25,
        [
            248, 250, 252, 254, 256, 256, 258, 260, 262, 264, 264, 266, 268, 270, 272, 272, 274,
            276, 278, 280, 280, 282, 284, 286, 288, 288, 290, 292, 294, 296, 296, 298, 300, 302,
            304, 304, 306, 308, 310, 312, 312, 314, 316, 318, 320, 320, 322, 324, 326, 328,
        ],
    ),
    (
        "WIZ",
        0.25,
        [
            248, 250, 252, 254, 256, 256, 258, 260, 262, 264, 264, 266, 268, 270, 272, 272, 274,
            276, 278, 280, 280, 282, 284, 286, 288, 288, 290, 292, 294, 296, 296, 298, 300, 302,
            304, 304, 306, 308, 310, 312, 312, 314, 316, 318, 320, 320, 322, 324, 326, 328,
        ],
    ),
    (
        "MAG",
        0.25,
        [
            248, 250, 252, 254, 256, 256, 258, 260, 262, 264, 264, 266, 268, 270, 272, 272, 274,
            276, 278, 280, 280, 282, 284, 286, 288, 288, 290, 292, 294, 296, 296, 298, 300, 302,
            304, 304, 306, 308, 310, 312, 312, 314, 316, 318, 320, 320, 322, 324, 326, 328,
        ],
    ),
    (
        "ENC",
        0.25,
        [
            248, 250, 252, 254, 256, 256, 258, 260, 262, 264, 264, 266, 268, 270, 272, 272, 274,
            276, 278, 280, 280, 282, 284, 286, 288, 288, 290, 292, 294, 296, 296, 298, 300, 302,
            304, 304, 306, 308, 310, 312, 312, 314, 316, 318, 320, 320, 322, 324, 326, 328,
        ],
    ),
    (
        "BST",
        0.28,
        [
            264, 266, 268, 270, 272, 272, 274, 276, 278, 280, 282, 282, 284, 286, 288, 290, 292,
            294, 294, 296, 298, 300, 302, 304, 306, 306, 308, 310, 312, 314, 316, 316, 318, 320,
            322, 324, 326, 328, 328, 330, 332, 334, 336, 338, 340, 340, 342, 344, 346, 348,
        ],
    ),
    (
        "BER",
        0.28,
        [
            264, 266, 268, 270, 272, 272, 274, 276, 278, 280, 282, 282, 284, 286, 288, 290, 292,
            294, 294, 296, 298, 300, 302, 304, 306, 306, 308, 310, 312, 314, 316, 316, 318, 320,
            322, 324, 326, 328, 328, 330, 332, 334, 336, 338, 340, 340, 342, 344, 346, 348,
        ],
    ),
];

/// Dual Wield skill cap per class, index = level - 1. Six classes have the skill at all, and a
/// class absent from this table has no Dual Wield whatsoever: that absence is the gate which
/// keeps a Cleric's off hand from filling up with every one-hander in the game.
///
/// Unsourced in this repository for exactly the reason [`SOFTCAPS`] above is, and owed the same
/// generated-table fix. Treat the numbers as unverified.
pub const DUAL_WIELD: [(&str, [u16; 50]); 6] = [
    (
        "WAR",
        [
            10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100, 105, 110,
            115, 120, 125, 130, 135, 140, 145, 150, 155, 160, 165, 170, 175, 180, 185, 190, 195,
            200, 205, 210, 210, 210, 210, 210, 210, 210, 210, 210, 210,
        ],
    ),
    (
        "RNG",
        [
            10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100, 105, 110,
            115, 120, 125, 130, 135, 140, 145, 150, 155, 160, 165, 170, 175, 180, 185, 190, 195,
            200, 205, 210, 210, 210, 210, 210, 210, 210, 210, 210, 210,
        ],
    ),
    (
        "MNK",
        [
            12, 19, 26, 33, 40, 47, 54, 61, 68, 75, 82, 89, 96, 103, 110, 117, 124, 131, 138, 145,
            152, 159, 166, 173, 180, 187, 194, 201, 208, 215, 222, 229, 236, 243, 250, 252, 252,
            252, 252, 252, 252, 252, 252, 252, 252, 252, 252, 252, 252, 252,
        ],
    ),
    (
        "BRD",
        [
            10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100, 105, 110,
            115, 120, 125, 130, 135, 140, 145, 150, 155, 160, 165, 170, 175, 180, 185, 190, 195,
            200, 205, 210, 210, 210, 210, 210, 210, 210, 210, 210, 210,
        ],
    ),
    (
        "ROG",
        [
            11, 17, 23, 29, 35, 41, 47, 53, 59, 65, 71, 77, 83, 89, 95, 101, 107, 113, 119, 125,
            131, 137, 143, 149, 155, 161, 167, 173, 179, 185, 191, 197, 203, 209, 210, 210, 210,
            210, 210, 210, 210, 210, 210, 210, 210, 210, 210, 210, 210, 210,
        ],
    ),
    (
        "BST",
        [
            10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100, 105, 110,
            115, 120, 125, 130, 135, 140, 145, 150, 155, 160, 165, 170, 175, 180, 185, 190, 195,
            200, 205, 210, 210, 210, 210, 210, 210, 210, 210, 210, 210,
        ],
    ),
];

fn level_index(level: u32) -> usize {
    (level.clamp(1, MAX_LEVEL) - 1) as usize
}

/// The softcap for a trio: the highest cap among the classes, with that class's multiplier. A
/// class with no table contributes nothing; no classes at all is a zero cap at a quarter rate.
pub fn softcap(classes: &[String], level: u32) -> (f64, f64) {
    let mut v = 0.0;
    let mut mult = 0.25;
    for c in classes {
        if let Some((_, m, caps)) = SOFTCAPS.iter().find(|(k, _, _)| k == c) {
            let capv = caps[level_index(level)] as f64;
            if capv > v {
                v = capv;
                mult = *m as f64;
            }
        }
    }
    (v, mult)
}

/// The best Dual Wield cap among the classes at the level.
pub fn dual_wield_cap(classes: &[String], level: u32) -> u16 {
    let mut v = 0;
    for c in classes {
        if let Some((_, caps)) = DUAL_WIELD.iter().find(|(k, _)| k == c) {
            v = v.max(caps[level_index(level)]);
        }
    }
    v
}

/* ------------------------------------------------------------- the weights -- */

/// One weight per STAT_DEFS key, in that order.
#[derive(Clone, Debug, PartialEq)]
pub struct Weights(pub [f64; 15]);

impl Weights {
    pub fn get(&self, k: &str) -> f64 {
        STAT_DEFS
            .iter()
            .position(|(s, _)| *s == k)
            .map(|i| self.0[i])
            .unwrap_or(0.0)
    }
    fn set(&mut self, k: &str, v: f64) {
        if let Some(i) = STAT_DEFS.iter().position(|(s, _)| *s == k) {
            self.0[i] = v;
        }
    }
}

/// The character's current stat totals, when a race is known. Never populated tonight (see the
/// file header), carried so a race can be wired in without reshaping the derivation.
#[derive(Clone, Debug, Default)]
pub struct CurrentStats {
    pub sta: f64,
    pub int: f64,
    pub wis: f64,
    pub str_: f64,
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

struct Pool {
    c: String,
    stat: &'static str,
    full: bool,
}

/// The best 2 of 3 mana pools, decided by the CLASSES and never by the
/// gear. A full caster's pool is structurally the big one, so full-caster status leads and the
/// shared-stat count only breaks a tie between equals.
fn pool_selection(classes: &[String]) -> (Vec<Pool>, usize) {
    let mut pools: Vec<Pool> = classes
        .iter()
        .filter_map(|c| {
            mana_stat(c).map(|stat| Pool {
                c: c.clone(),
                stat,
                full: FULL_CASTER.contains(&c.as_str()),
            })
        })
        .collect();
    let mut count: HashMap<&str, usize> = HashMap::new();
    for p in &pools {
        *count.entry(p.stat).or_insert(0) += 1;
    }
    let rank = |p: &Pool| (if p.full { 100 } else { 0 }) + (if count[p.stat] > 1 { 1 } else { 0 });
    /* stable sort, descending */
    pools.sort_by_key(|p| std::cmp::Reverse(rank(p)));
    let counted = pools.len().min(2);
    (pools, counted)
}

struct HpPerSta {
    rate: f64,
    counted: Vec<String>,
    note: &'static str,
}

/// HP per point of STA: STA feeds the two counted HP pools (the two highest class factors),
/// each gaining level x factor / 3000 per point; halved past STA 255, zero at the 510 cap.
fn hp_per_sta(classes: &[String], level: u32, cur: Option<&CurrentStats>) -> HpPerSta {
    let mut ranked: Vec<String> = classes.to_vec();
    ranked.sort_by(|a, b| {
        lm(b)
            .partial_cmp(&lm(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let counted: Vec<String> = ranked.into_iter().take(2).collect();
    let mut rate = level as f64 * counted.iter().map(|c| lm(c)).sum::<f64>() / 3000.0;
    let mut note = "";
    if let Some(c) = cur {
        if c.sta >= STAT_CAP {
            rate = 0.0;
            note = "cap";
        } else if c.sta >= STA_DR {
            rate /= 2.0;
            note = "dr";
        }
    }
    HpPerSta {
        rate: round1(rate),
        counted,
        note,
    }
}

/// Marginal mana: per point, per counted pool, at the character's position in the
/// EQL-measured bands (1 to 100, 2.5 to 200, 1.25 to 510, 0 past the cap), x the level-50 scaler.
fn mana_marginal(stat_val: Option<f64>) -> f64 {
    let s = stat_val.unwrap_or(150.0);
    let d = if s >= STAT_CAP {
        0.0
    } else if s > 200.0 {
        1.25
    } else if s > 100.0 {
        2.5
    } else {
        1.0
    };
    MANA_FAC_50 * d
}

/// The computed weights, each with one plain sentence saying where it came from.
pub fn computed_weights(
    classes: &[String],
    level: u32,
    cur: Option<&CurrentStats>,
) -> (Weights, Vec<(&'static str, String)>) {
    let has = |list: &[&str]| classes.iter().any(|c| list.contains(&c.as_str()));
    let n = classes.len();
    let grp = if n > 1 { "trio" } else { "class" };
    let (pools, counted_n) = pool_selection(classes);
    let counted: Vec<&Pool> = pools.iter().take(counted_n).collect();
    let full_count = classes
        .iter()
        .filter(|c| FULL_CASTER.contains(&c.as_str()))
        .count();
    let cast_heavy = full_count >= (if n > 1 { 2 } else { 1 }) && !has(&PURE_MELEE);
    let tank = has(&TANKS);
    let melee = has(&PURE_MELEE) || has(&["RNG", "BRD", "BST"]);

    let mut w = Weights([0.0; 15]);
    let mut why: Vec<(&'static str, String)> = Vec::new();

    w.set("hp", 1.0);
    why.push((
        "hp",
        "HP is the unit. Every weight is HP-equivalents: +1 HP on an item is worth exactly 1."
            .into(),
    ));

    let full_pools = counted.iter().filter(|p| p.full).count();
    let pool_max = n.min(2);
    let mana = if counted.is_empty() {
        0.0
    } else if full_pools >= pool_max {
        1.1
    } else if full_pools >= 1 {
        0.6
    } else {
        0.15
    };
    w.set("mana", mana);
    why.push((
        "mana",
        if counted.is_empty() {
            "No class here has a mana pool, so mana on gear is dead weight.".to_owned()
        } else {
            let names: Vec<&str> = counted.iter().map(|p| p.c.as_str()).collect();
            format!(
                "Counted pools (best {}): {}. {}",
                if pool_max > 1 { "2 of 3" } else { "of 1" },
                names.join(" + "),
                if full_pools >= pool_max {
                    format!("Full casting, so mana is this {grp}'s constraint and a mana point is worth slightly more than an HP.")
                } else if full_pools >= 1 {
                    "Part full casting, part hybrid: half the counted pool belongs to a class that mostly swings, valued at 0.6 HP per point.".to_owned()
                } else {
                    "Hybrid pools only: utility bars, not a casting rotation, valued at 0.15 HP per point.".to_owned()
                }
            )
        },
    ));

    let hs = hp_per_sta(classes, level, cur);
    w.set("sta", hs.rate * w.get("hp"));
    why.push((
        "sta",
        format!(
            "Counted HP pool{}: {}. Each gains level x class factor / 3000 HP per STA, at {level}: {} HP per STA{}. Weight = rate x HP weight.",
            if hs.counted.len() > 1 { "s (best 2 of 3)" } else { "" },
            hs.counted.join(" + "),
            hs.rate,
            match hs.note {
                "cap" => " (zero, at the 510 stat cap)",
                "dr" => " (halved past the 255 breakpoint)",
                _ => "",
            }
        ),
    ));

    for stat in ["int", "wis"] {
        let mine: Vec<&&Pool> = counted.iter().filter(|p| p.stat == stat).collect();
        let dropped: Vec<&Pool> = pools
            .iter()
            .skip(counted_n)
            .filter(|p| p.stat == stat)
            .collect();
        let sv = cur.map(|c| if stat == "int" { c.int } else { c.wis });
        let per_pool = mana_marginal(sv);
        let v = per_pool * mine.len() as f64 * w.get("mana");
        w.set(stat, round1(v));
        let band = match sv {
            None => "assuming the 101 to 200 band (no race is set)".to_owned(),
            Some(s) if s >= STAT_CAP => "past the 510 cap, worth nothing".to_owned(),
            Some(s) if s > 200.0 => "in the 201+ band (1.25 converted per point)".to_owned(),
            Some(s) if s > 100.0 => "in the 101 to 200 band (2.5 converted per point)".to_owned(),
            Some(_) => "under 100 (1 converted per point)".to_owned(),
        };
        let up = stat.to_ascii_uppercase();
        why.push((
            if stat == "int" { "int" } else { "wis" },
            if !mine.is_empty() {
                let feeds: Vec<&str> = mine.iter().map(|p| p.c.as_str()).collect();
                format!(
                    "{up} feeds {}: {band}, x the measured level-50 scaler 4.5 = {per_pool} mana per point per pool{} x the Mana weight = {} HP-equivalents per point.",
                    feeds.join(" and "),
                    if mine.len() > 1 { format!(" x {} counted pools on {up}", mine.len()) } else { String::new() },
                    w.get(stat)
                )
            } else if !dropped.is_empty() {
                let d: Vec<&str> = dropped.iter().map(|p| p.c.as_str()).collect();
                format!("{} draws from {up}, but that pool is the dropped third, so the stat adds nothing here.", d.join("+"))
            } else {
                format!("Nothing here draws mana from {up}.")
            },
        ));
    }

    let ac = if tank {
        3.0
    } else if melee {
        2.0
    } else {
        1.2
    };
    w.set("ac", ac);
    let (cap_v, cap_mult) = softcap(classes, level);
    why.push((
        "ac",
        format!(
            "Worn AC feeds mitigation, softcapped at {cap_v} at level {level} then x{cap_mult} past it. {} This is the least supported number in the model: players have no agreed AC to HP rate.",
            if tank { format!("A tank {grp} lives on mitigation: 3 HP-equivalents per AC.") } else if melee { format!("A melee {grp} takes hits but is not the wall: 2 per AC.") } else { "The lowest softcap band makes AC the weakest defensive buy here: 1.2 per AC.".to_owned() }
        ),
    ));

    let atk = if melee {
        3.0
    } else if tank {
        2.0
    } else {
        0.3
    };
    w.set("atk", atk);
    why.push((
        "atk",
        format!(
            "Worn ATK is a flat add to offense. {}",
            if melee { "For something that kills with melee, throughput is the build: 3 HP-equivalents per point.".to_owned() } else if tank { format!("Real value, but this {grp}'s job is holding, not racing: 2 per point.") } else { format!("This {grp} barely swings: 0.3 per point.") }
        ),
    ));

    let str_dead = cur.map(|c| c.str_ < 75.0).unwrap_or(false);
    w.set(
        "str",
        if str_dead {
            round1(atk * 0.1)
        } else {
            round1(0.67 * atk)
        },
    );
    why.push(("str", format!("Above 75, each STR is worth two thirds of an offense point, so the weight is two thirds of the Attack weight.{}", if str_dead { " You are below 75, where STR barely matters." } else { "" })));

    let agi_rate = 0.222;
    let mut agi = round1(agi_rate * ac);
    if agi == 0.0 {
        agi = 0.1;
    }
    w.set("agi", agi);
    why.push(("agi", format!("About {agi_rate} evasion per AGI above 40 (a linear model curve, no 75 knee), valued at the AC weight: {agi_rate} x {ac}.")));

    w.set("dex", if melee { 0.8 } else { 0.1 });
    why.push((
        "dex",
        format!(
            "Weapon proc rate, melee skill-ups, bard note fizzles. A melee lever{}.",
            if melee { "" } else { ", and this is not one" }
        ),
    ));

    w.set("cha", if has(&["ENC", "BRD"]) { 0.3 } else { 0.05 });
    why.push((
        "cha",
        format!(
            "Vendor prices and faction{}. Zero combat presence.",
            if has(&["ENC", "BRD"]) {
                "; with ENC or BRD here, charm gives it a small real value"
            } else {
                ""
            }
        ),
    ));

    w.set("end", if melee { 0.3 } else { 0.1 });
    why.push((
        "end",
        "Fuels stances and melee abilities. No published pool math, kept small.".into(),
    ));

    w.set("sv", 0.4);
    why.push(("sv", "Fire, cold, magic, disease and poison weighted equally; resist value is disputed in the player corpus, so the default stays modest. SV VOID is never scored.".into()));

    w.set(
        "haste",
        if cast_heavy {
            0.0
        } else if melee {
            3.0
        } else {
            1.5
        },
    );
    why.push((
        "haste",
        if cast_heavy {
            "Zeroed: the swing timer is gated by the cast bar and this is cast-heavy, so attack haste is nearly worthless for it.".into()
        } else {
            "A real melee lever, but capped and reached by gear alone at endgame: worth buying once, not stacking.".into()
        },
    ));

    w.set(
        "ratio",
        if cast_heavy {
            1.0
        } else if melee {
            15.0
        } else {
            8.0
        },
    );
    why.push(("ratio", "DMG/delay x10 for Primary, Secondary and Range. Two-handers carry double ratio in EQL. Tier raises DMG only; delay never changes.".into()));

    (w, why)
}

/* --------------------------------------------------------------- equipped -- */

/// One thing on the character, as the dump reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EqEntry {
    pub name: String,
    pub base: String,
    pub tier: u32,
    /// Index into the catalogue, once resolved. None when the wiki has no record of the name.
    pub rec: Option<usize>,
}

/// The worn set by slot, every WORN_SLOTS slot present even when empty.
#[derive(Clone, Debug, Default)]
pub struct Equipped {
    pub slots: Vec<(String, Vec<EqEntry>)>,
}

impl Equipped {
    pub fn empty() -> Equipped {
        Equipped {
            slots: WORN_SLOTS
                .iter()
                .map(|s| ((*s).to_owned(), Vec::new()))
                .collect(),
        }
    }

    pub fn get(&self, slot: &str) -> &[EqEntry] {
        self.slots
            .iter()
            .find(|(s, _)| s == slot)
            .map(|(_, v)| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn push(&mut self, slot: &str, e: EqEntry) {
        if let Some((_, v)) = self.slots.iter_mut().find(|(s, _)| s == slot) {
            v.push(e);
        }
    }

    /// Resolve every entry's name against the catalogue.
    pub fn resolve(&mut self, cat: &Catalogue) {
        for (_, v) in &mut self.slots {
            for e in v {
                e.rec = cat.find(&e.name);
            }
        }
    }

    pub fn count(&self) -> usize {
        self.slots.iter().map(|(_, v)| v.len()).sum()
    }
}

/* ----------------------------------------------------------------- scorer -- */

/// One line of a breakdown.
#[derive(Clone, Debug)]
pub struct Term {
    pub k: String,
    pub label: String,
    pub v: f64,
    pub pts: f64,
    /// A line the reader needs to see that the model does not price.
    pub unscored: bool,
}

/// The scorer for one class set at one level.
pub struct Scorer {
    pub classes: Vec<String>,
    pub level: u32,
    /// The weights in force: computed, then overridden.
    pub w: Weights,
    /// The weights before overrides, for the priorities panel's "auto" column.
    pub computed: Weights,
    pub why: Vec<(&'static str, String)>,
    pub cap_v: f64,
    pub cap_mult: f64,
    /// Total AC on the equipped set, the softcap position every candidate is priced from.
    pub worn_ac: f64,
}

impl Scorer {
    /// `equipped` and `cat` together give the worn AC; either missing means 0.
    pub fn make(
        classes: &[String],
        level: u32,
        equipped: Option<&Equipped>,
        cat: Option<&Catalogue>,
        overrides: &HashMap<String, f64>,
    ) -> Scorer {
        let (computed, why) = computed_weights(classes, level, None);
        let mut w = computed.clone();
        for (k, v) in overrides {
            w.set(k, *v);
        }
        let (cap_v, cap_mult) = softcap(classes, level);
        let worn_ac = match (equipped, cat) {
            (Some(eq), Some(cat)) => worn_ac(eq, cat),
            _ => 0.0,
        };
        Scorer {
            classes: classes.to_vec(),
            level,
            w,
            computed,
            why,
            cap_v,
            cap_mult,
            worn_ac,
        }
    }

    pub fn dual_wield_cap(&self) -> u16 {
        dual_wield_cap(&self.classes, self.level)
    }

    /// AC past the softcap counts at the class multiplier.
    pub fn eff_ac(&self, raw: f64) -> f64 {
        if raw <= self.cap_v {
            raw
        } else {
            self.cap_v + (raw - self.cap_v) * self.cap_mult
        }
    }

    /// Score one item at a tier, in the context of the slot it would occupy.
    /// `base_ac` is the worn AC minus the AC of whatever it replaces, so both sides of a comparison
    /// price their AC against the same softcap position.
    pub fn score(&self, rec: &GearRec, tier: u32, base_ac: f64, placement: &str) -> f64 {
        let mut sum = 0.0;
        let st = rec.stats_at(tier);
        for (k, v) in &st {
            if *k == "ac" {
                continue;
            }
            sum += self.w.get(k) * *v as f64;
        }
        let ac = st
            .iter()
            .find(|(k, _)| *k == "ac")
            .map(|(_, v)| *v)
            .unwrap_or(0);
        if ac != 0 {
            sum += self.w.get("ac") * (self.eff_ac(base_ac + ac as f64) - self.eff_ac(base_ac));
        }
        /* SV VOID ("v") is displayed but never scored: player consensus is that it is the
         * upgrade-tier marker with no combat effect yet */
        for (k, v) in &rec.sv {
            if k != "v" {
                sum += self.w.get("sv") * *v as f64;
            }
        }
        if rec.haste != 0 {
            sum += self.w.get("haste") * rec.haste as f64;
        }
        /* A weapon's ratio only counts where it can swing. Two-handers deal double damage and
         * double ratio in EQL, so the term doubles for them; without that a 2H read as a strictly
         * worse 1H. Range uses the same term as a stand-in. */
        if let (Some(dmg), Some(dly)) = (rec.dmg, rec.dly) {
            if dly > 0 && matches!(placement, "Primary" | "Secondary" | "Range") {
                sum += self.w.get("ratio") * 10.0 * stat_at(dmg, tier) as f64 / dly as f64
                    * if rec.is_two_h() { 2.0 } else { 1.0 };
            }
        }
        sum
    }

    /// score(), itemised, so a head-to-head can say WHY. The pts sum to score() exactly.
    pub fn terms(&self, rec: &GearRec, tier: u32, base_ac: f64, placement: &str) -> Vec<Term> {
        let mut out = Vec::new();
        let st = rec.stats_at(tier);
        for (k, label) in ITEM_STATS {
            let v = st
                .iter()
                .find(|(s, _)| *s == k)
                .map(|(_, v)| *v)
                .unwrap_or(0);
            if v == 0 {
                continue;
            }
            let pts = if k == "ac" {
                self.w.get("ac") * (self.eff_ac(base_ac + v as f64) - self.eff_ac(base_ac))
            } else {
                self.w.get(k) * v as f64
            };
            out.push(Term {
                k: k.to_owned(),
                label: label.to_owned(),
                v: v as f64,
                pts,
                unscored: false,
            });
        }
        for (k, v) in &rec.sv {
            out.push(Term {
                k: format!("sv:{k}"),
                label: format!("SV {}", sv_name(k)),
                v: *v as f64,
                pts: if k == "v" {
                    0.0
                } else {
                    self.w.get("sv") * *v as f64
                },
                unscored: k == "v",
            });
        }
        if rec.haste != 0 {
            out.push(Term {
                k: "haste".into(),
                label: "Haste".into(),
                v: rec.haste as f64,
                pts: self.w.get("haste") * rec.haste as f64,
                unscored: false,
            });
        }
        if let Some(dmg) = rec.dmg {
            out.push(Term {
                k: "dmg".into(),
                label: "DMG".into(),
                v: stat_at(dmg, tier) as f64,
                pts: 0.0,
                unscored: true,
            });
        }
        if let Some(dly) = rec.dly {
            out.push(Term {
                k: "dly".into(),
                label: "Delay".into(),
                v: dly as f64,
                pts: 0.0,
                unscored: true,
            });
        }
        if let (Some(dmg), Some(dly)) = (rec.dmg, rec.dly) {
            if dly > 0 {
                let swings = matches!(placement, "Primary" | "Secondary" | "Range");
                let ratio = stat_at(dmg, tier) as f64 / dly as f64;
                let mult = if rec.is_two_h() { 2.0 } else { 1.0 };
                out.push(Term {
                    k: "ratio".into(),
                    label: format!("DMG/DLY{}", if rec.is_two_h() { " (2H, x2)" } else { "" }),
                    v: (ratio * 100.0).round() / 100.0,
                    pts: if swings {
                        self.w.get("ratio") * 10.0 * ratio * mult
                    } else {
                        0.0
                    },
                    unscored: !swings,
                });
            }
        }
        out
    }

    /// Can one of the classes wear it at this level.
    pub fn legal(&self, rec: &GearRec) -> bool {
        if rec.req > 0 && rec.req > self.level {
            return false;
        }
        match &rec.cls {
            Set::Unstated => true,
            Set::NoOne => false,
            Set::All { except } => {
                except.is_empty() || self.classes.iter().any(|cl| !except.contains(cl))
            }
            Set::Only(c) => self.classes.iter().any(|cl| c.contains(cl)),
        }
    }

    /// The off hand gate the ranker, spare and the valet all apply: a
    /// two-hander never goes there, and a damaging one-hander only for a class that can Dual Wield.
    pub fn can_place(&self, rec: &GearRec, slot: &str) -> bool {
        slot != "Secondary" || (!rec.is_two_h() && (self.dual_wield_cap() > 0 || rec.dmg.is_none()))
    }

    /// The weaker equipped occupant of a placement; paired slots hold
    /// two, and an empty position scores 0 at the full worn AC.
    pub fn weakest_eq(&self, equipped: &Equipped, cat: &Catalogue, placement: &str) -> Weakest {
        let n = paired(placement);
        let mut scored: Vec<Weakest> = equipped
            .get(placement)
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let ac = e.rec.map(|r| ac_of(&cat.recs[r], e.tier)).unwrap_or(0.0);
                let base_ac = self.worn_ac - ac;
                let s = e
                    .rec
                    .map(|r| self.score(&cat.recs[r], e.tier, base_ac, placement))
                    .unwrap_or(0.0);
                Weakest {
                    entry: Some(i),
                    s,
                    base_ac,
                }
            })
            .collect();
        while scored.len() < n {
            scored.push(Weakest {
                entry: None,
                s: 0.0,
                base_ac: self.worn_ac,
            });
        }
        scored.sort_by(|a, b| a.s.partial_cmp(&b.s).unwrap_or(std::cmp::Ordering::Equal));
        scored.swap_remove(0)
    }
}

/// The weakest-occupant answer: which occupant (index into the slot's list, None for an
/// empty position), its score, and the AC base it was priced from.
#[derive(Clone, Debug, PartialEq)]
pub struct Weakest {
    pub entry: Option<usize>,
    pub s: f64,
    pub base_ac: f64,
}

/// Worn AC of an item: negative AC does not count.
pub fn ac_of(rec: &GearRec, tier: u32) -> f64 {
    let ac = rec.stat("ac");
    if ac > 0 {
        stat_at(ac, tier) as f64
    } else {
        0.0
    }
}

/// The worn set's AC, summed over WORN_SLOTS and not SLOTS (see WORN_SLOTS).
pub fn worn_ac(eq: &Equipped, cat: &Catalogue) -> f64 {
    let mut total = 0.0;
    for (_, list) in &eq.slots {
        for e in list {
            if let Some(r) = e.rec {
                total += ac_of(&cat.recs[r], e.tier);
            }
        }
    }
    total
}

/* ----------------------------------------------------------------- ranker -- */

/// Where an item stands in one (class, slot) catalogue.
#[derive(Clone, Debug, PartialEq)]
pub struct Rank {
    pub rank: usize,
    pub of: usize,
    pub score: f64,
    /// "1H" or "2H" in Primary, "" everywhere else.
    pub kind: &'static str,
    /// What `of` counts, said out loud.
    pub noun: &'static str,
    /// Up to three records scoring strictly above it.
    pub better: Vec<usize>,
}

/// One (class, slot) standing.
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub cls: &'static str,
    pub slot: String,
    pub rank: usize,
    pub of: usize,
    pub score: f64,
    pub kind: &'static str,
    pub noun: &'static str,
    /// The names of up to three catalogue records scoring strictly above it (`Rank::better`),
    /// best first: what "#4 of 120" is beaten BY, which is the half of a rank a person acts on.
    pub better: Vec<String>,
}

impl Standing {
    /// The cell's tooltip, with what beats it after the count.
    pub fn what(&self) -> String {
        let mut s = format!(
            "{} {} a {} can {}",
            self.of,
            self.noun,
            class_name(self.cls),
            match self.slot.as_str() {
                "Primary" => "put in the main hand".to_owned(),
                "Secondary" => "put in the off hand".to_owned(),
                s => format!("wear at {s}"),
            }
        );
        if !self.better.is_empty() {
            s.push_str(&format!("\nbeaten by {}", self.better.join(", ")));
            if self.rank > self.better.len() + 1 {
                s.push_str(&format!(" and {} more", self.rank - 1 - self.better.len()));
            }
        }
        s
    }
}

/// Every (class, slot) the item can be ranked in, best first, plus the
/// two answers a keep-or-toss column needs side by side.
#[derive(Clone, Debug, Default)]
pub struct Best {
    /// The best standing any class gets.
    pub best: Option<Standing>,
    pub all: Vec<Standing>,
    /// The standings kept to the caller's own classes, best first.
    pub trio: Vec<Standing>,
    /// The record names no class at all, so every class was tried.
    pub any_class: bool,
}

/// The catalogue ranker: how strong an item is for a class, as its
/// place among EVERY in-era item that class can wear in the slot, one scorer per class, every
/// candidate at +0. Pools are memoised per (class, slot, kind); scorers per class carry each
/// class's own computed weights and no overrides, on purpose (see the file header).
pub struct Ranker {
    pub level: u32,
    scorers: HashMap<String, Scorer>,
    pools: HashMap<String, Vec<(usize, f64)>>,
}

impl Ranker {
    pub fn new(level: u32) -> Ranker {
        Ranker {
            level,
            scorers: HashMap::new(),
            pools: HashMap::new(),
        }
    }

    fn scorer(&mut self, cls: &str) -> &Scorer {
        let level = self.level;
        self.scorers
            .entry(cls.to_owned())
            .or_insert_with(|| Scorer::make(&[cls.to_owned()], level, None, None, &HashMap::new()))
    }

    /// The main hand holds TWO catalogues: a two-hander is not an
    /// alternative to a one-hander for the same slot, and ranking them together pushed every 1H
    /// down a ladder it never climbs.
    pub fn kind_of(rec: &GearRec, slot: &str) -> &'static str {
        if slot == "Primary" {
            if rec.is_two_h() {
                "2H"
            } else {
                "1H"
            }
        } else {
            ""
        }
    }

    fn noun_for(kind: &str, slot: &str) -> &'static str {
        match kind {
            "1H" => "one-handers",
            "2H" => "two-handers",
            _ => {
                if slot == "Secondary" {
                    "off-hand items"
                } else {
                    "in-era items"
                }
            }
        }
    }

    /// The score-sorted pool for one (class, slot, kind), built on first use.
    pub fn pool(&mut self, cat: &Catalogue, cls: &str, slot: &str, kind: &str) -> &[(usize, f64)] {
        let key = format!("{cls}|{slot}|{kind}");
        if !self.pools.contains_key(&key) {
            let sc = self.scorer(cls);
            let mut p: Vec<(usize, f64)> = Vec::new();
            for (i, rec) in cat.recs.iter().enumerate() {
                if rec.oe || !rec.scorable() {
                    continue;
                }
                if !rec.sl.iter().any(|s| s == slot) {
                    continue;
                }
                if !kind.is_empty() && Ranker::kind_of(rec, slot) != kind {
                    continue;
                }
                if !sc.legal(rec) || !sc.can_place(rec, slot) {
                    continue;
                }
                p.push((i, sc.score(rec, 0, 0.0, slot)));
            }
            p.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| cat.recs[a.0].name.cmp(&cat.recs[b.0].name))
            });
            self.pools.insert(key.clone(), p);
        }
        &self.pools[&key]
    }

    /// 1 + the number scoring strictly above: ties share a rank and an item never counts itself.
    /// An out-of-era item is not in the catalogue and gets no rank.
    pub fn rank(
        &mut self,
        cat: &Catalogue,
        rec_i: usize,
        cls: &str,
        slot: &str,
        tier: u32,
    ) -> Option<Rank> {
        let rec = &cat.recs[rec_i];
        if rec.oe || !rec.scorable() {
            return None;
        }
        let s = {
            let sc = self.scorer(cls);
            if !sc.legal(rec) || !sc.can_place(rec, slot) {
                return None;
            }
            sc.score(rec, tier, 0.0, slot)
        };
        let kind = Ranker::kind_of(rec, slot);
        let p = self.pool(cat, cls, slot, kind);
        /* first index with score <= s */
        let (mut lo, mut hi) = (0usize, p.len());
        while lo < hi {
            let m = (lo + hi) / 2;
            if p[m].1 > s {
                lo = m + 1;
            } else {
                hi = m;
            }
        }
        Some(Rank {
            rank: lo + 1,
            of: p.len(),
            score: s,
            kind,
            noun: Ranker::noun_for(kind, slot),
            better: p.iter().take(lo.min(3)).map(|(i, _)| *i).collect(),
        })
    }

    /// The best standing: lowest rank number wins, a tie names the hand it is swung in,
    /// bigger pool breaks the next tie, then class order, then slot name.
    pub fn best(&mut self, cat: &Catalogue, rec_i: usize, own: &[String]) -> Best {
        let rec = &cat.recs[rec_i];
        let cls = rec.classes();
        let universe: Vec<&'static str> = match &cls {
            None => CLASSES.to_vec(),
            Some(c) => c.clone(),
        };
        let slots: Vec<String> = rec.real_slots().iter().map(|s| (*s).to_owned()).collect();
        let mut all = Vec::new();
        for c in &universe {
            for slot in &slots {
                if let Some(r) = self.rank(cat, rec_i, c, slot, 0) {
                    let better = r.better.iter().map(|&i| cat.recs[i].name.clone()).collect();
                    all.push(Standing {
                        cls: c,
                        slot: slot.clone(),
                        rank: r.rank,
                        of: r.of,
                        score: r.score,
                        kind: r.kind,
                        noun: r.noun,
                        better,
                    });
                }
            }
        }
        let hand = |s: &str| match s {
            "Primary" => 0,
            "Secondary" => 1,
            _ => 0,
        };
        all.sort_by(|a, b| {
            a.rank
                .cmp(&b.rank)
                .then_with(|| hand(&a.slot).cmp(&hand(&b.slot)))
                .then_with(|| b.of.cmp(&a.of))
                .then_with(|| {
                    CLASSES
                        .iter()
                        .position(|c| *c == a.cls)
                        .cmp(&CLASSES.iter().position(|c| *c == b.cls))
                })
                .then_with(|| a.slot.cmp(&b.slot))
        });
        let trio: Vec<Standing> = all
            .iter()
            .filter(|x| own.iter().any(|o| o == x.cls))
            .cloned()
            .collect();
        Best {
            best: all.first().cloned(),
            all,
            trio,
            any_class: cls.is_none(),
        }
    }
}

/* ------------------------------------------------------------------ spare -- */

/// One physical copy you own, as the spare analysis reads it. The
/// item's identity for the tie rule is the wiki title on its record, so the dump name is not
/// carried here.
#[derive(Clone, Debug)]
pub struct OwnedRow {
    /// Dump order.
    pub i: usize,
    pub rec: Option<usize>,
    pub tier: u32,
    pub exalt: bool,
}

/// One (class, slot) niche and how many owned things are ahead in it.
#[derive(Clone, Debug)]
pub struct Niche {
    pub cls: &'static str,
    pub slot: String,
    pub cap: usize,
    /// At parity: the fair number, and the sort key.
    pub ahead: usize,
    /// At the tiers the items are actually at: today's truth.
    pub ahead_as_is: usize,
    /// Up to eight (item index, score, tier the challenger was scaled to) ahead at parity.
    pub by: Vec<(usize, f64, u32)>,
}

#[derive(Clone, Debug)]
pub struct SpareItem {
    pub row: usize,
    pub rec: usize,
    pub tier: u32,
    pub key: String,
    pub slots: Vec<String>,
    pub classes: Vec<&'static str>,
    pub unscored: Vec<&'static str>,
    pub niches: Vec<Niche>,
    pub ahead: usize,
    pub ahead_as_is: usize,
    pub spare: bool,
    pub spare_as_is: bool,
    /// Classes for which it still holds a position somewhere.
    pub bis: Vec<&'static str>,
    /// The niche where it comes closest to being worn.
    pub best: Option<Niche>,
    /// The upgrade rank it has to reach before the parity verdict is something you own.
    pub upgrade_to: Option<u32>,
    pub no_class: bool,
}

/// What the spare analysis hands back.
#[derive(Clone, Debug, Default)]
pub struct SpareReport {
    pub items: Vec<SpareItem>,
    /// Indices into `items`, most-beaten first.
    pub ranked: Vec<usize>,
    pub skipped_exalt: usize,
    pub skipped_unknown: usize,
    pub skipped_not_wearable: usize,
}

/// Does a's race set contain b's? A subset test, never an enumeration.
pub fn race_covers(a: &Set, b: &Set) -> bool {
    let a_all_x = |x: &Vec<String>| x.is_empty();
    match b {
        Set::Unstated | Set::All { .. } => {
            let b_x: Option<&Vec<String>> = match b {
                Set::All { except } => Some(except),
                _ => None,
            };
            match a {
                Set::Unstated => true,
                Set::All { except } if a_all_x(except) => true,
                Set::All { except } => match b_x {
                    None => false,
                    Some(bx) => except.iter().all(|r| bx.contains(r)),
                },
                _ => false,
            }
        }
        Set::NoOne => true,
        Set::Only(bl) => match a {
            Set::Unstated => true,
            Set::All { except } if a_all_x(except) => true,
            Set::All { except } => !bl.iter().any(|r| except.contains(r)),
            Set::Only(al) => bl.iter().all(|r| al.contains(r)),
            Set::NoOne => bl.is_empty(),
        },
    }
}

/// A deity-locked item cannot obsolete an unlocked one.
fn deity_covers(a: &GearRec, b: &GearRec) -> bool {
    !a.deity || b.deity
}

/// A higher level requirement cannot obsolete a lower one.
fn req_covers(a: &GearRec, b: &GearRec) -> bool {
    a.req == 0 || a.req <= b.req
}

/// Which of the things you own does nobody need any more. An item is
/// beaten in a niche (one class, one slot) when other things you own are better there for that
/// class; it is spare only when every one of its niches is full.
pub fn analyze_spare(rows: &[OwnedRow], cat: &Catalogue, level: u32) -> SpareReport {
    let universe: Vec<&'static str> = CLASSES.to_vec();
    let scorers: HashMap<&str, Scorer> = universe
        .iter()
        .map(|c| {
            (
                *c,
                Scorer::make(&[(*c).to_owned()], level, None, None, &HashMap::new()),
            )
        })
        .collect();

    let mut items: Vec<SpareItem> = Vec::new();
    let mut skipped_exalt = 0;
    let mut skipped_unknown = 0;
    let mut skipped_not_wearable = 0;
    for r in rows {
        if r.exalt {
            skipped_exalt += 1;
            continue;
        }
        let Some(ri) = r.rec else {
            skipped_unknown += 1;
            continue;
        };
        let rec = &cat.recs[ri];
        let sl: Vec<String> = rec.worn_slots().iter().map(|s| (*s).to_owned()).collect();
        if sl.is_empty() {
            skipped_not_wearable += 1;
            continue;
        }
        let cls: Vec<&'static str> = rec.classes().unwrap_or_else(|| universe.clone());
        items.push(SpareItem {
            row: r.i,
            rec: ri,
            tier: r.tier,
            key: rec.key.to_ascii_lowercase(),
            slots: sl,
            classes: cls,
            unscored: rec.unscored(),
            niches: Vec::new(),
            ahead: 0,
            ahead_as_is: 0,
            spare: true,
            spare_as_is: true,
            bis: Vec::new(),
            best: None,
            upgrade_to: None,
            no_class: false,
        });
    }

    /* score(item, class, slot, tier), memoised. AC base 0 for every candidate: a niche compares
     * items against each other and pricing both sides at the same softcap position is the only
     * thing that matters. */
    let mut memo: HashMap<(usize, &str, String, u32), f64> = HashMap::new();
    let mut score =
        |it: usize, cls: &'static str, slot: &str, tier: u32, items: &[SpareItem]| -> f64 {
            let key = (it, cls, slot.to_owned(), tier);
            if let Some(v) = memo.get(&key) {
                return *v;
            }
            let v = scorers[cls].score(&cat.recs[items[it].rec], tier, 0.0, slot);
            memo.insert(key, v);
            v
        };

    let covers = |b: &SpareItem, a: &SpareItem| -> bool {
        let rb = &cat.recs[b.rec];
        let ra = &cat.recs[a.rec];
        race_covers(&rb.rc, &ra.rc) && deity_covers(ra, rb) && req_covers(ra, rb)
    };
    /* A TIE IS NOT A REASON TO GET UP, except between copies of the same item, where copies rank
     * by upgrade rank first and dump order second. */
    let beats = |bs: f64, b: &SpareItem, as_: f64, a: &SpareItem| -> bool {
        bs > as_
            || (bs == as_
                && !b.key.is_empty()
                && b.key == a.key
                && (b.tier > a.tier || (b.tier == a.tier && b.row < a.row)))
    };

    let dw: HashMap<&str, bool> = universe
        .iter()
        .map(|c| (*c, scorers[c].dual_wield_cap() > 0))
        .collect();
    let can_place = |it: &SpareItem, cls: &str, slot: &str| -> bool {
        let rec = &cat.recs[it.rec];
        slot != "Secondary" || (!rec.is_two_h() && (dw[cls] || rec.dmg.is_none()))
    };

    /* aheadIn: how many things are ahead of `a` in one niche, at a stated rank for `a`. The
     * bucket is score-sorted descending so the scan stops once a rival scores below a's unscaled
     * score: scaling only ever raises a.
     *
     * Ten arguments on purpose: as a closure this would reach the memo, the gates and the item
     * list through its enclosing scope instead. Passing them in keeps the rule in
     * one readable place instead of a struct that exists only to satisfy an argument count. */
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn ahead_in(
        bucket: &[usize],
        a: usize,
        cls: &'static str,
        slot: &str,
        a_tier: u32,
        scale: bool,
        items: &[SpareItem],
        score: &mut dyn FnMut(usize, &'static str, &str, u32, &[SpareItem]) -> f64,
        covers: &dyn Fn(&SpareItem, &SpareItem) -> bool,
        beats: &dyn Fn(f64, &SpareItem, f64, &SpareItem) -> bool,
    ) -> Vec<(usize, f64, u32)> {
        let floor = score(a, cls, slot, items[a].tier, items);
        let mut list = Vec::new();
        for &b in bucket {
            if b == a {
                continue;
            }
            let bs = score(b, cls, slot, items[b].tier, items);
            if bs < floor {
                break;
            }
            if !covers(&items[b], &items[a]) {
                continue;
            }
            let at = if scale {
                a_tier.max(items[b].tier)
            } else {
                a_tier
            };
            let as_ = score(a, cls, slot, at, items);
            if beats(bs, &items[b], as_, &items[a]) {
                list.push((b, bs, at));
            }
        }
        list
    }

    /* pass 1: the real slots */
    let mut buckets: HashMap<(&'static str, String), Vec<usize>> = HashMap::new();
    for c in &universe {
        for s in SLOTS {
            let mut pool: Vec<usize> = (0..items.len())
                .filter(|&i| {
                    items[i].classes.contains(c)
                        && items[i].slots.iter().any(|x| x == s)
                        && can_place(&items[i], c, s)
                })
                .collect();
            if pool.is_empty() {
                continue;
            }
            pool.sort_by(|&x, &y| {
                let sx = score(x, c, s, items[x].tier, &items);
                let sy = score(y, c, s, items[y].tier, &items);
                sy.partial_cmp(&sx)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| items[x].row.cmp(&items[y].row))
            });
            buckets.insert((c, s.to_owned()), pool);
        }
    }
    let mut keys: Vec<(&'static str, String)> = buckets.keys().cloned().collect();
    keys.sort();
    for key in &keys {
        let pool = buckets[key].clone();
        let (c, s) = (key.0, key.1.as_str());
        let cap = paired(s);
        for &a in &pool {
            let a_tier = items[a].tier;
            let par = ahead_in(
                &pool, a, c, s, a_tier, true, &items, &mut score, &covers, &beats,
            );
            let as_is = ahead_in(
                &pool, a, c, s, a_tier, false, &items, &mut score, &covers, &beats,
            );
            items[a].niches.push(Niche {
                cls: c,
                slot: s.to_owned(),
                cap,
                ahead: par.len(),
                ahead_as_is: as_is.len(),
                by: par.into_iter().take(8).collect(),
            });
        }
    }

    /* pass 2: Any Slot, over the leftovers. Any Slot is what is LEFT after every dedicated slot
     * is filled, so its bucket for a class is only the items that hold no position in any of
     * their own slots for that class, scored AS Any Slot (a weapon parked there does not swing). */
    let held_somewhere =
        |it: &SpareItem, c: &str| it.niches.iter().any(|n| n.cls == c && n.ahead < n.cap);
    for c in &universe {
        let mut pool: Vec<usize> = (0..items.len())
            .filter(|&i| items[i].classes.contains(c) && !held_somewhere(&items[i], c))
            .collect();
        if pool.is_empty() {
            continue;
        }
        pool.sort_by(|&x, &y| {
            let sx = score(x, c, ANY, items[x].tier, &items);
            let sy = score(y, c, ANY, items[y].tier, &items);
            sy.partial_cmp(&sx)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| items[x].row.cmp(&items[y].row))
        });
        let cap = paired(ANY);
        for &a in &pool {
            let a_tier = items[a].tier;
            let par = ahead_in(
                &pool, a, c, ANY, a_tier, true, &items, &mut score, &covers, &beats,
            );
            let as_is = ahead_in(
                &pool, a, c, ANY, a_tier, false, &items, &mut score, &covers, &beats,
            );
            items[a].niches.push(Niche {
                cls: c,
                slot: ANY.to_owned(),
                cap,
                ahead: par.len(),
                ahead_as_is: as_is.len(),
                by: par.into_iter().take(8).collect(),
            });
        }
        buckets.insert((c, ANY.to_owned()), pool);
    }

    /* the verdict per item */
    for a in 0..items.len() {
        if items[a].niches.is_empty() {
            let it = &mut items[a];
            it.spare = true;
            it.spare_as_is = true;
            it.ahead = 0;
            it.ahead_as_is = 0;
            it.best = None;
            it.no_class = true;
            continue;
        }
        /* the niche to report is the one where the item comes CLOSEST to being worn: most slack
         * first, then fewest ahead */
        items[a].niches.sort_by(|x, y| {
            let rx = (x.cap as i64 - x.ahead as i64, -(x.ahead as i64));
            let ry = (y.cap as i64 - y.ahead as i64, -(y.ahead as i64));
            ry.cmp(&rx)
        });
        let best = items[a].niches[0].clone();
        items[a].ahead = best.ahead;
        items[a].ahead_as_is = best.ahead_as_is;
        items[a].spare = best.ahead >= best.cap;
        items[a].spare_as_is = items[a].niches.iter().all(|n| n.ahead_as_is >= n.cap);
        let mut bis: Vec<&'static str> = Vec::new();
        for n in &items[a].niches {
            if n.ahead < n.cap && !bis.contains(&n.cls) {
                bis.push(n.cls);
            }
        }
        items[a].bis = bis;
        items[a].best = Some(best);

        /* "requires upgrades to equal parity": the rank this item has to reach before the parity
         * verdict is something you actually own */
        if items[a].spare_as_is && items[a].tier < MAX_TIER {
            let niches = items[a].niches.clone();
            let mut found = None;
            'outer: for t in (items[a].tier + 1)..=MAX_TIER {
                for n in &niches {
                    let Some(pool) = buckets.get(&(n.cls, n.slot.clone())) else {
                        continue;
                    };
                    if ahead_in(
                        pool, a, n.cls, &n.slot, t, false, &items, &mut score, &covers, &beats,
                    )
                    .len()
                        < n.cap
                    {
                        found = Some(t);
                        break 'outer;
                    }
                }
            }
            items[a].upgrade_to = found;
        }
    }

    /* "we sort items based on how many items are better than it. most items better than it at
     * the top." The count is the one from the item's best remaining niche. */
    let mut ranked: Vec<usize> = (0..items.len()).collect();
    ranked.sort_by(|&x, &y| {
        let tx: usize = items[x].niches.iter().map(|n| n.ahead).sum();
        let ty: usize = items[y].niches.iter().map(|n| n.ahead).sum();
        items[y]
            .ahead
            .cmp(&items[x].ahead)
            .then_with(|| ty.cmp(&tx))
            .then_with(|| items[x].row.cmp(&items[y].row))
    });

    SpareReport {
        items,
        ranked,
        skipped_exalt,
        skipped_unknown,
        skipped_not_wearable,
    }
}

/* ------------------------------------------------------------- priorities -- */

/// The settings key the stat priorities live under: `{"w": {stat: weight}}`.
///
/// THE ONE COORDINATION POINT WITH THE SETTINGS LANE. Assumed shape of the settings type:
/// `pub extra: serde_json::Map<String, serde_json::Value>`, the same convention the LFG lane
/// codes against. If the settings lane chose a `Value`, `read_overrides` and `write_overrides`
/// are the whole reconciliation.
pub const PRIORITIES_KEY: &str = "priorities";

/// The settings key the character's trio and level live under: `{"classes": [3 codes],
/// "level": n, "race": null}`. Written here and
/// read by anything that needs the trio (the Inventory rank columns read it too).
pub const CHAR_KEY: &str = "char";

pub fn read_overrides(settings: &Settings) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    if let Some(w) = settings
        .extra
        .get(PRIORITIES_KEY)
        .and_then(|v| v.get("w"))
        .and_then(Value::as_object)
    {
        for (k, v) in w {
            if let (true, Some(f)) = (STAT_DEFS.iter().any(|(s, _)| s == k), v.as_f64()) {
                out.insert(k.clone(), f.clamp(0.0, 99.0));
            }
        }
    }
    out
}

pub fn write_overrides(settings: &mut Settings, w: &HashMap<String, f64>) {
    let mut m = serde_json::Map::new();
    let mut keys: Vec<&String> = w.keys().collect();
    keys.sort();
    for k in keys {
        m.insert(k.clone(), serde_json::json!(w[k]));
    }
    settings
        .extra
        .insert(PRIORITIES_KEY.to_owned(), serde_json::json!({ "w": m }));
}

/// The line beside the panel's label: how many weights the player has overridden.
pub fn priorities_summary(n: usize) -> String {
    if n > 0 {
        format!("({n} custom)")
    } else {
        "(computed for your trio)".to_owned()
    }
}

/// The character store: three distinct class codes and a level in 1..=50.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharState {
    pub classes: Vec<String>,
    pub level: u32,
}

impl Default for CharState {
    fn default() -> Self {
        /* the default trio */
        CharState {
            classes: vec!["WAR".into(), "CLR".into(), "WIZ".into()],
            level: MAX_LEVEL,
        }
    }
}

impl CharState {
    /// A valid trio: exactly three, all known, no duplicates.
    pub fn valid3(a: &[String]) -> bool {
        a.len() == 3
            && a.iter().all(|c| class_code(c).is_some())
            && a[0] != a[1]
            && a[1] != a[2]
            && a[0] != a[2]
    }

    pub fn clamp_level(v: i64) -> u32 {
        if v <= 0 {
            MAX_LEVEL
        } else {
            (v as u32).clamp(1, MAX_LEVEL)
        }
    }

    /// Whether a trio was ever chosen and saved. Until it is, every score, rank and verdict on
    /// the Gear, Valet and Inventory screens is priced for the default trio, and
    /// each of those screens says so beside the trio rather than calling it "yours".
    pub fn stored(settings: &Settings) -> bool {
        settings
            .extra
            .get(CHAR_KEY)
            .is_some_and(|v| CharState::valid3(&str_list(v.get("classes"))))
    }

    /// The words beside a trio that was never set: what it is, and where to set it. Empty once a
    /// trio is stored, so a chosen trio is never second-guessed on screen.
    pub fn default_note(settings: &Settings) -> &'static str {
        if CharState::stored(settings) {
            ""
        } else {
            "(the default trio, not one you chose; every score here is priced for it until you pick yours on the Gear screen)"
        }
    }

    pub fn read(settings: &Settings) -> CharState {
        let Some(v) = settings.extra.get(CHAR_KEY) else {
            return CharState::default();
        };
        let classes = str_list(v.get("classes"));
        let level = v
            .get("level")
            .and_then(Value::as_i64)
            .map(CharState::clamp_level)
            .unwrap_or(MAX_LEVEL);
        if CharState::valid3(&classes) {
            CharState { classes, level }
        } else {
            CharState {
                level,
                ..CharState::default()
            }
        }
    }

    /// An invalid trio keeps the last good one rather than resetting to the default.
    pub fn write(&self, settings: &mut Settings) {
        let prev = CharState::read(settings);
        let classes = if CharState::valid3(&self.classes) {
            self.classes.clone()
        } else {
            prev.classes
        };
        settings.extra.insert(
            CHAR_KEY.to_owned(),
            serde_json::json!({ "classes": classes, "level": self.level.clamp(1, MAX_LEVEL), "race": Value::Null }),
        );
    }
}

/* ------------------------------------------------------------ the catalogue cache -- */

/// The catalogue, built once per snapshot. The snapshot is identified by its address and record
/// count, which is what changes when the data lane reloads it.
pub struct CatCache {
    key: (usize, usize),
    pub cat: Catalogue,
    pub problem: Option<String>,
}

impl CatCache {
    /// Build or reuse. None when there is no snapshot at all.
    pub fn refresh(slot: &mut Option<CatCache>, snap: Option<&crate::data::Snapshot>) {
        let Some(s) = snap else {
            *slot = None;
            return;
        };
        let key = (s as *const _ as usize, s.items.len());
        if slot.as_ref().map(|c| c.key) == Some(key) {
            return;
        }
        let (cat, problem) = match Catalogue::from_snapshot(s) {
            Ok(c) => (c, None),
            Err(e) => (Catalogue::from_values(std::iter::empty()), Some(e)),
        };
        *slot = Some(CatCache { key, cat, problem });
    }
}

/* ----------------------------------------------------------------- screen -- */

/// One slot line on the worn table, computed once per frame from the analysis.
struct SlotLine {
    slot: String,
    position: usize,
    entry: Option<EqEntry>,
    ac: Option<f64>,
    score: Option<f64>,
    /// The score itemised, for the hover on the number.
    terms: Vec<Term>,
    /// Score, then how many in-era items beat the weakest occupant and the top three of them.
    upgrades: Vec<(usize, f64)>,
    beaten_by: usize,
    caveat: Vec<&'static str>,
    unknown: bool,
}

/// The Gear screen: the worn set by slot with its score, the total, and what in the whole in-era
/// catalogue would beat each slot for the trio.
pub struct GearScreen {
    cat: Option<CatCache>,
    dump: crate::screens::inventory::DumpCache,
    char_: CharState,
    overrides: HashMap<String, f64>,
    loaded: bool,
    show_priorities: bool,
    show_all_upgrades: Option<String>,
    save_problem: Option<String>,
    /// Editable copies of the fifteen weights, so a DragValue has something to drag.
    edit: [f64; 15],
    edit_for: Option<(Vec<String>, u32)>,
}

impl Default for GearScreen {
    fn default() -> Self {
        GearScreen {
            cat: None,
            dump: crate::screens::inventory::DumpCache::default(),
            char_: CharState::default(),
            overrides: HashMap::new(),
            loaded: false,
            show_priorities: false,
            show_all_upgrades: None,
            save_problem: None,
            edit: [0.0; 15],
            edit_for: None,
        }
    }
}

impl GearScreen {
    fn load_once(&mut self, cx: &Cx) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.char_ = CharState::read(cx.settings);
        self.overrides = read_overrides(cx.settings);
    }

    fn persist(&mut self, cx: &mut Cx) {
        self.char_.write(cx.settings);
        write_overrides(cx.settings, &self.overrides);
        self.save_problem = cx.settings.save().err();
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        self.load_once(cx);
        CatCache::refresh(&mut self.cat, cx.data);
        self.dump.refresh(cx);

        ui.label(
            RichText::new("GEAR")
                .font(crate::fonts::display(16.0))
                .color(GOLD_HI),
        );
        ui.add_space(2.0);
        ui.label(
            RichText::new("Your worn set, scored in HP-equivalents for the trio, and what in the whole in-era catalogue would beat each slot.")
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
        ui.add_space(8.0);

        /* the trio and level, from the character store */
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("trio")
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
            );
            for i in 0..3 {
                let cur = self.char_.classes[i].clone();
                let mut sel = cur.clone();
                egui::ComboBox::from_id_salt(("gear-trio", i))
                    .icon(crate::chrome::combo_icon)
                    .selected_text(
                        RichText::new(&cur)
                            .font(FontId::monospace(11.5))
                            .color(TEXT),
                    )
                    .width(70.0)
                    .show_ui(ui, |ui| {
                        for c in CLASSES {
                            ui.selectable_value(
                                &mut sel,
                                c.to_owned(),
                                format!("{c}  {}", class_name(c)),
                            );
                        }
                    });
                if sel != cur {
                    let mut next = self.char_.classes.clone();
                    next[i] = sel;
                    if CharState::valid3(&next) {
                        self.char_.classes = next;
                        changed = true;
                    }
                }
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new("level")
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
            );
            let mut lvl = self.char_.level as i64;
            if ui
                .add(
                    egui::DragValue::new(&mut lvl)
                        .range(1..=MAX_LEVEL as i64)
                        .speed(0.2),
                )
                .changed()
            {
                self.char_.level = CharState::clamp_level(lvl);
                changed = true;
            }
        });
        if changed {
            self.persist(cx);
        }
        /* An unset trio is the default one, and a score for WAR/CLR/WIZ presented as a
         * score for "you" is an invented fact. Said on the line, not on a hover. */
        if !CharState::stored(cx.settings) {
            ui.label(RichText::new("This is the default trio (WAR/CLR/WIZ at 50), not one you chose. Every score below is priced for it until you pick your three classes above.").font(FontId::proportional(11.0)).color(TEXT_2));
        }
        ui.add_space(6.0);

        /* what this screen has to work with, said plainly */
        let dump_line = self.dump.status_line();
        ui.label(
            RichText::new(dump_line)
                .font(FontId::monospace(11.0))
                .color(TEXT_2),
        );
        match &self.cat {
            Some(c) if c.problem.is_none() => {
                let src = cx
                    .data
                    .map(|d| d.report().root.display().to_string())
                    .unwrap_or_default();
                let txt = if c.cat.is_empty() {
                    format!("the snapshot at {src} loaded but carries no item records, so nothing here can be scored")
                } else if c.cat.source_items != c.cat.len() {
                    format!(
                        "{} of {} item records readable from {src} (the rest have no name)",
                        c.cat.len(),
                        c.cat.source_items
                    )
                } else {
                    format!("{} item records from {src}", c.cat.len())
                };
                ui.label(
                    RichText::new(txt)
                        .font(FontId::monospace(11.0))
                        .color(TEXT_2),
                );
            }
            Some(c) => {
                ui.label(
                    RichText::new(format!(
                        "item records could not be read: {}",
                        c.problem.as_deref().unwrap_or("")
                    ))
                    .font(FontId::monospace(11.0))
                    .color(TEXT_2),
                );
            }
            None => {
                let why = cx.data_err.map(|e| format!("snapshot failed to load: {e}")).unwrap_or_else(|| {
                    "no snapshot loaded. Put gear-data.json in data/ beside the executable (the Settings screen lists every folder the loader tries) to get item stats.".to_owned()
                });
                ui.label(
                    RichText::new(why)
                        .font(FontId::monospace(11.0))
                        .color(TEXT_2),
                );
            }
        }
        if let Some(p) = &self.save_problem {
            ui.label(
                RichText::new(format!("settings did not save: {p}"))
                    .font(FontId::proportional(11.0))
                    .color(WRONG),
            );
        }
        ui.add_space(8.0);

        /* the no-dump state names where it looked, in the same words as the Inventory screen */
        if self.dump.found().is_none() {
            crate::screens::inventory::no_dump_ui(ui, &self.dump);
            self.priorities_panel(ui, cx);
            return;
        }

        let Some(cc) = self.cat.as_ref().filter(|c| c.problem.is_none()) else {
            ui.label(
                RichText::new("The dump is here, but scoring needs item stats from the snapshot.")
                    .font(FontId::proportional(12.5))
                    .color(TEXT),
            );
            self.priorities_panel(ui, cx);
            return;
        };

        /* the worn set, resolved, and the scorer built over it */
        let mut eq = self
            .dump
            .found()
            .map(|f| crate::screens::inventory::equipped_of(&f.rows))
            .unwrap_or_else(Equipped::empty);
        eq.resolve(&cc.cat);
        let scorer = Scorer::make(
            &self.char_.classes,
            self.char_.level,
            Some(&eq),
            Some(&cc.cat),
            &self.overrides,
        );
        let lines = worn_lines(&scorer, &eq, &cc.cat);
        let total: f64 = lines.iter().filter_map(|l| l.score).sum();
        let unknown = lines.iter().filter(|l| l.unknown).count();

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} worn", eq.count()))
                    .font(FontId::monospace(11.5))
                    .color(TEXT_2),
            );
            ui.label(
                RichText::new(format!("worn AC {}", scorer.worn_ac as i64))
                    .font(FontId::monospace(11.5))
                    .color(TEXT_2),
            );
            ui.label(
                RichText::new(format!(
                    "softcap {} at {} then x{}",
                    scorer.cap_v as i64, scorer.level, scorer.cap_mult
                ))
                .font(FontId::monospace(11.5))
                .color(TEXT_3),
            );
            ui.label(
                RichText::new(format!("total score {}", fmt1(total)))
                    .font(FontId::monospace(11.5))
                    .color(GOLD),
            );
            if unknown > 0 {
                ui.label(
                    RichText::new(format!(
                        "{unknown} worn item{} the wiki has no record of, unscored",
                        if unknown == 1 { "" } else { "s" }
                    ))
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
                );
            }
        });
        ui.label(
            RichText::new("Clicks, procs, worn effects and instruments are not part of the score; a slot carrying one is marked.")
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
        ui.add_space(6.0);

        let mut open_all: Option<Option<String>> = None;
        let show_all = self.show_all_upgrades.clone();
        egui::ScrollArea::vertical()
            .id_salt("gear-worn")
            .show(ui, |ui| {
                egui::Grid::new("gear-grid")
                    .striped(true)
                    .num_columns(6)
                    .min_col_width(40.0)
                    .spacing([14.0, 4.0])
                    .show(ui, |ui| {
                        for h in [
                            "Slot",
                            "Worn",
                            "AC",
                            "Score",
                            "Beaten by",
                            "Best upgrade in era",
                        ] {
                            ui.label(
                                RichText::new(h)
                                    .font(FontId::proportional(11.0))
                                    .color(GOLD_DIM),
                            );
                        }
                        ui.end_row();
                        for l in &lines {
                            let slot_txt = if paired(&l.slot) > 1 {
                                format!("{} {}", l.slot, l.position + 1)
                            } else {
                                l.slot.clone()
                            };
                            ui.label(
                                RichText::new(slot_txt)
                                    .font(FontId::proportional(12.0))
                                    .color(TEXT_2),
                            );
                            match &l.entry {
                                None => {
                                    ui.label(
                                        RichText::new("empty")
                                            .font(FontId::proportional(12.0))
                                            .color(TEXT_3),
                                    );
                                }
                                Some(e) => {
                                    let mut txt = e.name.clone();
                                    if l.unknown {
                                        txt.push_str("  (no wiki record for this name)");
                                    }
                                    let r = ui.label(
                                        RichText::new(txt)
                                            .font(FontId::proportional(12.0))
                                            .color(if l.unknown { TEXT_3 } else { TEXT }),
                                    );
                                    if !l.caveat.is_empty() {
                                        r.on_hover_text(format!(
                                            "unscored: {}",
                                            l.caveat.join(", ")
                                        ));
                                    }
                                }
                            }
                            mono_right(
                                ui,
                                l.ac.map(|a| format!("{}", a as i64)).unwrap_or_default(),
                            );
                            match l.score {
                                Some(s) => mono_right_hover(ui, fmt1(s), terms_text(&l.terms)),
                                None => mono_right(ui, String::new()),
                            }
                            mono_right(
                                ui,
                                if l.entry.is_some() || !l.upgrades.is_empty() {
                                    format!("{}", l.beaten_by)
                                } else {
                                    String::new()
                                },
                            );
                            let expanded = show_all.as_deref() == Some(l.slot.as_str());
                            let take = if expanded { 12 } else { 1 };
                            ui.vertical(|ui| {
                                if l.upgrades.is_empty() {
                                    if l.entry.is_some() && !l.unknown {
                                        ui.label(
                                            RichText::new("nothing in era beats it")
                                                .font(FontId::proportional(11.5))
                                                .color(GOLD_DIM),
                                        );
                                    } else if l.unknown {
                                        ui.label(
                                            RichText::new("cannot compare an item with no record")
                                                .font(FontId::proportional(11.5))
                                                .color(TEXT_3),
                                        );
                                    }
                                }
                                for (ri, s) in l.upgrades.iter().take(take) {
                                    let rec = &cc.cat.recs[*ri];
                                    let gain = s - l.score.unwrap_or(0.0);
                                    let mut line = format!("{}  +{}", rec.name, fmt1(gain));
                                    /* where to go for it: the first zone the wiki lists, or that it is a
                                     * quest reward, and the era the wiki files it under */
                                    let mut from: Vec<&str> = Vec::new();
                                    if let Some(z) = rec.src_zones.first() {
                                        from.push(z);
                                    } else if rec.quest {
                                        from.push("quest");
                                    }
                                    if let Some(e) = &rec.era {
                                        from.push(e);
                                    }
                                    if !from.is_empty() {
                                        line.push_str(&format!("  ({})", from.join(", ")));
                                    }
                                    let r = ui.label(
                                        RichText::new(line)
                                            .font(FontId::proportional(11.5))
                                            .color(TEXT),
                                    );
                                    let cav = rec.unscored();
                                    if !cav.is_empty() {
                                        r.on_hover_text(format!(
                                            "carries an unscored {}",
                                            cav.join(", ")
                                        ));
                                    }
                                }
                                if l.upgrades.len() > 1 {
                                    let label = if expanded {
                                        "fewer".to_owned()
                                    } else {
                                        format!("+{} more", l.upgrades.len() - 1)
                                    };
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(label)
                                                    .font(FontId::proportional(10.5))
                                                    .color(GOLD_DIM),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        open_all = Some(if expanded {
                                            None
                                        } else {
                                            Some(l.slot.clone())
                                        });
                                    }
                                }
                            });
                            ui.end_row();
                        }
                        ui.label(
                            RichText::new("total")
                                .font(FontId::proportional(12.0))
                                .color(GOLD_DIM),
                        );
                        ui.label("");
                        mono_right(ui, format!("{}", scorer.worn_ac as i64));
                        mono_right(ui, fmt1(total));
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    });
            });
        if let Some(o) = open_all {
            self.show_all_upgrades = o;
        }

        ui.add_space(10.0);
        self.priorities_panel(ui, cx);
    }

    /// The stat priorities list: every STAT_DEFS row with its computed value,
    /// an editable override, and "auto" or "custom". Edits are saved into settings as they land.
    fn priorities_panel(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let n = self.overrides.len();
        ui.horizontal(|ui| {
            let label = if self.show_priorities {
                "Stat priorities  (close)"
            } else {
                "Stat priorities"
            };
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(label)
                            .font(FontId::proportional(12.0))
                            .color(GOLD),
                    )
                    .frame(false),
                )
                .clicked()
            {
                self.show_priorities = !self.show_priorities;
            }
            ui.label(
                RichText::new(priorities_summary(n))
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
            );
        });
        if !self.show_priorities {
            return;
        }
        ui.label(
            RichText::new("HP-equivalents per point of the stat, computed from your trio and level. Hover a stat for the derivation. Edits here move the trio's scores; the per-class ranks on Inventory keep each class's own weights.")
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
        ui.add_space(4.0);
        let scorer = Scorer::make(
            &self.char_.classes,
            self.char_.level,
            None,
            None,
            &self.overrides,
        );
        let for_key = (self.char_.classes.clone(), self.char_.level);
        if self.edit_for.as_ref() != Some(&for_key) {
            self.edit = scorer.w.0;
            self.edit_for = Some(for_key);
        }
        let mut dirty = false;
        egui::Grid::new("gear-prio")
            .num_columns(4)
            .spacing([14.0, 3.0])
            .show(ui, |ui| {
                for (i, (k, label)) in STAT_DEFS.iter().enumerate() {
                    let custom = self.overrides.contains_key(*k);
                    let why = scorer
                        .why
                        .iter()
                        .find(|(s, _)| s == k)
                        .map(|(_, w)| w.clone())
                        .unwrap_or_default();
                    ui.label(
                        RichText::new(*label)
                            .font(FontId::proportional(12.0))
                            .color(if custom { GOLD } else { TEXT }),
                    )
                    .on_hover_text(why);
                    mono_right(ui, fmt1(scorer.computed.0[i]));
                    let mut v = self.edit[i];
                    let r = ui.add(
                        egui::DragValue::new(&mut v)
                            .range(0.0..=99.0)
                            .speed(0.05)
                            .fixed_decimals(1),
                    );
                    if r.changed() {
                        self.edit[i] = v;
                    }
                    if r.lost_focus() || r.drag_stopped() {
                        let c = v.clamp(0.0, 99.0);
                        /* an edit back to the computed value is not an override: only a real
                         * difference is stored */
                        if (c - scorer.computed.0[i]).abs() < 1e-9 {
                            if self.overrides.remove(*k).is_some() {
                                dirty = true;
                            }
                        } else if self
                            .overrides
                            .get(*k)
                            .map(|x| (x - c).abs() > 1e-9)
                            .unwrap_or(true)
                        {
                            self.overrides.insert((*k).to_owned(), c);
                            dirty = true;
                        }
                    }
                    ui.label(
                        RichText::new(if custom { "custom" } else { "auto" })
                            .font(FontId::proportional(10.5))
                            .color(if custom { GOLD_DIM } else { TEXT_3 }),
                    );
                    ui.end_row();
                }
            });
        if ui
            .add(egui::Button::new(
                RichText::new("reset to computed")
                    .font(FontId::proportional(11.0))
                    .color(TEXT_2),
            ))
            .clicked()
            && !self.overrides.is_empty()
        {
            self.overrides.clear();
            self.edit = scorer.computed.0;
            dirty = true;
        }
        if dirty {
            self.persist(cx);
        }
    }
}

/// The worn table's lines: each position of each WORN slot, the occupant's score, and the in-era
/// catalogue items that beat the weakest occupant for the trio (legal for
/// the trio, placeable, scored at +0 from the same softcap position).
fn worn_lines(scorer: &Scorer, eq: &Equipped, cat: &Catalogue) -> Vec<SlotLine> {
    let mut out = Vec::new();
    for slot in WORN_SLOTS {
        let list = eq.get(slot);
        let weakest = scorer.weakest_eq(eq, cat, slot);
        /* candidates that beat the weakest occupant, at +0, scored from the same AC base */
        let mut ups: Vec<(usize, f64)> = Vec::new();
        if slot != ANY {
            for (i, rec) in cat.recs.iter().enumerate() {
                if rec.oe || !rec.scorable() || !rec.sl.iter().any(|s| s == slot) {
                    continue;
                }
                if !scorer.legal(rec) || !scorer.can_place(rec, slot) {
                    continue;
                }
                let s = scorer.score(rec, 0, weakest.base_ac, slot);
                if s > weakest.s {
                    ups.push((i, s));
                }
            }
            ups.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| cat.recs[a.0].name.cmp(&cat.recs[b.0].name))
            });
        }
        let n = paired(slot);
        for position in 0..n {
            let entry = list.get(position).cloned();
            let (ac, score, terms, caveat, unknown) = match &entry {
                None => (None, None, Vec::new(), Vec::new(), false),
                Some(e) => match e.rec {
                    None => (None, None, Vec::new(), Vec::new(), true),
                    Some(r) => {
                        let rec = &cat.recs[r];
                        let a = ac_of(rec, e.tier);
                        let base_ac = scorer.worn_ac - a;
                        let s = scorer.score(rec, e.tier, base_ac, slot);
                        (
                            Some(a),
                            Some(s),
                            scorer.terms(rec, e.tier, base_ac, slot),
                            rec.unscored(),
                            false,
                        )
                    }
                },
            };
            /* the upgrade column belongs to the weakest position; the other position of a pair
             * shows nothing rather than repeating it */
            let mine = weakest.entry == Some(position)
                || (weakest.entry.is_none()
                    && entry.is_none()
                    && position == list.len().min(n - 1).max(list.len()));
            out.push(SlotLine {
                slot: slot.to_owned(),
                position,
                entry,
                ac,
                score,
                terms,
                upgrades: if mine { ups.clone() } else { Vec::new() },
                beaten_by: if mine { ups.len() } else { 0 },
                caveat,
                unknown,
            });
        }
    }
    out
}

pub fn fmt1(v: f64) -> String {
    format!("{:.1}", v)
}

/// A stat value as the item prints it: whole numbers whole, a ratio to two places.
fn fmt_v(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
    }
}

/// The breakdown as hover text, the "why this number": one line per term with the
/// value the item carries and what it is worth in HP-equivalents; an unscored line says so.
pub fn terms_text(terms: &[Term]) -> String {
    if terms.is_empty() {
        return "nothing on it that the score reads".to_owned();
    }
    /* an unscored line says why, keyed on the term: VOID is the upgrade-tier marker, DMG and
     * delay are priced through the ratio, and a ratio in a slot that never swings is worth
     * nothing there */
    let why_not = |k: &str| match k {
        "sv:v" => "unscored: SV VOID is the upgrade-tier marker, no combat effect yet",
        "dmg" | "dly" => "unscored on its own: priced through the ratio",
        "ratio" => "unscored: this slot does not swing",
        _ => "unscored",
    };
    let mut lines: Vec<String> = terms
        .iter()
        .map(|t| {
            if t.unscored {
                format!("{}: {}  ({})", t.label, fmt_v(t.v), why_not(&t.k))
            } else {
                format!("{}: {}  = {}", t.label, fmt_v(t.v), fmt1(t.pts))
            }
        })
        .collect();
    let total: f64 = terms.iter().map(|t| t.pts).sum();
    lines.push(format!("total = {}", fmt1(total)));
    lines.join("\n")
}

/// The full standing list as hover text: every (class, slot) the
/// item ranks in, best first, and whether it was every class because the wiki names none.
pub fn standings_text(b: &Best) -> String {
    let mut lines: Vec<String> = b
        .all
        .iter()
        .take(8)
        .map(|s| format!("{} {}  #{} of {} {}", s.cls, s.slot, s.rank, s.of, s.noun))
        .collect();
    if b.all.len() > 8 {
        lines.push(format!("and {} more", b.all.len() - 8));
    }
    if b.any_class {
        lines.push("the wiki names no class line, so every class was ranked".to_owned());
    }
    lines.join("\n")
}

/// A right-aligned monospace cell: numbers a person subtracts by eye have to line up.
pub fn mono_right(ui: &mut Ui, s: String) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(RichText::new(s).font(FontId::monospace(11.5)).color(TEXT));
    });
}

/// The same cell with a hover text, for a number that has a breakdown behind it.
pub fn mono_right_hover(ui: &mut Ui, s: String, hover: String) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(RichText::new(s).font(FontId::monospace(11.5)).color(TEXT))
            .on_hover_text(hover);
    });
}

/* ------------------------------------------------------------------ tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(json: &str) -> GearRec {
        GearRec::from_value(&serde_json::from_str::<Value>(json).unwrap()).unwrap()
    }
    fn trio() -> Vec<String> {
        vec!["WAR".into(), "CLR".into(), "WIZ".into()]
    }

    /// The real gear-data.json beside this crate. Fails LOUDLY when absent unless GRIMOIRE_NO_DATA=1.
    fn real_gear_data() -> Option<Value> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("gear-data.json");
        match std::fs::read(&p) {
            Ok(bytes) => Some(
                serde_json::from_slice(&bytes)
                    .unwrap_or_else(|e| panic!("{} is not JSON: {e}", p.display())),
            ),
            Err(e) => {
                if std::env::var("GRIMOIRE_NO_DATA").as_deref() == Ok("1") {
                    crate::data::testdata::say_skipped(&format!(
                        "GRIMOIRE_NO_DATA=1 and {} is absent ({e})",
                        p.display()
                    ));
                    None
                } else {
                    panic!(
                        "real data test needs {} ({e}). Set GRIMOIRE_NO_DATA=1 to skip on purpose.",
                        p.display()
                    )
                }
            }
        }
    }

    #[test]
    fn tier_math_uses_integer_arithmetic_and_the_plus_one_floor() {
        /* 45 x 1.4 = 62.999... in IEEE-754; the rule says 63 */
        assert_eq!(stat_at(45, 4), 63);
        /* base 1: floor never moves it, the +1 floor does */
        assert_eq!(stat_at(1, 3), 4);
        assert_eq!(stat_at(10, 0), 10);
        assert_eq!(stat_at(-10, 5), -10);
        assert_eq!(stat_at(0, 5), 0);
        assert_eq!(stat_at(20, 1), 22);
    }

    #[test]
    fn paired_slots_hold_two_and_two_handers_are_two_handers() {
        assert_eq!(paired("Ear"), 2);
        assert_eq!(paired("Wrist"), 2);
        assert_eq!(paired("Fingers"), 2);
        assert_eq!(paired(ANY), 2);
        assert_eq!(paired("Head"), 1);
        assert_eq!(paired("Primary"), 1);
        assert!(two_h(Some("2H Slashing")));
        assert!(two_h(Some("2h blunt")));
        assert!(two_h(Some("Two-Handed Piercing")));
        assert!(!two_h(Some("1H Slashing")));
        assert!(!two_h(Some("Piercing")));
        assert!(!two_h(None));
        assert_eq!(WORN_SLOTS.len(), SLOTS.len() + 1);
        assert_eq!(
            WORN_SLOTS[WORN_SLOTS.len() - 1],
            ANY,
            "the ingest's table ends with Any Slot, which is what this screen's ANY names"
        );
        assert_eq!(
            &WORN_SLOTS[..SLOTS.len()],
            &SLOTS[..],
            "and starts with the wiki's twenty, in SLOTS order"
        );
        assert_eq!(worn_slot("Ring"), Some("Fingers"));
        assert_eq!(worn_slot("Finger"), Some("Fingers"));
        assert_eq!(worn_slot("Any Slot"), Some(ANY));
        assert_eq!(worn_slot("General1"), None);
    }

    #[test]
    fn off_hand_gate_refuses_two_handers_and_damaging_one_handers_for_non_dual_wielders() {
        let two =
            rec(r#"{"n":"Big Sword","dmg":20,"dly":40,"skill":"2H Slashing","sl":["Primary"]}"#);
        let one = rec(
            r#"{"n":"Small Sword","dmg":5,"dly":20,"skill":"1H Slashing","sl":["Primary","Secondary"]}"#,
        );
        let shield = rec(r#"{"n":"Shield","st":{"ac":10},"sl":["Secondary"]}"#);
        let war = Scorer::make(&["WAR".to_owned()], 50, None, None, &HashMap::new());
        let clr = Scorer::make(&["CLR".to_owned()], 50, None, None, &HashMap::new());
        assert!(war.dual_wield_cap() > 0);
        assert_eq!(clr.dual_wield_cap(), 0);
        assert!(!war.can_place(&two, "Secondary"));
        assert!(war.can_place(&one, "Secondary"));
        assert!(!clr.can_place(&one, "Secondary"));
        assert!(clr.can_place(&shield, "Secondary"));
        assert!(clr.can_place(&two, "Primary"));
    }

    #[test]
    fn weights_derive_from_the_trio_as_gear_score_does() {
        let (w, _) = computed_weights(&trio(), 50, None);
        assert_eq!(w.get("hp"), 1.0);
        assert_eq!(w.get("mana"), 1.1);
        assert_eq!(w.get("sta"), 7.0);
        assert_eq!(w.get("int"), 12.4);
        assert_eq!(w.get("wis"), 12.4);
        assert_eq!(w.get("ac"), 3.0);
        assert_eq!(w.get("atk"), 3.0);
        assert_eq!(w.get("str"), 2.0);
        assert_eq!(w.get("agi"), 0.7);
        assert_eq!(w.get("dex"), 0.8);
        assert_eq!(w.get("cha"), 0.05);
        assert_eq!(w.get("end"), 0.3);
        assert_eq!(w.get("sv"), 0.4);
        assert_eq!(w.get("haste"), 3.0);
        assert_eq!(w.get("ratio"), 15.0);
        /* a cast-heavy trio zeroes haste and drops ratio to 1 */
        let (c, _) = computed_weights(&["CLR".into(), "WIZ".into(), "ENC".into()], 50, None);
        assert_eq!(c.get("haste"), 0.0);
        assert_eq!(c.get("ratio"), 1.0);
        assert_eq!(c.get("ac"), 1.2);
        /* RNG/PAL/ENC: one INT pool and one WIS pool, and both are counted */
        let (m, _) = computed_weights(&["RNG".into(), "PAL".into(), "ENC".into()], 50, None);
        assert!(
            m.get("int") > 0.0,
            "INT must be worth something with an Enchanter in the trio"
        );
        assert!(m.get("wis") > 0.0);
    }

    #[test]
    fn gear_score_prices_one_item_against_stat_defs() {
        let sc = Scorer::make(&trio(), 50, None, None, &HashMap::new());
        let r = rec(
            r#"{"n":"Test Neck","st":{"ac":10,"hp":25,"str":5},"sv":{"f":5,"v":10},"sl":["Neck"]}"#,
        );
        /* hp 25 x 1 + str 5 x 2 + ac 10 x 3 (under the 410 cap) + fire 5 x 0.4; VOID unscored */
        let s = sc.score(&r, 0, 0.0, "Neck");
        assert!((s - 67.0).abs() < 1e-9, "score was {s}");
        let terms = sc.terms(&r, 0, 0.0, "Neck");
        let sum: f64 = terms.iter().map(|t| t.pts).sum();
        assert!((sum - s).abs() < 1e-9, "terms must sum to the score");
        assert!(terms
            .iter()
            .any(|t| t.k == "sv:v" && t.unscored && t.pts == 0.0));
        /* the tier raises the stats through statAt: hp 25 to 35, str 5 to 9 (the +1 floor
         * dominates a small base: 6, 7, 8, 9), ac 10 to 14; resists do not tier */
        assert_eq!((stat_at(25, 4), stat_at(5, 4), stat_at(10, 4)), (35, 9, 14));
        let s4 = sc.score(&r, 4, 0.0, "Neck");
        assert!(
            (s4 - (35.0 + 9.0 * 2.0 + 3.0 * 14.0 + 2.0)).abs() < 1e-9,
            "tier 4 score was {s4}"
        );
        /* an override moves the number */
        let mut o = HashMap::new();
        o.insert("hp".to_owned(), 2.0);
        let sc2 = Scorer::make(&trio(), 50, None, None, &o);
        assert!((sc2.score(&r, 0, 0.0, "Neck") - 92.0).abs() < 1e-9);
        assert_eq!(sc2.computed.get("hp"), 1.0);
    }

    #[test]
    fn ac_past_the_softcap_counts_at_the_class_multiplier() {
        let sc = Scorer::make(&trio(), 50, None, None, &HashMap::new());
        assert_eq!(sc.cap_v, 410.0);
        assert!((sc.cap_mult - 0.35).abs() < 1e-6);
        let r = rec(r#"{"n":"Plate","st":{"ac":20},"sl":["Chest"]}"#);
        let under = sc.score(&r, 0, 0.0, "Chest");
        let over = sc.score(&r, 0, 410.0, "Chest");
        assert!((under - 60.0).abs() < 1e-9);
        assert!((over - 60.0 * 0.35).abs() < 1e-6, "over was {over}");
        /* no class table: zero cap, quarter rate */
        let (v, m) = softcap(&["XXX".to_owned()], 50);
        assert_eq!((v, m), (0.0, 0.25));
    }

    #[test]
    fn weapon_ratio_counts_only_where_it_swings_and_doubles_for_two_handers() {
        let sc = Scorer::make(&trio(), 50, None, None, &HashMap::new());
        let one = rec(
            r#"{"n":"Blade","dmg":10,"dly":20,"skill":"1H Slashing","sl":["Primary","Secondary"]}"#,
        );
        let two =
            rec(r#"{"n":"Claymore","dmg":10,"dly":20,"skill":"2H Slashing","sl":["Primary"]}"#);
        assert!((sc.score(&one, 0, 0.0, "Primary") - 15.0 * 10.0 * 0.5).abs() < 1e-9);
        assert!((sc.score(&two, 0, 0.0, "Primary") - 15.0 * 10.0 * 0.5 * 2.0).abs() < 1e-9);
        assert_eq!(
            sc.score(&one, 0, 0.0, ANY),
            0.0,
            "a weapon parked in Any Slot does not swing"
        );
        assert!(sc
            .terms(&one, 0, 0.0, ANY)
            .iter()
            .any(|t| t.k == "ratio" && t.unscored));
    }

    #[test]
    fn legal_reads_the_four_class_set_shapes_and_the_level_requirement() {
        let sc = Scorer::make(&trio(), 50, None, None, &HashMap::new());
        assert!(sc.legal(&rec(r#"{"n":"a","cls":{"all":1}}"#)));
        assert!(sc.legal(&rec(r#"{"n":"a"}"#)));
        assert!(!sc.legal(&rec(r#"{"n":"a","cls":{"none":1}}"#)));
        assert!(sc.legal(&rec(r#"{"n":"a","cls":{"all":1,"x":["WAR","CLR"]}}"#)));
        assert!(!sc.legal(&rec(r#"{"n":"a","cls":{"all":1,"x":["WAR","CLR","WIZ"]}}"#)));
        assert!(sc.legal(&rec(r#"{"n":"a","cls":{"c":["WIZ"]}}"#)));
        assert!(!sc.legal(&rec(r#"{"n":"a","cls":{"c":["MNK"]}}"#)));
        assert!(!sc.legal(&rec(r#"{"n":"a","req":51}"#)));
        let low = Scorer::make(&trio(), 48, None, None, &HashMap::new());
        assert!(!low.legal(&rec(r#"{"n":"a","req":49}"#)));
        assert!(sc.legal(&rec(r#"{"n":"a","req":49}"#)));
        /* the class-list rule, every shape */
        assert_eq!(rec(r#"{"n":"a"}"#).classes(), None);
        assert_eq!(rec(r#"{"n":"a","cls":{"all":1}}"#).classes(), None);
        assert_eq!(rec(r#"{"n":"a","cls":{"none":1}}"#).classes(), Some(vec![]));
        assert_eq!(
            rec(r#"{"n":"a","cls":{"all":1,"x":["WAR"]}}"#)
                .classes()
                .map(|c| c.len()),
            Some(15)
        );
        assert_eq!(
            rec(r#"{"n":"a","cls":{"c":["WIZ","XYZ"]}}"#).classes(),
            Some(vec!["WIZ"])
        );
    }

    #[test]
    fn names_key_exactly_and_fall_back_to_article_stripping() {
        assert_eq!(
            item_key("Ambassador D`Vinn's  Hat "),
            "ambassador dvinns hat"
        );
        assert_eq!(norm_name("A Sapphire"), "sapphire");
        assert_eq!(
            strip_decor("Giant Snake Fang +4 (Exaltation)*"),
            "Giant Snake Fang"
        );
        let cat = Catalogue::from_values(
            [
                serde_json::json!({"n":"Sapphire","st":{"cha":1}}),
                serde_json::json!({"n":"A Sapphire","st":{"cha":2}}),
                serde_json::json!({"n":"The Old Ring","st":{"hp":1}}),
            ]
            .iter(),
        );
        assert_eq!(
            cat.find("Sapphire").map(|i| cat.recs[i].name.as_str()),
            Some("Sapphire")
        );
        assert_eq!(
            cat.find("A Sapphire").map(|i| cat.recs[i].name.as_str()),
            Some("A Sapphire")
        );
        assert_eq!(
            cat.find("Old Ring +3").map(|i| cat.recs[i].name.as_str()),
            Some("The Old Ring")
        );
        assert_eq!(cat.find("Nothing"), None);
    }

    #[test]
    fn ranker_ranks_one_handers_and_two_handers_in_their_own_catalogues() {
        let cat = Catalogue::from_values(
            [
                serde_json::json!({"n":"Great Axe","dmg":30,"dly":40,"skill":"2H Slashing","sl":["Primary"],"cls":{"c":["WAR"]}}),
                serde_json::json!({"n":"Big Axe","dmg":20,"dly":40,"skill":"2H Slashing","sl":["Primary"],"cls":{"c":["WAR"]}}),
                serde_json::json!({"n":"Short Sword","dmg":6,"dly":20,"skill":"1H Slashing","sl":["Primary","Secondary"],"cls":{"c":["WAR"]}}),
                serde_json::json!({"n":"Long Sword","dmg":8,"dly":20,"skill":"1H Slashing","sl":["Primary","Secondary"],"cls":{"c":["WAR"]}}),
                serde_json::json!({"n":"Old Sword","dmg":9,"dly":20,"skill":"1H Slashing","sl":["Primary"],"cls":{"c":["WAR"]},"oe":1}),
                serde_json::json!({"n":"Clicky Only","eff":[{"n":"Hug"}],"sl":["Primary"]}),
            ]
            .iter(),
        );
        let mut rk = Ranker::new(50);
        let short = cat.find("Short Sword").unwrap();
        let r = rk.rank(&cat, short, "WAR", "Primary", 0).unwrap();
        assert_eq!(
            (r.rank, r.of, r.kind, r.noun),
            (2, 2, "1H", "one-handers"),
            "a 1H ranks among one-handers only"
        );
        let big = cat.find("Big Axe").unwrap();
        let r2 = rk.rank(&cat, big, "WAR", "Primary", 0).unwrap();
        assert_eq!((r2.rank, r2.of, r2.kind), (2, 2, "2H"));
        assert_eq!(r2.better, vec![cat.find("Great Axe").unwrap()]);
        /* out of era: no rank; effect only: no rank */
        assert!(rk
            .rank(&cat, cat.find("Old Sword").unwrap(), "WAR", "Primary", 0)
            .is_none());
        assert!(rk
            .rank(&cat, cat.find("Clicky Only").unwrap(), "WAR", "Primary", 0)
            .is_none());
        /* the off hand is its own mixture */
        let r3 = rk.rank(&cat, short, "WAR", "Secondary", 0).unwrap();
        assert_eq!((r3.rank, r3.of, r3.noun), (2, 2, "off-hand items"));
        /* best: trio and any-class answers side by side */
        let b = rk.best(
            &cat,
            short,
            &["CLR".to_owned(), "DRU".to_owned(), "WIZ".to_owned()],
        );
        assert_eq!(b.best.as_ref().map(|s| (s.cls, s.rank)), Some(("WAR", 2)));
        assert!(b.trio.is_empty(), "none of the trio can wear it");
        assert!(!b.any_class);
        /* the standing carries WHAT beats it, by name, and its tooltip says so */
        let s = b.best.as_ref().unwrap();
        assert_eq!(s.better.len(), 1, "one one-hander scores above it");
        assert_ne!(s.better[0], "Short Sword");
        assert!(
            s.what().ends_with(&format!("beaten by {}", s.better[0])),
            "{}",
            s.what()
        );
        /* the tier changes the rank asked for, not the pool */
        let r4 = rk.rank(&cat, short, "WAR", "Primary", 10).unwrap();
        assert_eq!(r4.rank, 1);
        assert_eq!(r4.of, 2);
    }

    #[test]
    fn spare_verdict_fills_niches_and_keeps_paired_slots_second_best() {
        /* Five all-class rings. Fingers holds two, so Gold and Silver are worn; Bronze and Copper
         * are the leftovers that fill Any Slot's two positions for every class; Tin is beaten in
         * both and is spare. The Bard-only ring has BRD niches only and is beaten in all of them. */
        let cat = Catalogue::from_values(
            [
                serde_json::json!({"n":"Gold Ring","st":{"hp":30},"sl":["Fingers"],"cls":{"all":1}}),
                serde_json::json!({"n":"Silver Ring","st":{"hp":20},"sl":["Fingers"],"cls":{"all":1}}),
                serde_json::json!({"n":"Bronze Ring","st":{"hp":15},"sl":["Fingers"],"cls":{"all":1}}),
                serde_json::json!({"n":"Copper Ring","st":{"hp":12},"sl":["Fingers"],"cls":{"all":1}}),
                serde_json::json!({"n":"Tin Ring","st":{"hp":10},"sl":["Fingers"],"cls":{"all":1}}),
                serde_json::json!({"n":"Bard Only Ring","st":{"hp":5},"sl":["Fingers"],"cls":{"c":["BRD"]}}),
                serde_json::json!({"n":"Lute","st":{"hp":1},"sl":["Primary"],"cls":{"c":["BRD"]},"inst":["Stringed"]}),
            ]
            .iter(),
        );
        let row = |i: usize, name: &str, tier: u32| OwnedRow {
            i,
            rec: cat.find(name),
            tier,
            exalt: false,
        };
        let rows = vec![
            row(0, "Gold Ring", 0),
            row(1, "Silver Ring", 0),
            row(2, "Bronze Ring", 0),
            row(3, "Copper Ring", 0),
            row(4, "Tin Ring", 0),
            row(5, "Bard Only Ring", 0),
            row(6, "Lute", 0),
            OwnedRow {
                i: 7,
                rec: None,
                tier: 0,
                exalt: false,
            },
            OwnedRow {
                i: 8,
                rec: cat.find("Gold Ring"),
                tier: 0,
                exalt: true,
            },
        ];
        let rep = analyze_spare(&rows, &cat, 50);
        let find = |name: &str| {
            rep.items
                .iter()
                .find(|it| cat.recs[it.rec].name == name)
                .unwrap()
        };
        assert!(!find("Gold Ring").spare);
        assert!(
            !find("Silver Ring").spare,
            "second best in a paired slot is still worn"
        );
        assert_eq!(
            find("Silver Ring")
                .best
                .as_ref()
                .map(|n| (n.slot.as_str(), n.ahead)),
            Some(("Fingers", 1))
        );
        assert!(
            !find("Bronze Ring").spare,
            "the best leftover holds an Any Slot position"
        );
        assert_eq!(
            find("Bronze Ring").best.as_ref().map(|n| n.slot.as_str()),
            Some(ANY)
        );
        assert!(!find("Copper Ring").spare, "Any Slot holds two");
        assert!(
            find("Tin Ring").spare,
            "two better rings in Fingers and two better leftovers in Any Slot"
        );
        assert_eq!(
            find("Tin Ring").ahead,
            2,
            "the reported count is the closest niche's: Any Slot"
        );
        assert_eq!(
            find("Tin Ring").best.as_ref().map(|n| n.slot.as_str()),
            Some(ANY)
        );
        assert!(find("Tin Ring").bis.is_empty());
        assert!(
            find("Bard Only Ring").spare,
            "beaten in every BRD niche it has"
        );
        assert_eq!(find("Bard Only Ring").classes, vec!["BRD"]);
        assert_eq!(rep.skipped_exalt, 1);
        assert_eq!(rep.skipped_unknown, 1);
        assert!(find("Lute").unscored.contains(&"inst"));
        assert!(
            !find("Lute").spare,
            "the only instrument is first pick in BRD Primary"
        );
        assert_eq!(find("Lute").bis, vec!["BRD"]);
        assert_eq!(
            find("Gold Ring").bis.len(),
            16,
            "first pick for every class"
        );
        assert_eq!(
            rep.ranked
                .first()
                .map(|&i| cat.recs[rep.items[i].rec].name.as_str()),
            Some("Bard Only Ring"),
            "most-beaten first: three leftovers ahead of it in BRD Any Slot"
        );
        assert_eq!(
            rep.ranked
                .get(1)
                .map(|&i| cat.recs[rep.items[i].rec].name.as_str()),
            Some("Tin Ring")
        );
    }

    #[test]
    fn spare_parity_scales_the_challenger_up_and_names_the_tier_it_needs() {
        /* Three +5 rings (10 hp base, 15 at +5) fill WAR Fingers; two helms beaten in Head fill
         * WAR Any Slot. The Plain Band (9 hp) is beaten everywhere, at parity and as it is: at +5
         * it would carry 14 hp against the rings' 15. */
        let cat = Catalogue::from_values(
            [
                serde_json::json!({"n":"Ruby Ring","st":{"hp":10},"sl":["Fingers"],"cls":{"c":["WAR"]}}),
                serde_json::json!({"n":"Ruby Ring Twin","st":{"hp":10},"sl":["Fingers"],"cls":{"c":["WAR"]}}),
                serde_json::json!({"n":"Ruby Ring Third","st":{"hp":10},"sl":["Fingers"],"cls":{"c":["WAR"]}}),
                serde_json::json!({"n":"Plain Band","st":{"hp":9},"sl":["Fingers"],"cls":{"c":["WAR"]}}),
                serde_json::json!({"n":"Great Helm","st":{"hp":50},"sl":["Head"],"cls":{"c":["WAR"]}}),
                serde_json::json!({"n":"Good Helm","st":{"hp":40},"sl":["Head"],"cls":{"c":["WAR"]}}),
                serde_json::json!({"n":"Fair Helm","st":{"hp":35},"sl":["Head"],"cls":{"c":["WAR"]}}),
            ]
            .iter(),
        );
        let row = |i: usize, name: &str, tier: u32| OwnedRow {
            i,
            rec: cat.find(name),
            tier,
            exalt: false,
        };
        let rows = vec![
            row(0, "Ruby Ring", 5),
            row(1, "Ruby Ring Twin", 5),
            row(2, "Ruby Ring Third", 5),
            row(3, "Plain Band", 0),
            row(4, "Great Helm", 0),
            row(5, "Good Helm", 0),
            row(6, "Fair Helm", 0),
        ];
        assert_eq!(stat_at(9, 5), 14);
        assert_eq!(stat_at(9, 7), 16);
        let rep = analyze_spare(&rows, &cat, 50);
        let find = |name: &str| {
            rep.items
                .iter()
                .find(|it| cat.recs[it.rec].name == name)
                .unwrap()
        };
        let band = find("Plain Band");
        assert!(band.spare);
        assert!(band.spare_as_is);
        let fingers = band.niches.iter().find(|n| n.slot == "Fingers").unwrap();
        assert_eq!(
            fingers.ahead, 3,
            "scaled to +5 it still loses to all three rings"
        );
        assert_eq!(fingers.ahead_as_is, 3);
        assert_eq!(
            fingers.by.iter().map(|(_, _, at)| *at).collect::<Vec<_>>(),
            vec![5, 5, 5],
            "the challenger is scaled UP to each rival's rank"
        );
        /* +6 is the first rank at which the band reaches the rings' 15 hp. A TIE between different
         * items is not "ahead" (nothing in the model separates them), so at +6 no ring is ahead
         * and a Fingers position is free. Strictly beating them would take +7 (16 hp). */
        assert_eq!(stat_at(9, 6), 15);
        assert_eq!(band.upgrade_to, Some(6));
        assert!(!find("Good Helm").spare, "beaten in Head, kept in Any Slot");
        assert!(!find("Fair Helm").spare);
        /* the same item at a higher tier is ahead of its twin; a tie between DIFFERENT items is not */
        let ruby = find("Ruby Ring");
        assert_eq!(ruby.ahead, 0);
    }

    #[test]
    fn race_deity_and_requirement_gates_are_pairwise_subset_tests() {
        let any = Set::All { except: vec![] };
        let human = Set::Only(vec!["HUM".into()]);
        let no_iksar = Set::All {
            except: vec!["IKS".into()],
        };
        assert!(race_covers(&any, &human));
        assert!(
            !race_covers(&human, &any),
            "an explicit list cannot cover all races"
        );
        assert!(race_covers(&no_iksar, &human));
        assert!(!race_covers(&human, &no_iksar));
        assert!(race_covers(&Set::Unstated, &Set::Unstated));
        assert!(race_covers(&no_iksar, &Set::NoOne));
        let locked = rec(r#"{"n":"a","deity":"Cazic-Thule"}"#);
        let free = rec(r#"{"n":"b"}"#);
        assert!(deity_covers(&free, &locked));
        assert!(!deity_covers(&locked, &free));
        let r49 = rec(r#"{"n":"a","req":49}"#);
        assert!(req_covers(&free, &r49));
        assert!(!req_covers(&r49, &free));
    }

    #[test]
    fn priorities_and_char_state_round_trip_through_settings_extra() {
        let mut s = Settings::default();
        assert!(read_overrides(&s).is_empty());
        let mut w = HashMap::new();
        w.insert("ac".to_owned(), 5.0);
        w.insert("bogus".to_owned(), 1.0);
        write_overrides(&mut s, &w);
        let back = read_overrides(&s);
        assert_eq!(back.get("ac"), Some(&5.0));
        assert_eq!(
            back.len(),
            1,
            "write stores what it is given; read drops the unknown key"
        );
        assert!(s.extra[PRIORITIES_KEY]["w"].get("bogus").is_some());
        assert_eq!(priorities_summary(0), "(computed for your trio)");
        assert_eq!(priorities_summary(3), "(3 custom)");
        let cs = CharState {
            classes: vec!["RNG".into(), "PAL".into(), "ENC".into()],
            level: 45,
        };
        cs.write(&mut s);
        assert_eq!(CharState::read(&s), cs);
        /* an invalid trio keeps the last good one */
        CharState {
            classes: vec!["RNG".into(), "RNG".into(), "ENC".into()],
            level: 60,
        }
        .write(&mut s);
        let got = CharState::read(&s);
        assert_eq!(got.classes, cs.classes);
        assert_eq!(got.level, 50);
        assert_eq!(CharState::clamp_level(0), 50);
        assert_eq!(CharState::clamp_level(-3), 50);
        assert_eq!(CharState::clamp_level(7), 7);
    }

    #[test]
    fn weakest_occupant_of_a_paired_slot_is_the_empty_position() {
        let cat = Catalogue::from_values(
            [serde_json::json!({"n":"Gold Ring","st":{"hp":30},"sl":["Fingers"]})].iter(),
        );
        let mut eq = Equipped::empty();
        eq.push(
            "Fingers",
            EqEntry {
                name: "Gold Ring".into(),
                base: "Gold Ring".into(),
                tier: 0,
                rec: None,
            },
        );
        eq.resolve(&cat);
        assert_eq!(eq.get("Fingers")[0].rec, Some(0));
        let sc = Scorer::make(&trio(), 50, Some(&eq), Some(&cat), &HashMap::new());
        let w = sc.weakest_eq(&eq, &cat, "Fingers");
        assert_eq!(w.entry, None);
        assert_eq!(w.s, 0.0);
        let h = sc.weakest_eq(&eq, &cat, "Head");
        assert_eq!(h.entry, None);
    }

    #[test]
    fn real_gear_data_loads_and_the_measured_shapes_hold() {
        let Some(g) = real_gear_data() else { return };
        let items = g
            .get("items")
            .and_then(Value::as_object)
            .expect("gear-data.json has an items map");
        let cat = Catalogue::from_values(items.values());
        assert_eq!(
            cat.source_items, 6891,
            "gear-data.json schema 2 carries 6891 items"
        );
        assert!(
            cat.len() >= 6800,
            "nearly every record has a name; got {}",
            cat.len()
        );
        let reaver = cat
            .find("A Dark Reaver")
            .expect("A Dark Reaver is in gear-data");
        assert!(cat.recs[reaver].is_two_h());
        assert_eq!(cat.recs[reaver].cls, Set::Only(vec!["SHD".into()]));
        assert_eq!(cat.recs[reaver].src_zones, vec!["Lower Guk".to_owned()]);
        let baton = cat
            .find("Baton of the Sky")
            .expect("Baton of the Sky is in gear-data");
        assert_eq!(cat.recs[baton].req, 49);
        assert_eq!(cat.recs[baton].stat("ac"), 25);
        let femur = cat.find("A Cracked Femur").unwrap();
        assert!(cat.recs[femur].oe);
        assert_eq!(
            cat.recs[femur].cls,
            Set::All {
                except: vec![
                    "CLR".into(),
                    "PAL".into(),
                    "DRU".into(),
                    "MNK".into(),
                    "SHM".into()
                ]
            }
        );
        let gauntlets = cat.find("Basoon Haste Gauntlets").unwrap();
        assert_eq!(cat.recs[gauntlets].haste, 36);
        /* the ranker over the real catalogue: SHD main hand two-handers is a real pool and the
         * reaver has a place in it */
        let mut rk = Ranker::new(50);
        let r = rk
            .rank(&cat, reaver, "SHD", "Primary", 0)
            .expect("a 2H the SHD can wear ranks");
        assert_eq!(r.kind, "2H");
        assert!(r.of > 20, "SHD two-hander pool is {} wide", r.of);
        assert!(r.rank >= 1 && r.rank <= r.of);
        let one_h = rk.pool(&cat, "SHD", "Primary", "1H").len();
        let two_h_n = rk.pool(&cat, "SHD", "Primary", "2H").len();
        assert!(one_h > 50 && two_h_n > 20, "1H {one_h}, 2H {two_h_n}");
        /* nothing out of era is in any pool */
        for (i, _) in rk.pool(&cat, "WAR", "Neck", "") {
            assert!(!cat.recs[*i].oe);
        }
    }
}
