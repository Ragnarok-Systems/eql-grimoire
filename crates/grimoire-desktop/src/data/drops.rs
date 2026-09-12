//! Drops: who drops what, from quest-items.json `drops` (schema 6).
//!
//! SHAPE, MEASURED 2026-09-02 ON THE COMMITTED SNAPSHOT.
//!   drops: { "<item name folded>": [ [mobName, mobSlug, zoneName, zoneSlug], ... ], ... 1637 }
//!   Every row is exactly four strings (3507 rows measured, no nulls). A row that is not four
//!   strings fails the load with the file's path, which is the policy for every shape drift: loud,
//!   never a silently shorter list. [`DropRow`] is still the four tuple for exactly that reason.
//!   Eight of the 3507 rows carry a mob and an EMPTY zone (the scrape found the dropper but not
//!   where it lives; "bronze bracers" from "goblin archeologist" is one). They are kept, because
//!   the mob is real data, and the screens say the zone is not on the wiki rather than drawing a
//!   blank.
//!
//! TWO OF THE FOUR COLUMNS ARE READ AND DISCARDED, WHICH IS SAID HERE AND NOT LEFT TO BE NOTICED.
//!   [`DropSource`] keeps [0] the mob's display name and [2] the zone's, and drops [1] and [3],
//!   the two wiki slugs. They were carried as fields until the Drops screen went (see `nav::NAV`):
//!   that screen printed them raw under its DROPPED BY heading, and it was the only thing that
//!   ever read them. Nothing builds a URL out of a slug and nothing can, because nothing measured
//!   says which site these resolve on: the atlas pages carry full `u` URLs, these carry none. So
//!   the choice was two String fields per row, 3507 of them, that no surface shows and no rule
//!   consumes, against parsing them and letting them go. `reach.rs` names the first of those a
//!   defect and it is right: an unread field behind a masking derive is uncalled code that rustc
//!   cannot see. The columns are still REQUIRED to be there and required to be strings.
//!
//! The file's `items`, `ids`, `npcs`, `src`, `geo`, `alias`, `zones` maps are not read tonight; no
//! screen in the contract asks for them and their shapes are only partly measured (see quests.rs).
//!
//! `Drop` shadows the prelude trait of the same name inside this module and in `crate::data`,
//! because the contract names the type. Nothing in these modules implements `std::ops::Drop`, so
//! the shadow costs nothing; a lane that needs the trait writes `std::ops::Drop`.

use std::collections::BTreeMap;

/// One positional row as the file holds it: (mob, mob slug, zone, zone slug).
pub type DropRow = (String, String, String, String);

/// One mob in one zone that drops the item. Two of the row's four columns, the wiki slugs, are
/// required of the file and not kept; see the module note.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DropSource {
    /// Row [0]: the mob's display name ("a false treasure chest").
    pub mob: String,
    /// Row [2]: the zone's display name ("The Plane of Mischief"). Empty on eight measured rows.
    pub zone: String,
}

/// Every known dropper of one item.
///
/// NO `extra` AND NO SERDE, WHICH MAKES THIS THE ONE RECORD IN THE SNAPSHOT SHAPED UNLIKE THE
/// OTHERS, so the difference is argued here. Item, Zone, Quest and Spell each carry an `extra` bag
/// of the fields this build did not type, and every one of those bags is really read: the detail
/// panes print them and `screens::exalt` reaches into `Item::extra` by key. A drop's value in the
/// file is a BARE ARRAY. There is no object to keep unknown fields from, so this bag was always
/// empty by construction, and its stated reason for existing was that every record should have the
/// same three part shape. Its only reader was the Drops screen's detail pane, which printed the
/// empty bag under a MORE heading, and that screen is gone (`nav::NAV`).
///
/// So what was left was an always empty map that nothing read, plus `Serialize` and `Deserialize`
/// with nothing to do: the file is decoded as `BTreeMap<String, Vec<DropRow>>` in `quests.rs` and
/// never as this type, and the one thing that serialised a `Drop` was that same screen's
/// `screens::items::view`. Measured, not assumed: the crate builds with all of it deleted.
///
/// `reach.rs` COULD NOT HAVE TOLD ANYONE. Its field floor is a text floor, so the `extra` that
/// Item, Zone, Quest and Spell all legitimately carry vouched for this one. That is the name
/// collision blind spot the file documents, hit for the second time; `Facts::kills` was the first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drop {
    /// The map key: the item name case folded ("a black crown"). The file has no display spelling
    /// here; `Snapshot::item` has it when the item is also in gear-data.
    pub name: String,
    /// In file order, which is wiki page order.
    pub sources: Vec<DropSource>,
}

/* THE `impl Drop` BLOCK STOOD HERE AND IS DELETED.
 *
 * It held `zones`, `matches`, `detail` and `hit`, and all four were reached by one thing: the
 * finder's `d:` hit, on its way to a Drops screen that no longer exists (the argument is in
 * `nav::NAV`, the routing half in `data::HitKind`). `hit` called `detail`, `detail` called
 * `zones`, `matches` gated the search, and nothing outside that chain called any of them. Kept,
 * they would have been four public methods on a record with no caller but their own tests, which
 * is the shape of uncalled code this tree keeps paying for.
 *
 * THE RECORD ITSELF IS UNTOUCHED. `name`, `sources` and `extra` are what `Snapshot::drop_sources`
 * hands out, and both screens that draw droppers read the `sources` rows straight: Items prints
 * mob and zone per row under the item, Zones indexes the same rows by zone. Grouping them by zone
 * was the Drops screen's own `by_zone` and it went with that file. */

/// The file's drops map to records, key order.
pub(crate) fn from_table(table: BTreeMap<String, Vec<DropRow>>) -> Vec<Drop> {
    table
        .into_iter()
        .map(|(name, rows)| Drop {
            name,
            /* The two slugs are bound and dropped, deliberately and visibly: the tuple pattern is
             * what keeps the four column shape a compile time fact, and `_` is what says the
             * column was seen and not wanted. See the module note for why they are not kept. */
            sources: rows
                .into_iter()
                .map(|(mob, _mob_slug, zone, _zone_slug)| DropSource { mob, zone })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    /* ---- no data needed ---- */

    fn table(json: &str) -> BTreeMap<String, Vec<DropRow>> {
        serde_json::from_str(json).expect("test table parses")
    }

    #[test]
    fn rows_decode_positionally_in_the_measured_order() {
        let t = table(
            r#"{"b black crown":[["Ratmlet","Ratmlet","The Plane of Mischief","Plane_of_Mischief"],["Ruttus","Ruttus_slug","The Plane of Mischief","Plane_of_Mischief"]],"a first":[]}"#,
        );
        let drops = from_table(t);
        assert_eq!(drops.len(), 2);
        assert_eq!(drops[0].name, "a first", "key order, not file order");
        assert!(drops[0].sources.is_empty());
        let crown = &drops[1];
        assert_eq!(crown.sources.len(), 2);
        /* THE COLUMNS ARE READ BY POSITION AND THE RIGHT TWO ARE KEPT. The fixture's slugs are
         * deliberately unlike their display names ("Ruttus_slug", not "Ruttus"), so a decode that
         * kept [1] and [3] instead of [0] and [2] fails here rather than passing on a file whose
         * columns happen to agree. */
        assert_eq!(
            crown.sources[1],
            DropSource {
                mob: "Ruttus".into(),
                zone: "The Plane of Mischief".into(),
            }
        );
        assert_eq!(
            crown
                .sources
                .iter()
                .filter(|s| s.zone == "The Plane of Mischief")
                .count(),
            2
        );
    }

    #[test]
    fn a_blank_zone_keeps_the_mob_and_says_the_zone_is_unknown() {
        let drops = from_table(table(
            r#"{"bronze bracers":[["goblin archeologist","Goblin_Archeologist","",""]]}"#,
        ));
        let d = &drops[0];
        assert_eq!(d.sources.len(), 1);
        assert_eq!(d.sources[0].mob, "goblin archeologist");
        /* THE BLANK ZONE IS KEPT, not dropped and not invented into a placeholder. The mob is
         * real data and this is the row the screens have to say something honest about. */
        assert_eq!(d.sources[0].zone, "");
    }

    #[test]
    fn a_row_that_is_not_four_strings_is_a_parse_error_not_a_short_list() {
        let short =
            serde_json::from_str::<BTreeMap<String, Vec<DropRow>>>(r#"{"x":[["a","b","c"]]}"#);
        assert!(short.is_err());
        let nulls =
            serde_json::from_str::<BTreeMap<String, Vec<DropRow>>>(r#"{"x":[["a",null,"c","d"]]}"#);
        assert!(nulls.is_err());
    }

    /* ---- the real file ---- */

    #[test]
    fn a_black_crown_drops_in_the_plane_of_mischief() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let d = s.drop_sources("a black crown").expect("a black crown");
        assert_eq!(d.sources.len(), 4);
        assert!(
            d.sources.iter().all(|x| x.zone == "The Plane of Mischief"),
            "{:?}",
            d.sources
        );
        assert!(d.sources.iter().any(|x| x.mob == "Ratmlet"));
    }

    #[test]
    fn every_row_has_a_mob_and_nearly_every_row_a_zone() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        assert_eq!(s.drops[0].name, "a black crown");
        let mut rows = 0;
        let mut blank_zone = 0;
        for d in &s.drops {
            assert!(!d.name.is_empty());
            for x in &d.sources {
                rows += 1;
                assert!(!x.mob.is_empty(), "a row with no mob under {}", d.name);
                if x.zone.is_empty() {
                    blank_zone += 1;
                }
            }
        }
        assert!(rows > 3000, "rows {rows}");
        /* eight on the measured file; under one percent is the claim that survives a refresh */
        assert!(
            blank_zone > 0,
            "the measured file has blank zones and the load must keep them"
        );
        assert!(
            blank_zone * 100 < rows,
            "blank zones {blank_zone} of {rows}"
        );
        let bracers = s.drop_sources("bronze bracers").expect("bronze bracers");
        assert!(bracers
            .sources
            .iter()
            .any(|x| x.mob == "goblin archeologist" && x.zone.is_empty()));
    }
}
