//! Screen: inventory. Decision D9: every row of an `/outputfile inventory` dump, read three ways.
//!
//! WHAT IS HERE. The dump reader (the column model, the two header rows skipped by shape, the
//! three-column storage rows, the `Empty` placeholders, the `-SlotN` chain), the location
//! vocabulary (which section a root location belongs to, and how to say where a row is), and the
//! copies rule (what a duplicate donates, and where merging every donor into the host lands). The
//! rank columns and the spare verdict come from `gear.rs`. Every rule carries a test that fails
//! without it.
//!
//! THE MODES. Three readings of ONE table: BY SLOT (what is on the character, in WORN_SLOTS
//! order), BY LOCATION (every row, under section tabs: worn, bags, equipment storage,
//! exaltations, bank, shared bank, depot, hoard, key ring, elsewhere) and SPARE (what every class
//! that can wear it has better options for, most-beaten first, with how to get rid of it). Quest
//! turn-ins belong to the Quests screen and exaltations to the Exaltations screen in the D5 nav.
//! The rank columns ride every mode.
//!
//! WHERE THE DUMP COMES FROM. `/outputfile inventory` writes `<Char>_<server>-Inventory.txt` into
//! the EverQuest folder, the PARENT of the Logs folder. The ingest's source list is asked first
//! (D7) and that folder and the Logs folder are scanned as the fallback, on a three second
//! throttle, so the screen works the moment Settings points at a Logs folder.
//!
//! WHAT IS NOT BUILT. The quest-reference column and the held-count ledger (the Quests lane), the
//! exaltation socket reading (the Exaltations lane), and an item-tooltip fallback for a row the
//! wiki has no gear record for (item-tooltips.json is not in the snapshot contract). A row with
//! no gear record says so; nothing is guessed for it.

use crate::screens::gear::{
    self, mono_right, Best, CatCache, Catalogue, CharState, EqEntry, Equipped, OwnedRow, Ranker,
    SpareReport, WORN_SLOTS,
};
use crate::screens::Cx;
use crate::theme::*;
use egui::{FontId, RichText, Stroke, Ui};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/* --------------------------------------------------------------- the rows -- */

/// The location sections, in the dump's own walking order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Worn,
    Bags,
    Storage,
    Exalts,
    Bank,
    Shared,
    Depot,
    Hoard,
    KeyRing,
    Other,
}

impl Section {
    pub const ALL: [Section; 10] = [
        Section::Worn,
        Section::Bags,
        Section::Storage,
        Section::Exalts,
        Section::Bank,
        Section::Shared,
        Section::Depot,
        Section::Hoard,
        Section::KeyRing,
        Section::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Worn => "Worn",
            Section::Bags => "Bags",
            Section::Storage => "Equipment storage",
            Section::Exalts => "Exaltations",
            Section::Bank => "Bank",
            Section::Shared => "Shared bank",
            Section::Depot => "Depot",
            Section::Hoard => "Dragon's hoard",
            Section::KeyRing => "Key ring",
            Section::Other => "Elsewhere",
        }
    }
}

/// One non-empty row of the dump, at full granularity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Dump order among the rows kept.
    pub idx: usize,
    pub loc: String,
    /// The location with every `-SlotN` peeled off: the place you walk to.
    pub root: String,
    pub sec: Section,
    pub name: String,
    /// The name without its `+N` (and, for a stone, without ` (Exaltation)`).
    pub base: String,
    pub tier: u32,
    /// An exaltation stone, named after the item it was rendered from. Not one of that item.
    pub exalt: bool,
    /// The dump's `Empty` placeholder, kept only when asked for.
    pub empty: bool,
    /// Nested inside another row: a bag's contents, or a stone socketed into a worn item.
    pub sub: bool,
    /// The client's item ID; 0 when the column is absent or not a number.
    pub item_id: u64,
    pub count: u32,
}

/// Base name and tier: the client prints the tier on the name (`Giant Snake Fang +4`, same ID
/// as +0) and stars some items. Tier is capped at 10.
pub fn base_name(name: &str) -> (String, u32) {
    let plain = name.trim_end_matches('*');
    if let Some(at) = plain.rfind(" +") {
        let digits = &plain[at + 2..];
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            let n: u32 = digits
                .parse::<u64>()
                .map(|v| v.min(u64::from(gear::MAX_TIER)) as u32)
                .unwrap_or(gear::MAX_TIER);
            return (plain[..at].to_owned(), n);
        }
    }
    (plain.to_owned(), 0)
}

/// `loc` minus one trailing `-SlotN`, when it has one.
fn strip_slot(loc: &str) -> Option<&str> {
    let at = loc.rfind("-Slot")?;
    let digits = &loc[at + 5..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(&loc[..at])
}

/// Does the location end in `-SlotN`.
pub fn is_sub(loc: &str) -> bool {
    strip_slot(loc).is_some()
}

/// A location string is a chain: `General 8-Slot6-Slot7` is a thing inside a thing inside bag 8.
/// The root decides which place you walk to.
pub fn root_loc(loc: &str) -> String {
    let mut s = loc;
    while let Some(rest) = strip_slot(s) {
        s = rest;
    }
    s.to_owned()
}

fn after_prefix_is_digits(s: &str, prefix: &str, digits_required: bool) -> bool {
    let Some(rest) = s.strip_prefix(prefix) else {
        return false;
    };
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    if rest.is_empty() {
        return !digits_required;
    }
    rest.bytes().all(|b| b.is_ascii_digit())
}

/// The section a ROOT location belongs to. The client writes `General 1` with a space and
/// `Bank1` without, so every matcher takes an optional one. Anything nothing matches lands in
/// Elsewhere rather than vanishing.
pub fn loc_section(root: &str) -> Section {
    if gear::worn_slot(root).is_some() {
        Section::Worn
    } else if root == "Held" || after_prefix_is_digits(root, "General", true) {
        Section::Bags
    } else if root == "Equipment" || root == "Activated" {
        Section::Storage
    } else if root == "Augmentation" {
        Section::Exalts
    } else if after_prefix_is_digits(root, "Bank", true) {
        Section::Bank
    } else if after_prefix_is_digits(root, "SharedBank", false) {
        Section::Shared
    } else if root.starts_with("Personal-Depot") {
        Section::Depot
    } else if root.to_ascii_lowercase().starts_with("dragon") {
        Section::Hoard
    } else if root.starts_with("KeyRing") {
        Section::KeyRing
    } else {
        Section::Other
    }
}

/// Every non-empty row the dump holds. The dump is tab separated, CRLF,
/// with TWO header rows (the KeyRing table repeats Location/Name/ID), both skipped by SHAPE
/// (`Name` then `ID` in columns two and three), not by first column. Storage rows are three
/// columns where the rest are five. `keep_empty` keeps the `Empty` placeholders, flagged, which
/// is the only evidence that a socket is open and empty.
pub fn parse_rows(text: &str, keep_empty: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 3 || (f[1] == "Name" && f[2] == "ID") {
            continue;
        }
        let loc = f[0];
        let name = f[1];
        if name.is_empty() {
            continue;
        }
        let root = root_loc(loc);
        let sec = loc_section(&root);
        if name == "Empty" {
            if keep_empty {
                rows.push(Row {
                    idx: rows.len(),
                    loc: loc.to_owned(),
                    root,
                    sec,
                    name: name.to_owned(),
                    base: String::new(),
                    tier: 0,
                    exalt: false,
                    empty: true,
                    sub: is_sub(loc),
                    item_id: 0,
                    count: 0,
                });
            }
            continue;
        }
        let exalt = name.ends_with("(Exaltation)");
        let stem = name
            .strip_suffix("(Exaltation)")
            .map(str::trim_end)
            .unwrap_or(name);
        let (base, tier) = base_name(stem);
        /* a missing or non-numeric ID reads as 0, a
         * missing, non-numeric or zero count is 1 */
        let item_id = f[2].trim().parse::<u64>().unwrap_or(0);
        let count = if f.len() > 3 {
            leading_int(f[3]).filter(|n| *n > 0).unwrap_or(1)
        } else {
            1
        };
        rows.push(Row {
            idx: rows.len(),
            loc: loc.to_owned(),
            root,
            sec,
            name: name.to_owned(),
            base,
            tier,
            exalt,
            empty: false,
            sub: is_sub(loc),
            item_id,
            count,
        });
    }
    rows
}

/// The leading digits of a field, or nothing.
fn leading_int(s: &str) -> Option<u32> {
    let t = s.trim_start();
    let end = t.bytes().take_while(|b| b.is_ascii_digit()).count();
    if end == 0 {
        return None;
    }
    t[..end].parse::<u32>().ok()
}

/// The worn slots only, in the shape the scorer wants: a `-SlotN`
/// row is a stone in the item, not the item, and `Finger` or `Ring` fold into `Fingers`.
pub fn equipped_of(rows: &[Row]) -> Equipped {
    let mut eq = Equipped::empty();
    for r in rows {
        if r.sub || r.empty {
            continue;
        }
        let Some(slot) = gear::worn_slot(&r.loc) else {
            continue;
        };
        eq.push(
            slot,
            EqEntry {
                name: r.name.clone(),
                base: r.base.clone(),
                tier: r.tier,
                rec: None,
            },
        );
    }
    eq
}

/// The ingest's parse of the dump (D7: one ingest for dumps and logs), as this screen's rows. The
/// ingest reads `/outputfile inventory` on its own thread and keeps every row including the
/// `Empty` placeholders; this drops the placeholders and renumbers, which is exactly the list
/// `parse_rows(text, false)` yields on the same text. A test below holds the two parsers to that,
/// so a drift in either lane's reading of a column is a red test and not a quiet disagreement
/// between the SOURCES ledger's record count and this table.
pub fn rows_from_ingest(dump: &crate::ingest::InventoryDump) -> Vec<Row> {
    let mut rows = Vec::new();
    for r in dump.rows() {
        let root = root_loc(&r.loc);
        let sec = loc_section(&root);
        rows.push(Row {
            idx: rows.len(),
            loc: r.loc.clone(),
            root,
            sec,
            name: r.name.clone(),
            base: r.base.clone(),
            tier: u32::from(r.tier),
            exalt: r.exalt,
            empty: false,
            sub: r.sub,
            item_id: r.item_id,
            count: r.count,
        });
    }
    rows
}

/// Where to walk, as words. Bags and bank get a
/// container number and a slot number because that is what gets your hand on the item; everywhere
/// else a number means nothing and gets a word. A socket is the one case the raw location cannot
/// express: `Head-Slot7` is a stone inside the thing on your head, and the number is a socket
/// id, not a position.
pub fn where_text(loc: &str) -> String {
    /* a -SlotN hanging off a worn slot, or off something already nested
     * inside a container, is a socket */
    let socket = strip_slot(loc).and_then(|host| {
        if gear::worn_slot(host).is_some() || is_sub(host) {
            Some((host, &loc[host.len() + 5..]))
        } else {
            None
        }
    });
    let base = socket.map(|(h, _)| h).unwrap_or(loc);
    let head = badge(base);
    match socket {
        Some((_, n)) => format!("{head} · socket {n}"),
        None => head,
    }
}

fn badge(loc: &str) -> String {
    if let Some(rest) = loc.strip_prefix("General") {
        if let Some((n, sub)) = number_then_slot(rest) {
            return match sub {
                Some(s) => format!("Bag {n} · {s}"),
                None => format!("Bag {n}"),
            };
        }
    }
    if let Some(rest) = loc.strip_prefix("Bank") {
        if let Some((n, sub)) = number_then_slot(rest) {
            return match sub {
                Some(s) => format!("Bank {n} · {s}"),
                None => format!("Bank {n}"),
            };
        }
    }
    for (prefix, word) in [
        ("SharedBank", "shared bank"),
        ("Personal-Depot", "depot"),
        ("KeyRing", "key ring"),
        ("Cursor", "cursor"),
        ("Equipment", "storage"),
        ("Activated", "activated"),
        ("Augmentation", "exaltation"),
    ] {
        if loc.starts_with(prefix) {
            return word.to_owned();
        }
    }
    if loc.to_ascii_lowercase().starts_with("dragon") {
        return "hoard".to_owned();
    }
    if gear::worn_slot(&root_loc(loc)).is_some() {
        return loc.to_owned();
    }
    loc.to_ascii_lowercase()
}

/// `( ?\d+)(?:-Slot(\d+))?` after a `General` or `Bank` prefix.
fn number_then_slot(rest: &str) -> Option<(u32, Option<u32>)> {
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    let end = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if end == 0 {
        return None;
    }
    let n: u32 = rest[..end].parse().ok()?;
    let tail = &rest[end..];
    let sub = tail.strip_prefix("-Slot").and_then(|d| {
        let e = d.bytes().take_while(|b| b.is_ascii_digit()).count();
        if e == 0 {
            None
        } else {
            d[..e].parse::<u32>().ok()
        }
    });
    Some((n, sub))
}

/// The character a dump belongs to, from its file name: everything before the
/// first underscore of `<Char>_<server>-Inventory.txt`.
pub fn character_of(file_name: &str) -> String {
    match file_name.find('_') {
        Some(0) | None => String::new(),
        Some(i) => file_name[..i].to_owned(),
    }
}

/* ----------------------------------------------------------------- copies -- */

/// Total exp a tier needs: 2^t - 1.
pub fn exp_of_tier(t: u32) -> u64 {
    (1u64 << t) - 1
}

/// What a tier-t duplicate gives when merged: 2^t.
pub fn donation(t: u32) -> u64 {
    1u64 << t
}

/// Where merging every donor into the host lands. The host's exp
/// within its tier is unknown, so `exp` and `tier` are floors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landing {
    pub exp: u64,
    pub tier: u32,
    pub rises: bool,
    /// Progress toward the tier after `tier`, from `tier`'s own threshold.
    pub toward: u64,
    pub next_cost: u64,
}

pub fn merge_landing(host_tier: u32, donor_tiers: &[u32]) -> Landing {
    let mut exp = exp_of_tier(host_tier);
    for t in donor_tiers {
        exp += donation(*t);
    }
    let mut tier = host_tier;
    while tier < gear::MAX_TIER && exp >= exp_of_tier(tier + 1) {
        tier += 1;
    }
    let next_cost = if tier < gear::MAX_TIER {
        exp_of_tier(tier + 1) - exp_of_tier(tier)
    } else {
        0
    };
    Landing {
        exp,
        tier,
        rises: tier > host_tier,
        toward: if tier < gear::MAX_TIER {
            exp - exp_of_tier(tier)
        } else {
            0
        },
        next_cost,
    }
}

/// The same item in more than one place. Identity is the client item
/// ID the dump prints; the base name only when the row has no ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyGroup {
    pub id: String,
    pub base: String,
    /// Row indices, host first: highest tier, then the worn copy, then dump order.
    pub rows: Vec<usize>,
    pub total: u32,
    /// The biggest stack held: the lowest the cap can be.
    pub cap: u32,
    pub stack: bool,
    pub host: usize,
    pub donors: Vec<usize>,
    /// Two copies worn in a paired slot: merging them costs the slot, so no action.
    pub worn_pair: bool,
    pub gear: bool,
    /// The fewest slots the copies could occupy: stacks packed to the cap, gear merged to one.
    pub slots_min: usize,
    pub saves: usize,
    pub landing: Option<Landing>,
}

impl CopyGroup {
    /// The arithmetic behind the Copies cell, for its hover: where the merge lands and how far
    /// that is from the next tier (the landing's `exp`, `toward` and
    /// `next_cost`), or for a stack, how many slots the pieces could pack into.
    pub fn arithmetic(&self, rows: &[Row]) -> Option<String> {
        if self.stack {
            return Some(format!(
                "{} in {} slot{}; packed to the biggest stack held ({}) that is {} slot{}",
                self.total,
                self.rows.len(),
                if self.rows.len() == 1 { "" } else { "s" },
                self.cap,
                self.slots_min,
                if self.slots_min == 1 { "" } else { "s" }
            ));
        }
        let l = self.landing.as_ref()?;
        let host = rows.get(self.host).map(|r| r.tier).unwrap_or(0);
        let mut s = format!("merged: {} exp, +{} from +{host}", l.exp, l.tier);
        if l.tier < gear::MAX_TIER {
            s.push_str(&format!(
                "; {} of the {} exp toward +{}",
                l.toward,
                l.next_cost,
                l.tier + 1
            ));
        } else {
            s.push_str("; at the cap");
        }
        Some(s)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Copies {
    pub groups: Vec<CopyGroup>,
    /// Row index to group index, for every row that is one of 2+ copies.
    pub by_row: HashMap<usize, usize>,
}

/// Worn is a root that is a worn slot, and a socket is never worn.
pub fn is_worn_loc(loc: &str) -> bool {
    !is_sub(loc) && gear::worn_slot(&root_loc(loc)).is_some()
}

fn is_ornament(name: &str) -> bool {
    name.trim_end_matches('*').ends_with("(Ornamentation)")
}

/// A container, the half of the question that needs no tooltip: it has positional
/// children that are not sockets. A child of a worn slot, or of something already nested, is a
/// socket; a child of a plain bag or bank position is a thing in a bag.
pub fn is_container(rows: &[Row], r: &Row) -> bool {
    if r.sub || gear::worn_slot(&r.loc).is_some() {
        return false;
    }
    let prefix = format!("{}-Slot", r.loc);
    rows.iter().any(|k| {
        k.loc.starts_with(&prefix)
            && k.loc[prefix.len()..].bytes().all(|b| b.is_ascii_digit())
            && !k.loc[prefix.len()..].is_empty()
    })
}

pub fn copies(
    rows: &[Row],
    is_container: impl Fn(&Row) -> bool,
    is_gear: impl Fn(&Row) -> bool,
) -> Copies {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for r in rows {
        if r.empty || r.exalt || is_ornament(&r.name) || is_container(r) {
            continue;
        }
        let id = if r.item_id > 0 {
            format!("id:{}", r.item_id)
        } else {
            format!("n:{}", r.base.to_lowercase())
        };
        let e = groups.entry(id.clone()).or_default();
        if e.is_empty() {
            order.push(id);
        }
        e.push(r.idx);
    }
    let mut out = Copies::default();
    for id in order {
        let mut g_rows = groups.remove(&id).unwrap_or_default();
        if g_rows.len() < 2 {
            continue;
        }
        let worn = |i: usize| is_worn_loc(&rows[i].loc);
        /* Host first, and the order matters in that order: the highest tier wins, so a +9
         * sitting in a bag absorbs the worn +5 rather than the other way round; being worn
         * breaks a tie between equal tiers; whatever is still tied keeps the order the dump
         * printed. That last clause is only true because `sort_by` is stable, which is why the
         * comparator stops at two keys instead of inventing a third. */
        g_rows.sort_by(|&a, &b| {
            rows[b]
                .tier
                .cmp(&rows[a].tier)
                .then_with(|| worn(b).cmp(&worn(a)))
        });
        let total: u32 = g_rows.iter().map(|&i| rows[i].count.max(1)).sum();
        let cap: u32 = g_rows
            .iter()
            .map(|&i| rows[i].count.max(1))
            .max()
            .unwrap_or(1);
        let stack = cap > 1;
        let worn_rows: Vec<usize> = g_rows.iter().copied().filter(|&i| worn(i)).collect();
        let host = g_rows[0];
        /* a worn copy other than the host is not a donor when the host is worn too (both Ears);
         * a worn copy IS a donor when the host is the better one in a bag */
        let donors: Vec<usize> = g_rows[1..]
            .iter()
            .copied()
            .filter(|&i| !(worn(i) && worn(host)))
            .collect();
        /* `worn_rows` is consumed here and not stored: the worn pair is the only thing it says */
        let worn_pair = worn_rows.len() >= 2 && donors.is_empty();
        let gear = g_rows.iter().any(|&i| is_gear(&rows[i]));
        let (slots_min, saves, landing) = if stack {
            let slots_min = total.div_ceil(cap) as usize;
            (slots_min, g_rows.len().saturating_sub(slots_min), None)
        } else if !gear {
            /* nothing to merge into; whether it stacks is not the dump's to say */
            (g_rows.len(), 0, None)
        } else {
            let landing = if donors.is_empty() {
                None
            } else {
                Some(merge_landing(
                    rows[host].tier,
                    &donors.iter().map(|&i| rows[i].tier).collect::<Vec<_>>(),
                ))
            };
            (1, donors.len(), landing)
        };
        let gi = out.groups.len();
        for &i in &g_rows {
            out.by_row.insert(i, gi);
        }
        out.groups.push(CopyGroup {
            id,
            base: rows[host].base.clone(),
            rows: g_rows,
            total,
            cap,
            stack,
            host,
            donors,
            worn_pair,
            gear,
            slots_min,
            saves,
            landing,
        });
    }
    out
}

/// ONE rule for "is there something to merge here": a worn pair, a set of
/// stacks with no slot to gain, a host already at the cap and a pair of things that carry no tier
/// all say no.
pub fn copies_todo(g: &CopyGroup) -> bool {
    if g.worn_pair {
        return false;
    }
    if g.stack {
        return g.saves > 0;
    }
    match &g.landing {
        None => false,
        /* a host already at the cap that does not rise gains nothing */
        Some(l) => l.tier < gear::MAX_TIER || l.rises,
    }
}

/// What the Copies cell says for one row of a group.
pub fn copies_action(g: &CopyGroup, rows: &[Row], r: usize) -> String {
    if g.worn_pair {
        return "both worn".into();
    }
    if g.stack {
        return if g.saves > 0 {
            format!(
                "frees {} slot{}",
                g.saves,
                if g.saves > 1 { "s" } else { "" }
            )
        } else {
            "no slot to gain".into()
        };
    }
    let Some(l) = &g.landing else {
        return if g.gear {
            String::new()
        } else {
            format!("{} places", g.rows.len())
        };
    };
    if l.tier >= gear::MAX_TIER && !l.rises {
        return format!("already +{}", gear::MAX_TIER);
    }
    let n = g.donors.len();
    if r == g.host {
        return if l.rises {
            format!("merge {n} in, lands +{}", l.tier)
        } else {
            format!(
                "merge {n} in, frees {n} slot{}",
                if n > 1 { "s" } else { "" }
            )
        };
    }
    if g.donors.contains(&r) {
        let host = where_text(&rows[g.host].loc);
        return if l.rises {
            format!("into {host}, lands +{}", l.tier)
        } else {
            format!("into {host}")
        };
    }
    "stays on".into()
}

/* --------------------------------------------------------------- the dump -- */

/// What `/outputfile inventory` writes: `<Char>_<server>-Inventory.txt`.
pub fn looks_like_inventory_dump(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_ascii_lowercase().ends_with("-inventory.txt"))
        .unwrap_or(false)
}

/// The two directories a dump can be in: the parent of the Logs folder,
/// which is the EverQuest folder the client writes into, and the Logs folder itself.
pub fn candidate_dirs(log_dir: Option<&Path>) -> Vec<PathBuf> {
    let Some(d) = log_dir else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(p) = d.parent() {
        if !p.as_os_str().is_empty() {
            out.push(p.to_path_buf());
        }
    }
    out.push(d.to_path_buf());
    out
}

/// The newest of the given files by modification time. A file that cannot be stat'ed is skipped.
pub fn newest_dump(paths: impl Iterator<Item = PathBuf>) -> Option<(PathBuf, SystemTime)> {
    let mut best: Option<(PathBuf, SystemTime)> = None;
    for p in paths {
        let Ok(md) = std::fs::metadata(&p) else {
            continue;
        };
        let Ok(m) = md.modified() else { continue };
        if best.as_ref().map(|(_, bm)| m > *bm).unwrap_or(true) {
            best = Some((p, m));
        }
    }
    best
}

/// Scan the candidate directories for dumps. `readable` is false when none of them could be
/// listed, which is the "Game folder is unreadable" case, named rather than shown as
/// "you have not dumped yet".
pub fn scan_dirs(dirs: &[PathBuf]) -> (Option<(PathBuf, SystemTime)>, bool) {
    let mut readable = false;
    let mut found = Vec::new();
    for d in dirs {
        let Ok(entries) = std::fs::read_dir(d) else {
            continue;
        };
        readable = true;
        for e in entries.flatten() {
            let p = e.path();
            if looks_like_inventory_dump(&p) {
                found.push(p);
            }
        }
    }
    (newest_dump(found.into_iter()), readable)
}

/// The dump, parsed once per (path, mtime).
#[derive(Clone, Debug)]
pub struct Found {
    pub path: PathBuf,
    pub mtime: SystemTime,
    /// Every non-empty row, dump order, from the ingest's parse when the ingest has this very
    /// file and from `parse_rows` when this screen found a newer one first.
    pub rows: Vec<Row>,
    pub character: String,
}

/// The dump discovery state one screen keeps.
#[derive(Default)]
pub struct DumpCache {
    /// The directories scanned on the last look: the EverQuest folder and the Logs folder.
    checked: Vec<PathBuf>,
    /// Where the ingest looked for a Logs folder when Settings names none, so the no-dump state
    /// can say "none of the usual places exist" and list them instead of "nowhere to look".
    tried: Vec<PathBuf>,
    found: Option<Found>,
    last_scan: Option<Instant>,
    problem: Option<String>,
    /// What the ingest itself says about the dump, in its own words.
    ingest_problem: Option<String>,
}

/// How often the dump folders are looked at again.
const SCAN_EVERY: Duration = Duration::from_millis(3000);

impl DumpCache {
    /// Look again, at most every three seconds.
    ///
    /// D7: THE INGEST IS THE FIRST ANSWER. It resolves the Logs folder (Settings first, then the
    /// usual places) and polls for the dump on its own thread, so its parsed rows are taken as they
    /// are and the file is not read twice. The two directories a dump can be in (the EverQuest
    /// folder and the Logs folder) are scanned as well, so a dump written
    /// seconds ago is picked up before the ingest's next poll. Newest wins; on a tie the ingest's
    /// copy wins, because it is already parsed.
    pub fn refresh(&mut self, cx: &Cx) {
        if let Some(t) = self.last_scan {
            if t.elapsed() < SCAN_EVERY {
                return;
            }
        }
        self.last_scan = Some(Instant::now());
        let report = cx.ingest.log_dir();
        let log_dir = report.dir.clone().or_else(|| cx.settings.log_dir.clone());
        self.tried = report.tried.clone();
        self.ingest_problem = cx.ingest.inventory_problem().map(str::to_owned);
        let dirs = candidate_dirs(log_dir.as_deref());
        let ingest_dump: Option<(PathBuf, SystemTime)> = cx.ingest.inventory().map(|d| {
            let mtime = d
                .modified
                .map(SystemTime::from)
                .or_else(|| {
                    std::fs::metadata(&d.path)
                        .ok()
                        .and_then(|m| m.modified().ok())
                })
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (d.path.clone(), mtime)
        });
        let from_sources = newest_dump(
            cx.ingest
                .sources()
                .into_iter()
                .map(|s| s.path)
                .filter(|p| looks_like_inventory_dump(p)),
        );
        let (scanned, readable) = scan_dirs(&dirs);
        self.checked = dirs;
        self.problem = if !self.checked.is_empty() && !readable {
            Some("Game folder is unreadable.".to_owned())
        } else {
            None
        };
        /* max_by_key keeps the LAST of equal maxima, so the ingest's copy sits last and wins a tie */
        let best = [from_sources, scanned, ingest_dump.clone()]
            .into_iter()
            .flatten()
            .max_by_key(|(_, m)| *m);
        let Some((path, mtime)) = best else {
            self.found = None;
            return;
        };
        if self
            .found
            .as_ref()
            .map(|f| f.path == path && f.mtime == mtime)
            .unwrap_or(false)
        {
            return;
        }
        let character = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(character_of)
            .unwrap_or_default();
        let ingest_has_it = ingest_dump
            .as_ref()
            .map(|(p, m)| *p == path && *m == mtime)
            .unwrap_or(false);
        if ingest_has_it {
            if let Some(d) = cx.ingest.inventory() {
                self.found = Some(Found {
                    path,
                    mtime,
                    rows: rows_from_ingest(d),
                    character,
                });
                return;
            }
        }
        match std::fs::read(&path) {
            Ok(bytes) => {
                let rows = parse_rows(&String::from_utf8_lossy(&bytes), false);
                self.found = Some(Found {
                    path,
                    mtime,
                    rows,
                    character,
                });
            }
            Err(e) => {
                self.problem = Some(format!("{} could not be read: {e}", path.display()));
                self.found = None;
            }
        }
    }

    pub fn found(&self) -> Option<&Found> {
        self.found.as_ref()
    }

    pub fn checked(&self) -> &[PathBuf] {
        &self.checked
    }

    pub fn tried(&self) -> &[PathBuf] {
        &self.tried
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn ingest_problem(&self) -> Option<&str> {
        self.ingest_problem.as_deref()
    }

    pub fn stamp(&self) -> Option<(PathBuf, SystemTime)> {
        self.found.as_ref().map(|f| (f.path.clone(), f.mtime))
    }

    /// One monospace line: the file, when it was dumped, whose it is.
    pub fn status_line(&self) -> String {
        match &self.found {
            None => "no inventory dump found".to_owned(),
            Some(f) => {
                let when: chrono::DateTime<chrono::Local> = f.mtime.into();
                let name = f.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let who = if f.character.is_empty() {
                    String::new()
                } else {
                    format!(" · {}'s dump", f.character)
                };
                format!("{name} · dumped {}{who}", when.format("%Y-%m-%d %H:%M"))
            }
        }
    }
}

/* ------------------------------------------------------------- the analysis -- */

/// One row joined to its gear record, minus the quest and tooltip columns.
struct RowView {
    rec: Option<usize>,
    stats_txt: String,
    ac: Option<i32>,
    sv_txt: String,
    ratio_txt: String,
    slot_txt: String,
    /// The class line: "ALL", "ALL except WAR", or the codes.
    cls_txt: String,
    eff_txt: String,
    /// "yes", "attunable", "no trade", "no drop", or "" when nothing here knows.
    trade: &'static str,
    wt: Option<f64>,
    hay: String,
    rank: Option<Best>,
    /// Carries an effect and nothing the score reads: not ranked, and the cell says why.
    eff_only: bool,
}

/// Everything computed once per dump (and per catalogue, level and trio), never per frame.
struct Analysis {
    stamp: (PathBuf, SystemTime),
    cat_len: Option<usize>,
    level: u32,
    trio: Vec<String>,
    rows: Vec<Row>,
    views: Vec<RowView>,
    copies: Copies,
    spare: Option<SpareReport>,
    /// Row index to spare item index.
    spare_by_row: HashMap<usize, usize>,
    to_merge: usize,
    slots_back: usize,
}

const STAT_ORDER: [(&str, &str); 11] = [
    ("hp", "HP"),
    ("mana", "MANA"),
    ("end", "END"),
    ("str", "STR"),
    ("sta", "STA"),
    ("agi", "AGI"),
    ("dex", "DEX"),
    ("wis", "WIS"),
    ("int", "INT"),
    ("cha", "CHA"),
    ("atk", "ATK"),
];

const NO_SCORE_WHY: &str = "Not ranked: it carries an effect and none of the stats the score reads. Clicky, proc and focus effects are all unscored, so an empty rank here is not a verdict on it.";

fn build_views(
    rows: &[Row],
    cat: Option<&Catalogue>,
    ranker: Option<&mut Ranker>,
    trio: &[String],
) -> Vec<RowView> {
    let mut memo: HashMap<usize, Best> = HashMap::new();
    let mut ranker = ranker;
    rows.iter()
        .map(|r| {
            let rec_i = cat.and_then(|c| c.find(&r.name));
            let rec = rec_i.and_then(|i| cat.map(|c| &c.recs[i]));
            let mut stats_txt = String::new();
            let mut ac = None;
            let mut sv_txt = String::new();
            let mut ratio_txt = String::new();
            let mut slot_txt = String::new();
            let mut cls_txt = String::new();
            let mut eff_txt = String::new();
            let mut trade = "";
            let mut wt = None;
            let mut eff_only = false;
            if let Some(g) = rec {
                let st = g.stats_at(r.tier);
                ac = st.iter().find(|(k, _)| *k == "ac").map(|(_, v)| *v);
                let mut sb = Vec::new();
                for (k, label) in STAT_ORDER {
                    if let Some((_, v)) = st.iter().find(|(s, _)| *s == k) {
                        if *v != 0 {
                            sb.push(format!("{}{v} {label}", if *v > 0 { "+" } else { "" }));
                        }
                    }
                }
                stats_txt = sb.join(" ");
                let mut svb = Vec::new();
                for k in ["f", "c", "m", "d", "p", "v"] {
                    if let Some((_, n)) = g.sv.iter().find(|(s, _)| s == k) {
                        if *n != 0 {
                            let label = if k == "v" {
                                "VOID"
                            } else {
                                &k.to_ascii_uppercase()
                            };
                            svb.push(format!("{label}{}{n}", if *n > 0 { "+" } else { "" }));
                        }
                    }
                }
                sv_txt = svb.join(" ");
                if let (Some(dmg), Some(dly)) = (g.dmg, g.dly) {
                    ratio_txt = format!("{}/{dly}", gear::stat_at(dmg, r.tier));
                }
                slot_txt = g.sl.join(" ");
                cls_txt = g.cls.text();
                wt = g.wt;
                let mut effs = Vec::new();
                if g.haste != 0 {
                    effs.push(format!("haste +{}%", g.haste));
                }
                effs.extend(g.eff.iter().cloned());
                if g.charges > 0 {
                    effs.push(format!("{} charges", g.charges));
                }
                eff_txt = effs.join(", ");
                trade = g.trade();
                eff_only = !r.exalt && (!g.eff.is_empty() || g.foc) && !g.scorable();
            }
            let rank = match (rec_i, cat, ranker.as_deref_mut()) {
                (Some(i), Some(c), Some(rk)) if !r.exalt && !is_ornament(&r.name) => {
                    let b = memo.entry(i).or_insert_with(|| rk.best(c, i, trio)).clone();
                    if b.best.is_some() {
                        Some(b)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let hay = [
                r.name.as_str(),
                r.loc.as_str(),
                slot_txt.as_str(),
                cls_txt.as_str(),
                stats_txt.as_str(),
                sv_txt.as_str(),
                eff_txt.as_str(),
                trade,
            ]
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" | ");
            RowView {
                rec: rec_i,
                stats_txt,
                ac,
                sv_txt,
                ratio_txt,
                slot_txt,
                cls_txt,
                eff_txt,
                trade,
                wt,
                hay,
                rank,
                eff_only,
            }
        })
        .collect()
}

/* ----------------------------------------------------------------- screen -- */

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    BySlot,
    ByLocation,
    Spare,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::BySlot, Mode::ByLocation, Mode::Spare];
    pub fn chip(self) -> &'static str {
        match self {
            Mode::BySlot => "By slot",
            Mode::ByLocation => "By location",
            Mode::Spare => "Spare",
        }
    }
}

/// The Inventory screen: the dump as a table, three ways.
pub struct InventoryScreen {
    mode: Mode,
    search: String,
    section: Option<Section>,
    dump: DumpCache,
    cat: Option<CatCache>,
    ranker: Option<Ranker>,
    analysis: Option<Analysis>,
    char_: CharState,
    loaded: bool,
}

impl Default for InventoryScreen {
    fn default() -> Self {
        InventoryScreen {
            mode: Mode::ByLocation,
            search: String::new(),
            section: None,
            dump: DumpCache::default(),
            cat: None,
            ranker: None,
            analysis: None,
            char_: CharState::default(),
            loaded: false,
        }
    }
}

impl InventoryScreen {
    fn refresh_analysis(&mut self) {
        let Some(stamp) = self.dump.stamp() else {
            self.analysis = None;
            return;
        };
        let cat = self
            .cat
            .as_ref()
            .filter(|c| c.problem.is_none())
            .map(|c| &c.cat);
        let cat_len = cat.map(Catalogue::len);
        let fresh = self
            .analysis
            .as_ref()
            .map(|a| {
                a.stamp == stamp
                    && a.cat_len == cat_len
                    && a.level == self.char_.level
                    && a.trio == self.char_.classes
            })
            .unwrap_or(false);
        if fresh {
            return;
        }
        if self
            .ranker
            .as_ref()
            .map(|r| r.level != self.char_.level)
            .unwrap_or(true)
            || self
                .analysis
                .as_ref()
                .map(|a| a.cat_len != cat_len)
                .unwrap_or(true)
        {
            self.ranker = Some(Ranker::new(self.char_.level));
        }
        let rows = self
            .dump
            .found()
            .map(|f| f.rows.clone())
            .unwrap_or_default();
        let views = build_views(&rows, cat, self.ranker.as_mut(), &self.char_.classes);
        let is_gear = |r: &Row| {
            r.tier > 0
                || views[r.idx]
                    .rec
                    .and_then(|i| cat.map(|c| !c.recs[i].sl.is_empty()))
                    .unwrap_or(false)
        };
        let cps = copies(&rows, |r| is_container(&rows, r), is_gear);
        let mut to_merge = 0;
        let mut slots_back = 0;
        for g in &cps.groups {
            if copies_todo(g) {
                to_merge += 1;
                slots_back += if g.stack { g.saves } else { g.donors.len() };
            }
        }
        let (spare, spare_by_row) = match cat {
            None => (None, HashMap::new()),
            Some(c) => {
                let owned: Vec<OwnedRow> = rows
                    .iter()
                    .map(|r| OwnedRow {
                        i: r.idx,
                        rec: views[r.idx].rec,
                        tier: r.tier,
                        exalt: r.exalt,
                    })
                    .collect();
                let rep = gear::analyze_spare(&owned, c, self.char_.level);
                let by: HashMap<usize, usize> = rep
                    .items
                    .iter()
                    .enumerate()
                    .map(|(si, it)| (it.row, si))
                    .collect();
                (Some(rep), by)
            }
        };
        self.analysis = Some(Analysis {
            stamp,
            cat_len,
            level: self.char_.level,
            trio: self.char_.classes.clone(),
            rows,
            views,
            copies: cps,
            spare,
            spare_by_row,
            to_merge,
            slots_back,
        });
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        if !self.loaded {
            self.loaded = true;
        }
        /* the trio and level are the Gear screen's to edit; read every frame so an edit there
         * shows here without a restart */
        self.char_ = CharState::read(cx.settings);
        CatCache::refresh(&mut self.cat, cx.data);
        self.dump.refresh(cx);
        self.refresh_analysis();

        ui.label(
            RichText::new("INVENTORY")
                .font(crate::fonts::display(16.0))
                .color(GOLD_HI),
        );
        /* The "Your trio" column is priced for whatever trio is stored; when none is, it is the
         * default trio and the column head would be a lie without this line. */
        let note = CharState::default_note(cx.settings);
        if !note.is_empty() {
            ui.label(
                RichText::new(format!(
                    "trio {} at {} {note}",
                    self.char_.classes.join("/"),
                    self.char_.level
                ))
                .font(FontId::proportional(11.0))
                .color(TEXT_2),
            );
        }
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            for m in Mode::ALL {
                let on = m == self.mode;
                let txt = RichText::new(m.chip())
                    .font(FontId::proportional(12.5))
                    .color(if on { FLARE } else { TEXT_2 });
                let b = egui::Button::new(txt)
                    .fill(if on {
                        PANEL_2
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .stroke(Stroke::new(1.0, if on { GOLD_DIM } else { RULE }))
                    .corner_radius(egui::CornerRadius::ZERO);
                if ui.add(b).clicked() {
                    self.mode = m;
                }
            }
            ui.add_space(12.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("search name, slot, stats, effect")
                    .desired_width(260.0)
                    .font(FontId::monospace(12.0)),
            );
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(self.dump.status_line())
                .font(FontId::monospace(11.0))
                .color(TEXT_2),
        );

        if self.dump.found().is_none() {
            ui.add_space(6.0);
            no_dump_ui(ui, &self.dump);
            return;
        }

        let Some(a) = self.analysis.as_ref() else {
            return;
        };
        let cat = self
            .cat
            .as_ref()
            .filter(|c| c.problem.is_none())
            .map(|c| &c.cat);
        if cat.is_none() {
            let why = match (&self.cat, cx.data_err) {
                (Some(c), _) => format!("item records could not be read: {}", c.problem.as_deref().unwrap_or("")),
                (None, Some(e)) => format!("snapshot failed to load: {e}"),
                (None, None) => "no snapshot loaded. Put gear-data.json in data/ beside the executable; the Settings screen lists every folder the loader tries.".to_owned(),
            };
            ui.label(
                RichText::new(format!(
                    "Item stats, slots, ranks and the spare verdict need the gear dataset: {why}"
                ))
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
            );
        }

        /* the meta line: what the whole dump says, whatever the filters show */
        let needle = self.search.trim().to_ascii_lowercase();
        let mut meta = vec![format!("{} items", a.rows.len())];
        if a.to_merge > 0 {
            meta.push(format!(
                "{} to merge · {} bag slot{} back",
                a.to_merge,
                a.slots_back,
                if a.slots_back == 1 { "" } else { "s" }
            ));
        }
        if let Some(sp) = &a.spare {
            let n = sp.ranked.iter().filter(|&&i| sp.items[i].spare).count();
            meta.push(format!("{n} spare of {} rankable", sp.items.len()));
            /* what the verdict could not rank, so "rankable" is not read as "everything" */
            if sp.skipped_unknown > 0 {
                meta.push(format!("{} with no wiki record", sp.skipped_unknown));
            }
            if sp.skipped_not_wearable > 0 {
                meta.push(format!("{} not wearable", sp.skipped_not_wearable));
            }
            if sp.skipped_exalt > 0 {
                meta.push(format!(
                    "{} exaltation stone{}",
                    sp.skipped_exalt,
                    if sp.skipped_exalt == 1 { "" } else { "s" }
                ));
            }
        }
        ui.label(
            RichText::new(meta.join(" · "))
                .font(FontId::monospace(11.0))
                .color(TEXT_3),
        );
        ui.add_space(6.0);

        match self.mode {
            Mode::BySlot => self.draw_by_slot(ui, cat, &needle),
            Mode::ByLocation => {
                /* the section tab picked this frame comes back as a value: the draw borrows the
                 * analysis and the catalogue, so it cannot also write to self */
                let pick = self.draw_by_location(ui, cat, &needle);
                self.section = pick;
            }
            Mode::Spare => self.draw_spare(ui, cat, &needle),
        }
    }

    fn header(ui: &mut Ui, cols: &[&str]) {
        for h in cols {
            ui.label(
                RichText::new(*h)
                    .font(FontId::proportional(11.0))
                    .color(GOLD_DIM),
            );
        }
        ui.end_row();
    }

    /// Is this row one the spare verdict calls spare, so the other modes can mark it too.
    fn is_spare(a: &Analysis, r: &Row) -> bool {
        match (&a.spare, a.spare_by_row.get(&r.idx)) {
            (Some(sp), Some(&si)) => sp.items[si].spare,
            _ => false,
        }
    }

    fn item_cell(ui: &mut Ui, r: &Row, v: &RowView, cat: Option<&Catalogue>, spare: bool) {
        let mut txt = r.name.clone();
        if v.rec.is_none() && cat.is_some() && !r.exalt {
            txt.push_str("  (no wiki record)");
        }
        let col = if v.rec.is_none() && cat.is_some() {
            TEXT_3
        } else {
            TEXT
        };
        ui.horizontal(|ui| {
            let resp = ui.label(RichText::new(txt).font(FontId::proportional(12.0)).color(col));
            if let Some(i) = v.rec {
                if let Some(c) = cat {
                    let cav = c.recs[i].unscored();
                    if !cav.is_empty() {
                        resp.on_hover_text(format!("carries an unscored {}", cav.join(", ")));
                    }
                }
            }
            if spare {
                ui.label(RichText::new("spare").font(FontId::proportional(10.5)).color(TEXT_3)).on_hover_text("every class that can wear it has better options you own in every slot it fits (the Spare mode has the detail)");
            }
        });
    }

    fn rank_cell(ui: &mut Ui, s: Option<&gear::Standing>, v: &RowView, echo: bool) {
        match s {
            Some(x) => {
                let txt = format!("#{} of {}  {} {}", x.rank, x.of, x.cls, x.slot);
                let col = if echo {
                    TEXT_3
                } else if x.rank <= 3 {
                    GOLD
                } else {
                    TEXT
                };
                let mut hover = x.what();
                if let Some(b) = &v.rank {
                    hover.push_str("\n\n");
                    hover.push_str(&gear::standings_text(b));
                }
                ui.label(RichText::new(txt).font(FontId::monospace(11.5)).color(col))
                    .on_hover_text(hover);
            }
            None if v.eff_only => {
                ui.label(
                    RichText::new("effect only")
                        .font(FontId::proportional(11.0))
                        .color(TEXT_3),
                )
                .on_hover_text(NO_SCORE_WHY);
            }
            None => {
                ui.label("");
            }
        }
    }

    fn copies_cell(ui: &mut Ui, a: &Analysis, r: &Row) {
        match a.copies.by_row.get(&r.idx) {
            None => {
                ui.label("");
            }
            Some(&gi) => {
                let g = &a.copies.groups[gi];
                let hot =
                    copies_todo(g) && (g.stack || g.donors.contains(&r.idx) || r.idx == g.host);
                let txt = format!("x{}  {}", g.rows.len(), copies_action(g, &a.rows, r.idx));
                let mut places: Vec<String> = g
                    .rows
                    .iter()
                    .map(|&i| {
                        format!(
                            "{}{}{}",
                            where_text(&a.rows[i].loc),
                            if a.rows[i].tier > 0 {
                                format!(" +{}", a.rows[i].tier)
                            } else {
                                String::new()
                            },
                            if a.rows[i].count > 1 {
                                format!(" x{}", a.rows[i].count)
                            } else {
                                String::new()
                            }
                        )
                    })
                    .collect();
                /* the landing arithmetic (or the packing count) under the places, so "lands +9"
                 * comes with how far +9 is from +10 */
                if let Some(ar) = g.arithmetic(&a.rows) {
                    places.push(ar);
                }
                ui.label(
                    RichText::new(txt)
                        .font(FontId::proportional(11.5))
                        .color(if hot { GOLD } else { TEXT_3 }),
                )
                .on_hover_text(places.join("\n"));
            }
        }
    }

    fn draw_by_slot(&self, ui: &mut Ui, cat: Option<&Catalogue>, needle: &str) {
        let Some(a) = self.analysis.as_ref() else {
            return;
        };
        let mut worn: Vec<&Row> = a
            .rows
            .iter()
            .filter(|r| r.sec == Section::Worn && !r.sub)
            .collect();
        worn.sort_by_key(|r| {
            WORN_SLOTS
                .iter()
                .position(|s| gear::worn_slot(&r.loc) == Some(s))
                .unwrap_or(usize::MAX)
        });
        let worn: Vec<&Row> = worn
            .into_iter()
            .filter(|r| needle.is_empty() || a.views[r.idx].hay.contains(needle))
            .collect();
        if worn.is_empty() {
            ui.label(
                RichText::new(if needle.is_empty() {
                    "Nothing in the dump is worn."
                } else {
                    "Nothing worn matches that search."
                })
                .font(FontId::proportional(12.0))
                .color(TEXT_3),
            );
            return;
        }
        egui::ScrollArea::both().id_salt("inv-slot").show(ui, |ui| {
            egui::Grid::new("inv-slot-grid")
                .striped(true)
                .num_columns(10)
                .spacing([14.0, 3.0])
                .show(ui, |ui| {
                    Self::header(
                        ui,
                        &[
                            "Slot", "Item", "ID", "Tier", "AC", "Stats", "Resists", "Dmg/Dly",
                            "Effect", "Copies",
                        ],
                    );
                    for r in worn {
                        let v = &a.views[r.idx];
                        ui.label(
                            RichText::new(gear::worn_slot(&r.loc).unwrap_or(r.loc.as_str()))
                                .font(FontId::proportional(12.0))
                                .color(TEXT_2),
                        );
                        Self::item_cell(ui, r, v, cat, Self::is_spare(a, r));
                        mono_right(
                            ui,
                            if r.item_id > 0 {
                                r.item_id.to_string()
                            } else {
                                String::new()
                            },
                        );
                        mono_right(
                            ui,
                            if r.tier > 0 {
                                format!("+{}", r.tier)
                            } else {
                                String::new()
                            },
                        );
                        mono_right(ui, v.ac.map(|x| x.to_string()).unwrap_or_default());
                        ui.label(
                            RichText::new(&v.stats_txt)
                                .font(FontId::monospace(11.0))
                                .color(TEXT),
                        );
                        ui.label(
                            RichText::new(&v.sv_txt)
                                .font(FontId::monospace(11.0))
                                .color(TEXT_2),
                        );
                        mono_right(ui, v.ratio_txt.clone());
                        ui.label(
                            RichText::new(&v.eff_txt)
                                .font(FontId::proportional(11.0))
                                .color(TEXT_2),
                        );
                        Self::copies_cell(ui, a, r);
                        ui.end_row();
                    }
                });
        });
    }

    /// Draws the location table and returns the section tab that should be active afterwards.
    fn draw_by_location(
        &self,
        ui: &mut Ui,
        cat: Option<&Catalogue>,
        needle: &str,
    ) -> Option<Section> {
        let Some(a) = self.analysis.as_ref() else {
            return self.section;
        };
        /* the section tabs, with counts that respect the search: searching shows WHERE the hits
         * live */
        let pool: Vec<&Row> = a
            .rows
            .iter()
            .filter(|r| needle.is_empty() || a.views[r.idx].hay.contains(needle))
            .collect();
        let mut counts: HashMap<Section, usize> = HashMap::new();
        for r in &pool {
            *counts.entry(r.sec).or_insert(0) += 1;
        }
        /* a filter that emptied the active tab drops back to All */
        let mut pick = self
            .section
            .filter(|s| counts.get(s).copied().unwrap_or(0) > 0);
        ui.horizontal_wrapped(|ui| {
            let tab = |ui: &mut Ui, label: String, on: bool| -> bool {
                let txt = RichText::new(label)
                    .font(FontId::proportional(11.5))
                    .color(if on { FLARE } else { TEXT_2 });
                ui.add(
                    egui::Button::new(txt)
                        .fill(if on {
                            PANEL_2
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(Stroke::NONE)
                        .corner_radius(egui::CornerRadius::ZERO),
                )
                .clicked()
            };
            if tab(ui, format!("All {}", pool.len()), pick.is_none()) {
                pick = None;
            }
            for s in Section::ALL {
                let n = counts.get(&s).copied().unwrap_or(0);
                if n == 0 {
                    continue;
                }
                if tab(ui, format!("{} {n}", s.label()), pick == Some(s)) {
                    pick = Some(s);
                }
            }
        });
        let rows: Vec<&Row> = pool
            .into_iter()
            .filter(|r| pick.map(|s| r.sec == s).unwrap_or(true))
            .collect();
        if rows.is_empty() {
            ui.label(
                RichText::new(if needle.is_empty() {
                    "Nothing in the dump."
                } else {
                    "Nothing matches that search."
                })
                .font(FontId::proportional(12.0))
                .color(TEXT_3),
            );
            return pick;
        }
        ui.add_space(4.0);
        egui::ScrollArea::both().id_salt("inv-loc").show(ui, |ui| {
            egui::Grid::new("inv-loc-grid").striped(true).num_columns(12).spacing([14.0, 3.0]).show(ui, |ui| {
                Self::header(ui, &["Where", "Item", "Qty", "ID", "Tier", "Slot", "Classes", "Stats", "Trade", "Your trio", "Any class", "Copies"]);
                for r in rows {
                    let v = &a.views[r.idx];
                    ui.label(RichText::new(where_text(&r.loc)).font(FontId::proportional(11.5)).color(TEXT_2)).on_hover_text(&r.loc);
                    Self::item_cell(ui, r, v, cat, Self::is_spare(a, r));
                    mono_right(ui, if r.count > 1 { r.count.to_string() } else { String::new() });
                    mono_right(ui, if r.item_id > 0 { r.item_id.to_string() } else { String::new() });
                    mono_right(ui, if r.tier > 0 { format!("+{}", r.tier) } else { String::new() });
                    ui.label(RichText::new(&v.slot_txt).font(FontId::proportional(11.0)).color(TEXT_2));
                    ui.label(RichText::new(&v.cls_txt).font(FontId::monospace(11.0)).color(TEXT_2));
                    ui.label(RichText::new(&v.stats_txt).font(FontId::monospace(11.0)).color(TEXT));
                    ui.label(RichText::new(v.trade).font(FontId::proportional(11.0)).color(TEXT_2));
                    let trio = v.rank.as_ref().and_then(|b| b.trio.first());
                    let any = v.rank.as_ref().and_then(|b| b.best.as_ref());
                    Self::rank_cell(ui, trio, v, false);
                    let echo = matches!((trio, any), (Some(t), Some(x)) if t.cls == x.cls && t.slot == x.slot);
                    Self::rank_cell(ui, any, v, echo);
                    Self::copies_cell(ui, a, r);
                    ui.end_row();
                }
            });
        });
        pick
    }

    fn draw_spare(&self, ui: &mut Ui, cat: Option<&Catalogue>, needle: &str) {
        let Some(a) = self.analysis.as_ref() else {
            return;
        };
        /* Spare states the rule it ranks by: without it the number in the Better column is
         * uninterpretable, and this is the one mode whose output is a suggestion to get rid of
         * something. */
        ui.label(
            RichText::new("Every class that can wear it has better options in every slot it fits. Better counts what beats it in the slot where it does best; Ear, Wrist, Fingers and Any Slot keep two. An item at a lower upgrade rank is scaled up to its rival's before the comparison. Clicks, procs, worn effects and instruments are not part of the score; items carrying one are marked.")
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
        ui.add_space(4.0);
        let (Some(sp), Some(c)) = (a.spare.as_ref(), cat) else {
            ui.label(
                RichText::new("The spare verdict needs the gear dataset.")
                    .font(FontId::proportional(12.0))
                    .color(TEXT_3),
            );
            return;
        };
        let rows: Vec<(&Row, &gear::SpareItem)> = sp
            .ranked
            .iter()
            .map(|&si| &sp.items[si])
            .filter(|it| it.spare)
            .map(|it| (&a.rows[it.row], it))
            .filter(|(r, _)| needle.is_empty() || a.views[r.idx].hay.contains(needle))
            .collect();
        if rows.is_empty() {
            let msg = if !needle.is_empty() {
                "Nothing spare matches that search."
            } else if sp.items.is_empty() {
                "Nothing in the dump resolves to an item the wiki has stats for."
            } else {
                "Nothing you own is beaten in every slot it fits."
            };
            ui.label(
                RichText::new(msg)
                    .font(FontId::proportional(12.0))
                    .color(TEXT_3),
            );
            return;
        }
        egui::ScrollArea::both().id_salt("inv-spare").show(ui, |ui| {
            egui::Grid::new("inv-spare-grid").striped(true).num_columns(9).spacing([14.0, 3.0]).show(ui, |ui| {
                Self::header(ui, &["Where", "Item", "Better", "Beaten by", "Get rid of it by", "Wt", "Your trio", "Any class", "Copies"]);
                for (r, it) in rows {
                    let v = &a.views[r.idx];
                    ui.label(RichText::new(where_text(&r.loc)).font(FontId::proportional(11.5)).color(TEXT_2)).on_hover_text(&r.loc);
                    Self::item_cell(ui, r, v, cat, false);
                    match &it.best {
                        Some(b) => {
                            let t = format!(
                                "{} of the items you own beat this for a {} in {}{}{}",
                                it.ahead,
                                gear::class_name(b.cls),
                                b.slot,
                                if b.cap > 1 { format!(", and that slot takes {}", b.cap) } else { String::new() },
                                if it.ahead_as_is != it.ahead { format!(". At the tiers they are actually at: {}.", it.ahead_as_is) } else { String::new() }
                            );
                            let mut txt = format!("{}  {} {}", it.ahead, b.cls, b.slot);
                            if let Some(u) = it.upgrade_to {
                                txt.push_str(&format!("  (kept at +{u})"));
                            }
                            /* the score does not read a click, a proc or an instrument: an item
                             * carrying one is marked, as the note above promises */
                            if !it.unscored.is_empty() {
                                txt.push_str(&format!("  [{}]", it.unscored.join(", ")));
                            }
                            ui.label(RichText::new(txt).font(FontId::monospace(11.5)).color(TEXT)).on_hover_text(t);
                        }
                        None => {
                            ui.label(RichText::new("no class").font(FontId::proportional(11.0)).color(TEXT_3)).on_hover_text("the wiki lists no class that can equip this");
                        }
                    }
                    /* one wrapping line, not one line per rival */
                    let by: Vec<String> = it
                        .best
                        .as_ref()
                        .map(|b| b.by.iter().take(3).map(|(bi, _, _)| format!("{} ({})", c.recs[sp.items[*bi].rec].name, where_text(&a.rows[sp.items[*bi].row].loc))).collect())
                        .unwrap_or_default();
                    let more = it.best.as_ref().map(|b| b.ahead.saturating_sub(by.len())).unwrap_or(0);
                    let mut by_txt = by.join("; ");
                    if more > 0 {
                        by_txt.push_str(&format!("  +{more} more"));
                    }
                    ui.label(RichText::new(by_txt).font(FontId::proportional(11.0)).color(TEXT_2));
                    /* Spare proves an item is beaten and then says nothing about what to do with
                     * it. Same rule and same words as the Sky cleanout list: a merchant is the
                     * wrong place for anything another player can take. */
                    match v.trade {
                        "" => {
                            ui.label(RichText::new("not known").font(FontId::proportional(11.0)).color(TEXT_3)).on_hover_text("no item page on the wiki, so nothing here knows whether it is NO DROP");
                        }
                        "no drop" | "no trade" => {
                            ui.label(RichText::new("destroy").font(FontId::proportional(11.0)).color(TEXT)).on_hover_text(format!("{}: nobody else can take it, so destroying it is what frees the slot", if v.trade == "no drop" { "NO DROP" } else { "No Trade" }));
                        }
                        _ => {
                            ui.label(RichText::new("sell to a player").font(FontId::proportional(11.0)).color(TEXT)).on_hover_text("not NO DROP, so another player can take it");
                        }
                    }
                    mono_right(ui, v.wt.map(|w| format!("{w}")).unwrap_or_default());
                    let trio = v.rank.as_ref().and_then(|b| b.trio.first());
                    let any = v.rank.as_ref().and_then(|b| b.best.as_ref());
                    Self::rank_cell(ui, trio, v, false);
                    let echo = matches!((trio, any), (Some(t), Some(x)) if t.cls == x.cls && t.slot == x.slot);
                    Self::rank_cell(ui, any, v, echo);
                    Self::copies_cell(ui, a, r);
                    ui.end_row();
                }
            });
        });
    }
}

/// The no-dump state, one place for both the Inventory and the Gear screen: what to type in game,
/// where this looked, and what went wrong. Every path printed is one that was actually tried.
pub fn no_dump_ui(ui: &mut Ui, dump: &DumpCache) {
    ui.label(
        RichText::new("No inventory dump found.")
            .font(FontId::proportional(12.5))
            .color(TEXT),
    );
    ui.label(
        RichText::new("Type /outputfile inventory in game. The client writes <Character>_<server>-Inventory.txt into the EverQuest folder, the parent of the Logs folder. The dump only includes the Dragon's Hoard and the depot while their windows are open.")
            .font(FontId::proportional(11.0))
            .color(TEXT_3),
    );
    if dump.checked().is_empty() {
        if dump.tried().is_empty() {
            ui.label(RichText::new("No log folder is set in Settings and this platform has no usual place to look, so there was nowhere to look for the dump.").font(FontId::proportional(11.0)).color(TEXT_3));
        } else {
            ui.label(
                RichText::new(
                    "No log folder is set in Settings, and none of the usual places exist:",
                )
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
            );
            for p in dump.tried() {
                ui.label(
                    RichText::new(p.display().to_string())
                        .font(FontId::monospace(11.0))
                        .color(TEXT_2),
                );
            }
        }
    } else {
        ui.label(
            RichText::new("Looked in:")
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
        for p in dump.checked() {
            ui.label(
                RichText::new(p.display().to_string())
                    .font(FontId::monospace(11.0))
                    .color(TEXT_2),
            );
        }
    }
    if let Some(p) = dump.problem() {
        ui.label(
            RichText::new(p)
                .font(FontId::proportional(11.0))
                .color(WRONG),
        );
    }
    if let Some(p) = dump.ingest_problem() {
        ui.label(
            RichText::new(format!("ingest: {p}"))
                .font(FontId::monospace(11.0))
                .color(TEXT_3),
        );
    }
}

/* ------------------------------------------------------------------ tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    /// Six lines, as the client writes them: the header, an empty worn slot, a paired slot with
    /// two items (one at +2), a bag, and a stack inside the bag.
    const SIX: &str = "Location\tName\tID\tCount\tSlots\r\nCharm\tEmpty\t0\t0\t0\r\nEar\tGolden Earring +2\t11111\t1\t0\r\nEar\tSilver Earring\t22222\t1\t0\r\nGeneral1\tBackpack\t17969\t1\t8\r\nGeneral1-Slot1\tSteel Fletch\t12345\t292\t0\r\n";

    #[test]
    fn parse_rows_reads_the_six_line_fixture() {
        let rows = parse_rows(SIX, false);
        assert_eq!(
            rows.len(),
            4,
            "the header and the Empty placeholder are dropped"
        );
        let ear = &rows[0];
        assert_eq!(
            (
                ear.loc.as_str(),
                ear.name.as_str(),
                ear.base.as_str(),
                ear.tier,
                ear.item_id,
                ear.count
            ),
            ("Ear", "Golden Earring +2", "Golden Earring", 2, 11111, 1)
        );
        assert_eq!(ear.sec, Section::Worn);
        assert!(!ear.sub && !ear.exalt);
        let fletch = &rows[3];
        assert_eq!(
            (
                fletch.loc.as_str(),
                fletch.root.as_str(),
                fletch.count,
                fletch.item_id
            ),
            ("General1-Slot1", "General1", 292, 12345)
        );
        assert!(fletch.sub);
        assert_eq!(fletch.sec, Section::Bags);
        assert_eq!(rows[2].sec, Section::Bags);
        assert_eq!(
            rows.iter().map(|r| r.idx).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        /* kept placeholders are flagged and carry nothing */
        let kept = parse_rows(SIX, true);
        assert_eq!(kept.len(), 5);
        assert!(
            kept[0].empty && kept[0].count == 0 && kept[0].item_id == 0 && kept[0].loc == "Charm"
        );
    }

    #[test]
    fn both_header_rows_are_skipped_by_shape_and_storage_rows_have_three_columns() {
        let text = "Location\tName\tID\tCount\tSlots\nEquipment\tOld Helm\t555\nKeyRing\tName\tID\nKeyRing\tKey of Beasts\t777\nAugmentation\tShining Robe (Exaltation)\t888\nBank1\tName\t0\t1\t0\n";
        let rows = parse_rows(text, false);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Old Helm",
                "Key of Beasts",
                "Shining Robe (Exaltation)",
                "Name"
            ],
            "a row NAMED Name with a numeric ID column is an item, not a header"
        );
        assert_eq!(rows[0].count, 1, "a three-column storage row counts as one");
        assert_eq!(rows[0].sec, Section::Storage);
        assert_eq!(rows[1].sec, Section::KeyRing);
        assert!(rows[2].exalt);
        assert_eq!(rows[2].base, "Shining Robe");
        assert_eq!(rows[2].sec, Section::Exalts);
        assert_eq!(rows[3].sec, Section::Bank);
        /* junk: too few columns, a blank name, a non-numeric id, a zero count */
        let junk = parse_rows(
            "one\ttwo\nBank2\t\t1\t1\nGeneral 3\tThing\tabc\t0\t0\n",
            false,
        );
        assert_eq!(junk.len(), 1);
        assert_eq!((junk[0].item_id, junk[0].count), (0, 1));
    }

    #[test]
    fn base_name_caps_the_tier_and_peels_stars() {
        assert_eq!(
            base_name("Giant Snake Fang +4"),
            ("Giant Snake Fang".into(), 4)
        );
        assert_eq!(base_name("Odd Item +99*"), ("Odd Item".into(), 10));
        assert_eq!(base_name("Plain*"), ("Plain".into(), 0));
        assert_eq!(
            base_name("Not+2"),
            ("Not+2".into(), 0),
            "the tier needs the space before the plus"
        );
        assert_eq!(base_name("Trailing +"), ("Trailing +".into(), 0));
    }

    #[test]
    fn parse_inventory_keeps_worn_roots_only_and_folds_finger_and_ring() {
        /* the worn set is `equipped_of` over the parsed rows, which is exactly
         * the path the screen takes with the ingest's rows (`rows_from_ingest`); the test goes
         * through the same two calls rather than a wrapper nothing else called. */
        let parse_inventory = |text: &str| equipped_of(&parse_rows(text, false));
        let eq = parse_inventory(SIX);
        assert_eq!(eq.get("Ear").len(), 2);
        assert_eq!(
            eq.get("Ear")[0],
            EqEntry {
                name: "Golden Earring +2".into(),
                base: "Golden Earring".into(),
                tier: 2,
                rec: None
            }
        );
        assert!(eq.get("Charm").is_empty());
        assert_eq!(eq.count(), 2, "the bag and its contents are not worn");
        let more = parse_inventory("Location\tName\tID\tCount\tSlots\nRing\tA\t1\t1\t0\nFinger\tB\t2\t1\t0\nHead\tC\t3\t1\t0\nHead-Slot7\tC Stone (Exaltation)\t4\t1\t0\nAny Slot\tD\t5\t1\t0\n");
        assert_eq!(
            more.get("Fingers")
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B"]
        );
        assert_eq!(
            more.get("Head").len(),
            1,
            "a -SlotN stone is not the worn item"
        );
        assert_eq!(more.get(gear::ANY).len(), 1);
    }

    #[test]
    fn the_ingest_parser_and_this_one_read_the_same_rows() {
        /* D7: the ingest lane parses the dump on its own thread and this screen consumes its rows
         * as they are. Both lanes read the same columns; if either lane's reading
         * of a column drifts (a header skipped by name instead of shape, a tier left on the base
         * name, a three-column storage row counted as zero), this is the test that says so. */
        let text = format!(
            "{SIX}Equipment\tOld Helm\t555\r\nKeyRing\tName\tID\r\nKeyRing\tKey of Beasts\t777\r\nAugmentation\tShining Robe (Exaltation)\t888\r\nHead\tShining Robe +3\t888\t1\t0\r\nHead-Slot7\tShining Robe (Exaltation)\t888\t1\t0\r\nBank 3-Slot2\tGiant Snake Fang +4*\t999\t1\t0\r\nGeneral 2\tEmpty\t0\t0\t0\r\n"
        );
        let d = crate::ingest::InventoryDump::parse(
            Path::new("Stoic_eqlegends-Inventory.txt"),
            &text,
            None,
        );
        let mine = parse_rows(&text, false);
        /* SIX's four, plus six: the KeyRing header and the Empty row are the two skipped */
        assert_eq!(mine.len(), 10);
        assert_eq!(rows_from_ingest(&d), mine);
        /* and the worn set off either is the same set */
        let eq = equipped_of(&rows_from_ingest(&d));
        assert_eq!(eq.get("Ear").len(), 2);
        assert_eq!(
            eq.get("Head")
                .iter()
                .map(|e| (e.base.as_str(), e.tier))
                .collect::<Vec<_>>(),
            vec![("Shining Robe", 3)],
            "the socketed stone is not the worn item"
        );
        assert_eq!(eq.count(), 3);
    }

    #[test]
    fn locations_chain_and_classify_by_their_root() {
        assert_eq!(root_loc("General 8-Slot6-Slot7"), "General 8");
        assert_eq!(root_loc("Head"), "Head");
        assert!(is_sub("Bank1-Slot3"));
        assert!(!is_sub("Bank1-Slot"));
        assert!(!is_sub("Bank1"));
        assert_eq!(loc_section("General 1"), Section::Bags);
        assert_eq!(loc_section("General12"), Section::Bags);
        assert_eq!(loc_section("Held"), Section::Bags);
        assert_eq!(loc_section("Bank 3"), Section::Bank);
        assert_eq!(loc_section("Bank"), Section::Other, "Bank needs a number");
        assert_eq!(loc_section("SharedBank"), Section::Shared);
        assert_eq!(loc_section("SharedBank 2"), Section::Shared);
        assert_eq!(loc_section("Personal-Depot 4"), Section::Depot);
        assert_eq!(loc_section("Dragons Hoard 1"), Section::Hoard);
        assert_eq!(loc_section("KeyRing"), Section::KeyRing);
        assert_eq!(loc_section("Equipment"), Section::Storage);
        assert_eq!(loc_section("Activated"), Section::Storage);
        assert_eq!(loc_section("Augmentation"), Section::Exalts);
        assert_eq!(loc_section("Any Slot"), Section::Worn);
        assert_eq!(loc_section("Ring"), Section::Worn);
        assert_eq!(loc_section("Cursor"), Section::Other);
        assert_eq!(loc_section("Generally"), Section::Other);
    }

    #[test]
    fn where_text_gives_bags_and_bank_numbers_and_sockets_their_host() {
        assert_eq!(where_text("General 8-Slot6"), "Bag 8 · 6");
        assert_eq!(where_text("General8"), "Bag 8");
        assert_eq!(where_text("Bank 3-Slot2"), "Bank 3 · 2");
        assert_eq!(where_text("Head-Slot7"), "Head · socket 7");
        assert_eq!(where_text("General 8-Slot6-Slot7"), "Bag 8 · 6 · socket 7");
        assert_eq!(where_text("SharedBank1-Slot2"), "shared bank");
        assert_eq!(where_text("Augmentation"), "exaltation");
        assert_eq!(where_text("Equipment"), "storage");
        assert_eq!(where_text("Dragons Hoard 1"), "hoard");
        assert_eq!(where_text("Ear"), "Ear");
        assert_eq!(where_text("Cursor"), "cursor");
        assert_eq!(where_text("Oddity"), "oddity");
    }

    #[test]
    fn character_comes_off_the_file_name() {
        assert_eq!(character_of("Stoic_eqlegends-Inventory.txt"), "Stoic");
        assert_eq!(character_of("NoUnderscore-Inventory.txt"), "");
        assert_eq!(character_of("_odd-Inventory.txt"), "");
        assert!(looks_like_inventory_dump(Path::new(
            "C:/EQ/Stoic_eqlegends-Inventory.txt"
        )));
        assert!(looks_like_inventory_dump(Path::new(
            "stoic_x-INVENTORY.TXT"
        )));
        assert!(!looks_like_inventory_dump(Path::new(
            "eqlog_Stoic_eqlegends.txt"
        )));
        assert!(!looks_like_inventory_dump(Path::new(
            "Stoic_eqlegends-Achievements.txt"
        )));
    }

    #[test]
    fn merge_landing_is_the_upgrades_page_arithmetic() {
        assert_eq!(exp_of_tier(0), 0);
        assert_eq!(exp_of_tier(3), 7);
        assert_eq!(donation(2), 4);
        /* two +2 copies make a +3 */
        let l = merge_landing(2, &[2]);
        assert_eq!(
            (l.exp, l.tier, l.rises, l.toward, l.next_cost),
            (7, 3, true, 0, 8)
        );
        /* a +0 merged into a +6 is still +6 with 1 of the 64 exp to +7 */
        let l = merge_landing(6, &[0]);
        assert_eq!(
            (l.exp, l.tier, l.rises, l.toward, l.next_cost),
            (64, 6, false, 1, 64)
        );
        /* at the cap nothing rises and nothing is owed */
        let l = merge_landing(10, &[9, 9]);
        assert_eq!((l.tier, l.rises, l.toward, l.next_cost), (10, false, 0, 0));
    }

    fn row(idx: usize, loc: &str, name: &str, id: u64, count: u32) -> Row {
        let (base, tier) = base_name(name);
        Row {
            idx,
            loc: loc.into(),
            root: root_loc(loc),
            sec: loc_section(&root_loc(loc)),
            name: name.into(),
            base,
            tier,
            exalt: name.ends_with("(Exaltation)"),
            empty: false,
            sub: is_sub(loc),
            item_id: id,
            count,
        }
    }

    #[test]
    fn copies_find_stacks_worn_pairs_and_the_host_by_tier() {
        let rows = vec![
            row(0, "General1-Slot1", "Steel Fletch", 1, 292),
            row(1, "General2-Slot3", "Steel Fletch", 1, 68),
            row(2, "Bank1", "Steel Fletch", 1, 1000),
            row(3, "Ear", "Gold Hoop", 2, 1),
            row(4, "Ear", "Gold Hoop", 2, 1),
            row(5, "Head", "Iron Helm +5", 3, 1),
            row(6, "General3-Slot2", "Iron Helm +9", 3, 1),
            row(7, "Bank4-Slot1", "Iron Helm +2", 3, 1),
            row(8, "General4", "Cyclops Skull", 0, 1),
            row(9, "Bank2", "Cyclops Skull", 0, 1),
            row(10, "Augmentation", "Iron Helm (Exaltation)", 3, 1),
            row(11, "General5", "Fancy Hat (Ornamentation)*", 4, 1),
            row(12, "General6", "Fancy Hat (Ornamentation)", 4, 1),
            row(13, "General7", "Backpack", 9, 1),
            row(14, "General7-Slot1", "Pebble", 8, 1),
            row(15, "Bank3", "Backpack", 9, 1),
            row(16, "Bank3-Slot1", "Pebble", 8, 1),
        ];
        let cps = copies(
            &rows,
            |r| is_container(&rows, r),
            |r| r.tier > 0 || r.name == "Gold Hoop",
        );
        let by = |i: usize| &cps.groups[cps.by_row[&i]];
        /* the stack split across slots: the biggest stack is the lowest the cap can be */
        let f = by(0);
        assert!(f.stack);
        assert_eq!((f.total, f.cap, f.slots_min, f.saves), (1360, 1000, 2, 1));
        assert!(copies_todo(f));
        assert_eq!(copies_action(f, &rows, 0), "frees 1 slot");
        /* both Ears worn: a pair with no action */
        let e = by(3);
        assert!(e.worn_pair);
        assert!(e.donors.is_empty());
        assert!(!copies_todo(e));
        assert_eq!(copies_action(e, &rows, 3), "both worn");
        /* the +9 in a bag is the host; the worn +5 and the +2 merge into it */
        let h = by(5);
        assert_eq!(h.host, 6);
        assert_eq!(h.donors, vec![5, 7]);
        assert!(h.gear);
        let l = h.landing.as_ref().unwrap();
        assert_eq!((l.exp, l.tier, l.rises), (511 + 32 + 4, 9, false));
        /* the hover prints the landing arithmetic: exp, tier, and the distance to the next tier */
        assert_eq!(
            h.arithmetic(&rows).as_deref(),
            Some("merged: 547 exp, +9 from +9; 36 of the 512 exp toward +10")
        );
        assert_eq!(
            f.arithmetic(&rows).as_deref(),
            Some("1360 in 3 slots; packed to the biggest stack held (1000) that is 2 slots")
        );
        assert_eq!(copies_action(h, &rows, 6), "merge 2 in, frees 2 slots");
        assert_eq!(copies_action(h, &rows, 5), "into Bag 3 · 2");
        assert_eq!(copies_action(h, &rows, 7), "into Bag 3 · 2");
        assert!(copies_todo(h));
        /* not gear: places, not a merge */
        let s = by(8);
        assert!(!s.gear);
        assert_eq!((s.slots_min, s.saves), (2, 0));
        assert!(s.landing.is_none());
        assert_eq!(copies_action(s, &rows, 8), "2 places");
        assert!(!copies_todo(s));
        /* stones, ornaments and containers are never copies of anything; their contents are */
        assert!(!cps.by_row.contains_key(&10));
        assert!(!cps.by_row.contains_key(&11));
        assert!(!cps.by_row.contains_key(&13));
        assert!(!cps.by_row.contains_key(&15));
        assert!(cps.by_row.contains_key(&14) && cps.by_row.contains_key(&16));
        assert!(is_container(&rows, &rows[13]));
        assert!(!is_container(&rows, &rows[14]));
    }

    #[test]
    fn a_worn_copy_donates_to_a_better_copy_in_a_bag_and_rises() {
        let rows = vec![
            row(0, "Fingers", "Band +2", 7, 1),
            row(1, "General1", "Band +2", 7, 1),
        ];
        let cps = copies(&rows, |_| false, |_| true);
        let g = &cps.groups[0];
        assert_eq!(g.host, 0, "same tier: the worn copy hosts");
        assert_eq!(g.donors, vec![1]);
        let l = g.landing.as_ref().unwrap();
        assert!(l.rises);
        assert_eq!(l.tier, 3);
        assert_eq!(copies_action(g, &rows, 0), "merge 1 in, lands +3");
        assert_eq!(copies_action(g, &rows, 1), "into Fingers, lands +3");
        /* a capped host gains nothing */
        let rows2 = vec![
            row(0, "Head", "Crown +10", 5, 1),
            row(1, "Bank1", "Crown +3", 5, 1),
        ];
        let cps2 = copies(&rows2, |_| false, |_| true);
        assert_eq!(copies_action(&cps2.groups[0], &rows2, 0), "already +10");
        assert!(!copies_todo(&cps2.groups[0]));
    }

    #[test]
    fn dump_discovery_picks_the_newest_and_names_where_it_looked() {
        let dir = std::env::temp_dir().join(format!("grimoire-inv-test-{}", std::process::id()));
        let logs = dir.join("Logs");
        std::fs::create_dir_all(&logs).unwrap();
        let old = dir.join("Stoic_eqlegends-Inventory.txt");
        let newer = logs.join("Stoic_eqlegends-Inventory.txt");
        let not = dir.join("eqlog_Stoic_eqlegends.txt");
        std::fs::write(&old, "Location\tName\tID\tCount\tSlots\n").unwrap();
        std::fs::write(&not, "x").unwrap();
        std::fs::write(
            &newer,
            "Location\tName\tID\tCount\tSlots\nHead\tHat\t1\t1\t0\n",
        )
        .unwrap();
        let t = SystemTime::now() + Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&newer)
            .unwrap()
            .set_modified(t)
            .unwrap();
        let dirs = candidate_dirs(Some(&logs));
        assert_eq!(
            dirs,
            vec![dir.clone(), logs.clone()],
            "the EverQuest folder first, then the Logs folder"
        );
        let (found, readable) = scan_dirs(&dirs);
        assert!(readable);
        assert_eq!(found.map(|(p, _)| p), Some(newer.clone()));
        let (none, unreadable) = scan_dirs(&[dir.join("does-not-exist")]);
        assert!(none.is_none() && !unreadable);
        assert!(candidate_dirs(None).is_empty());
        assert!(newest_dump(std::iter::empty()).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
