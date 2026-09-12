//! Factions, from factions.json. Pulled from eqlwiki.com's `Category:Factions` and licensed
//! CC BY-SA 4.0 like every other file in the snapshot.
//!
//! WHY THIS FILE HAD NO LOADER UNTIL NOW, AND WHAT CHANGED.
//! factions.json shipped in `data/` on 2026-09-03 and nothing opened it. The data module's own
//! header said so out loud, which was honest and still left 0.6MB on disk paying rent. It has a
//! reader now because the two questions it answers land on screens that already exist: a zone page
//! can say which factions you move by killing there, and a quest page can say which factions the
//! turn in moves. Neither needed a new rail row.
//!
//! SHAPE, MEASURED 2026-09-03 ON THE FILE THIS MODULE SHIPS WITH.
//!   factions.json { schema: 1, base, meta, factions: { "<name folded>": Faction, ... 255 } }
//!   Faction { n, t, d?, ml?, mr?, zl?, zr?, ql?, qr? }
//! Field census over all 255 records: `n` 255, `t` 255, `d` 249, `ml` 245, `zl` 242, `qr` 143,
//! `zr` 120, `mr` 115, `ql` 101. Every list is an array of strings; there is not one numeric leaf
//! anywhere in the file, on any record, in any field. That measurement decides a question below.
//!
//! WHAT `l` AND `r` MEAN, PROVED RATHER THAN ASSUMED.
//! The six list fields are `zl` `zr` `ql` `qr` `ml` `mr`, and the obvious reading is zones, quests
//! and mobs crossed with lower and raise. Obvious is not proved, so here is the proof. The wiki
//! has two mirrored lizardman factions: Allize Taeew (the Tae Ew of Cazic Thule) and Allize Volew
//! (the lizardmen of the Feerrott). They hate each other, so killing one side's mobs must move the
//! two factions in opposite directions, and the file says exactly that:
//!   allize taeew  ml = Tae Ew Archon, Tae Ew Diviner, ... (Cazic Thule)  <- its OWN mobs
//!                 mr = a lizardman forager, a lizardman scout, ... (The Feerrott)
//!   allize volew  ml = a lizardman forager, a lizardman scout, ... (The Feerrott)  <- its OWN
//!                 mr = All lizards in Cazic Thule, Tae Ew Archon, ...
//! `ml` is the faction's own roster on both records and `mr` is the enemy's on both. Killing your
//! own lowers you and killing theirs raises you, so `l` is lower and `r` is raise. That is the
//! evidence the field names in this module are built on, and it is why they are spelled out
//! (`mobs_lower`, not `ml`) where the item and spell records keep the file's short keys: those
//! keys are read by rules that spell them the file's way, and these are read by nothing but
//! this app.
//!
//! THE MOB AND ZONE STRINGS ARE KEPT VERBATIM AND NOTHING IS PARSED OUT OF THEM.
//! 9560 of the 9866 mob entries look like "a dark ritualist (Castle Mistmoore)" and it is tempting
//! to split the zone out of the parentheses. Measured, the other 306 are why not: the suffix
//! nests ("a dwarven mercenary (Cazic Thule (Zone))", "a bile golem (Howling Stones (Charasis))"),
//! carries a body type ("a helot skeleton (Howling Stones (Charasis) - Undead)"), or is simply
//! absent ("Ankhetperure", "Groi Gutblade"). A split would be right 97 percent of the time and
//! quietly wrong the rest, and this app has no mob screen to link the pieces to anyway: the Drops
//! row left the rail. So the strings are shown as the wiki wrote them.
//!
//! THE ZONE AND QUEST STRINGS ARE NOT PARSED EITHER, BUT THEY ARE LOOKED UP.
//! 120 of the 133 distinct zone strings and 446 of the 552 distinct quest titles resolve against
//! the snapshot's own zone and quest indexes, case folded, with no rewriting at all. The 13 and
//! 106 that do not are the wiki's own variants ("Highpass Keep", "Felwithe", "Rathe Mountains The
//! Feerrott") and its placeholders ("All", "None known", "Placeholder", "unknown"). A screen draws
//! those as plain text instead of a link, which is the difference between "we have no page for
//! this" and "this is not a zone", and neither is a reason to drop the line.
//!
//! WHY THERE IS NO STANDING SCREEN AT THE END OF THIS, and it is a measurement not an opinion:
//! see [`NO_STANDINGS`].
//!
//! No em dashes and no en dashes anywhere in this module, by house rule.

use super::{read_json, DataError, FACTIONS_FILE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const FACTIONS_SCHEMA: u64 = 1;

/// Why `ScreenId::Standing` is still unbuilt after this file gained a loader.
///
/// The Standing row was an obvious candidate: `grimoire_core::Regard` already models a seven rung
/// ladder, Ally down to Dubiously, and "faction standing" is what those words mean in EverQuest.
/// Two measurements say no, and both are in the tree rather than in a memo.
///
/// FIRST, THE FILE CARRIES NO NUMBER. A census over all 255 records and all nine fields finds zero
/// numeric leaves: `d` is prose and the six lists are arrays of strings. There is no standing
/// value, no cap, no starting value and no delta anywhere in factions.json. A Standing screen fed
/// from this file would have to invent every number on it, and an invented rung is worse than the
/// unbuilt row it replaced. `the_file_carries_no_number_anywhere` is the test.
///
/// SECOND, `Regard` IS NOT THIS LADDER. Its floors are 4.85, 4.55, 4.15, 3.70, 2.60, 1.60, 0.00
/// and `Regard::of` takes "a mean score out of 5"; `Standing::rated` folds in one rating out of
/// five and `Standing::UNPROVEN` is 3.0 on zero ratings. That is a Broken Stoic marketplace
/// reputation borrowing EverQuest's words for its rungs, and `order::may_commission` weighs it
/// against a crafter's `least_regard`. EverQuest faction standing is an integer per player per
/// faction, unbounded below, and no part of it is a mean of anybody's ratings. Pointing one at the
/// other would put wiki faction names on a scale they were never measured against.
///
/// So the factions are cross linked into the zone and quest pages, which is what the data actually
/// supports, and the Standing row keeps the words it already had.
pub const NO_STANDINGS: &str =
    "factions.json carries no standing value: 255 records, nine fields, \
     zero numeric leaves. Regard's ladder is a mean out of five ratings, not an EverQuest faction \
     number.";

/// One faction page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Faction {
    /// The map key: the faction name case folded ("agents of mistmoore"). Not in the record body;
    /// the loader fills it from the key, as [`super::Spell`] does. Measured equal to `fold(n)` on
    /// all 255 records, and kept anyway because the lookups accept either and a future key that
    /// disambiguates two same named pages must not silently become the display name.
    #[serde(skip)]
    pub key: String,
    /// JSON `n`: the display name ("Agents of Mistmoore"). On all 255.
    #[serde(rename = "n", default)]
    pub name: String,
    /// JSON `t`: the wiki page title with underscores ("Agents_of_Mistmoore"), the tail of its URL.
    #[serde(default)]
    pub t: String,
    /// JSON `d`: the wiki's prose description of who this faction is. On 249 of 255.
    #[serde(rename = "d", default)]
    pub desc: Option<String>,
    /// JSON `ml`: mobs whose death LOWERS this faction, "name (zone)" as the wiki writes it. On
    /// 245 records, 8241 entries. See the module note for why they are not split.
    #[serde(rename = "ml", default)]
    pub mobs_lower: Vec<String>,
    /// JSON `mr`: mobs whose death RAISES this faction. On 115 records, 1625 entries.
    #[serde(rename = "mr", default)]
    pub mobs_raise: Vec<String>,
    /// JSON `zl`: zones where this faction can be lowered. On 242 records.
    #[serde(rename = "zl", default)]
    pub zones_lower: Vec<String>,
    /// JSON `zr`: zones where it can be raised. On 120 records.
    #[serde(rename = "zr", default)]
    pub zones_raise: Vec<String>,
    /// JSON `ql`: quests that lower it, by wiki title. On 101 records.
    #[serde(rename = "ql", default)]
    pub quests_lower: Vec<String>,
    /// JSON `qr`: quests that raise it. On 143 records.
    #[serde(rename = "qr", default)]
    pub quests_raise: Vec<String>,
    /// Every field the file gains after this build was measured. Never dropped.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/* NO `matches`, NO `detail`, NO `hit` AND NO `page` ON THIS RECORD, AND THAT IS THE POINT.
 *
 * Every other record in this module has them, because every other record has a screen: a search
 * needs `matches`, a finder row needs `detail`, a heading needs `page`. A faction has none of
 * those. It is drawn as a row inside somebody else's pane, by a zone page and a quest page, and
 * a convenience method written for a screen that does not exist is the exact defect this lane was
 * opened to remove, in miniature. They go in with the screen, if a screen is ever argued for. */

/// Which way a faction moves. The screens print the word; nothing computes with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Way {
    Raise,
    Lower,
}

impl Way {
    pub fn label(self) -> &'static str {
        match self {
            Way::Raise => "raises",
            Way::Lower => "lowers",
        }
    }
}

/// A faction named by a zone page or a quest page, and which way that page moves it.
///
/// Borrowed from the snapshot rather than cloned: a zone with sixteen factions on it is drawn
/// every frame, and the strings it prints already live in the `Faction` records.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Move<'a> {
    pub faction: &'a Faction,
    pub way: Way,
}

#[derive(Debug, Default, Deserialize)]
struct FactionsFile {
    #[serde(default)]
    schema: Option<u64>,
    #[serde(default)]
    factions: BTreeMap<String, Faction>,
}

/// factions.json, keyed by folded faction name, sorted by key.
///
/// OPTIONAL, FOR THE SAME REASON spells.json IS. It is not in [`super::FILES`], so a root without
/// it still loads; treating its absence as a broken snapshot would refuse a root whose other
/// files are fine. An absent file is an empty list. A file that IS there and does not parse is
/// still an error, with its path, because a broken file and a missing one must not read the same.
pub(crate) fn load(root: &Path) -> Result<Vec<Faction>, DataError> {
    let path = root.join(FACTIONS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let file: FactionsFile = read_json(&path)?;
    super::note_schema(&path, file.schema, FACTIONS_SCHEMA);
    if file.factions.is_empty() {
        return Err(DataError::new(
            path,
            "parsed, but its factions map is empty",
        ));
    }
    Ok(file
        .factions
        .into_iter()
        .map(|(key, mut f)| {
            if f.name.is_empty() {
                f.name = key.clone();
            }
            f.key = key;
            f
        })
        .collect())
}

/* ------------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    /* ---- no data needed ---- */

    fn parse(json: &str) -> Faction {
        serde_json::from_str(json).expect("a Faction")
    }

    #[test]
    fn the_six_lists_land_on_their_spelled_out_fields_and_unknowns_flatten() {
        let f = parse(
            r#"{"n":"Agents of Mistmoore","t":"Agents_of_Mistmoore","d":"Drachnids.",
                "ml":["a dark offerer (Castle Mistmoore)"],"mr":["a paladin (Felwithe)"],
                "zl":["Mistmoore Castle","Dreadlands"],"zr":["Felwithe"],
                "ql":["Bandit Sisters"],"qr":["Orc Vest","Princess Lenya"],"newfield":7}"#,
        );
        assert_eq!(f.name, "Agents of Mistmoore");
        assert_eq!(f.desc.as_deref(), Some("Drachnids."));
        assert_eq!(f.mobs_lower, vec!["a dark offerer (Castle Mistmoore)"]);
        assert_eq!(f.mobs_raise, vec!["a paladin (Felwithe)"]);
        assert_eq!(f.zones_lower, vec!["Mistmoore Castle", "Dreadlands"]);
        assert_eq!(f.zones_raise, vec!["Felwithe"]);
        assert_eq!(f.quests_lower, vec!["Bandit Sisters"]);
        assert_eq!(f.quests_raise, vec!["Orc Vest", "Princess Lenya"]);
        assert_eq!(
            f.t, "Agents_of_Mistmoore",
            "the wiki page title, which the zone pane prints"
        );
        assert!(
            f.extra.contains_key("newfield"),
            "a field this build never measured is kept, not dropped"
        );
        for typed in ["ml", "mr", "zl", "zr", "ql", "qr", "d", "n", "t"] {
            assert!(
                !f.extra.contains_key(typed),
                "{typed} is typed and must not also be in extra"
            );
        }
    }

    #[test]
    fn a_thin_page_parses_and_carries_nothing_rather_than_failing() {
        /* Six of the 255 records have no description and three have no list at all. They are
         * records, not errors: the zone pane draws the ones it needs and says so when a field is
         * empty, which is why there is no computed summary line on this type to go wrong. */
        let prose = parse(r#"{"n":"X","d":"Some words."}"#);
        assert_eq!(prose.desc.as_deref(), Some("Some words."));
        assert!(prose.mobs_lower.is_empty() && prose.zones_lower.is_empty());
        let bare = parse(r#"{"n":"X"}"#);
        assert_eq!(bare.desc, None);
        assert!(bare.t.is_empty());
        assert!(bare.extra.is_empty());
    }

    #[test]
    fn way_labels_are_the_words_the_panes_print() {
        assert_eq!(Way::Raise.label(), "raises");
        assert_eq!(Way::Lower.label(), "lowers");
        assert_ne!(Way::Raise, Way::Lower);
    }

    /* ---- the real file. Fails loudly when absent; GRIMOIRE_NO_DATA=1 skips on purpose. ---- */

    #[test]
    fn the_real_file_parses_whole_and_keeps_its_measured_counts() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        assert_eq!(s.factions.len(), 255, "Category:Factions");
        assert!(s
            .factions
            .iter()
            .all(|f| !f.name.is_empty() && !f.key.is_empty()));
        assert!(s.factions.iter().all(|f| !f.t.is_empty()));

        /* the field census this module's header states, asserted rather than described */
        let with = |p: fn(&Faction) -> bool| s.factions.iter().filter(|f| p(f)).count();
        assert_eq!(with(|f| f.desc.is_some()), 249, "d");
        assert_eq!(with(|f| !f.mobs_lower.is_empty()), 245, "ml");
        assert_eq!(with(|f| !f.zones_lower.is_empty()), 242, "zl");
        assert_eq!(with(|f| !f.quests_raise.is_empty()), 143, "qr");
        assert_eq!(with(|f| !f.zones_raise.is_empty()), 120, "zr");
        assert_eq!(with(|f| !f.mobs_raise.is_empty()), 115, "mr");
        assert_eq!(with(|f| !f.quests_lower.is_empty()), 101, "ql");
        let entries: usize = s
            .factions
            .iter()
            .map(|f| {
                f.mobs_lower.len()
                    + f.mobs_raise.len()
                    + f.zones_lower.len()
                    + f.zones_raise.len()
                    + f.quests_lower.len()
                    + f.quests_raise.len()
            })
            .sum();
        assert_eq!(entries, 13720, "every list entry in the file");

        let am = s
            .faction("Agents of Mistmoore")
            .expect("Agents of Mistmoore");
        assert_eq!(am.key, "agents of mistmoore");
        assert_eq!(am.t, "Agents_of_Mistmoore");
        assert!(am.desc.as_deref().unwrap_or("").contains("Drachnids"));
        assert!(am.zones_lower.iter().any(|z| z == "Mistmoore Castle"));
    }

    /// THE MIRROR THAT PROVES `l` IS LOWER AND `r` IS RAISE. See the module note. If the builder
    /// ever swaps the two, every zone and quest pane in the app starts telling players to kill the
    /// wrong things, and nothing but this test would notice: both fields are arrays of strings and
    /// both parse fine either way round.
    #[test]
    fn the_two_lizardman_factions_mirror_each_other_which_is_what_names_l_and_r() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let taeew = s.faction("Allize Taeew").expect("Allize Taeew");
        let volew = s.faction("Allize Volew").expect("Allize Volew");
        let feerrott = |e: &String| e.contains("(The Feerrott)");
        let cazic = |e: &String| e.contains("(Cazic Thule)");
        /* Tae Ew live in Cazic Thule: their own mobs are the ones that LOWER their faction. */
        assert!(
            taeew.mobs_lower.iter().any(cazic) && !taeew.mobs_lower.iter().any(feerrott),
            "ml on Allize Taeew should be its own Cazic Thule roster: {:?}",
            taeew.mobs_lower.iter().take(4).collect::<Vec<_>>()
        );
        assert!(
            taeew.mobs_raise.iter().any(feerrott),
            "mr on Allize Taeew should be the Feerrott lizardmen: {:?}",
            taeew.mobs_raise.iter().take(4).collect::<Vec<_>>()
        );
        /* and the other way round on the enemy faction, which is the half that makes it a proof
         * rather than a coincidence: one record could be read either way, two mirrored ones cannot */
        assert!(
            volew.mobs_lower.iter().any(feerrott) && !volew.mobs_lower.iter().any(cazic),
            "ml on Allize Volew should be its own Feerrott roster: {:?}",
            volew.mobs_lower.iter().take(4).collect::<Vec<_>>()
        );
        assert!(
            volew.mobs_raise.iter().any(cazic),
            "mr on Allize Volew should be the Cazic Thule lizards: {:?}",
            volew.mobs_raise.iter().take(4).collect::<Vec<_>>()
        );
    }

    /// THE MEASUREMENT BEHIND [`NO_STANDINGS`], and the reason `ScreenId::Standing` is still
    /// unbuilt. If a later pull ever adds a number to this file, this test goes red and the
    /// Standing question is worth reopening. Until then it is settled, in the tree.
    #[test]
    fn the_file_carries_no_number_anywhere() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        fn numeric(v: &Value) -> bool {
            match v {
                Value::Number(_) => true,
                Value::Array(a) => a.iter().any(numeric),
                Value::Object(o) => o.values().any(numeric),
                _ => false,
            }
        }
        /* THE DETECTOR IS CHECKED BEFORE IT IS TRUSTED. This test's whole claim is an ABSENCE,
         * and an absence test passes just as green when the thing looking for it is broken. A
         * `numeric` that always answered false would certify the file as number free forever and
         * nothing downstream would notice. So it is shown a number it must find and a document it
         * must not, first, on values written here rather than read from the file. */
        assert!(numeric(&serde_json::json!({"a": [{"b": 1}]})), "the detector cannot see a nested number, so its silence on the real file means nothing");
        assert!(numeric(&serde_json::json!([0])));
        assert!(!numeric(&serde_json::json!({"a": ["x", null, true]})));

        /* every typed field is a String or a Vec<String> by construction, so the only place a
         * number could hide is `extra`, and that is exactly where an added field would land */
        let with_number: Vec<&str> = s
            .factions
            .iter()
            .filter(|f| f.extra.values().any(numeric))
            .map(|f| f.name.as_str())
            .collect();
        assert!(
            with_number.is_empty(),
            "factions.json grew a number, so {NO_STANDINGS} is out of date: {with_number:?}"
        );
        /* and the whole file, re-read raw, to catch a number in a field this build DOES type but
         * would coerce (serde would refuse, but the claim is about the file, not about serde) */
        let raw = std::fs::read_to_string(s.root.join(FACTIONS_FILE)).expect("the file");
        let v: Value = serde_json::from_str(&raw).expect("json");
        let factions = v.get("factions").expect("factions map");
        assert!(
            !numeric(factions),
            "a numeric leaf appeared under factions: {NO_STANDINGS} is out of date"
        );
    }

    #[test]
    fn a_root_without_the_file_loads_as_an_empty_list_and_not_an_error() {
        let dir = std::env::temp_dir().join("grimoire_factions_absent_probe");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let _ = std::fs::remove_file(dir.join(FACTIONS_FILE));
        assert_eq!(load(&dir).expect("absent is not an error"), Vec::new());
        std::fs::write(dir.join(FACTIONS_FILE), "{ not json").expect("write");
        let err = load(&dir).expect_err("a broken file IS an error");
        assert!(err.path.ends_with(FACTIONS_FILE));
        std::fs::write(dir.join(FACTIONS_FILE), r#"{"schema":1,"factions":{}}"#).expect("write");
        let err = load(&dir).expect_err("an empty map is an error too");
        assert!(err.reason.contains("factions map is empty"));
        let _ = std::fs::remove_file(dir.join(FACTIONS_FILE));
    }
}
