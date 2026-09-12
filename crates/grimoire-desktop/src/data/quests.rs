//! Quests, from quest-items.json `quests` (schema 6). This module also owns the file's top level,
//! because `drops` (drops.rs) comes from the same 4.2MB file and parsing it twice would double the
//! biggest cost after gear-data for nothing.
//!
//! SHAPE, MEASURED 2026-09-02 ON THE COMMITTED SNAPSHOT.
//!   quest-items.json { schema: 6, base, meta, alias, drops, geo, ids, items, npcs, src, zones,
//!                      quests: [Quest, ... 924] }
//!   Quest { n, t, era|null, giver|null, lvl|null, zone|null, items: [name], need: [[name, qty]],
//!           want: [[name, qty]], rewards: [name], classes: [code], relatedNpcs: [name],
//!           relatedZones: [name], parts: [{n, g?, c: [...], r?: [...]}], steps?: [{i, k, txt, ...}],
//!           from?, roles?, oe?, split?, lvlUse? }
//!   `need` and `want` are [string, number] on every measured pair (3947 and 3580), so they are
//!   typed as pairs. `parts[].c`, `parts[].r`, `steps[].in`, `steps[].out`, `steps[].pre` and
//!   `steps[].tool` are arrays whose element shapes were not measured tonight; they stay in `extra`
//!   on their record rather than being guessed at.
//!
//! The other top level maps (alias, geo, ids, items, npcs, src, zones) are partly measured and have
//! no reader in this crate yet. They are skipped at the file level; the file itself is not a record
//! and the flatten contract is about records.

use super::drops::DropRow;
use super::{fold, read_json, DataError, Hit, HitKind, QUEST_ITEMS_FILE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const QUEST_ITEMS_SCHEMA: u64 = 6;

/// One quest as the wiki lists it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Quest {
    /// JSON `n`: the quest's title ("10th Coldain Ring Quest").
    #[serde(rename = "n", default)]
    pub name: String,
    /// JSON `t`: wiki page title.
    #[serde(default)]
    pub t: String,
    /// JSON `era`: null on 204 of 924.
    #[serde(default)]
    pub era: Option<String>,
    /// JSON `giver`: the NPC who starts it; null on 26.
    #[serde(default)]
    pub giver: Option<String>,
    /// JSON `lvl`: the level the wiki files it under; null on 75.
    #[serde(default)]
    pub lvl: Option<f64>,
    /// JSON `zone`: where it starts; null on 28.
    #[serde(default)]
    pub zone: Option<String>,
    /// JSON `items`: every item the quest touches, by name.
    #[serde(default)]
    pub items: Vec<String>,
    /// JSON `need`: (item name, quantity) the quest consumes.
    #[serde(default)]
    pub need: Vec<(String, f64)>,
    /// JSON `want`: (item name, quantity) still to gather, as the wiki's checklist lists them.
    #[serde(default)]
    pub want: Vec<(String, f64)>,
    /// JSON `rewards`: item names.
    #[serde(default)]
    pub rewards: Vec<String>,
    /// JSON `classes`: class codes it is restricted to; empty means any.
    #[serde(default)]
    pub classes: Vec<String>,
    /// JSON `relatedNpcs`.
    #[serde(rename = "relatedNpcs", default)]
    pub related_npcs: Vec<String>,
    /// JSON `relatedZones`.
    #[serde(rename = "relatedZones", default)]
    pub related_zones: Vec<String>,
    /// JSON `parts`: the hand in sequence in prose.
    #[serde(default)]
    pub parts: Vec<QuestPart>,
    /// JSON `steps`: the structured walk, on 333 of 924 quests.
    #[serde(default)]
    pub steps: Vec<QuestStep>,
    /// `from`, `roles`, `oe`, `split`, `lvlUse`, and anything newer.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One prose part of a quest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestPart {
    /// JSON `n`: the sentence, cut at 80 characters by the scrape.
    #[serde(rename = "n", default)]
    pub text: String,
    /// JSON `g`: the NPC this part is handed to, when the scrape found one.
    #[serde(default)]
    pub g: Option<String>,
    /// `c` (consumed) and `r` (received): arrays not measured tonight.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One structured step of a quest walk.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestStep {
    /// JSON `i`: 1 based step number.
    #[serde(default)]
    pub i: Option<f64>,
    /// JSON `k`: the step kind ("give", "kill", ...). Only "give" and "kill" were seen in the
    /// sample read tonight; the full set was not enumerated, so this is a String and not an enum.
    #[serde(default)]
    pub k: String,
    /// JSON `txt`: the step in prose.
    #[serde(default)]
    pub txt: String,
    /// JSON `npc`: who to talk to.
    #[serde(default)]
    pub npc: Option<String>,
    /// JSON `mob`: what to kill, on kill steps.
    #[serde(default)]
    pub mob: Option<String>,
    /// JSON `z`: the zone this step happens in, when the scrape found one.
    #[serde(default)]
    pub z: Option<String>,
    /// `npct` (the npc's wiki page title), `in`, `out`, `pre`, `tool`, `loc`, `fac`, `gold`,
    /// `iid`: kept as read, not typed, because no screen reads them through this struct (the
    /// Quests screen parses the same file into its own step shape). A typed field without a
    /// reader is refused by the reachability floor in `reach.rs`.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Quest {
    /// The wiki's out of era flag (`oe: true` on 265 quests).
    pub fn out_of_era(&self) -> bool {
        matches!(self.extra.get("oe"), Some(Value::Bool(true)))
            || self
                .extra
                .get("oe")
                .and_then(Value::as_f64)
                .is_some_and(|v| v != 0.0)
    }

    /// Substring match on the folded title.
    pub fn matches(&self, needle: &str) -> bool {
        fold(&self.name).contains(needle)
    }

    /// One computed line: era, zone, giver, level, the era flag. Says so when the page has none.
    pub fn detail(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(e) = &self.era {
            parts.push(e.clone());
        }
        if let Some(z) = &self.zone {
            parts.push(z.clone());
        }
        if let Some(g) = &self.giver {
            parts.push(g.clone());
        }
        if let Some(l) = self.lvl {
            parts.push(format!("level {l}"));
        }
        if self.out_of_era() {
            parts.push("out of era".to_string());
        }
        if parts.is_empty() {
            parts.push("the wiki page lists no era, zone, giver or level".to_string());
        }
        parts.join(" · ")
    }

    pub fn hit(&self) -> Hit {
        Hit {
            kind: HitKind::Quest,
            name: self.name.clone(),
            detail: self.detail(),
        }
    }
}

/// The parts of quest-items.json this crate reads.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct QuestItemsFile {
    #[serde(default)]
    pub schema: Option<u64>,
    #[serde(default)]
    pub drops: BTreeMap<String, Vec<DropRow>>,
    #[serde(default)]
    pub quests: Vec<Quest>,
}

/// Parse quest-items.json once. Both `drops` and `quests` come out of it.
pub(crate) fn load_file(root: &Path) -> Result<QuestItemsFile, DataError> {
    let path = root.join(QUEST_ITEMS_FILE);
    let file: QuestItemsFile = read_json(&path)?;
    super::note_schema(&path, file.schema, QUEST_ITEMS_SCHEMA);
    if file.drops.is_empty() {
        return Err(DataError::new(path, "parsed, but its drops map is empty"));
    }
    if file.quests.is_empty() {
        return Err(DataError::new(path, "parsed, but its quests list is empty"));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    /* ---- no data needed ---- */

    #[test]
    fn pairs_type_and_unknowns_flatten() {
        let q: Quest = serde_json::from_str(
            r#"{"n":"Q","need":[["Dirk of the Dain",2]],"want":[],"oe":true,"roles":{"x":"base"},"steps":[{"i":1,"k":"give","txt":"t","in":[["a",1]]}]}"#,
        )
        .unwrap();
        assert_eq!(q.need, vec![("Dirk of the Dain".to_string(), 2.0)]);
        assert!(q.out_of_era());
        assert!(q.extra.contains_key("roles"));
        assert!(!q.extra.contains_key("need"));
        assert_eq!(q.steps[0].k, "give");
        assert!(
            q.steps[0].extra.contains_key("in"),
            "unmeasured arrays stay in extra"
        );
        assert_eq!(q.detail(), "out of era");
        let bare: Quest = serde_json::from_str(r#"{"n":"Q"}"#).unwrap();
        assert!(!bare.out_of_era());
        assert_eq!(
            bare.detail(),
            "the wiki page lists no era, zone, giver or level"
        );
    }

    #[test]
    fn a_pair_that_is_not_name_and_number_is_a_parse_error() {
        assert!(serde_json::from_str::<Quest>(r#"{"n":"Q","need":[["only one"]]}"#).is_err());
        assert!(serde_json::from_str::<Quest>(r#"{"n":"Q","need":[[1,"swapped"]]}"#).is_err());
    }

    /* ---- the real file ---- */

    #[test]
    fn the_tenth_coldain_ring_reads_as_measured() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let q = s
            .quests
            .iter()
            .find(|q| q.name == "10th Coldain Ring Quest")
            .expect("in quests");
        assert_eq!(q.t, "10th_Coldain_Ring_Quest");
        assert_eq!(q.era.as_deref(), Some("Velious Era"));
        assert!(q.giver.is_none());
        assert!(q.zone.is_none());
        assert!(q.lvl.is_none());
        assert!(q.out_of_era());
        assert!(q.need.contains(&("Dirk of the Dain".to_string(), 2.0)));
        assert!(q.items.iter().any(|i| i == "Ring of Dain Frostreaver IV"));
        assert!(q.parts.len() >= 5);
        assert_eq!(q.parts[0].g.as_deref(), Some("Dain Frostreaver IV"));
        assert_eq!(q.steps[0].k, "give");
        assert_eq!(q.steps[0].npc.as_deref(), Some("Dain Frostreaver IV"));
        assert_eq!(q.steps[1].k, "kill");
        assert_eq!(q.steps[1].mob.as_deref(), Some("Narandi the Wretched"));
        assert!(q.extra.contains_key("from"));
        assert!(q.extra.contains_key("roles"));
    }

    #[test]
    fn a_job_for_nanrum_carries_zone_giver_and_level() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let q = s
            .quests
            .iter()
            .find(|q| q.name == "A Job for Nanrum")
            .expect("in quests");
        assert_eq!(q.zone.as_deref(), Some("Grobb"));
        assert_eq!(q.giver.as_deref(), Some("Basher Nanrum"));
        assert_eq!(q.lvl, Some(1.0));
        assert!(q.era.is_none());
        assert_eq!(q.detail(), "Grobb · Basher Nanrum · level 1");
    }

    #[test]
    fn every_quest_has_a_title_and_most_have_a_zone() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let with_zone = s.quests.iter().filter(|q| q.zone.is_some()).count();
        assert!(
            with_zone * 10 > s.quests.len() * 9,
            "{with_zone} of {}",
            s.quests.len()
        );
        for q in &s.quests {
            assert!(!q.name.is_empty());
            assert!(!q.t.is_empty(), "no wiki title on {}", q.name);
        }
    }
}
