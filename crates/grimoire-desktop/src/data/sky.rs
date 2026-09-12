//! The Plane of Sky, from sky.json (schema 2): its islands, the mobs and keys on each, the class
//! test quests, the sky items, and the alternatives table.
//!
//! SHAPE, MEASURED 2026-09-02 ON THE COMMITTED SNAPSHOT.
//!   sky.json { schema: 2, meta: {license, page: "Plane of Sky", source},
//!              islands: { "1": "Fairy Island", "1.5": "Noble Island", ... 9 },     id -> name
//!              isles:   [ Isle, ... 9 ],                                            in island order
//!              items:   { "<name folded>": SkyItem, ... 3057 },
//!              names:   { "<name folded>": "<name folded, punctuation stripped>", ... 2996 },
//!              classes: { "BER": SkyClass, ... 16 },
//!              alts:    { "Ammo": [name, ...], ... 17 slots } }
//!   Island ids are strings because "1.5" is one (Noble Island sits between 1 and 2). They sort
//!   numerically here, so 1, 1.5, 2, ... 8, not the string order 1, 1.5, 2, ... that happens to
//!   agree on nine islands but would put "10" before "2".
//!
//! The positional arrays: SkyItem `src` is [[zoneName, [[mobName, levelText or null], ...]], ...],
//! two deep like gear-data's `src.d` but WITHOUT the rarity column (every mob row is length 2 on
//! the 5592 measured). It is kept as a Value and decoded by [`SkyItem::drop_rows`].
//!
//! ONE DISCREPANCY AGAINST THE LANE BRIEF, RESOLVED BY MEASURING. The brief described `islands` as
//! "map[9] (positional arrays)" and asked for it to be kept positional. On the real file `islands`
//! is a map of id string to name string ("1.5": "Noble Island") and `isles` is an array of objects
//! with named fields; there is no positional array at either level, so there is nothing to keep
//! positional and typing them is not inventing field names. The only positional array in sky.json
//! is `SkyItem::src`, and that one IS kept as a Value, per the brief.

use super::{fold, read_json, DataError, SKY_FILE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub const SKY_SCHEMA: u64 = 2;

/// One sky item. Same short keys as [`super::Item`], and `sb` (the stat block) besides.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkyItem {
    /// JSON `n`: display name.
    #[serde(rename = "n", default)]
    pub name: String,
    /// The map key (name folded). Filled by `load`.
    #[serde(skip)]
    pub key: String,
    /// JSON `t`: wiki page title.
    #[serde(default)]
    pub t: String,
    /// JSON `era`.
    #[serde(default)]
    pub era: Option<String>,
    /// JSON `fl`: flags ("lore", "magic", "no_drop").
    #[serde(default)]
    pub fl: Vec<String>,
    /// JSON `sl`: worn slots.
    #[serde(default)]
    pub sl: Vec<String>,
    /// JSON `sb`: the stat block, one wiki line per entry. On 2744 of 3057.
    #[serde(default)]
    pub sb: Vec<String>,
    /// JSON `st`: stats, short name to number.
    #[serde(default)]
    pub st: BTreeMap<String, f64>,
    /// JSON `wt`.
    #[serde(default)]
    pub wt: Option<f64>,
    /// JSON `src`: positional, see the module doc. On 1059 of 3057.
    #[serde(default)]
    pub src: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One decoded row of a sky item's drop table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkyDrop<'a> {
    pub zone: &'a str,
    pub mob: &'a str,
    pub level: Option<&'a str>,
}

impl SkyItem {
    /// The drop table, flattened from the positional `src`.
    pub fn drop_rows(&self) -> Vec<SkyDrop<'_>> {
        let mut out = Vec::new();
        let src = match &self.src {
            Some(Value::Array(a)) => a,
            _ => return out,
        };
        for entry in src {
            let zone = match entry.get(0).and_then(Value::as_str) {
                Some(z) => z,
                None => continue,
            };
            let mobs = match entry.get(1) {
                Some(Value::Array(m)) => m,
                _ => continue,
            };
            for row in mobs {
                if let Some(mob) = row.get(0).and_then(Value::as_str) {
                    out.push(SkyDrop {
                        zone,
                        mob,
                        level: row.get(1).and_then(Value::as_str),
                    });
                }
            }
        }
        out
    }

    pub fn matches(&self, needle: &str) -> bool {
        fold(&self.key).contains(needle) || fold(&self.name).contains(needle)
    }
}

/// One island, from the `islands` id to name map.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Island {
    /// "1" through "8", and "1.5" for Noble Island.
    pub id: String,
    pub name: String,
}

/// A mob on an island, from `isles[].mobs`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IsleMob {
    /// JSON `n`.
    #[serde(rename = "n", default)]
    pub name: String,
    /// JSON `role`: "boss", "other", and whatever else the scrape used; not enumerated tonight.
    #[serde(default)]
    pub role: String,
    /// JSON `drops`: item names, display spelling.
    #[serde(default)]
    pub drops: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One island's contents, from `isles`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Isle {
    /// JSON `isl`: the island id, matching [`Island::id`].
    #[serde(default)]
    pub isl: String,
    /// JSON `name`.
    #[serde(default)]
    pub name: String,
    /// JSON `key`: the key items that drop HERE (for the next island). Empty on 1, 1.5 and 8.
    #[serde(default)]
    pub key: Vec<String>,
    /// JSON `req`: the key item needed to reach this island. Empty on 1.
    #[serde(default)]
    pub req: Vec<String>,
    /// JSON `mobs`.
    #[serde(default)]
    pub mobs: Vec<IsleMob>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One item a class test needs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkyTestItem {
    /// JSON `n`.
    #[serde(rename = "n", default)]
    pub name: String,
    /// JSON `isl`: which island it drops on, when it drops in Sky.
    #[serde(default)]
    pub isl: Option<String>,
    /// JSON `mob`: who drops it.
    #[serde(default)]
    pub mob: Option<String>,
    /// JSON `nodrop`.
    #[serde(default)]
    pub nodrop: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One class test quest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkyTest {
    /// JSON `id`: the wiki's numeric quest id.
    #[serde(default)]
    pub id: Option<u64>,
    /// JSON `n`: "Berserker Test of Sharpness".
    #[serde(rename = "n", default)]
    pub name: String,
    /// JSON `reward`: the item the test grants.
    #[serde(default)]
    pub reward: String,
    /// JSON `say`: the word to say to the giver.
    #[serde(default)]
    pub say: String,
    /// JSON `rune`: the rune items the test wants.
    #[serde(default)]
    pub rune: Vec<String>,
    /// JSON `items`.
    #[serde(default)]
    pub items: Vec<SkyTestItem>,
    /// `client`, `wiki`, and anything newer.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One class's test quests.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkyClass {
    /// JSON `giver`: the NPC in Sky who runs the tests for this class.
    #[serde(default)]
    pub giver: String,
    #[serde(default)]
    pub tests: Vec<SkyTest>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The whole of sky.json, typed.
#[derive(Debug, Clone, Default)]
pub struct Sky {
    /// Key order (folded name order).
    pub items: Vec<SkyItem>,
    /// Numeric id order: 1, 1.5, 2, ... 8.
    pub islands: Vec<Island>,
    /// File order, which is island order on the measured file.
    pub isles: Vec<Isle>,
    /// Class code to its tests.
    pub classes: BTreeMap<String, SkyClass>,
    /// Slot name to the item names that count as alternatives in that slot.
    pub alts: BTreeMap<String, Vec<String>>,
    /// Folded name to its punctuation stripped form, the scrape's own alias table.
    pub names: BTreeMap<String, String>,
    /// Top level fields not typed here (`meta`, `schema`, and anything newer).
    pub extra: Map<String, Value>,
    item_index: HashMap<String, usize>,
}

impl Sky {
    /// An island by id ("1.5").
    pub fn island(&self, id: &str) -> Option<&Island> {
        self.islands.iter().find(|i| i.id == id)
    }

    /// A sky item by name, exact after case folding, display spelling or key.
    pub fn item(&self, name: &str) -> Option<&SkyItem> {
        self.item_index.get(&fold(name)).map(|&i| &self.items[i])
    }

    /// Where an item drops in the Plane of Sky, from the isle rosters: one line per island that
    /// lists it, "Noble Island (I1.5): a thunder spirit princess", island names through the
    /// `islands` map. Empty when no roster names it, which is most items: the isle rosters carry
    /// the boss drops, the item windows carry the rest.
    pub fn dropped_on(&self, name: &str) -> Vec<String> {
        let want = fold(name);
        let mut out = Vec::new();
        for isle in &self.isles {
            let mobs: Vec<&str> = isle
                .mobs
                .iter()
                .filter(|m| m.drops.iter().any(|d| fold(d) == want))
                .map(|m| m.name.as_str())
                .collect();
            if mobs.is_empty() {
                continue;
            }
            let island = self
                .island(&isle.isl)
                .map(|i| i.name.as_str())
                .unwrap_or(&isle.name);
            out.push(format!("{island} (I{}): {}", isle.isl, mobs.join(", ")));
        }
        out
    }

    /// The slots this item stands in for, from the `alts` table: "Ammo" when the item is listed
    /// as an alternative for the Ammo slot. Empty for most items.
    pub fn alt_slots(&self, name: &str) -> Vec<&str> {
        let want = fold(name);
        self.alts
            .iter()
            .filter(|(_, names)| names.iter().any(|n| fold(n) == want))
            .map(|(slot, _)| slot.as_str())
            .collect()
    }
}

/// Order island ids numerically, so "1.5" sits between "1" and "2" and "10" would sit after "9".
/// An id that is not a number sorts after every number, by string.
pub(crate) fn island_order(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

#[derive(Deserialize)]
struct SkyFile {
    #[serde(default)]
    schema: Option<u64>,
    #[serde(default)]
    islands: BTreeMap<String, String>,
    #[serde(default)]
    isles: Vec<Isle>,
    #[serde(default)]
    items: BTreeMap<String, SkyItem>,
    #[serde(default)]
    classes: BTreeMap<String, SkyClass>,
    #[serde(default)]
    alts: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    names: BTreeMap<String, String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

pub(crate) fn load(root: &Path) -> Result<Sky, DataError> {
    let path = root.join(SKY_FILE);
    let file: SkyFile = read_json(&path)?;
    super::note_schema(&path, file.schema, SKY_SCHEMA);
    if file.items.is_empty() {
        return Err(DataError::new(path, "parsed, but its items map is empty"));
    }
    if file.islands.is_empty() {
        return Err(DataError::new(path, "parsed, but its islands map is empty"));
    }
    let items: Vec<SkyItem> = file
        .items
        .into_iter()
        .map(|(key, mut it)| {
            if it.name.is_empty() {
                it.name = key.clone();
            }
            it.key = key;
            it
        })
        .collect();
    let mut item_index = HashMap::with_capacity(items.len() * 2);
    for (i, it) in items.iter().enumerate() {
        item_index.entry(fold(&it.key)).or_insert(i);
        item_index.entry(fold(&it.name)).or_insert(i);
    }
    let mut islands: Vec<Island> = file
        .islands
        .into_iter()
        .map(|(id, name)| Island { id, name })
        .collect();
    islands.sort_by(|a, b| island_order(&a.id, &b.id));
    let mut extra = file.extra;
    if let Some(s) = file.schema {
        extra.insert("schema".to_string(), Value::from(s));
    }
    Ok(Sky {
        items,
        islands,
        isles: file.isles,
        classes: file.classes,
        alts: file.alts,
        names: file.names,
        extra,
        item_index,
    })
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    /* ---- no data needed ---- */

    #[test]
    fn island_ids_sort_numerically_not_lexically() {
        let mut ids = vec!["10", "1.5", "2", "1", "x", "8"];
        ids.sort_by(|a, b| island_order(a, b));
        assert_eq!(ids, vec!["1", "1.5", "2", "8", "10", "x"]);
    }

    #[test]
    fn sky_src_decodes_two_columns_and_no_rarity() {
        let it: SkyItem = serde_json::from_str(
            r#"{"n":"A Crude Stein","src":[["The Feerrott",[["Bouncer Flerb","37"],["Bouncer Hurd",null]]]],"sb":["WT: 1.0"],"unknown":1}"#,
        )
        .unwrap();
        let rows = it.drop_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            SkyDrop {
                zone: "The Feerrott",
                mob: "Bouncer Flerb",
                level: Some("37")
            }
        );
        assert_eq!(rows[1].level, None);
        assert!(it.extra.contains_key("unknown"));
        assert!(!it.extra.contains_key("sb"));
    }

    /* ---- the real file ---- */

    #[test]
    fn nine_islands_in_numeric_order_with_noble_between_one_and_two() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let ids: Vec<&str> = s.sky.islands.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "1.5", "2", "3", "4", "5", "6", "7", "8"]);
        assert_eq!(
            s.sky.island("1.5").map(|i| i.name.as_str()),
            Some("Noble Island")
        );
        assert_eq!(
            s.sky.island("8").map(|i| i.name.as_str()),
            Some("Veeshan Island")
        );
        assert_eq!(s.sky.isles.len(), 9);
        for isle in &s.sky.isles {
            assert!(
                s.sky.island(&isle.isl).is_some(),
                "isle {} has no island",
                isle.isl
            );
            assert_eq!(s.sky.island(&isle.isl).unwrap().name, isle.name);
        }
    }

    #[test]
    fn island_two_needs_the_misplaced_key_and_drops_the_misfortune_key() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let isle = |id: &str| s.sky.isles.iter().find(|i| i.isl == id);
        let two = isle("2").expect("isle 2");
        assert_eq!(two.name, "Azarack Island");
        assert_eq!(two.req, vec!["Key of the Misplaced"]);
        assert_eq!(two.key, vec!["Key of Misfortune"]);
        let one = isle("1").expect("isle 1");
        assert!(one.req.is_empty(), "the first island needs no key");
        let boss = one
            .mobs
            .iter()
            .find(|m| m.role == "boss")
            .expect("a boss on island 1");
        assert_eq!(boss.name, "Thunder Spirit Princess");
        assert!(boss.drops.iter().any(|d| d == "Symbol of Marr"));
    }

    #[test]
    fn class_tests_carry_giver_reward_and_say_word() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        assert_eq!(s.sky.classes.len(), 16);
        let ber = s.sky.classes.get("BER").expect("BER");
        assert_eq!(ber.giver, "Stragen The Hewer");
        let t = &ber.tests[0];
        assert_eq!(t.name, "Berserker Test of Sharpness");
        assert_eq!(t.reward, "Skycleaver");
        assert_eq!(t.say, "sharpness");
        assert_eq!(t.id, Some(177756));
        assert_eq!(t.rune, vec!["Wind Rune Jaka"]);
        let blade = t
            .items
            .iter()
            .find(|i| i.name == "Djinni War Blade")
            .expect("item");
        assert_eq!(blade.isl.as_deref(), Some("7"));
        assert_eq!(blade.mob.as_deref(), Some("Sister of the Spire"));
        assert!(blade.nodrop);
    }

    #[test]
    fn items_and_alts_read_as_measured() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        assert_eq!(s.sky.items[0].key, "a crude stein");
        let stein = s.sky.item("A Crude Stein").expect("A Crude Stein");
        assert_eq!(stein.st.get("cha"), Some(&15.0));
        assert_eq!(stein.sl, vec!["Secondary"]);
        assert!(stein.sb.iter().any(|l| l.starts_with("Effect:")));
        let rows = stein.drop_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].zone, "The Feerrott");
        assert_eq!(s.sky.alts.len(), 17);
        assert!(s.sky.alts.get("Ammo").is_some_and(|v| !v.is_empty()));
        assert!(s.sky.names.len() > 2000);
        assert!(s.sky.extra.contains_key("meta"));
        for it in &s.sky.items {
            assert!(!it.name.is_empty() && !it.key.is_empty());
        }
    }
}
