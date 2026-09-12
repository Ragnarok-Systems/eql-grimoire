//! Zones: kills-data.json (the mob rosters, 122 zones) merged with the atlas pages under
//! atlas-wiki/ (the wiki's zone pages, 122 files).
//!
//! SHAPE, MEASURED 2026-09-02 ON THE COMMITTED SNAPSHOT.
//!   kills-data.json  { base, meta, zones: { "<key>": { name, city: bool, mobs: [ZoneMob] } } }
//!   atlas-wiki/<key>.json
//!                    { zone: "<key>", wikiTitle, wikiUrl, updated, meta, mobs: [AtlasMob],
//!                      items: [AtlasItem] }
//!   <key> is the short zone key ("befallen", "airplane"); it is the kills-data map key, the atlas
//!   file stem AND the atlas page's `zone` field, and it is what the merge joins on. All 122 kills
//!   zones match an atlas page by key and zero need a name match.
//!
//! THE ROSTER GREW FROM 76 ZONES TO 122 ON 2026-09-03, and `city` DID NOT.
//! kills-data used to hold 76 zones and 3843 mobs while the 122 atlas pages beside it held 6833;
//! the 46 Kunark and Velious pages had simply never been run through the generator. They are in
//! the file now, so every atlas page has a roster. Their `city` flag is ABSENT, not false: that
//! flag came from the kill tracker's own zone table, which only ever covered the original 76, and
//! the wiki has no city category and no city field to read a replacement off. `false` would be a
//! claim that Cabilis is not a city, and nothing measured says that.
//!
//! WHICH IS WHY [`Zone::in_tracker`] IS A FLAG AND NOT `city.is_some()`.
//! It used to read the presence of `city` as the membership test, on the reasoning that `city` is
//! the tracker's field and the wiki page has no such thing. That was true of a file where every
//! roster carried a city flag and stopped being true the moment 46 rosters arrived without one:
//! the proxy would have called a zone with 69 mobs "not in the kill tracker". The merge sets
//! `tracked` from where the zone actually came from, which is the thing the question was always
//! about.
//!
//! TWO NAMES PER ZONE, AND WHICH ONE IS `name`.
//! The tracker calls it "The Plane of Sky" and the wiki page is titled "Plane of Sky". `name` is
//! the tracker's spelling when there is one, because that is the string the kill parser will meet
//! in the log, and `wiki_title` keeps the page title beside it. Search matches both, and the key.
//! One page (lakenerius) has null for wikiTitle, wikiUrl and updated; those are Options for it.

use super::{fold, read_json, DataError, Hit, HitKind, ATLAS_DIR, KILLS_FILE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One mob as the kill tracker knows it (kills-data.json). This is the list the parser's kill
/// grammar resolves names against.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ZoneMob {
    /// JSON `n`: the mob's name as it appears in the log ("a dread bone", "A Soul Harvester").
    #[serde(rename = "n", default)]
    pub name: String,
    /// JSON `lv`: a single representative level, fractional when the wiki gives a range
    /// ("7-20 / 11-13" becomes 12.8). Null on 57 of 3843 measured mobs.
    #[serde(default)]
    pub lv: Option<f64>,
    /// JSON `lvl`: the level text as the wiki prints it ("8-10", "~53", "53 (Max, can be charmed)").
    #[serde(default)]
    pub lvl: Option<String>,
    /// JSON `named`: a named or boss mob rather than a trash spawn.
    #[serde(default)]
    pub named: bool,
    /// JSON `t`: wiki page title ("A_Dread_Bone").
    #[serde(default)]
    pub t: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One mob on the wiki's zone page (atlas-wiki). Its drops are INDEXES into the page's item list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AtlasMob {
    /// JSON `n`.
    #[serde(rename = "n", default)]
    pub name: String,
    /// JSON `lvl`: level text. Null on 1 of 6833 measured mobs.
    #[serde(default)]
    pub lvl: Option<String>,
    /// JSON `named`.
    #[serde(default)]
    pub named: bool,
    /// JSON `u`: the mob's wiki URL, absolute.
    #[serde(default)]
    pub u: String,
    /// JSON `loc`: positional, kept as observed. Null on 1898 mobs; on the other 4935 it is
    /// [0] a number, [1] a number, [2] a number on 143 and null on 4792. The wiki prints /loc as
    /// "x, y" so [0] and [1] are the two /loc coordinates in the wiki's order and [2] is the
    /// optional third; which axis is which was not verified tonight, so nothing labels them.
    #[serde(default)]
    pub loc: Option<Vec<Value>>,
    /// JSON `drops`: indexes into the page's `items`, every one in range on the measured files.
    /// Resolve with [`AtlasMob::drop_list`].
    #[serde(default)]
    pub drops: Vec<usize>,
    /// JSON `dr`: rarity text parallel to `drops` ("Always", "Common", "Rare", "Ultra Rare"),
    /// null where the wiki gives none. 3512 nulls against 32209 strings.
    #[serde(default)]
    pub dr: Vec<Option<String>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One item on the wiki's zone page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AtlasItem {
    /// JSON `n`.
    #[serde(rename = "n", default)]
    pub name: String,
    /// JSON `u`: the item's wiki URL, absolute.
    #[serde(default)]
    pub u: String,
    /// JSON `g`: a number on 7389 of 12871 measured items, absent otherwise. Not decoded tonight;
    /// nothing reads it, so it is here only so it is not silently lost.
    #[serde(default)]
    pub g: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl AtlasMob {
    /// The drop table with the indexes resolved: (item, rarity text). An index past the end of
    /// `items` is skipped rather than panicking; the measured files have none.
    pub fn drop_list<'a>(
        &'a self,
        items: &'a [AtlasItem],
    ) -> Vec<(&'a AtlasItem, Option<&'a str>)> {
        self.drops
            .iter()
            .enumerate()
            .filter_map(|(i, &idx)| {
                let item = items.get(idx)?;
                let rarity = self.dr.get(i).and_then(|r| r.as_deref());
                Some((item, rarity))
            })
            .collect()
    }
}

/// One zone, from both sources. A zone that is only in kills-data has no wiki fields; a zone that
/// is only on the wiki has an empty `mobs`. Neither is an error: the two lists were scraped
/// separately and cover different things.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    /// The short key both sources share ("befallen"). The atlas file is `atlas-wiki/<key>.json`.
    #[serde(default)]
    pub key: String,
    /// The tracker's name when kills-data has the zone, else the wiki title, else the key.
    #[serde(default)]
    pub name: String,
    /// kills-data `city`: the kill tracker ignores kills in a city when the setting asks it to.
    /// None on the 46 zones kills-data gained in 2026-09 and on any zone only the wiki knows; see
    /// the module doc for why absent is the honest answer there rather than `false`.
    #[serde(default)]
    pub city: Option<bool>,
    /// True when kills-data holds this zone at all. Set by [`merge`] from which file the zone came
    /// out of, never inferred from another field. Read through [`Zone::in_tracker`].
    #[serde(default)]
    pub tracked: bool,
    /// kills-data `mobs`: the kill tracker's list. Empty when only the wiki knows the zone.
    #[serde(default)]
    pub mobs: Vec<ZoneMob>,
    /// atlas `wikiTitle`.
    #[serde(default)]
    pub wiki_title: Option<String>,
    /// atlas `wikiUrl`.
    #[serde(default)]
    pub wiki_url: Option<String>,
    /// atlas `updated`: an RFC 3339 timestamp string of the scrape ("2026-08-13T15:13:06Z").
    #[serde(default)]
    pub updated: Option<String>,
    /// The atlas file that was merged in ("befallen.json"), None when only kills-data knows the
    /// zone. This, not the presence of mobs or a url, is what "has a wiki page" means: one page
    /// (lakenerius) exists with null title, null url and no mobs, and it is still a page.
    #[serde(default)]
    pub atlas_file: Option<String>,
    /// atlas `mobs`.
    #[serde(default)]
    pub atlas_mobs: Vec<AtlasMob>,
    /// atlas `items`.
    #[serde(default)]
    pub atlas_items: Vec<AtlasItem>,
    /// Unknown fields from BOTH files. A key present in both keeps the kills-data value.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Zone {
    /// True when an atlas page for this zone was merged in, even an empty one.
    pub fn has_wiki_page(&self) -> bool {
        self.atlas_file.is_some()
    }

    /// True when kills-data lists the zone, even with no mobs and even with no `city` flag.
    pub fn in_tracker(&self) -> bool {
        self.tracked
    }

    /// Substring match on the tracker name, the key or the wiki title, case folded.
    pub fn matches(&self, needle: &str) -> bool {
        fold(&self.name).contains(needle)
            || fold(&self.key).contains(needle)
            || self
                .wiki_title
                .as_deref()
                .is_some_and(|w| fold(w).contains(needle))
    }

    /// One computed line: what each source holds for this zone.
    pub fn detail(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.in_tracker() {
            parts.push(format!("{} tracked mobs", self.mobs.len()));
        } else {
            parts.push("not in the kill tracker".to_string());
        }
        if self.has_wiki_page() {
            parts.push(format!("{} wiki mobs", self.atlas_mobs.len()));
            parts.push(format!("{} wiki items", self.atlas_items.len()));
        } else {
            parts.push("no wiki page in the snapshot".to_string());
        }
        if self.city == Some(true) {
            parts.push("city".to_string());
        }
        parts.join(" · ")
    }

    pub fn hit(&self) -> Hit {
        Hit {
            kind: HitKind::Zone,
            name: self.name.clone(),
            detail: self.detail(),
        }
    }
}

#[derive(Deserialize)]
struct KillsFile {
    #[serde(default)]
    zones: BTreeMap<String, KillZone>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct KillZone {
    #[serde(default)]
    name: String,
    #[serde(default)]
    city: Option<bool>,
    #[serde(default)]
    mobs: Vec<ZoneMob>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AtlasPage {
    /// The short key. Empty on no measured page; the file stem is the fallback.
    #[serde(default)]
    zone: String,
    #[serde(rename = "wikiTitle", default)]
    wiki_title: Option<String>,
    #[serde(rename = "wikiUrl", default)]
    wiki_url: Option<String>,
    #[serde(default)]
    updated: Option<String>,
    #[serde(default)]
    mobs: Vec<AtlasMob>,
    #[serde(default)]
    items: Vec<AtlasItem>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// THE MERGE RULE. Join on the short key; the tracker's name wins; a zone from either side alone
/// is still a zone. Output is key order, which is what the Zones screen lists by.
pub(crate) fn merge(
    kills: BTreeMap<String, KillZone>,
    pages: Vec<(String, AtlasPage)>,
) -> Vec<Zone> {
    let mut zones: BTreeMap<String, Zone> = kills
        .into_iter()
        .map(|(key, kz)| {
            let z = Zone {
                key: key.clone(),
                name: kz.name,
                city: kz.city,
                tracked: true,
                mobs: kz.mobs,
                extra: kz.extra,
                ..Default::default()
            };
            (key, z)
        })
        .collect();
    for (stem, page) in pages {
        let key = if page.zone.is_empty() {
            stem.clone()
        } else {
            page.zone
        };
        let z = zones.entry(key.clone()).or_insert_with(|| Zone {
            key: key.clone(),
            ..Default::default()
        });
        if z.name.is_empty() {
            z.name = page.wiki_title.clone().unwrap_or_else(|| key.clone());
        }
        z.wiki_title = page.wiki_title;
        z.wiki_url = page.wiki_url;
        z.updated = page.updated;
        z.atlas_file = Some(format!(
            "{}.json",
            if stem.is_empty() { &key } else { &stem }
        ));
        z.atlas_mobs = page.mobs;
        z.atlas_items = page.items;
        for (k, v) in page.extra {
            z.extra.entry(k).or_insert(v);
        }
    }
    for z in zones.values_mut() {
        if z.name.is_empty() {
            z.name = z.key.clone();
        }
    }
    zones.into_values().collect()
}

/// kills-data.json plus every atlas-wiki/*.json, merged. A missing atlas directory is an error:
/// decision D6 names it as part of the snapshot, and a zone list with no wiki pages would look
/// like a scrape that lost half its data rather than a directory that was not copied.
pub(crate) fn load(root: &Path) -> Result<Vec<Zone>, DataError> {
    let kills_path = root.join(KILLS_FILE);
    let kills: KillsFile = read_json(&kills_path)?;
    if kills.zones.is_empty() {
        return Err(DataError::new(
            kills_path,
            "parsed, but its zones map is empty",
        ));
    }

    let dir = root.join(ATLAS_DIR);
    let listing =
        std::fs::read_dir(&dir).map_err(|e| DataError::new(&dir, format!("cannot list: {e}")))?;
    let mut files: Vec<PathBuf> = listing
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("json"))
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(DataError::new(dir, "holds no .json zone pages"));
    }
    let mut pages = Vec::with_capacity(files.len());
    for path in files {
        let page: AtlasPage = read_json(&path)?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        pages.push((stem, page));
    }
    Ok(merge(kills.zones, pages))
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    /* ---- no data needed: the merge rule ---- */

    fn kill(name: &str, city: bool, mobs: usize) -> KillZone {
        KillZone {
            name: name.into(),
            city: Some(city),
            mobs: (0..mobs)
                .map(|i| ZoneMob {
                    name: format!("mob {i}"),
                    ..Default::default()
                })
                .collect(),
            extra: Map::new(),
        }
    }

    fn page(zone: &str, title: Option<&str>) -> AtlasPage {
        AtlasPage {
            zone: zone.into(),
            wiki_title: title.map(str::to_string),
            wiki_url: title.map(|t| format!("https://eqlwiki.com/index.php/{t}")),
            items: vec![AtlasItem {
                name: "Bandages".into(),
                ..Default::default()
            }],
            mobs: vec![AtlasMob {
                name: "a thing".into(),
                drops: vec![0, 9],
                dr: vec![Some("Always".into())],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn merge_joins_on_key_and_the_tracker_name_wins() {
        let mut kills = BTreeMap::new();
        kills.insert("airplane".to_string(), kill("The Plane of Sky", false, 3));
        kills.insert("onlykills".to_string(), kill("Only Kills", true, 1));
        let pages = vec![
            (
                "airplane".to_string(),
                page("airplane", Some("Plane of Sky")),
            ),
            ("onlywiki".to_string(), page("onlywiki", Some("Only Wiki"))),
            ("nulltitle".to_string(), page("", None)),
        ];
        let zones = merge(kills, pages);
        let keys: Vec<&str> = zones.iter().map(|z| z.key.as_str()).collect();
        assert_eq!(keys, vec!["airplane", "nulltitle", "onlykills", "onlywiki"]);

        let sky = &zones[0];
        assert_eq!(sky.name, "The Plane of Sky");
        assert_eq!(sky.wiki_title.as_deref(), Some("Plane of Sky"));
        assert_eq!(sky.mobs.len(), 3);
        assert_eq!(sky.atlas_mobs.len(), 1);
        assert_eq!(sky.city, Some(false));
        assert_eq!(sky.atlas_file.as_deref(), Some("airplane.json"));
        assert!(sky.in_tracker() && sky.has_wiki_page());
        assert_eq!(sky.detail(), "3 tracked mobs · 1 wiki mobs · 1 wiki items");

        let nt = &zones[1];
        assert_eq!(
            nt.name, "nulltitle",
            "no tracker name and no wiki title falls back to the key"
        );
        assert!(nt.wiki_title.is_none());
        assert_eq!(
            nt.atlas_file.as_deref(),
            Some("nulltitle.json"),
            "the stem names the file"
        );

        let ok = &zones[2];
        assert_eq!(ok.name, "Only Kills");
        assert!(!ok.has_wiki_page());
        assert!(ok.in_tracker());
        assert_eq!(
            ok.detail(),
            "1 tracked mobs · no wiki page in the snapshot · city"
        );

        let ow = &zones[3];
        assert_eq!(ow.name, "Only Wiki");
        assert!(ow.mobs.is_empty());
        assert_eq!(ow.city, None);
        assert!(!ow.in_tracker());
        assert_eq!(
            ow.detail(),
            "not in the kill tracker · 1 wiki mobs · 1 wiki items"
        );
    }

    #[test]
    fn drop_list_resolves_indexes_and_skips_out_of_range() {
        let p = page("x", Some("X"));
        let list = p.mobs[0].drop_list(&p.items);
        assert_eq!(list.len(), 1, "index 9 has no item and is skipped");
        assert_eq!(list[0].0.name, "Bandages");
        assert_eq!(list[0].1, Some("Always"));
    }

    #[test]
    fn matches_looks_at_name_key_and_wiki_title() {
        let z = Zone {
            key: "beholder".into(),
            name: "The Gorge of King Xorbb".into(),
            wiki_title: Some("Beholder's Maze".into()),
            ..Default::default()
        };
        assert!(z.matches("xorbb"));
        assert!(z.matches("behold"));
        assert!(z.matches("maze"));
        assert!(!z.matches("befallen"));
    }

    /* ---- the real files ---- */

    #[test]
    fn befallen_has_both_sources() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let z = s.zone("befallen").expect("befallen");
        assert_eq!(z.name, "Befallen");
        assert_eq!(z.wiki_title.as_deref(), Some("Befallen"));
        assert_eq!(z.city, Some(false));
        assert!(!z.mobs.is_empty());
        assert!(!z.atlas_mobs.is_empty());
        assert!(!z.atlas_items.is_empty());
        assert!(z.wiki_url.as_deref().unwrap_or("").contains("Befallen"));
        assert!(z.updated.is_some());
        let dread = z
            .atlas_mobs
            .iter()
            .find(|m| m.name == "a dread bone")
            .expect("a dread bone");
        let drops = dread.drop_list(&z.atlas_items);
        assert_eq!(drops.len(), dread.drops.len(), "every index resolves");
        assert_eq!(drops[0].0.name, "Bandages");
        assert_eq!(drops[0].1, Some("Always"));
        let tracked = z
            .mobs
            .iter()
            .find(|m| m.name == "a dread bone")
            .expect("tracked too");
        assert_eq!(tracked.lvl.as_deref(), Some("8-10"));
        assert_eq!(tracked.lv, Some(9.0));
        assert!(!tracked.named);
    }

    #[test]
    fn every_zone_is_on_both_sources_and_only_76_of_them_carry_a_city_flag() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        /* Both sources now cover the same 122 zones. This test used to assert the opposite,
         * that wiki-only zones EXIST, because kills-data held 76 rosters against 122 atlas
         * pages. Closing that gap is the change; the assertion had to move with it. */
        let tracked = s.zones.iter().filter(|z| z.in_tracker()).count();
        let wiki_only = s.zones.iter().filter(|z| !z.in_tracker()).count();
        let unpaged = s.zones.iter().filter(|z| !z.has_wiki_page()).count();
        assert_eq!(tracked, 122, "every atlas page has a roster now");
        assert_eq!(wiki_only, 0, "no zone is on the wiki and not in the roster");
        assert_eq!(
            unpaged, 0,
            "every zone has a wiki page on the measured snapshot"
        );
        /* AND THE FLAG THAT DID NOT GROW WITH IT. `city` came from the kill tracker's own zone
         * table, which covered the original 76 and nothing since, so it is absent on the 46 that
         * arrived in 2026-09 rather than false. That is why `in_tracker` above is a flag and not
         * `city.is_some()`: the proxy would answer "not in the kill tracker" for a zone holding
         * a full roster. If this number ever reaches 122 someone found a source for the flag. */
        let flagged = s.zones.iter().filter(|z| z.city.is_some()).count();
        assert_eq!(
            flagged, 76,
            "city is the tracker table's field, not the wiki's"
        );
        assert!(
            s.zones
                .iter()
                .any(|z| z.in_tracker() && z.city.is_none() && !z.mobs.is_empty()),
            "a zone with a roster and no city flag is the case the proxy got wrong"
        );
        /* One tracker zone has an empty mob list on the measured file, and it is the same zone
         * whose wiki page has null title and url. If this list grows the data changed, not the
         * merge. */
        let empty: Vec<&str> = s
            .zones
            .iter()
            .filter(|z| z.city.is_some() && z.mobs.is_empty())
            .map(|z| z.key.as_str())
            .collect();
        assert_eq!(empty, vec!["lakenerius"]);
        for z in &s.zones {
            assert!(!z.name.is_empty(), "nameless zone {}", z.key);
        }
    }

    #[test]
    fn the_two_spellings_of_the_plane_of_sky_both_resolve() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let z = s.zone("airplane").expect("airplane");
        assert_eq!(z.name, "The Plane of Sky");
        assert_eq!(z.wiki_title.as_deref(), Some("Plane of Sky"));
        assert!(std::ptr::eq(z, s.zone("The Plane of Sky").unwrap()));
        assert!(std::ptr::eq(z, s.zone("plane of sky").unwrap()));
    }

    #[test]
    fn the_one_page_with_null_wiki_fields_still_loads() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let z = s.zone("lakenerius").expect("lakenerius");
        assert_eq!(
            z.name, "Lake Nerius",
            "the tracker's name is the only one it has"
        );
        assert!(z.wiki_title.is_none());
        assert!(z.wiki_url.is_none());
        assert!(z.updated.is_none());
        assert_eq!(z.city, Some(false));
        assert!(z.mobs.is_empty(), "the tracker lists the zone with no mobs");
        assert!(z.in_tracker());
        assert!(z.has_wiki_page(), "the page file exists, it is just empty");
        assert_eq!(z.atlas_file.as_deref(), Some("lakenerius.json"));
        assert!(z.atlas_mobs.is_empty() && z.atlas_items.is_empty());
        assert_eq!(z.detail(), "0 tracked mobs · 0 wiki mobs · 0 wiki items");
    }
}
