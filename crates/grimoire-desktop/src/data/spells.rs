//! Spells, from spells.json. Pulled from eqlwiki.com's `Category:Spells` on 2026-09-03 and
//! licensed CC BY-SA 4.0 like every other file in the snapshot.
//!
//! WHY THIS FILE EXISTS AT ALL. It is the one thing the snapshot had NOTHING of. gear-data.json
//! carries an `effects` map, 548 entries, and 546 of them resolve to real spell pages, but that is
//! the item-proc slice: what a click or a proc does, with no classes, no level, and no idea where
//! a caster would buy it. FIND / Spells was in the rail with no screen behind it and a page saying
//! so. 2001 spell pages later it has one.
//!
//! SHAPE, MEASURED 2026-09-03 ON THE FILE THIS MODULE SHIPS WITH.
//!   spells.json { schema: 1, base, meta, spells: { "<title folded>": Spell, ... 2001 } }
//!   Spell { n, t, era?, d?, cls?, note?, s?, skill?, mana?, rng?, ct?, ft?, rt?, dur?, tgt?,
//!           type?, res?, icon?, obtain?, vend?, items?, focus?, npc?, dup? }
//!
//! WHY THE NUMBERS ARE STRINGS. `mana`, `ct`, `rt`, `ft` and `rng` are the wiki's own text and it
//! is not always a number: casting time reads "4.00", range reads "150", and mana on a bard song
//! reads "0", but duration reads "1 hour 40 minutes" and resist reads "Fire (0)". Parsing the ones
//! that happen to be numeric into f64 would put a type on half a column and force every reader to
//! handle both, for no question anyone asks of this screen. They are shown, not computed with.
//!
//! WHAT IS NOT HERE, AND IT WAS A CHOICE. The page also carries `msg_cast_on_you`,
//! `msg_cast_on_other` and `msg_wears_off`, the three lines the game prints when a spell lands.
//! They are on 1428, 1578 and 1034 pages and would add about 110KB. Nothing in this app reads a
//! combat log for spell text, so they are not in the file. The `Table`/`TableLevel`/`TableEra`
//! fields are MediaWiki plumbing for the wiki's own tables and are not data.
//!
//! 2001 RECORDS FROM 2051 CATEGORY MEMBERS. 40 pages in `Category:Spells` are not spells: they are
//! `Namedmobpage` records for mobs that cast one ("A Lava Defender" is a 65 drake in the Temple of
//! Veeshan), miscategorised on the wiki. They carry no `Spellpage` template and are skipped rather
//! than half parsed. The other 10 are five pairs of live pages that differ only in capitalisation
//! ("Skin like Rock" and "Skin Like Rock" are two pages with different numbers on them); the
//! longer page is kept and the dropped title is on the record as `dup`.
//!
//! No em dashes and no en dashes anywhere in this module, by house rule.

use super::{fold, read_json, DataError, Hit, HitKind, SPELLS_FILE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const SPELLS_SCHEMA: u64 = 1;
/// THE THREE THINGS THE GAME PRINTS WHEN A SPELL LANDS OR ENDS.
///
/// Ingested from eqlwiki 2026-09-05: 1,418 spells carry `you`, 1,569 carry `other`, 1,024 carry
/// `off`. They are the ONLY route from a log line back to a spell name, because the log announces
/// a buff in flavour text and never by name: `Your endurance to magic fades.` is Endure Magic, and
/// nothing about the words `Endure Magic` produces `endurance to magic`.
///
/// `other` CARRIES A PLACEHOLDER THE GAME SUBSTITUTES A NAME INTO, and that is the whole reason
/// this type earns its place. The wiki writes `Someone has been charmed.`; the client writes
/// `a dry bone skeleton has been charmed.`. So a landing line NAMES ITS TARGET, which is the only
/// evidence in the file for which mob is charmed and who a damage shield was put on. Measured over
/// the reference capture: 33 distinct lines resolve to a spell and a real name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Msg {
    /// What is printed when it lands on the reader.
    #[serde(default)]
    pub you: Option<String>,
    /// What is printed when it lands on somebody else, with `Someone` where the name goes.
    #[serde(default)]
    pub other: Option<String>,
    /// What is printed when it wears off the reader. There is no equivalent for anybody else,
    /// which is why uptime for another person can be STARTED and not ended.
    #[serde(default)]
    pub off: Option<String>,
}

/// One spell page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spell {
    /// The map key: the wiki page title, case folded ("true north"). Not in the file; the loader
    /// puts it here so a record knows its own key, exactly as [`super::Item`] does.
    #[serde(default)]
    pub key: String,
    /// JSON `n`: the display name, the template's own `spellname` ("True North").
    #[serde(rename = "n", default)]
    pub name: String,
    /// JSON `t`: the wiki page title with underscores ("True_North"), the tail of its URL.
    #[serde(default)]
    pub t: String,
    /// JSON `msg`: what the game prints when this spell lands or ends. See [`Msg`].
    #[serde(default)]
    pub msg: Msg,
    /// JSON `era`: "Classic Era", "Kunark Era", "Velious Era". Absent on 617 of 2001, where the
    /// page carries no era template.
    #[serde(default)]
    pub era: Option<String>,
    /// JSON `d`: the spell's description as the game prints it. On 1997 of 2001.
    #[serde(rename = "d", default)]
    pub desc: Option<String>,
    /// JSON `cls`: (class, level) pairs, the class name canonical. On 1460 of 2001; the level is
    /// None on the handful of pages that name a class without one.
    #[serde(rename = "cls", default)]
    pub classes: Vec<(String, Option<f64>)>,
    /// JSON `note`: the lines of the wiki's `classes` field that name no class ("This spell is
    /// cast by NPCs only.", "No eligible class"). On 501 of 2001. Kept because they are the only
    /// place the page says a spell is unobtainable, and dropping them would have looked like the
    /// spell simply had no classes listed.
    #[serde(default)]
    pub note: Vec<String>,
    /// JSON `s`: one line per spell slot, the effect it applies ("Decrease Hitpoints by 401").
    #[serde(rename = "s", default)]
    pub slots: Vec<String>,
    /// JSON `skill`: the casting skill ("Evocation", "Alteration"). On 1471.
    #[serde(default)]
    pub skill: Option<String>,
    /// JSON `mana`, as text. See the module doc for why it is not an f64.
    #[serde(default)]
    pub mana: Option<String>,
    /// JSON `rng`: range in the wiki's text. On 1478.
    #[serde(default)]
    pub rng: Option<String>,
    /// JSON `ct`: casting time ("2.00").
    #[serde(default)]
    pub ct: Option<String>,
    /// JSON `ft`: fizzle time.
    #[serde(default)]
    pub ft: Option<String>,
    /// JSON `rt`: recast time.
    #[serde(default)]
    pub rt: Option<String>,
    /// JSON `dur`: duration ("Instant", "27 minutes", "3 ticks").
    #[serde(default)]
    pub dur: Option<String>,
    /// JSON `tgt`: target type ("Single", "Self", "Targeted AE", "Group v2").
    #[serde(default)]
    pub tgt: Option<String>,
    /// JSON `type`: the wiki's spell type ("Beneficial", "Detrimental", "Heal", "Slow", ...).
    #[serde(rename = "type", default)]
    pub spell_type: Option<String>,
    /// JSON `res`: resist ("Unresistable", "Fire (0)", "Magic (-50)").
    #[serde(default)]
    pub res: Option<String>,
    /// JSON `icon`: the spell gem the wiki names, a number or a letter. Shown as read; this build
    /// ships no gem art, so it is a label and not an image handle.
    #[serde(default)]
    pub icon: Option<String>,
    /// JSON `obtain`: the prose half of `where_to_obtain`. A line ending in a colon heads the
    /// lines under it (the wiki nests zone then mobs); see [`Spell::obtain_groups`].
    #[serde(default)]
    pub obtain: Vec<String>,
    /// JSON `vend`: the table half of `where_to_obtain`, (zone, vendor, area, /loc) on 2385 of
    /// 2387 rows and three fields on the other two. Kept as a row of strings so a short row is
    /// data rather than a parse error; [`Spell::vendors`] reads it positionally.
    #[serde(default)]
    pub vend: Vec<Vec<String>>,
    /// JSON `items`: the items whose click or proc casts this spell, by name. On 758.
    #[serde(default)]
    pub items: Vec<String>,
    /// JSON `focus`: the focus effect line, on 70 pages.
    #[serde(default)]
    pub focus: Option<String>,
    /// JSON `npc`: 1 when the page is also in `Category:NPC Only Spells`, 349 of them.
    #[serde(default)]
    pub npc: Option<f64>,
    /// JSON `dup`: the title of the OTHER live wiki page that folds to this key, on the 10
    /// records where the wiki has two. See the module doc.
    #[serde(default)]
    pub dup: Option<String>,
    /// Every field the file gains after this build was measured.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A level as the wiki wrote it: the file types every number as f64, so 14.0 prints as 14. The
/// screens have their own `num_text` for the same reason, and the data lane must not reach into a
/// screen module to borrow it.
fn level_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

/// One vendor row of `where_to_obtain`, read off [`Spell::vend`] positionally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vendor<'a> {
    pub zone: &'a str,
    pub who: &'a str,
    pub area: &'a str,
    pub loc: &'a str,
}

/// One group of `obtain` lines: a heading and the lines filed under it. A line with no heading
/// above it lands in a group whose `head` is empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObtainGroup<'a> {
    pub head: &'a str,
    pub lines: Vec<&'a str>,
}

impl Spell {
    /// True when this page is in `Category:NPC Only Spells`: a spell no player can cast.
    pub fn npc_only(&self) -> bool {
        self.npc.is_some_and(|v| v != 0.0)
    }

    /// The lowest level any class gets this spell at, or None when no class lists a level. The
    /// list column sorts and reads by it, because "what level is this" is the first question.
    pub fn min_level(&self) -> Option<f64> {
        self.classes
            .iter()
            .filter_map(|(_, l)| *l)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// "Druid 14 · Ranger 21", the class line. Empty when the page lists no class.
    pub fn class_line(&self) -> String {
        self.classes
            .iter()
            .map(|(c, l)| match l {
                Some(l) => format!("{c} {}", level_text(*l)),
                None => c.clone(),
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// The vendor rows, positionally. A row shorter than four fields yields empty strings for
    /// what it does not have rather than being dropped.
    pub fn vendors(&self) -> Vec<Vendor<'_>> {
        self.vend
            .iter()
            .map(|r| {
                let at = |i: usize| r.get(i).map(String::as_str).unwrap_or_default();
                Vendor {
                    zone: at(0),
                    who: at(1),
                    area: at(2),
                    loc: at(3),
                }
            })
            .collect()
    }

    /// `obtain` regrouped under its headings. The builder marks a heading with a trailing colon,
    /// which is the one byte that survived the wiki's nesting; this puts the shape back.
    pub fn obtain_groups(&self) -> Vec<ObtainGroup<'_>> {
        let mut out: Vec<ObtainGroup<'_>> = Vec::new();
        for line in &self.obtain {
            if let Some(head) = line.strip_suffix(':') {
                out.push(ObtainGroup {
                    head,
                    lines: Vec::new(),
                });
            } else {
                match out.last_mut() {
                    Some(g) if !g.head.is_empty() => g.lines.push(line),
                    _ => out.push(ObtainGroup {
                        head: "",
                        lines: vec![line],
                    }),
                }
            }
        }
        out
    }

    /// Substring match on the display name, the key and the class names, case folded. Class names
    /// are in it so "druid" finds the druid list, which is the second question after the name.
    pub fn matches(&self, needle: &str) -> bool {
        fold(&self.name).contains(needle)
            || fold(&self.key).contains(needle)
            || self.classes.iter().any(|(c, _)| fold(c).contains(needle))
    }

    /// One computed line: who casts it at what level, what kind it is, what it costs. Says what
    /// the page lacks when it lacks it, rather than drawing a blank.
    pub fn detail(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.npc_only() {
            parts.push("NPC only".to_string());
        }
        let cl = self.class_line();
        if !cl.is_empty() {
            parts.push(cl);
        } else if !self.note.is_empty() {
            parts.push(self.note[0].clone());
        } else {
            parts.push("no class on the page".to_string());
        }
        if let Some(t) = &self.spell_type {
            parts.push(t.clone());
        }
        if let Some(m) = &self.mana {
            parts.push(format!("{m} mana"));
        }
        if let Some(d) = &self.dur {
            parts.push(d.clone());
        }
        parts.join(" · ")
    }

    pub fn hit(&self) -> Hit {
        Hit {
            kind: HitKind::Spell,
            name: self.name.clone(),
            detail: self.detail(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct SpellsFile {
    #[serde(default)]
    schema: Option<u64>,
    #[serde(default)]
    spells: BTreeMap<String, Spell>,
}

/// spells.json, keyed by folded page title, sorted by key.
///
/// THIS FILE IS OPTIONAL AND THAT IS DELIBERATE. It is not in [`super::FILES`], the five a
/// snapshot root must hold, so a root without it still loads: treating its absence as a broken
/// snapshot would refuse to load a root whose other six sources are fine. So
/// an absent file is an empty list and the screen says which path it looked at. A file that IS
/// there and does not parse is still an error, loudly, with its path: that is a broken file, not
/// a missing one, and the two must not read the same.
pub(crate) fn load(root: &Path) -> Result<Vec<Spell>, DataError> {
    let path = root.join(SPELLS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let file: SpellsFile = read_json(&path)?;
    super::note_schema(&path, file.schema, SPELLS_SCHEMA);
    if file.spells.is_empty() {
        return Err(DataError::new(path, "parsed, but its spells map is empty"));
    }
    Ok(file
        .spells
        .into_iter()
        .map(|(key, mut s)| {
            if s.name.is_empty() {
                s.name = key.clone();
            }
            s.key = key;
            s
        })
        .collect())
}

/* ------------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    /* ---- no data needed ---- */

    fn parse(json: &str) -> Spell {
        serde_json::from_str(json).expect("a Spell")
    }

    #[test]
    fn class_pairs_type_and_unknown_fields_flatten() {
        let s = parse(
            r#"{"n":"True North","t":"True_North","cls":[["Cleric",1],["Paladin",3],["Bard",null]],
                "mana":"5","type":"Beneficial","dur":"Instant","newfield":[1,2]}"#,
        );
        assert_eq!(s.name, "True North");
        assert_eq!(s.classes[0], ("Cleric".to_string(), Some(1.0)));
        assert_eq!(s.classes[2], ("Bard".to_string(), None));
        assert_eq!(s.min_level(), Some(1.0));
        assert_eq!(s.class_line(), "Cleric 1 · Paladin 3 · Bard");
        assert_eq!(
            s.detail(),
            "Cleric 1 · Paladin 3 · Bard · Beneficial · 5 mana · Instant"
        );
        assert!(
            s.extra.contains_key("newfield"),
            "a field this build never measured is kept, not dropped"
        );
        assert!(
            !s.extra.contains_key("cls"),
            "a typed field is not also in extra"
        );
    }

    #[test]
    fn a_page_with_no_class_says_which_kind_of_nothing_it_is() {
        let quiet = parse(r#"{"n":"X"}"#);
        assert_eq!(quiet.detail(), "no class on the page");
        assert_eq!(quiet.min_level(), None);
        assert!(quiet.class_line().is_empty());
        let noted = parse(r#"{"n":"X","note":["This spell is cast by NPCs only."],"npc":1}"#);
        assert_eq!(
            noted.detail(),
            "NPC only · This spell is cast by NPCs only.",
            "the wiki's own sentence beats an invented one"
        );
        assert!(noted.npc_only());
        assert!(!quiet.npc_only());
    }

    #[test]
    fn obtain_regroups_under_its_headings_and_a_headless_line_stands_alone() {
        let s = parse(
            r#"{"n":"X","obtain":["Velious Level 50+ Mob Drop","Plane of Growth:",
                "Ail the Elder","Rumbleroot","Western Wastes:","Harla Dar"]}"#,
        );
        let g = s.obtain_groups();
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].head, "");
        assert_eq!(g[0].lines, vec!["Velious Level 50+ Mob Drop"]);
        assert_eq!(g[1].head, "Plane of Growth");
        assert_eq!(g[1].lines, vec!["Ail the Elder", "Rumbleroot"]);
        assert_eq!(g[2].head, "Western Wastes");
        assert_eq!(g[2].lines, vec!["Harla Dar"]);
        assert!(parse(r#"{"n":"X"}"#).obtain_groups().is_empty());
    }

    #[test]
    fn a_short_vendor_row_is_read_and_not_dropped() {
        let s = parse(
            r#"{"n":"X","vend":[["Ak'Anon","Clockwork Merchant","Library","(1100,-982)"],
                ["Erudin","Danilla"]]}"#,
        );
        let v = s.vendors();
        assert_eq!(v.len(), 2, "two rows in, two rows out");
        assert_eq!(v[0].zone, "Ak'Anon");
        assert_eq!(v[0].who, "Clockwork Merchant");
        assert_eq!(v[0].area, "Library");
        assert_eq!(v[0].loc, "(1100,-982)");
        assert_eq!(v[1].who, "Danilla");
        assert_eq!(v[1].area, "", "what the row does not carry reads empty");
        assert_eq!(v[1].loc, "");
    }

    #[test]
    fn matches_looks_at_name_key_and_class() {
        let s = parse(r#"{"n":"Skin Like Rock","cls":[["Druid",14]]}"#);
        assert!(s.matches("skin like"));
        assert!(s.matches("druid"), "a class name finds the class's list");
        assert!(!s.matches("cleric"));
    }

    /* ---- the real file. Fails loudly when absent; GRIMOIRE_NO_DATA=1 skips on purpose. ---- */

    #[test]
    fn the_real_file_parses_whole_and_keeps_its_measured_counts() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        assert_eq!(s.spells.len(), 2001, "Category:Spells minus 40 non spells");
        /* every record has the two fields the screen cannot draw a row without */
        assert!(s
            .spells
            .iter()
            .all(|p| !p.name.is_empty() && !p.t.is_empty()));
        assert!(s.spells.iter().all(|p| !p.key.is_empty()));

        let tn = s.spell("True North").expect("True North");
        assert_eq!(tn.key, "true north");
        assert_eq!(tn.t, "True_North");
        assert_eq!(tn.era.as_deref(), Some("Classic Era"));
        assert_eq!(tn.skill.as_deref(), Some("Divination"));
        assert_eq!(tn.mana.as_deref(), Some("5"));
        assert_eq!(tn.min_level(), Some(1.0));
        assert_eq!(tn.slots, vec!["True North".to_string()]);
        assert!(tn.obtain.iter().any(|o| o.contains("Cleric Spell Vendors")));

        /* the two fields the goal calls the point of the pull: a class with its level, and a
         * place to get the spell. Measured, not asserted as a shape. */
        let with_class = s.spells.iter().filter(|p| !p.classes.is_empty()).count();
        let with_where = s
            .spells
            .iter()
            .filter(|p| !p.obtain.is_empty() || !p.vend.is_empty())
            .count();
        assert_eq!(with_class, 1460);
        assert_eq!(with_where, 1339);
        assert_eq!(s.spells.iter().filter(|p| p.npc_only()).count(), 349);
        assert_eq!(s.spells.iter().filter(|p| p.dup.is_some()).count(), 10);

        /* the class names are the canonical fifteen and nothing else. The first cut of the
         * builder let the wiki's prose into this list and produced 126 "classes", 111 of them
         * sentences; this is the test that would have caught it. */
        let mut names: Vec<&str> = s
            .spells
            .iter()
            .flat_map(|p| p.classes.iter().map(|(c, _)| c.as_str()))
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names,
            vec![
                "Bard",
                "Beastlord",
                "Cleric",
                "Druid",
                "Enchanter",
                "Magician",
                "Necromancer",
                "Paladin",
                "Ranger",
                "Rogue",
                "Shadow Knight",
                "Shaman",
                "Wizard"
            ]
        );
        /* and no record still carries wiki markup in a string the screen prints */
        assert!(
            !s.spells.iter().any(|p| {
                p.desc.as_deref().unwrap_or("").contains("{{")
                    || p.obtain.iter().any(|o| o.contains("{{"))
                    || p.slots.iter().any(|o| o.contains("[["))
            }),
            "a template or a link survived the parse"
        );
    }

    #[test]
    fn a_root_without_the_file_loads_as_an_empty_list_and_not_an_error() {
        let dir = std::env::temp_dir().join("grimoire_spells_absent_probe");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let _ = std::fs::remove_file(dir.join(SPELLS_FILE));
        assert_eq!(load(&dir).expect("absent is not an error"), Vec::new());
        std::fs::write(dir.join(SPELLS_FILE), "{ not json").expect("write");
        let err = load(&dir).expect_err("a broken file IS an error");
        assert!(err.path.ends_with(SPELLS_FILE));
        std::fs::write(dir.join(SPELLS_FILE), r#"{"schema":1,"spells":{}}"#).expect("write");
        let err = load(&dir).expect_err("an empty map is an error too");
        assert!(err.reason.contains("spells map is empty"));
        let _ = std::fs::remove_file(dir.join(SPELLS_FILE));
    }
}
