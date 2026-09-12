//! Merchants, from merchants.json. Pulled from eqlwiki.com and licensed CC BY-SA 4.0 like every
//! other file in the snapshot.
//!
//! THE QUESTION THIS FILE ANSWERS IS THE INVERSE OF THE ONE IT LOOKS LIKE IT ANSWERS.
//! A merchant record is a shop: who they are, where they stand, and a `sells` list. Read forwards
//! that is a shop window, and a shop window needs a screen of its own. Read backwards it is the
//! sibling of `Item::drop_rows`: gear-data already answers "who DROPS this", and 369 item records
//! carry `src.s`, the wiki's flag for "sold by a vendor", with no vendor on them. This file names
//! the vendor for 322 of those 369, and for 40 more items the flag never claimed. So the shape
//! that gets built is an index from item name to the merchants selling it, and it hangs off the
//! Items detail pane under the drop table, where the reader is already standing.
//!
//! SHAPE, MEASURED 2026-09-03 ON THE FILE THIS MODULE SHIPS WITH.
//!   merchants.json { schema: 1, base, meta, merchants: { "<key>": Merchant, ... 1373 } }
//!   Merchant { n, t, zone, loc, race, cls?, lvl?, sells?, fac?, ofac?, quests?, loot? }
//! Census over all 1373: `loc` 1373, `n` 1373, `race` 1373, `t` 1373, `zone` 1373, `cls` 1372,
//! `lvl` 1371, `sells` 1345 (23634 lines), `fac` 847 (2014 lines), `ofac` 245 (404 lines),
//! `quests` 179, `loot` 1.
//!
//! THE MAP KEY IS NOT THE NAME, AND HERE IT REALLY MATTERS.
//! 87 of the 1373 keys differ from the folded `n`, and 14 names have more than one record behind
//! them: "Clockwork Merchant" is thirty two separate merchants in Ak'Anon, disambiguated only by
//! the key ("clockwork merchant (lg-7)"), and Barsk the ogre in Oggok and Barsk the troll in Grobb
//! are two shops with different stock. A name keyed index would have silently kept one of each and
//! thrown away thirty one clockwork merchants, so there is no name keyed index here: the key is
//! the identity, and the lookups this crate needs (by zone, by item sold) never ask by name.
//!
//! `sells` EMBEDS THE PRICE AND `fac` EMBEDS THE STANDING DELTA. WHAT THIS MODULE DOES ABOUT IT.
//!   "Belt Pouch (1gp 5sp 7cp)"   "Brownie (-30)"
//! Splitting is not optional: an index from item name to seller cannot be keyed on
//! "Belt Pouch (1gp 5sp 7cp)", and a faction cannot be linked to its page while the standing delta
//! is still glued to its name. So the line IS split. What is NOT done is arithmetic.
//!
//!   THE PRICE STAYS TEXT. It is never summed into copper and never sorted on. Measured over the
//!   23634 sell lines: 18060 end in a bracket holding nothing but coin tokens, 103 end in the
//!   wiki's own "?" or "??" for a price it does not know, 5423 have no bracket at all, and 48 end
//!   in a bracket this module refuses. Those 48 are the wiki being the wiki: "x" (18), "0 Money",
//!   "Book", "Item", "Bottom", "1gp 4cpp", "2gp, 2sp, 6cp", "1p 3g 6 s", "~106pp",
//!   "1,007pp 1gp 9cp", "2021.12.21 [green]: not for sale", and two lines with a stray bracket of
//!   their own. A grammar wide enough to swallow those is a grammar that has decided "4cpp" is a
//!   denomination and "Book" is a price, which is inventing precision the wiki does not have. A
//!   number would also invite a UI that sorts and compares prices across merchants, and a wiki
//!   price is one player's screenshot at one charisma and one faction, not a quote. So: coin
//!   tokens, or "?" and "??", and anything else stays part of the name and the record simply has
//!   no price. Those 48 lines are still drawn, whole, on their merchant.
//!
//!   THE STANDING DELTA STAYS TEXT for the same reason and a sharper one. 1847 of the 2014 `fac`
//!   lines end in a signed integer, and 167 do not: 14 read "(-)", 4 read "(?)" or "(??)", and 149
//!   have no parenthesis at all, some of which are not faction names but sentences the wiki left
//!   in the field ("Your faction standing with HighHoldCitizens got worse.", "Killing her yields:",
//!   "Information Needed."). Those sentences are shown as they are written. A parse that turned
//!   them into a faction with an unknown delta would be putting a shape on prose.
//!
//!   NOTHING IS LOST EITHER WAY. `raw` is on every split, the panes print it verbatim when the
//!   split does not apply, and `round_trips_on_every_line_of_the_real_file` proves over all 23634
//!   sell lines and all 2418 faction lines that name plus price reassembles the source exactly.
//!
//! "(Faction)" IS PART OF A FACTION NAME AND THE GRAMMAR KNOWS IT. The wiki has pages titled
//! "King Ak'Anon (Faction)" and "Ankhefenmut (Faction)", and factions.json keys them with the
//! suffix. The delta grammar accepts only a sign, digits and the unknown markers, so "(Faction)"
//! is never peeled off and the name still matches its page. That is one place where a narrow
//! grammar buys a JOIN rather than costing one.
//!
//! THE INVERTED INDEX IS BUILT AT LOAD AND NOT LAZILY, and the measurement behind that choice is
//! on [`SellersIndex`].
//!
//! No em dashes and no en dashes anywhere in this module, by house rule.

use super::{fold, read_json, DataError, MERCHANTS_FILE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub const MERCHANTS_SCHEMA: u64 = 1;

/// One merchant page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Merchant {
    /// The map key, and the identity: "clockwork merchant (lg-7)", "barsk - ogre". Not in the
    /// record body; the loader fills it from the key. See the module note for why this is not the
    /// folded name.
    #[serde(skip)]
    pub key: String,
    /// JSON `n`: the display name as the wiki spells it ("a brownie merchant"). On all 1373. Not
    /// unique: 1373 records share 1324 folded names.
    #[serde(rename = "n", default)]
    pub name: String,
    /// JSON `t`: the wiki page title with underscores ("A_Brownie_Merchant").
    #[serde(default)]
    pub t: String,
    /// JSON `zone`: where the merchant stands, as the wiki writes it. On all 1373. 88 distinct
    /// strings; a few name two zones separated by a comma. See [`Merchant::zone_pieces`].
    #[serde(default)]
    pub zone: String,
    /// JSON `loc`: the /loc, verbatim. On all 1373 and NOT parsed: 1191 read "(x, y)" but the rest
    /// carry a second and third spawn point ("(1792, 3088), (1768, 3109), (1814, 3119)"), a
    /// landmark ("Basement (x, y)", "(x, y) Coldain Town"), an at sign, or the words "Need Info".
    /// A coordinate pair pulled out of that would be right most of the time and would quietly drop
    /// the second spawn point, and nothing in this app navigates to a coordinate.
    #[serde(default)]
    pub loc: String,
    /// JSON `race`: "Brownie", "Gnome", "Human". On all 1373.
    #[serde(default)]
    pub race: String,
    /// JSON `cls`: "Merchant" on 1359 of 1372, and on the others the wiki's own text, wiki markup
    /// and all ("Parcels Merchant", "Banker", "Shopkeepr", a name with an archive.org link glued
    /// to it). Shown as read.
    #[serde(default)]
    pub cls: Option<String>,
    /// JSON `lvl`: the level as TEXT, on 1371. 1363 are a plain integer and the rest are the wiki
    /// being the wiki ("?", "35?", "30 ~ 35", "41 (strangely cons dark blue to me at lvl 30, at
    /// night)"). Typed as f64 it would drop eight records or fail; see spells.rs for the same call.
    #[serde(default)]
    pub lvl: Option<String>,
    /// JSON `sells`: one line per item, price glued on. On 1345 records, 23634 lines. Read with
    /// [`Merchant::sales`].
    #[serde(default)]
    pub sells: Vec<String>,
    /// JSON `fac`: the faction hits for killing this merchant, delta glued on. On 847 records,
    /// 2014 lines. Read with [`Merchant::faction_hits`].
    #[serde(default)]
    pub fac: Vec<String>,
    /// JSON `ofac`: a second faction list on 245 records, 404 lines, same shape as `fac`. What the
    /// wiki means by the second list was not established from the data alone, so this module does
    /// not name it: the panes print it under its own heading with the file's own key, and the
    /// reader is told it is a second list rather than told a story about it.
    #[serde(default)]
    pub ofac: Vec<String>,
    /// JSON `quests`: quests this merchant is part of, by wiki title. On 179 records.
    #[serde(default)]
    pub quests: Vec<String>,
    /// JSON `loot`: what killing them drops, on exactly one record (petcas coldbeard). Typed
    /// because it is in the file, and a flattened one field bag would be drawn as an unexplained
    /// blob under MORE.
    #[serde(default)]
    pub loot: Vec<String>,
    /// Every field the file gains after this build was measured. Never dropped.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One line of `sells`, split. `raw` is the source line; `item` and `price` are borrowed slices of
/// it, so the split costs nothing and can always be put back together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sale<'a> {
    pub raw: &'a str,
    /// The item name with the price taken off, or the whole line when there was no price to take.
    pub item: &'a str,
    /// The price exactly as the wiki wrote it, without its brackets ("1gp 5sp 7cp", "?"). None
    /// when the line carries no price the grammar recognises; see the module note.
    pub price: Option<&'a str>,
}

/// One line of `fac` or `ofac`, split the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FactionHit<'a> {
    pub raw: &'a str,
    /// The faction name with the delta taken off, or the whole line. A line the wiki left as prose
    /// comes through here whole, which is what the pane prints.
    pub faction: &'a str,
    /// The delta exactly as written, without its brackets ("-30", "+10", "-", "??"). None when the
    /// line carries none.
    pub delta: Option<&'a str>,
}

/// Split a line on a trailing parenthesised group when `accept` says the group is the thing being
/// looked for. Returns the head, trimmed, and the group's contents.
///
/// The LAST `(` in the line is the one that opens the group, so a name with brackets earlier in it
/// keeps them, and a group holding a bracket of its own is not a group this can see (there is no
/// such line in the measured file).
fn split_trailing(line: &str, accept: fn(&str) -> bool) -> (&str, Option<&str>) {
    let t = line.trim();
    let Some(rest) = t.strip_suffix(')') else {
        return (t, None);
    };
    let Some(open) = rest.rfind('(') else {
        return (t, None);
    };
    let inside = &rest[open + 1..];
    if !accept(inside) {
        return (t, None);
    }
    (rest[..open].trim_end(), Some(inside))
}

/// Is this the contents of a price bracket?
///
/// Coin tokens ("25pp 1gp 9sp 9cp"), or the wiki's own mark for a price it does not know. Nothing
/// else; see the module note for the 149 lines this turns down and why that is the right answer.
pub fn is_price(s: &str) -> bool {
    let s = s.trim();
    if s == "?" || s == "??" {
        return true;
    }
    let mut any = false;
    for tok in s.split_whitespace() {
        any = true;
        let digits = tok.len() - tok.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 {
            return false;
        }
        let denom = &tok[digits..];
        if !matches!(
            denom.to_ascii_lowercase().as_str(),
            "pp" | "gp" | "sp" | "cp" | "p" | "g" | "s" | "c"
        ) {
            return false;
        }
    }
    any
}

/// Is this the contents of a standing delta bracket?
///
/// A sign, digits, or both ("-30", "+10", "1", "-", "+"), or the wiki's unknown marks. NOT
/// "Faction", which is part of several faction page titles; see the module note.
pub fn is_delta(s: &str) -> bool {
    let s = s.trim();
    if s == "?" || s == "??" {
        return true;
    }
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    if body.is_empty() {
        /* a bare sign, which the wiki writes when it knows the direction and not the number */
        return s.len() == 1;
    }
    !s.is_empty() && body.chars().all(|c| c.is_ascii_digit())
}

impl Merchant {
    /// The wiki page URL tail, or the key when the record has no title.
    pub fn page(&self) -> &str {
        if self.t.is_empty() {
            &self.key
        } else {
            &self.t
        }
    }

    /// One line of `sells`, split, without walking the rest of the list. The Items pane resolves a
    /// posting per drawn row and a merchant can carry over a hundred lines, so the per row cost is
    /// one split rather than a whole vector.
    pub fn sale_at(&self, line: usize) -> Option<Sale<'_>> {
        let raw = self.sells.get(line)?;
        let (item, price) = split_trailing(raw, is_price);
        Some(Sale {
            raw: raw.as_str(),
            item,
            price,
        })
    }

    /// `sells`, split into item and price. One entry per line, in the file's order.
    pub fn sales(&self) -> Vec<Sale<'_>> {
        self.sells
            .iter()
            .map(|raw| {
                let (item, price) = split_trailing(raw, is_price);
                Sale {
                    raw: raw.as_str(),
                    item,
                    price,
                }
            })
            .collect()
    }

    /// `fac`, split into faction and delta.
    pub fn faction_hits(&self) -> Vec<FactionHit<'_>> {
        split_hits(&self.fac)
    }

    /// `ofac`, the second list, split the same way.
    pub fn other_faction_hits(&self) -> Vec<FactionHit<'_>> {
        split_hits(&self.ofac)
    }

    /// The zone strings this merchant should be filed under: the whole `zone` field, plus each
    /// comma separated piece when there is more than one.
    ///
    /// WHY THE COMMA IS SPLIT AND NOTHING ELSE IS. Four of the 88 zone strings name two zones
    /// ("Highpass Hold, Qeynos Hills", "Grobb, Neriak Foreign Quarter", "South Qeynos, North
    /// Freeport") and filing those merchants under the joined string alone puts them on no zone
    /// page at all. Splitting on the comma is the only rewriting done here: no aliasing, no "West
    /// Karana means Western Plains of Karana", because that table would be invented. The merchant's
    /// own `zone` string is still what the pane prints.
    pub fn zone_pieces(&self) -> Vec<&str> {
        let whole = self.zone.trim();
        let mut out = vec![whole];
        if whole.contains(',') {
            for p in whole.split(',') {
                let p = p.trim();
                if !p.is_empty() && p != whole {
                    out.push(p);
                }
            }
        }
        out
    }
}

fn split_hits(lines: &[String]) -> Vec<FactionHit<'_>> {
    lines
        .iter()
        .map(|raw| {
            let (faction, delta) = split_trailing(raw, is_delta);
            FactionHit {
                raw: raw.as_str(),
                faction,
                delta,
            }
        })
        .collect()
}

/* ------------------------------------------------------------------ the inverse -- */

/// Where one merchant sells one thing: which record, and which line of its `sells`.
///
/// Indexes rather than borrows, because the index lives in the same struct as the vector and a
/// self referential borrow is not a thing. u32 is deliberate: 23634 postings times four bytes
/// saved is worth having, and 1373 merchants with 23634 lines are nowhere near the ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Posting {
    pub merchant: u32,
    pub line: u32,
}

/// Item name, case folded, to every merchant selling it.
///
/// WHY THIS IS BUILT IN `Snapshot::load` AND NOT ON FIRST USE. The goal asked for lazy if it is
/// expensive, so it was MEASURED rather than added up from struct headers, on a fresh release
/// build of the whole app, before and after, in an isolated checkout so no other lane's work was
/// in the number:
///
///   Working Set - Private, steady after a 32 second settle, two runs before and three after:
///     before  179.05 and 180.23 MB      after  185.93, 186.82 and 186.44 MB
///   Working Set          210.89 MB  ->  216.77 to 217.71 MB
///   Private Bytes        292.98 MB  ->  299.06 to 299.72 MB
///
/// So about 6.8MB for factions.json (0.60MB), merchants.json (1.21MB), the six indexes and this
/// one, which is 3.6 times the 1.81MB of JSON. The snapshot's other files sit nearer 5 times
/// theirs, and the difference is the shape: these two records are mostly short strings in flat
/// vectors, where the others are nested maps. This index's own share of that is 2720 owned key
/// strings and 23634 eight byte postings.
///
/// Cheap enough that lazy would buy back well under a megabyte.
///
/// Cheap is only half the argument. Lazy would cost something real: `Snapshot` is handed to the UI
/// thread as `&Snapshot` and `snapshot_crosses_threads` pins it as `Send + Sync`, so a lazily
/// filled index would have to be a `OnceLock` or a lock, on a path the Items detail pane touches
/// every frame it is open. Eager keeps the type plainly immutable, keeps the cost inside the load
/// time the SOURCES ledger already prints, and keeps the first click on an item as fast as the
/// hundredth.
#[derive(Debug, Default)]
pub struct SellersIndex {
    by_item: HashMap<String, Vec<Posting>>,
    /// How many `sells` lines went in. Reported so a screen can say what the index covers rather
    /// than implying it covers everything.
    lines: usize,
}

impl SellersIndex {
    pub fn build(merchants: &[Merchant]) -> SellersIndex {
        let mut by_item: HashMap<String, Vec<Posting>> = HashMap::new();
        let mut lines = 0usize;
        for (m, merch) in merchants.iter().enumerate() {
            for (l, raw) in merch.sells.iter().enumerate() {
                lines += 1;
                let (item, _) = split_trailing(raw, is_price);
                if item.is_empty() {
                    continue;
                }
                by_item.entry(fold(item)).or_default().push(Posting {
                    merchant: m as u32,
                    line: l as u32,
                });
            }
        }
        SellersIndex { by_item, lines }
    }

    /// Every merchant selling this item, by name, case folded. Empty when nobody does, which is a
    /// different fact from the item not existing.
    pub fn sellers_of(&self, item: &str) -> &[Posting] {
        self.by_item
            .get(&fold(item))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Distinct item names in the index.
    pub fn items(&self) -> usize {
        self.by_item.len()
    }

    /// `sells` lines read, including any that produced no key.
    pub fn lines(&self) -> usize {
        self.lines
    }
}

#[derive(Debug, Default, Deserialize)]
struct MerchantsFile {
    #[serde(default)]
    schema: Option<u64>,
    #[serde(default)]
    merchants: BTreeMap<String, Merchant>,
}

/// merchants.json, keyed by the file's own key, sorted by key.
///
/// OPTIONAL, exactly as spells.json and factions.json are, and for the same reason: a root without
/// it still loads every screen that does not need it. Absent is an empty list; present and
/// broken is an error naming the file.
pub(crate) fn load(root: &Path) -> Result<Vec<Merchant>, DataError> {
    let path = root.join(MERCHANTS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let file: MerchantsFile = read_json(&path)?;
    super::note_schema(&path, file.schema, MERCHANTS_SCHEMA);
    if file.merchants.is_empty() {
        return Err(DataError::new(
            path,
            "parsed, but its merchants map is empty",
        ));
    }
    Ok(file
        .merchants
        .into_iter()
        .map(|(key, mut m)| {
            if m.name.is_empty() {
                m.name = key.clone();
            }
            m.key = key;
            m
        })
        .collect())
}

/* ------------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    /* ---- no data needed ---- */

    fn parse(json: &str) -> Merchant {
        serde_json::from_str(json).expect("a Merchant")
    }

    #[test]
    fn the_price_grammar_takes_coins_and_the_wikis_question_marks_and_nothing_else() {
        for good in [
            "5sp 2cp",
            "1gp 5sp 7cp",
            "25pp 1gp 9sp 9cp",
            "7cp",
            "10p 4g 9s 2c",
            "3PP 1GP",
            "?",
            "??",
        ] {
            assert!(is_price(good), "{good:?} is a price");
        }
        /* every one of these is a real trailing bracket from the measured file */
        for bad in [
            "",
            "x",
            "0 Money",
            "Book",
            "Item",
            "Bottom",
            "1gp 4cpp",
            "2gp, 2sp, 6cp",
            "1p 3g 6 s",
            "~106pp",
            "1,007pp 1gp 9cp",
            "2021.12.21 [green]: not for sale",
            "4p 2g 2s 2x",
            "2pp 1 gp 1 sp",
            "Faction",
            "-30",
        ] {
            assert!(
                !is_price(bad),
                "{bad:?} is not a price this build claims to read"
            );
        }
    }

    #[test]
    fn the_delta_grammar_takes_signs_and_digits_and_never_the_word_faction() {
        for good in [
            "-30", "-20", "-50", "-1", "+10", "1", "10", "-", "+", "?", "??",
        ] {
            assert!(is_delta(good), "{good:?} is a delta");
        }
        for bad in [
            "",
            "Faction",
            "Primary Faction",
            "-30 or so",
            "1sp",
            "--",
            "Killing her yields:",
        ] {
            assert!(!is_delta(bad), "{bad:?} is not a delta");
        }
    }

    #[test]
    fn a_sell_line_splits_into_item_and_price_and_an_unreadable_bracket_stays_on_the_name() {
        let m = parse(
            r#"{"n":"a brownie merchant","sells":[
                "Bandages (5sp 2cp)",
                "Small Chainmail Sleeve Pattern",
                "Loaf of Bread (1sp 9cp) (2021.12.21 [green]: not for sale)",
                "Gems (?)",
                "Cloak of the Ordo (Kunark) (12pp)"]}"#,
        );
        let s = m.sales();
        assert_eq!(s[0].item, "Bandages");
        assert_eq!(s[0].price, Some("5sp 2cp"));
        assert_eq!(s[1].item, "Small Chainmail Sleeve Pattern");
        assert_eq!(
            s[1].price, None,
            "no bracket at all is not an unknown price"
        );
        assert_eq!(
            s[2].item, "Loaf of Bread (1sp 9cp) (2021.12.21 [green]: not for sale)",
            "the last bracket is not a price, so nothing is peeled and the line stands whole"
        );
        assert_eq!(s[2].price, None);
        assert_eq!(s[3].item, "Gems");
        assert_eq!(
            s[3].price,
            Some("?"),
            "the wiki's own mark for a price it does not know"
        );
        assert_eq!(
            s[4].item, "Cloak of the Ordo (Kunark)",
            "a bracket earlier in the name survives the split"
        );
        assert_eq!(s[4].price, Some("12pp"));
    }

    #[test]
    fn a_faction_line_splits_and_prose_comes_through_whole() {
        let m = parse(
            r#"{"n":"x","fac":["Brownie (-30)","King Ak'Anon (Faction) (-30)","Knights of Truth",
                "Merchants of Highpass (-)","Your faction standing with HighHoldCitizens got worse."],
                "ofac":["Kromrif (10)","Guardians of the Vale"]}"#,
        );
        let h = m.faction_hits();
        assert_eq!(h[0].faction, "Brownie");
        assert_eq!(h[0].delta, Some("-30"));
        assert_eq!(
            h[1].faction, "King Ak'Anon (Faction)",
            "\"(Faction)\" is part of the page title and must survive, or the link breaks"
        );
        assert_eq!(h[1].delta, Some("-30"));
        assert_eq!(h[2].faction, "Knights of Truth");
        assert_eq!(h[2].delta, None);
        assert_eq!(
            h[3].delta,
            Some("-"),
            "a direction with no number is still what the wiki said"
        );
        assert_eq!(
            h[4].faction, "Your faction standing with HighHoldCitizens got worse.",
            "a sentence the wiki left in the field is printed, not reshaped"
        );
        let o = m.other_faction_hits();
        assert_eq!((o[0].faction, o[0].delta), ("Kromrif", Some("10")));
        assert_eq!((o[1].faction, o[1].delta), ("Guardians of the Vale", None));
        /* THE CASE THIS TEST WAS MISSING, AND A MUTATION RUN IS WHY IT IS HERE. Every line above
         * either has no trailing bracket at all or has one the grammar accepts, so `is_delta` was
         * never asked to REFUSE anything and a version of it that said yes to everything passed
         * this test green. `ofac` really does carry "King Ak'Anon (Faction)" with no number after
         * it, and that is the line where refusing matters: peel it and the name stops matching its
         * page in factions.json, which is keyed "king ak'anon (faction)". */
        let bare = parse(r#"{"n":"x","ofac":["King Ak'Anon (Faction)","Miners Guild 628"]}"#);
        let b = bare.other_faction_hits();
        assert_eq!(
            (b[0].faction, b[0].delta),
            ("King Ak'Anon (Faction)", None),
            "a trailing bracket that is not a delta is part of the faction's name"
        );
        assert_eq!((b[1].faction, b[1].delta), ("Miners Guild 628", None));
    }

    #[test]
    fn a_two_zone_merchant_is_filed_under_both_and_a_one_zone_merchant_under_one() {
        let two = parse(r#"{"n":"x","zone":"Highpass Hold, Qeynos Hills"}"#);
        assert_eq!(
            two.zone_pieces(),
            vec![
                "Highpass Hold, Qeynos Hills",
                "Highpass Hold",
                "Qeynos Hills"
            ]
        );
        let one = parse(r#"{"n":"x","zone":"Lesser Faydark"}"#);
        assert_eq!(one.zone_pieces(), vec!["Lesser Faydark"]);
    }

    #[test]
    fn the_index_finds_every_seller_of_a_name_and_says_nothing_when_there_is_none() {
        let ms = vec![
            parse(r#"{"n":"a","sells":["Bandages (5sp 2cp)","Cloth Cap (1sp)"]}"#),
            parse(r#"{"n":"b","sells":["bandages (6sp)"]}"#),
            parse(r#"{"n":"c"}"#),
        ];
        let ix = SellersIndex::build(&ms);
        assert_eq!(ix.lines(), 3);
        assert_eq!(
            ix.items(),
            2,
            "case folded, so the two bandages are one key"
        );
        let b = ix.sellers_of("Bandages");
        assert_eq!(b.len(), 2);
        assert_eq!(
            b[0],
            Posting {
                merchant: 0,
                line: 0
            }
        );
        assert_eq!(
            b[1],
            Posting {
                merchant: 1,
                line: 0
            }
        );
        assert_eq!(
            ix.sellers_of("BANDAGES").len(),
            2,
            "the query is folded too"
        );
        assert!(ix.sellers_of("no such item, ever").is_empty());
        assert_eq!(
            ix.sellers_of("Cloth Cap")[0],
            Posting {
                merchant: 0,
                line: 1
            }
        );
    }

    /* ---- the real file. Fails loudly when absent; GRIMOIRE_NO_DATA=1 skips on purpose. ---- */

    #[test]
    fn the_real_file_parses_whole_and_keeps_its_measured_counts() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        assert_eq!(s.merchants.len(), 1373);
        assert!(s
            .merchants
            .iter()
            .all(|m| !m.key.is_empty() && !m.name.is_empty()));
        let with = |p: fn(&Merchant) -> bool| s.merchants.iter().filter(|m| p(m)).count();
        assert_eq!(with(|m| !m.zone.is_empty()), 1373, "zone");
        assert_eq!(with(|m| !m.loc.is_empty()), 1373, "loc");
        assert_eq!(with(|m| !m.race.is_empty()), 1373, "race");
        assert_eq!(with(|m| m.cls.is_some()), 1372, "cls");
        assert_eq!(with(|m| m.lvl.is_some()), 1371, "lvl");
        assert_eq!(with(|m| !m.sells.is_empty()), 1345, "sells");
        assert_eq!(with(|m| !m.fac.is_empty()), 847, "fac");
        assert_eq!(with(|m| !m.ofac.is_empty()), 245, "ofac");
        assert_eq!(with(|m| !m.quests.is_empty()), 179, "quests");
        assert_eq!(with(|m| !m.loot.is_empty()), 1, "loot");
        let lines: usize = s.merchants.iter().map(|m| m.sells.len()).sum();
        assert_eq!(lines, 23634, "sells lines");
        let fac: usize = s.merchants.iter().map(|m| m.fac.len() + m.ofac.len()).sum();
        assert_eq!(fac, 2418, "fac plus ofac lines");
    }

    /// THE KEY IS THE IDENTITY. Thirty two clockwork merchants share one display name, and this is
    /// the assertion that a later "index merchants by name" would have to break on purpose.
    #[test]
    fn the_keys_are_unique_where_the_names_are_not() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let mut keys: Vec<&str> = s.merchants.iter().map(|m| m.key.as_str()).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "the map keys are unique");
        let mut names: Vec<String> = s.merchants.iter().map(|m| fold(&m.name)).collect();
        names.sort();
        names.dedup();
        assert_eq!(
            names.len(),
            1324,
            "and the names are not: 1373 records, 1324 names"
        );
        let clockwork = s
            .merchants
            .iter()
            .filter(|m| fold(&m.name) == "clockwork merchant")
            .count();
        assert_eq!(
            clockwork, 32,
            "one name, thirty two shops with different stock"
        );
    }

    /// NOTHING IS DROPPED BY THE SPLIT, over every line of the real file. The head plus the
    /// bracket reassembles the source, so a pane can print either half and lose nothing.
    #[test]
    fn round_trips_on_every_line_of_the_real_file() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let mut sales = 0usize;
        let mut priced = 0usize;
        let mut refused = 0usize;
        for m in &s.merchants {
            for sale in m.sales() {
                sales += 1;
                let t = sale.raw.trim();
                match sale.price {
                    Some(p) => {
                        priced += 1;
                        let tail = format!("({p})");
                        assert!(t.ends_with(&tail), "{t:?} should end with {tail:?}");
                        assert_eq!(&t[..t.len() - tail.len()].trim_end(), &sale.item);
                    }
                    None => {
                        assert_eq!(t, sale.item, "an unsplit line is the whole line");
                        if t.ends_with(')') {
                            refused += 1;
                        }
                    }
                }
                assert!(!sale.item.is_empty(), "{:?} in {}", sale.raw, m.key);
            }
            for h in m.faction_hits().iter().chain(m.other_faction_hits().iter()) {
                let t = h.raw.trim();
                match h.delta {
                    Some(d) => {
                        let tail = format!("({d})");
                        assert!(t.ends_with(&tail), "{t:?} should end with {tail:?}");
                        assert_eq!(&t[..t.len() - tail.len()].trim_end(), &h.faction);
                    }
                    None => assert_eq!(t, h.faction),
                }
            }
        }
        assert_eq!(sales, 23634);
        assert_eq!(priced, 18163, "coin brackets plus the wiki's ? and ??");
        assert_eq!(
            refused, 48,
            "trailing brackets this build declines to read as a price: the 46 the wiki fumbled \n             (\"Book\", \"1gp 4cpp\", \"~106pp\") plus 2 lines with an unclosed bracket inside the \n             group, which is the same refusal for the same reason"
        );
    }

    /// THE INDEX AGAINST THE REAL FILE, and against the item records it is there to serve. The
    /// numbers are the argument for the feature: 369 item pages say "sold by a vendor" and name
    /// nobody.
    #[test]
    fn the_index_names_a_vendor_for_the_items_that_only_claimed_to_have_one() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        assert_eq!(s.sellers.lines(), 23634);
        assert_eq!(s.sellers.items(), 2720, "distinct item names, case folded");

        let flagged: Vec<&crate::data::Item> = s
            .items
            .iter()
            .filter(|it| it.source_kinds().iter().any(|k| k == &"sold"))
            .collect();
        assert_eq!(flagged.len(), 369, "items whose src carries the sold flag");
        let named = flagged
            .iter()
            .filter(|it| !s.sellers_of(&it.name).is_empty())
            .count();
        assert_eq!(named, 322, "of those, merchants.json names the seller");
        let unflagged_but_sold = s
            .items
            .iter()
            .filter(|it| !it.source_kinds().iter().any(|k| k == &"sold"))
            .filter(|it| !s.sellers_of(&it.name).is_empty())
            .count();
        assert_eq!(unflagged_but_sold, 40, "and 40 the sold flag never claimed");

        /* the sample from the goal, end to end */
        let sellers = s.sellers_of("Belt Pouch");
        assert!(!sellers.is_empty());
        let (m, sale) = s.sale(sellers[0]).expect("a posting resolves");
        assert_eq!(fold(sale.item), "belt pouch");
        assert!(sale.price.is_some(), "{:?}", sale.raw);
        assert!(!m.zone.is_empty());
    }

    #[test]
    fn a_root_without_the_file_loads_as_an_empty_list_and_not_an_error() {
        let dir = std::env::temp_dir().join("grimoire_merchants_absent_probe");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let _ = std::fs::remove_file(dir.join(MERCHANTS_FILE));
        assert_eq!(load(&dir).expect("absent is not an error"), Vec::new());
        std::fs::write(dir.join(MERCHANTS_FILE), "{ not json").expect("write");
        let err = load(&dir).expect_err("a broken file IS an error");
        assert!(err.path.ends_with(MERCHANTS_FILE));
        std::fs::write(dir.join(MERCHANTS_FILE), r#"{"schema":1,"merchants":{}}"#).expect("write");
        let err = load(&dir).expect_err("an empty map is an error too");
        assert!(err.reason.contains("merchants map is empty"));
        let _ = std::fs::remove_file(dir.join(MERCHANTS_FILE));
    }
}
