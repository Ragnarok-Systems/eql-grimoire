//! Items: gear-data.json (the wiki's item pages, schema 2) and item-tooltips.json (schema 1).
//!
//! SHAPE, MEASURED 2026-09-02 ON THE COMMITTED SNAPSHOT.
//!   gear-data.json     { schema: 2, base, meta, effects, names, zonePages, zone_oe, zones,
//!                        items: { "<name folded>": Item, ... 6891 } }
//!   item-tooltips.json { schema: 1, base, meta, items: { "<name folded>": Tooltip, ... 11456 } }
//! Only `items` is read from either. `names` (aliases), `effects` (spell text), `zonePages`,
//! `zones` and `zone_oe` (the wiki's zone list and its out of era zones) have no reader in this
//! crate yet; typing them without a consumer would be shape without a reader, and they stay in the
//! file until a screen asks.
//!
//! WHAT IS TYPED AND WHAT IS LEFT IN `extra`.
//! A field is typed here when it was measured on the real file and something reads it. `cls` and
//! `rc` (class and race sets) are measured on 6890 of 6891 records and read by the
//! `classes` rule below, and they are still left in `extra` on purpose: their shape is a four way
//! union ({all:1} | {all:1, x:[..]} | {c:[..]} | {none:1}) that the rule decodes from the Value, and
//! keeping them there is what lets a test prove flatten works on the first record of the real
//! file. The rarer fields (dly, dmg, skill, sv, charges, eff, foc, rcp, deity, haste, req, x,
//! stray, ...) are in `extra` too, kept whole for whichever screen builds the rule that reads them.

use super::{fold, read_json, DataError, Hit, HitKind, GEAR_FILE, TOOLTIPS_FILE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// The schema numbers this file's shapes were measured against.
pub const GEAR_SCHEMA: u64 = 2;
pub const TOOLTIPS_SCHEMA: u64 = 1;

/// The sixteen class codes, in the game's class order. The `classes` rule filters against this
/// list, so a stray code in the data never reaches a screen.
pub const CLASSES: [&str; 16] = [
    "WAR", "CLR", "PAL", "RNG", "SHD", "DRU", "MNK", "BRD", "ROG", "SHM", "NEC", "WIZ", "MAG",
    "ENC", "BST", "BER",
];

/// One wiki item page. Field names are the file's own short keys, deliberately: a reader of the
/// JSON and a reader of this code see the same `sl`, `st`, `fl`, and a rename here would be a
/// translation table every reader has to carry. The one exception is `n`, which the contract calls
/// `name`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// JSON `n`: the display name as the wiki spells it ("A Bone Necklace"). Present on every one
    /// of the 6891 measured records; the map key is the fallback if it is ever absent.
    #[serde(rename = "n", default)]
    pub name: String,
    /// The map key gear-data indexes by: the name case folded ("a bone necklace"). It is not in the
    /// record body, so serde skips it and `load` fills it in from the key.
    #[serde(skip)]
    pub key: String,
    /// JSON `t`: the wiki page title ("A_Bone_Necklace"). The page is the file's `base` plus this.
    #[serde(default)]
    pub t: String,
    /// JSON `era`: "Classic Era", "Kunark Era", "Velious Era". Absent on 1876 records.
    #[serde(default)]
    pub era: Option<String>,
    /// JSON `fl`: flags, lower case with underscores: "lore", "no_drop", "magic", ...
    #[serde(default)]
    pub fl: Vec<String>,
    /// JSON `sl`: worn slots as the wiki spells them ("Neck", "Primary", "Secondary").
    #[serde(default)]
    pub sl: Vec<String>,
    /// JSON `st`: stats, short name to number ("ac": 2, "sta": 10, "cha": 15). Every measured
    /// value is a number, so the map is typed as one.
    #[serde(default)]
    pub st: BTreeMap<String, f64>,
    /// JSON `size`: "TINY", "SMALL", "MEDIUM", "LARGE", "GIANT".
    #[serde(default)]
    pub size: Option<String>,
    /// JSON `wt`: weight in tenths as the wiki prints it (1 is WT: 1.0).
    #[serde(default)]
    pub wt: Option<f64>,
    /// JSON `oe`: out of era, 1 when the item is not obtainable in the current era. Absent when in
    /// era.
    #[serde(default)]
    pub oe: Option<f64>,
    /// JSON `src`: where the item comes from. Kept as a Value because `d` is positional.
    /// Observed shape (gear-data schema 2):
    ///   { "d": [ [zoneName, [ [mobName, levelText or null, rarityText or null], ... ] ], ... ],
    ///     "q": 1,   quest reward
    ///     "c": 1,   crafted
    ///     "s": 1,   sold by a vendor
    ///     "f": 1,   foraged
    ///     "v": 1 }  various
    /// Index meaning inside `d`: [0] zone name, [1] the mob rows; a mob row is [0] mob name,
    /// [1] level text ("34", "20-33", or null), [2] rarity text ("Common", "Rare", or null).
    /// Use [`Item::drop_rows`] rather than indexing this by hand.
    #[serde(default)]
    pub src: Option<Value>,
    /// Every field this build did not type. Never dropped.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One row of an item's drop table, decoded from the positional `src.d`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDrop<'a> {
    pub zone: &'a str,
    pub mob: &'a str,
    pub level: Option<&'a str>,
    pub rarity: Option<&'a str>,
}

/// Truthiness for the flag numbers the wiki scrape emits (`all: 1`, `none: 1`): absent, null,
/// false, zero and the empty string are false, and anything else is true.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// The strings in a JSON array, skipping anything that is not a string.
fn strings(v: Option<&Value>) -> Vec<&str> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

impl Item {
    /// The drop table, flattened: one row per (zone, mob). Order is page order.
    pub fn drop_rows(&self) -> Vec<ItemDrop<'_>> {
        let mut out = Vec::new();
        let d = match self.src.as_ref().and_then(|s| s.get("d")) {
            Some(Value::Array(d)) => d,
            _ => return out,
        };
        for entry in d {
            let zone = match entry.get(0).and_then(Value::as_str) {
                Some(z) => z,
                None => continue,
            };
            let mobs = match entry.get(1) {
                Some(Value::Array(m)) => m,
                _ => continue,
            };
            for row in mobs {
                let mob = match row.get(0).and_then(Value::as_str) {
                    Some(m) => m,
                    None => continue,
                };
                out.push(ItemDrop {
                    zone,
                    mob,
                    level: row.get(1).and_then(Value::as_str),
                    rarity: row.get(2).and_then(Value::as_str),
                });
            }
        }
        out
    }

    /// Zones the wiki lists this item dropping in, page order, each once.
    pub fn drop_zones(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for r in self.drop_rows() {
            if !out.contains(&r.zone) {
                out.push(r.zone);
            }
        }
        out
    }

    /// The non drop source words, one per `src` letter, in a fixed order.
    pub fn source_kinds(&self) -> Vec<&'static str> {
        const KINDS: [(&str, &str); 5] = [
            ("q", "quest"),
            ("c", "crafted"),
            ("s", "sold"),
            ("f", "foraged"),
            ("v", "various"),
        ];
        let src = match &self.src {
            Some(s) => s,
            None => return Vec::new(),
        };
        KINDS
            .iter()
            .filter(|(k, _)| truthy(src.get(k)))
            .map(|(_, word)| *word)
            .collect()
    }

    /// Which classes can wear it. The rule over the four `cls` shapes the wiki scrape writes:
    ///   no `cls` at all         -> None            any class (the wiki's ALL)
    ///   {none: 1}               -> Some([])        no class
    ///   {all: 1}                -> None            any class
    ///   {all: 1, x: [codes]}    -> Some(all but x) all except
    ///   {c: [codes]}            -> Some(c)         exactly these, unknown codes dropped
    /// None and Some(empty) are different answers and the callers treat them differently, which is
    /// why this is not a plain Vec.
    pub fn classes(&self) -> Option<Vec<&'static str>> {
        let c = match self.extra.get("cls") {
            Some(Value::Object(m)) => m,
            _ => return None,
        };
        if truthy(c.get("none")) {
            return Some(Vec::new());
        }
        if truthy(c.get("all")) {
            let x = strings(c.get("x"));
            if x.is_empty() {
                return None;
            }
            return Some(CLASSES.iter().copied().filter(|k| !x.contains(k)).collect());
        }
        Some(
            strings(c.get("c"))
                .into_iter()
                .filter_map(|k| CLASSES.iter().copied().find(|known| *known == k))
                .collect(),
        )
    }

    /// The wiki's out of era flag, as a bool.
    pub fn out_of_era(&self) -> bool {
        self.oe.is_some_and(|v| v != 0.0)
    }

    /// Substring match against the display name or the map key, both case folded. `needle` must
    /// already be folded (see `super::parse_query`).
    pub fn matches(&self, needle: &str) -> bool {
        fold(&self.key).contains(needle) || fold(&self.name).contains(needle)
    }

    /// One computed line: slots, era, where it drops, the other source words, the era flag. When
    /// the page carries none of those the line says so rather than going blank.
    pub fn detail(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.sl.is_empty() {
            parts.push(self.sl.join("/"));
        }
        if let Some(e) = &self.era {
            parts.push(e.clone());
        }
        let zones = self.drop_zones();
        if !zones.is_empty() {
            let shown: Vec<&str> = zones.iter().take(2).copied().collect();
            let more = zones.len().saturating_sub(shown.len());
            if more > 0 {
                parts.push(format!("drops in {} (+{more} more)", shown.join(", ")));
            } else {
                parts.push(format!("drops in {}", shown.join(", ")));
            }
        }
        for k in self.source_kinds() {
            parts.push(k.to_string());
        }
        if self.out_of_era() {
            parts.push("out of era".to_string());
        }
        if parts.is_empty() {
            parts.push("the wiki page lists no slot, era or source".to_string());
        }
        parts.join(" · ")
    }

    pub fn hit(&self) -> Hit {
        Hit {
            kind: HitKind::Item,
            name: self.name.clone(),
            detail: self.detail(),
        }
    }
}

/// One item-tooltips.json record: the wiki's stat block as lines, for hover text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tooltip {
    /// JSON `n`: display name.
    #[serde(rename = "n", default)]
    pub name: String,
    /// The map key (name folded). Filled by `load_tooltips`.
    #[serde(skip)]
    pub key: String,
    /// JSON `t`: wiki page title.
    #[serde(default)]
    pub t: String,
    /// JSON `sb`: the stat block, one wiki line per entry ("MAGIC ITEM LORE ITEM", "Slot: NECK",
    /// "AC: 2", "WT: 1.0 Size: SMALL", ...). Rendered in monospace, compared line by line.
    #[serde(default)]
    pub sb: Vec<String>,
    /// Every other key, kept as read. `ic` rides here: a number on 11434 of 11456 records, null
    /// on 22, most likely an index into the wiki's icon sheet; nothing in this build draws icons,
    /// so it is not typed. A typed field with no reader would be a claim about shape that no
    /// screen backs, and the reachability floor (`reach.rs`) refuses exactly that.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Deserialize)]
struct GearFile {
    #[serde(default)]
    schema: Option<u64>,
    #[serde(default)]
    items: BTreeMap<String, Item>,
}

#[derive(Deserialize)]
struct TooltipsFile {
    #[serde(default)]
    schema: Option<u64>,
    #[serde(default)]
    items: BTreeMap<String, Tooltip>,
}

/// gear-data.json to items, in key order (which is folded name order).
pub(crate) fn load(root: &Path) -> Result<Vec<Item>, DataError> {
    let path = root.join(GEAR_FILE);
    let file: GearFile = read_json(&path)?;
    super::note_schema(&path, file.schema, GEAR_SCHEMA);
    if file.items.is_empty() {
        return Err(DataError::new(path, "parsed, but its items map is empty"));
    }
    Ok(file
        .items
        .into_iter()
        .map(|(key, mut it)| {
            if it.name.is_empty() {
                it.name = key.clone();
            }
            it.key = key;
            it
        })
        .collect())
}

/// item-tooltips.json to tooltips, in key order.
pub(crate) fn load_tooltips(root: &Path) -> Result<Vec<Tooltip>, DataError> {
    let path = root.join(TOOLTIPS_FILE);
    let file: TooltipsFile = read_json(&path)?;
    super::note_schema(&path, file.schema, TOOLTIPS_SCHEMA);
    if file.items.is_empty() {
        return Err(DataError::new(path, "parsed, but its items map is empty"));
    }
    Ok(file
        .items
        .into_iter()
        .map(|(key, mut t)| {
            if t.name.is_empty() {
                t.name = key.clone();
            }
            t.key = key;
            t
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    fn item(json: &str) -> Item {
        serde_json::from_str(json).expect("test item parses")
    }

    /* ---- no data needed: the classes rule, every branch ---- */

    #[test]
    fn classes_none_when_cls_is_absent_or_all() {
        assert_eq!(item(r#"{"n":"T"}"#).classes(), None);
        assert_eq!(item(r#"{"n":"T","cls":{"all":1}}"#).classes(), None);
        /* all with an EMPTY exclusion list is still ALL: there is nothing to except */
        assert_eq!(item(r#"{"n":"T","cls":{"all":1,"x":[]}}"#).classes(), None);
    }

    #[test]
    fn classes_all_except_removes_the_listed_codes_in_canonical_order() {
        let c = item(r#"{"n":"T","cls":{"all":1,"x":["WAR","BER"]}}"#)
            .classes()
            .unwrap();
        assert_eq!(c.len(), 14);
        assert!(!c.contains(&"WAR"));
        assert!(!c.contains(&"BER"));
        assert_eq!(c[0], "CLR");
        assert_eq!(*c.last().unwrap(), "BST");
    }

    #[test]
    fn classes_explicit_list_keeps_only_known_codes() {
        let c = item(r#"{"n":"T","cls":{"c":["CLR","XXX","DRU"]}}"#)
            .classes()
            .unwrap();
        assert_eq!(c, vec!["CLR", "DRU"]);
    }

    #[test]
    fn classes_none_flag_is_an_empty_list_not_any_class() {
        assert_eq!(
            item(r#"{"n":"T","cls":{"none":1}}"#).classes(),
            Some(Vec::new())
        );
        /* a zero flag is false */
        assert_eq!(
            item(r#"{"n":"T","cls":{"none":0,"all":1}}"#).classes(),
            None
        );
    }

    #[test]
    fn drop_rows_decode_the_positional_src_d() {
        let it = item(
            r#"{"n":"T","src":{"d":[["Droga",[["a goblin penmaster",null,null]]],["Temple of Droga",[["a goblin penmaster","34","Common"],["x","1-2",null]]]],"q":1,"s":0}}"#,
        );
        let rows = it.drop_rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0],
            ItemDrop {
                zone: "Droga",
                mob: "a goblin penmaster",
                level: None,
                rarity: None
            }
        );
        assert_eq!(rows[1].level, Some("34"));
        assert_eq!(rows[1].rarity, Some("Common"));
        assert_eq!(it.drop_zones(), vec!["Droga", "Temple of Droga"]);
        assert_eq!(it.source_kinds(), vec!["quest"]);
    }

    #[test]
    fn unknown_fields_land_in_extra_and_typed_ones_do_not() {
        let it = item(r#"{"n":"T","sl":["Neck"],"st":{"ac":2},"cls":{"all":1},"mystery":[1,2]}"#);
        assert_eq!(it.sl, vec!["Neck"]);
        assert_eq!(it.st.get("ac"), Some(&2.0));
        assert!(it.extra.contains_key("cls"));
        assert!(it.extra.contains_key("mystery"));
        assert!(!it.extra.contains_key("sl"));
        assert!(!it.extra.contains_key("n"));
    }

    #[test]
    fn detail_never_goes_blank() {
        assert_eq!(
            item(r#"{"n":"T"}"#).detail(),
            "the wiki page lists no slot, era or source"
        );
        assert_eq!(item(r#"{"n":"T","oe":1}"#).detail(), "out of era");
    }

    /* ---- the real file ---- */

    #[test]
    fn a_bone_necklace_reads_as_measured() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let it = s
            .item("A Bone Necklace")
            .expect("A Bone Necklace is in gear-data");
        assert_eq!(it.key, "a bone necklace");
        assert_eq!(it.t, "A_Bone_Necklace");
        assert_eq!(it.era.as_deref(), Some("Kunark Era"));
        assert_eq!(it.sl, vec!["Neck"]);
        assert_eq!(it.fl, vec!["lore", "no_drop"]);
        assert_eq!(it.st.get("ac"), Some(&2.0));
        assert_eq!(it.wt, Some(1.0));
        assert!(it.out_of_era());
        assert_eq!(it.classes(), None, "cls is {{all:1}} on this record");
        assert_eq!(it.drop_zones(), vec!["Droga", "Temple of Droga"]);
        assert_eq!(it.source_kinds(), vec!["quest"]);
        assert!(it.extra.contains_key("rc"));
    }

    #[test]
    fn every_item_has_a_name_and_a_key_and_the_first_is_the_first_key() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        assert_eq!(s.items[0].key, "a bone necklace");
        for it in &s.items {
            assert!(!it.name.is_empty(), "nameless item at key {}", it.key);
            assert!(!it.key.is_empty(), "keyless item {}", it.name);
        }
    }

    #[test]
    fn the_class_rule_produces_every_shape_on_the_real_file() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let mut any = 0;
        let mut except = 0;
        let mut listed = 0;
        for it in &s.items {
            match it.classes() {
                None => any += 1,
                Some(c) if c.len() == CLASSES.len() => except += 1,
                Some(c) if c.is_empty() => {}
                Some(_) => {
                    if it.extra.get("cls").and_then(|c| c.get("x")).is_some() {
                        except += 1
                    } else {
                        listed += 1
                    }
                }
            }
        }
        assert!(any > 1000, "any class {any}");
        assert!(except > 100, "all except {except}");
        assert!(listed > 1000, "explicit list {listed}");
    }

    #[test]
    fn tooltips_carry_stat_block_lines() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let t = s
            .tooltip("10 Dose Adrenaline Tap")
            .expect("in item-tooltips");
        assert_eq!(t.key, "10 dose adrenaline tap");
        assert_eq!(t.t, "10_Dose_Adrenaline_Tap");
        assert!(t.sb.iter().any(|l| l.starts_with("WT:")), "{:?}", t.sb);

        /* 11253 until 2026-09-03. The wiki's Category:Items has 11170 ns0 members and 204 of them
         * had no tooltip here; 203 were appended from their pages' `statsblock` field, which is
         * the same wiki lines this array already held. The one left out is "Exaltations", a wiki
         * page in the category that carries no Itempage template because it is not an item. */
        assert_eq!(s.tooltips.len(), 11456);
        let added = s
            .tooltip("Airtight Metal Box")
            .expect("appended 2026-09-03");
        assert_eq!(added.name, "Airtight Metal Box");
        assert_eq!(added.t, "Airtight_Metal_Box");
        assert!(
            added.sb.iter().any(|l| l.starts_with("Class:")),
            "an appended stat block reads like every other one: {:?}",
            added.sb
        );
        /* and no appended line kept the wiki markup its page had around it */
        assert!(
            !s.tooltips
                .iter()
                .any(|t| t.sb.iter().any(|l| l.contains("[[") || l.contains("<br"))),
            "a link or a line break survived into a stat block"
        );
    }
}
