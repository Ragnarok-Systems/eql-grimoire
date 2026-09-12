//! Plane of Sky. Decision D5 group SKY, decision D9: the poSky checklist, the island drop board
//! and the key ladder, as a data model and a set of rules.
//!
//! WHAT MEASURES WHAT. The log says what arrived and what was handed over, the inventory dump says
//! what is in your bags right now, and sky.json says what each test wants. Every rule below is one
//! of those three read carefully, and every one carries a test in `tests` that fails without it.
//!
//! WHAT THIS FILE OWNS, in order:
//!   1. the log grammar (`Rx`) and the fold over it (`Stream`, `Ledger`)
//!   2. the inventory read (`parse_dump_text`, `held_of_rows`) and "where is it" (`loc_badge`)
//!   3. what you hold, on whose word (`log_held`, `log_floor`, `held`, `reconcile`)
//!   4. the poSky data model (`SkyData`) and the rules over it (`completions`, `needs_of`,
//!      `test_state`, `boss_board`, `sky_index`, `disposal`, `reward_proves`, `cleanout_rows`)
//!   5. the arranged model both surfaces read (`build_model`)
//!   6. the screen (`SkyScreen`)
//!
//! THREE DECISIONS RESOLVED AGAINST THE GOAL DOC, because the owner is asleep:
//!
//! The poSky data comes from `cx.data.sky` (D6), converted once into this module's own shapes by
//! `SkyData::from_snapshot`. That is the ONE place the data lane's field names are spelled here,
//! so a rename there is a one-function fix and no rule moves. The screen says which root the
//! snapshot was read from, because the snapshot is what it read.
//!
//! The achievement record (`/outputfile achievements`) and the reward ranker belong to the
//! unlocks and gear lanes. Without them this screen has two turn-in witnesses (the log, and the
//! reward in your dump) and no "first time / repeat" verdict from the record. It says so in the
//! banner instead of pretending; the skip recommendation (which needs the ranker) is not built,
//! and the "get rid of" view keys on the skip mark alone.
//!
//! The nav's SKY group (D5) has three rows and this file is one screen with four views. The
//! integrator maps `poSky checklist` to `View::Giver`, `Islands` to `View::Boss` (the island
//! drop board IS the island list) and `Keys` to `View::Keys` (the key ladder) through
//! `SkyScreen::set_view`. The Keys ladder is sky.json's own `req` and
//! `key` per isle, crossed with what the dump and the log say you hold, and nothing more.

use crate::ingest::{read_appended, read_tail_capped, SourceKind, Tail, TAIL_CAP};
/* The marks file takes the settings file's guard whole rather than a second copy of it that can
 * drift from it: see `Marks::write_file`. */
use crate::settings::OnUnreadable;
use crate::theme::*;
use chrono::{DateTime, NaiveDateTime, Utc};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/* ================================================================== 1. the log grammar ==
 * Six lines carry everything the board needs, plus the five the quest tracker
 * reads off the same fold. The tail after "corpse" on a loot line decides whether the item ever
 * reached your bags; "You offered" proves nothing on its own and is held pending until
 * "You complete the trade with X" names the same NPC. */

/* The day is `[ \d]\d` and not `\d{2}`: the client space pads a single digit day ("Aug  3"), so a
 * two-digit-only rule silently drops every line written on the first nine days of a month.
 * `crate::ingest`'s grammar reads the day the same way: one client, one spelling. */
const TS: &str = r"^\[(\w{3} \w{3} [ \d]\d \d{2}:\d{2}:\d{2} \d{4})\] ";

struct Rx {
    loot: Regex,
    offer: Regex,
    back: Regex,
    done: Regex,
    gone: Regex,
    zone: Regex,
    vendor: Regex,
    bought: Regex,
    made: Regex,
    given: Regex,
    parcel: Regex,
    cancel: Regex,
}

fn rx() -> &'static Rx {
    static RX: OnceLock<Rx> = OnceLock::new();
    RX.get_or_init(|| {
        let r = |tail: &str| Regex::new(&format!("{TS}{tail}")).expect("sky grammar regex");
        Rx {
            /* Both loot phrasings in one, with everything after "corpse" kept. */
            loot: r(r"(?:--)?You (?:have )?looted (?:an? |([\d,]+) )?(.+?) from .+?'s corpse(.*)$"),
            offer: r(r"You offered ([\d,]+) (.+?) to (.+?)\.$"),
            back: r(r"(.+?) says, 'I have no need for this, (.+?)\. You can have it back\.'$"),
            done: r(r"You complete the trade with (.+?)\.$"),
            /* Destroying an item is the one disposal with an exact quantity on it. */
            gone: r(r"You successfully destroyed ([\d,]+) (.+?)\.$"),
            zone: r(r"You have entered (.+?)\.$"),
            /* A merchant sale you made yourself. No quantity on the line, so it counts
             * transactions, not items. */
            vendor: r(r"You receive (.+?) from .+? for the (.+?)\(s\)\.$"),
            bought: r(r"You purchased ([\d,]+) (.+?) from (.+?) for +(.*)\.$"),
            made: r(r"You have fashioned the items together to create something new: (.+?)\.$"),
            given: r(r"(.+?) has offered you ([\d,]+) (.+?)\.$"),
            parcel: r(r".+? hands you the (.+?) that was sent from .+?\.$"),
            /* one trade window at a time, so a cancel empties everything pending */
            cancel: r(r"You have cancelled the trade\.$"),
        }
    })
}

/// "You offered 500 Platinum to Franchise." Coin rides the trade lines and is not an item.
fn is_coin(n: &str) -> bool {
    ["platinum", "gold", "silver", "copper"]
        .iter()
        .any(|c| n.eq_ignore_ascii_case(c))
}

/// A LOOT LINE IS NOT A STATEMENT THAT YOU KEPT ANYTHING, and more often than not it is the
/// opposite. What the line's tail says happened is what decides: a sale tail means the item went
/// straight to a merchant without ever passing through your bags, and a combine tail means it was
/// consumed on the spot making something else. Counting either as loot credits you with a holding
/// you never held.
///
/// THE TAILS ARE THE COMMON CASE, NOT AN EDGE ONE. Over the two real logs tracked in this
/// repository (`web/fixtures/eqlog-tail-200k.txt` and
/// `crates/grimoire-desktop/tests/fixtures/princess-night.txt`) there are 64 loot lines: 29 carry
/// a sale tail and 9 a combine tail, leaving 26 that were actually kept. A reader that ignored
/// the tail would be wrong more often than it was right.
pub fn kept_it(tail: &str) -> bool {
    !(tail.contains(" and sold it for ")
        || tail.contains(" to create a ")
        || tail.contains(" to create an "))
}

/// The autosell tail, including "for free".
pub fn was_sold(tail: &str) -> bool {
    tail.contains(" and sold it for ")
}

/// A printed quantity: "1,200" is 1200, and nothing (or nonsense, or zero) is 1.
pub fn qty(s: Option<&str>) -> u32 {
    s.and_then(|s| s.replace(',', "").parse::<u32>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1)
}

/// The base name under the client's decorations: the dump stars some items ("Backpack*") and the
/// log names items at their upgrade tier ("Efreeti Magi Staff +1"). Sky components and rewards
/// are tracked by base name, or a tier-suffixed name in the log would never match the test that
/// wants it. Returns the base and the tier, capped at 10.
pub fn base_name(name: &str) -> (String, u8) {
    static TIER: OnceLock<Regex> = OnceLock::new();
    let tier = TIER.get_or_init(|| Regex::new(r"\s\+(\d+)$").expect("tier regex"));
    let plain = name.trim_end_matches('*');
    match tier.captures(plain) {
        Some(m) => {
            let t: u32 = m[1].parse().unwrap_or(0);
            (
                plain[..m.get(0).map(|g| g.start()).unwrap_or(plain.len())]
                    .trim()
                    .to_owned(),
                t.min(10) as u8,
            )
        }
        None => (plain.trim().to_owned(), 0),
    }
}

pub fn base_of(name: &str) -> String {
    base_name(name).0
}

/// A wind rune is currency rather than an item: the client keeps runes on its currency tab, and
/// an `/outputfile inventory` dump carries no currency rows, so a rune never shows up in one
/// however many you are holding. The log is the only witness for a rune, and a dump's silence
/// about one must never be read as a count of zero.
///
/// NOT CONFIRMED AGAINST A DUMP HERE: no inventory dump is tracked in this repository, so this
/// rests on the client's behaviour rather than on a file a reader can open. If it is ever wrong,
/// the symptom is a rune count that ignores a dump that did have the row, which is the safe
/// direction to be wrong in.
pub fn is_rune(name: &str) -> bool {
    name.starts_with("Wind Rune")
}

/// The identity key every lookup here runs on. Lower case, apostrophes and
/// backticks gone, whitespace collapsed. NOT article stripping: 'Sapphire' and 'A Sapphire' are
/// two items the wiki keeps apart on purpose.
pub fn item_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for ch in s.chars() {
        if ch == '\'' || ch == '`' {
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
        out.extend(ch.to_lowercase());
    }
    out
}

/// A log timestamp, "Tue Aug 11 20:15:03 2026", as the client writes it (local time).
///
/// The weekday is dropped before parsing. chrono checks a weekday against the date, so a stamp
/// whose weekday disagrees
/// would otherwise become "unparsable" and its arrival silently uncounted. The weekday carries no
/// information the date does not.
pub fn log_ts(ts: &str) -> Option<NaiveDateTime> {
    let rest = ts.split_once(' ').map(|(_, r)| r).unwrap_or(ts);
    NaiveDateTime::parse_from_str(rest.trim(), "%b %e %H:%M:%S %Y").ok()
}

/* ------------------------------------------------------------------- the ledger -- */

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trade {
    pub npc: String,
    /// base name -> quantity handed over in this one closed trade
    pub items: BTreeMap<String, u32>,
    pub ts: String,
}

/// Something that reached your hands and printed a line: a merchant buy, a combine, a player
/// trade, a parcel, or (kept) loot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Arrival {
    pub n: String,
    pub q: u32,
    pub from: String,
    pub ts: String,
    /// "loot" | "buy" | "combine" | "trade" | "parcel"
    pub via: &'static str,
}

/// One fold over log lines; the page hands it a file, the app hands it
/// the lines that landed in the last second, and nothing here looks ahead, so the two produce the
/// same ledger for the same bytes.
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    /// base name -> times looted AND kept
    pub looted: BTreeMap<String, u32>,
    /// base name -> times autosold on pickup
    pub sold: BTreeMap<String, u32>,
    /// base name -> merchant sales you made yourself (transactions, not items)
    pub vendored: BTreeMap<String, u32>,
    /// base name -> destroyed by hand
    pub destroyed: BTreeMap<String, u32>,
    /// base name -> times handed over in a closed trade
    pub delivered: BTreeMap<String, u32>,
    pub trades: Vec<Trade>,
    pub bought: Vec<Arrival>,
    pub made: Vec<Arrival>,
    pub received: Vec<Arrival>,
    /// Kept loot with its timestamp, so loot that landed after the dump was written is counted
    /// once and only once.
    pub looted_at: Vec<Arrival>,
    pub first: Option<String>,
    pub last: Option<String>,
    pub saw_sky: bool,
    /// Lines this ledger has actually consumed. A second of combat spam moves nothing here.
    pub n: usize,
}

impl Ledger {
    fn stamp(&mut self, ts: &str) {
        if self.first.is_none() {
            self.first = Some(ts.to_owned());
        }
        self.last = Some(ts.to_owned());
    }
}

fn bump(m: &mut BTreeMap<String, u32>, k: &str, v: u32) {
    *m.entry(k.to_owned()).or_insert(0) += v;
}

/// `me` is the character in the log's filename: the handback line names
/// YOU and prints for every other player handing in at the same NPC too.
#[derive(Clone, Debug, Default)]
pub struct Stream {
    me: String,
    pub led: Ledger,
    /// npc -> offered but not yet closed
    pending: HashMap<String, Vec<(String, u32)>>,
    /// player -> offered TO you, not yet closed
    incoming: HashMap<String, Vec<(String, u32)>>,
    /// npc -> items refused since the offer opened
    handback: HashMap<String, u32>,
}

impl Stream {
    pub fn new(me: &str) -> Stream {
        Stream {
            me: me.to_owned(),
            ..Default::default()
        }
    }

    /// One line. Returns whether the grammar consumed it.
    pub fn line(&mut self, ln: &str) -> bool {
        if !ln.starts_with('[') {
            return false; // every real line starts '['
        }
        let took = self.consume(ln);
        if took {
            self.led.n += 1;
        }
        took
    }

    fn consume(&mut self, ln: &str) -> bool {
        let rx = rx();
        if let Some(m) = rx.loot.captures(ln) {
            let tail = m.get(4).map(|g| g.as_str()).unwrap_or("");
            let name = base_of(&m[3]);
            let q = qty(m.get(2).map(|g| g.as_str()));
            if kept_it(tail) {
                bump(&mut self.led.looted, &name, q);
                self.led.looted_at.push(Arrival {
                    n: name,
                    q,
                    from: String::new(),
                    ts: m[1].to_owned(),
                    via: "loot",
                });
            } else if was_sold(tail) {
                bump(&mut self.led.sold, &name, q);
            }
            self.led.stamp(&m[1]);
            return true;
        }
        if let Some(m) = rx.offer.captures(ln) {
            if !is_coin(&m[3]) {
                self.pending
                    .entry(m[4].to_owned())
                    .or_default()
                    .push((base_of(&m[3]), qty(Some(&m[2]))));
            }
            self.led.stamp(&m[1]);
            return true;
        }
        if let Some(m) = rx.given.captures(ln) {
            if !is_coin(&m[4]) {
                self.incoming
                    .entry(m[2].to_owned())
                    .or_default()
                    .push((base_of(&m[4]), qty(Some(&m[3]))));
            }
            self.led.stamp(&m[1]);
            return true;
        }
        if let Some(m) = rx.parcel.captures(ln) {
            self.led.received.push(Arrival {
                n: base_of(&m[2]),
                q: 1,
                from: String::new(),
                ts: m[1].to_owned(),
                via: "parcel",
            });
            self.led.stamp(&m[1]);
            return true;
        }
        if let Some(m) = rx.cancel.captures(ln) {
            self.pending.clear();
            self.incoming.clear();
            self.handback.clear();
            self.led.stamp(&m[1]);
            return true;
        }
        if let Some(m) = rx.back.captures(ln) {
            /* Only yours, and only against an offer that is actually open: the same NPC refusing
             * someone else's pieces prints an identical line. */
            let npc = &m[2];
            if (self.me.is_empty() || &m[3] == self.me.as_str()) && self.pending.contains_key(npc) {
                *self.handback.entry(npc.to_owned()).or_insert(0) += 1;
            }
            return true;
        }
        if let Some(m) = rx.done.captures(ln) {
            let npc = m[2].to_owned();
            let held = self.pending.remove(&npc);
            let refused = self.handback.remove(&npc).unwrap_or(0);
            /* One refusal per item, so a refusal for every offer line is the whole trade coming
             * back: nothing was handed over and no test advanced. A partial refusal has no line
             * naming WHICH item bounced, so the rest is counted as delivered. */
            if let Some(held) = held.filter(|h| !h.is_empty() && refused < h.len() as u32) {
                let mut items = BTreeMap::new();
                for (n, q) in &held {
                    bump(&mut items, n, *q);
                }
                for (k, v) in &items {
                    bump(&mut self.led.delivered, k, *v);
                }
                self.led.trades.push(Trade {
                    npc: npc.clone(),
                    items,
                    ts: m[1].to_owned(),
                });
            }
            /* the other side of a player trade: what they put in is yours now */
            for (n, q) in self.incoming.remove(&npc).unwrap_or_default() {
                self.led.received.push(Arrival {
                    n,
                    q,
                    from: npc.clone(),
                    ts: m[1].to_owned(),
                    via: "trade",
                });
            }
            self.led.stamp(&m[1]);
            return true;
        }
        if let Some(m) = rx.gone.captures(ln) {
            bump(&mut self.led.destroyed, &base_of(&m[3]), qty(Some(&m[2])));
            self.led.stamp(&m[1]);
            return true;
        }
        if let Some(m) = rx.vendor.captures(ln) {
            bump(&mut self.led.vendored, &base_of(&m[3]), 1);
            self.led.stamp(&m[1]);
            return true;
        }
        if let Some(m) = rx.bought.captures(ln) {
            self.led.bought.push(Arrival {
                n: base_of(&m[3]),
                q: qty(Some(&m[2])),
                from: m[4].to_owned(),
                ts: m[1].to_owned(),
                via: "buy",
            });
            self.led.stamp(&m[1]);
            return true;
        }
        if let Some(m) = rx.made.captures(ln) {
            self.led.made.push(Arrival {
                n: base_of(&m[2]),
                q: 1,
                from: String::new(),
                ts: m[1].to_owned(),
                via: "combine",
            });
            self.led.stamp(&m[1]);
            return true;
        }
        if let Some(m) = rx.zone.captures(ln) {
            /* The game says "The Plane of Sky"; the wiki page is "Plane of Sky". */
            let z = &m[2];
            if z.strip_prefix("The ").unwrap_or(z) == "Plane of Sky" {
                self.led.saw_sky = true;
            }
            self.led.stamp(&m[1]);
            return true;
        }
        false
    }

    pub fn feed<'a>(&mut self, lines: impl IntoIterator<Item = &'a str>) -> &Ledger {
        for ln in lines {
            self.line(ln);
        }
        &self.led
    }

    pub fn text(&mut self, t: &str) -> &Ledger {
        for ln in t.split('\n') {
            self.line(ln.trim_end_matches('\r'));
        }
        &self.led
    }
}

/* ================================================================ 2. the inventory read ==
 * The dump is tab separated, CRLF, with "Empty"
 * placeholder rows and TWO header rows (the KeyRing table repeats Location/Name/ID), both skipped
 * by shape. STORAGE rows are three columns where the rest are five. */

/// One non-empty row of `/outputfile inventory`, at the granularity this screen needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DumpRow {
    pub loc: String,
    pub name: String,
    pub count: u32,
}

/// The dump's rows for the tests, through the ONE parser the app has: the ingest's
/// `InventoryDump::parse` (D7) and this screen's `dump_rows` over it.
/// This screen once carried a second reading of the same columns for its tests alone; two
/// parsers of one file is how a Sources count and a Sky count come to disagree, so the tests now
/// go through the production path.
#[cfg(test)]
pub fn parse_dump_text(text: &str) -> Vec<DumpRow> {
    dump_rows(&crate::ingest::InventoryDump::parse(
        std::path::Path::new("Test_server-Inventory.txt"),
        text,
        None,
    ))
}

fn worn_rx() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(Charm|Ear|Head|Face|Neck|Shoulders|Arms|Back|Wrist|Range|Hands|Primary|Secondary|Fingers?|Ring|Chest|Legs|Feet|Waist|Ammo|Power Source|Any Slot)$")
            .expect("worn regex")
    })
}

/// The root of a location chain: "General 8-Slot6-Slot7" is a thing inside a thing inside bag 8.
/// The root decides which place you walk to.
pub fn root_loc(loc: &str) -> &str {
    let mut s = loc;
    loop {
        let Some(i) = s.rfind("-Slot") else { return s };
        if s[i + 5..].chars().all(|c| c.is_ascii_digit()) && s.len() > i + 5 {
            s = &s[..i];
        } else {
            return s;
        }
    }
}

fn is_sub(loc: &str) -> bool {
    root_loc(loc) != loc
}

/// The sections, in the dump's own walking order.
pub const SECTIONS: [(&str, &str); 10] = [
    ("worn", "Worn"),
    ("bags", "Bags"),
    ("storage", "Equipment storage"),
    ("exalts", "Exaltations"),
    ("bank", "Bank"),
    ("shared", "Shared bank"),
    ("depot", "Depot"),
    ("hoard", "Dragon's hoard"),
    ("keyring", "Key ring"),
    ("other", "Elsewhere"),
];

/// Which section a root location is. The client writes "General 1" WITH a space and "Bank1"
/// without, so every matcher takes an optional one. Anything nothing matches lands in "other".
pub fn loc_section(loc: &str) -> &'static str {
    static R: OnceLock<[Regex; 8]> = OnceLock::new();
    let r = R.get_or_init(|| {
        let rx = |s: &str| Regex::new(s).expect("section regex");
        [
            rx(r"^(General ?\d+|Held)$"),
            rx(r"^(Equipment|Activated)$"),
            rx(r"^Augmentation$"),
            rx(r"^Bank ?\d+$"),
            rx(r"^SharedBank ?\d*$"),
            rx(r"^Personal-Depot"),
            rx(r"(?i)^Dragon"),
            rx(r"^KeyRing"),
        ]
    });
    let root = root_loc(loc);
    if worn_rx().is_match(root) {
        return "worn";
    }
    for (i, key) in [
        "bags", "storage", "exalts", "bank", "shared", "depot", "hoard", "keyring",
    ]
    .iter()
    .enumerate()
    {
        if r[i].is_match(root) {
            return key;
        }
    }
    "other"
}

pub fn section_index(sec: &str) -> usize {
    SECTIONS.iter().position(|(k, _)| *k == sec).unwrap_or(99)
}

/// Where a row is, as data. Bags and bank get a container number and a slot number
/// because that is what gets your hand on the item; everywhere else a number means nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Badge {
    Bag { n: u32, sub: Option<u32> },
    Bank { n: u32, sub: Option<u32> },
    Word(String),
}

impl Badge {
    pub fn text(&self) -> String {
        match self {
            Badge::Bag { n, sub } => match sub {
                Some(s) => format!("bag {n}\u{00b7}{s}"),
                None => format!("bag {n}"),
            },
            Badge::Bank { n, sub } => match sub {
                Some(s) => format!("bank {n}\u{00b7}{s}"),
                None => format!("bank {n}"),
            },
            Badge::Word(w) => w.clone(),
        }
    }
}

pub fn loc_badge(loc: &str) -> Badge {
    static R: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (bag, bank) = R.get_or_init(|| {
        (
            Regex::new(r"^General ?(\d+)(?:-Slot(\d+))?").expect("bag regex"),
            Regex::new(r"^Bank ?(\d+)(?:-Slot(\d+))?").expect("bank regex"),
        )
    });
    let num = |m: &regex::Captures, i: usize| m.get(i).and_then(|g| g.as_str().parse::<u32>().ok());
    if let Some(m) = bag.captures(loc) {
        return Badge::Bag {
            n: num(&m, 1).unwrap_or(0),
            sub: num(&m, 2),
        };
    }
    if let Some(m) = bank.captures(loc) {
        return Badge::Bank {
            n: num(&m, 1).unwrap_or(0),
            sub: num(&m, 2),
        };
    }
    let words: [(&str, &str); 8] = [
        ("SharedBank", "shared bank"),
        ("Personal-Depot", "depot"),
        ("Dragon", "hoard"),
        ("KeyRing", "key ring"),
        ("Cursor", "cursor"),
        ("Equipment", "storage"),
        ("Activated", "activated"),
        ("Augmentation", "exaltation"),
    ];
    for (prefix, word) in words {
        let hit = if prefix == "Dragon" {
            loc.to_ascii_lowercase().starts_with("dragon")
        } else {
            loc.starts_with(prefix)
        };
        if hit {
            return Badge::Word(word.to_owned());
        }
    }
    if worn_rx().is_match(root_loc(loc)) {
        return Badge::Word("worn".to_owned());
    }
    Badge::Word(loc.to_lowercase())
}

/// One place the dump found copies of a thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub loc: String,
    pub sec: &'static str,
    pub count: u32,
    pub tier: u8,
}

/// One entry of the dump read: base name -> what the dump says, locations kept rather than
/// collapsed. "You hold 1" and "it is in bank slot 12" are the same question asked twice, and the
/// second one is what gets you off the island.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeldEntry {
    pub n: u32,
    pub tier: u8,
    pub worn: u32,
    pub places: Vec<Place>,
}

#[derive(Clone, Debug, Default)]
pub struct Held {
    /// item_key(base name) -> entry
    pub items: HashMap<String, HeldEntry>,
    /// Exaltation stones by the item they were rendered from. A stone is named after an item you
    /// no longer own, so it is never a held copy; it is counted here so the log floor can take it
    /// off (one stone is one copy the log still counts).
    pub stones: HashMap<String, u32>,
    pub rows: usize,
}

/// The dump read whole. Every place the dump can hold a thing, not just
/// the worn slots; exaltation stones excluded; the upgrade tier kept because the dump is the only
/// place it is written.
pub fn held_of_rows(rows: &[DumpRow]) -> Held {
    let mut held = Held {
        rows: rows.len(),
        ..Default::default()
    };
    for r in rows {
        let stripped = r
            .name
            .trim_end()
            .strip_suffix("(Exaltation)")
            .map(str::trim_end);
        if let Some(inner) = stripped {
            *held.stones.entry(item_key(&base_of(inner))).or_insert(0) += 1;
            continue;
        }
        let (base, tier) = base_name(&r.name);
        let e = held.items.entry(item_key(&base)).or_default();
        e.n += r.count;
        e.tier = e.tier.max(tier);
        /* What you are wearing is not what is crowding your bank. An aug in a -SlotN row is not
         * the worn item itself, so it is not counted as worn. */
        if !is_sub(&r.loc) && worn_rx().is_match(root_loc(&r.loc)) {
            e.worn += r.count;
        }
        e.places.push(Place {
            loc: r.loc.clone(),
            sec: loc_section(&r.loc),
            count: r.count,
            tier,
        });
    }
    held
}

/* ================================================================== 3. what you hold ==
 * From the log alone: looted, minus handed over, minus destroyed. The dump, when given, is the
 * authority for everything except wind runes, which it cannot see. */

fn list_count(arr: &[Arrival], name: &str) -> u32 {
    arr.iter().filter(|e| e.n == name).map(|e| e.q.max(1)).sum()
}

pub fn sold_count(led: &Ledger, name: &str) -> u32 {
    led.sold.get(name).copied().unwrap_or(0)
}
pub fn vendor_count(led: &Ledger, name: &str) -> u32 {
    led.vendored.get(name).copied().unwrap_or(0)
}
pub fn gone_count(led: &Ledger, name: &str) -> u32 {
    led.destroyed.get(name).copied().unwrap_or(0)
}

/// Every way a thing arrives that prints a line minus every way it leaves
/// that prints one. Exits that print nothing leave this HIGH, so a caller floors on it and never
/// replaces the dump with it.
pub fn log_held(led: &Ledger, name: &str) -> u32 {
    let arrived = led.looted.get(name).copied().unwrap_or(0)
        + list_count(&led.bought, name)
        + list_count(&led.made, name)
        + list_count(&led.received, name);
    let left = led.delivered.get(name).copied().unwrap_or(0) + gone_count(led, name);
    arrived.saturating_sub(left)
}

/// The number a dump holder may safely floor on.
///
/// Selling to a merchant by hand prints a line that names the item but never how many of it went,
/// so a single hand sale permanently destroys the log's ability to count that item: the
/// arithmetic can still say how many arrived and can no longer say how many left. The honest
/// floor in that case is 0, and 0 is what this returns. A guess would be worse than nothing,
/// because the caller uses this number to fill in a row the dump shows as empty, so a guessed
/// floor does not read as a guess on screen. It reads as a holding.
pub fn log_floor(led: &Ledger, name: &str) -> u32 {
    if vendor_count(led, name) > 0 {
        0
    } else {
        log_held(led, name)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeldAnswer {
    pub n: u32,
    /// "log" | "inv"
    pub src: &'static str,
    pub tier: u8,
    pub worn: u32,
    pub places: Vec<Place>,
}

/// The held count: the dump's word where it has one, the log's where the dump shows none (the
/// storage export drops rows) or where the dump cannot see at all (runes).
pub fn held(led: &Ledger, inv: Option<&Held>, name: &str) -> HeldAnswer {
    let from_log = log_held(led, name);
    let Some(inv) = inv else {
        return HeldAnswer {
            n: from_log,
            src: "log",
            ..Default::default()
        };
    };
    if is_rune(name) {
        return HeldAnswer {
            n: from_log,
            src: "log",
            ..Default::default()
        };
    }
    let e = inv.items.get(&item_key(name));
    let n = e.map(|e| e.n).unwrap_or(0);
    if n == 0 {
        let f = log_floor(led, name);
        if f > 0 {
            return HeldAnswer {
                n: f,
                src: "log",
                ..Default::default()
            };
        }
    }
    HeldAnswer {
        n,
        src: "inv",
        tier: e.map(|e| e.tier).unwrap_or(0),
        worn: e.map(|e| e.worn).unwrap_or(0),
        places: e.map(|e| e.places.clone()).unwrap_or_default(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconciled {
    pub log: u32,
    pub inv: Option<u32>,
    /// inv minus log. Neither number is the "right" one; a caller shows both when non-zero.
    pub gap: i64,
}

/// The two witnesses side by side. Wind runes have no dump number at all.
pub fn reconcile(led: &Ledger, inv: Option<&Held>, name: &str) -> Reconciled {
    let log = log_held(led, name);
    match inv {
        Some(inv) if !is_rune(name) => {
            let n = inv.items.get(&item_key(name)).map(|e| e.n).unwrap_or(0);
            Reconciled {
                log,
                inv: Some(n),
                gap: n as i64 - log as i64,
            }
        }
        _ => Reconciled {
            log,
            inv: None,
            gap: 0,
        },
    }
}

/* ================================================================ 4. the poSky data ==
 * sky.json schema 2, as the data lane typed it (`crate::data::Sky`), measured from the real
 * file: classes (16, code -> giver + tests), isles (9, with mob rosters and drop tables), islands
 * ("1".."8" and "1.5" -> island name), items (3057 item windows), names (lower cased name ->
 * items key). The rules below run on these shapes and nothing else, so the snapshot's own
 * field names appear in exactly one function, `SkyData::from_snapshot`. */

#[derive(Clone, Debug, Default)]
pub struct SkyData {
    pub classes: BTreeMap<String, Class>,
    pub isles: Vec<Isle>,
    /// "1" -> "Fairy Island" ... "1.5" -> "Noble Island"
    pub islands: BTreeMap<String, String>,
    /// the file's own key (lower cased name) -> the item window
    pub items: HashMap<String, ItemRec>,
    /// lower cased display name -> items key, the scrape's alias table
    pub names: HashMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct Class {
    pub giver: String,
    pub tests: Vec<Test>,
}

#[derive(Clone, Debug, Default)]
pub struct Test {
    pub n: String,
    pub say: String,
    pub reward: String,
    pub rune: Vec<String>,
    pub items: Vec<Comp>,
}

#[derive(Clone, Debug, Default)]
pub struct Comp {
    pub n: String,
    pub isl: String,
    pub mob: String,
    pub nodrop: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Isle {
    pub isl: String,
    pub name: String,
    pub req: Vec<String>,
    pub key: Vec<String>,
    pub mobs: Vec<Mob>,
}

#[derive(Clone, Debug, Default)]
pub struct Mob {
    pub n: String,
    pub role: String,
    pub drops: Vec<String>,
}

/// One item window. `fl` carries the flags; `window` says there was a window to read at all
/// (an item with neither a flags line nor a stat block has no window to read). Measured on
/// the committed file: neither array is ever present and empty (0 of 3057 either way), so
/// "present" and "non-empty" are the same fact and the snapshot's `Vec` loses nothing.
#[derive(Clone, Debug, Default)]
pub struct ItemRec {
    pub fl: Vec<String>,
    pub window: bool,
    pub wt: f64,
}

impl ItemRec {
    pub fn weight(&self) -> f64 {
        self.wt
    }
    pub fn flag(&self, f: &str) -> bool {
        self.fl.iter().any(|x| x == f)
    }
}

pub const SKY_CLASS: [(&str, &str); 16] = [
    ("BRD", "Bard"),
    ("BST", "Beastlord"),
    ("BER", "Berserker"),
    ("CLR", "Cleric"),
    ("DRU", "Druid"),
    ("ENC", "Enchanter"),
    ("MAG", "Magician"),
    ("MNK", "Monk"),
    ("NEC", "Necromancer"),
    ("PAL", "Paladin"),
    ("RNG", "Ranger"),
    ("ROG", "Rogue"),
    ("SHD", "Shadow Knight"),
    ("SHM", "Shaman"),
    ("WAR", "Warrior"),
    ("WIZ", "Wizard"),
];

pub fn class_name(code: &str) -> &str {
    SKY_CLASS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| *n)
        .unwrap_or(code)
}

impl SkyData {
    /// From the snapshot the data lane loaded (D6). The one place its field names are spelled.
    /// `isl` and `mob` are Options there and plain strings here because every rule reads "no
    /// island" as the empty string, the way the file itself spells it.
    pub fn from_snapshot(s: &crate::data::Sky) -> SkyData {
        SkyData {
            classes: s
                .classes
                .iter()
                .map(|(code, c)| {
                    let tests = c
                        .tests
                        .iter()
                        .map(|t| Test {
                            n: t.name.clone(),
                            say: t.say.clone(),
                            reward: t.reward.clone(),
                            rune: t.rune.clone(),
                            items: t.items.iter().map(comp_of).collect(),
                        })
                        .collect();
                    (
                        code.clone(),
                        Class {
                            giver: c.giver.clone(),
                            tests,
                        },
                    )
                })
                .collect(),
            isles: s
                .isles
                .iter()
                .map(|i| Isle {
                    isl: i.isl.clone(),
                    name: i.name.clone(),
                    req: i.req.clone(),
                    key: i.key.clone(),
                    mobs: i
                        .mobs
                        .iter()
                        .map(|m| Mob {
                            n: m.name.clone(),
                            role: m.role.clone(),
                            drops: m.drops.clone(),
                        })
                        .collect(),
                })
                .collect(),
            islands: s
                .islands
                .iter()
                .map(|i| (i.id.clone(), i.name.clone()))
                .collect(),
            items: s
                .items
                .iter()
                .map(|it| (it.key.clone(), item_rec(it)))
                .collect(),
            names: s
                .names
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    /// The item window for a display name, via the `names` index. Same lookup the old
    /// app's `skyRec` ran.
    pub fn rec(&self, name: &str) -> Option<&ItemRec> {
        let lower = name.to_lowercase();
        if let Some(k) = self.names.get(&lower) {
            if let Some(r) = self.items.get(k) {
                return Some(r);
            }
        }
        self.items.get(&lower)
    }

    /// The isle as a person names it: "Drake Island (I7)", or "Isle 7" when the map does not
    /// name it.
    pub fn isle_name(&self, isl: &str) -> String {
        match self.islands.get(isl) {
            Some(nm) => format!("{nm} (I{isl})"),
            None => format!("Isle {isl}"),
        }
    }

    pub fn test_count(&self) -> usize {
        self.classes.values().map(|c| c.tests.len()).sum()
    }
}

/// One snapshot item window to the shape the disposal rules read.
fn item_rec(it: &crate::data::sky::SkyItem) -> ItemRec {
    ItemRec {
        fl: it.fl.clone(),
        window: !it.fl.is_empty() || !it.sb.is_empty(),
        wt: it.wt.unwrap_or(0.0),
    }
}

/// One snapshot test component to the shape the rules read. None reads as the empty string.
fn comp_of(i: &crate::data::sky::SkyTestItem) -> Comp {
    Comp {
        n: i.name.clone(),
        isl: i.isl.clone().unwrap_or_default(),
        mob: i.mob.clone().unwrap_or_default(),
        nodrop: i.nodrop,
    }
}

/* ---------------------------------------------------------------- the key ladder -- */

/// One key an island takes, and what is known about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyNeed {
    pub n: String,
    /// The isle whose own `key` list names it as dropping there, per sky.json. None when no
    /// island in the file says so (Key of Swords, Veeshan's Key on the measured file): the
    /// ladder then says "not on any island's drop list here" rather than guessing a source.
    pub from: Option<String>,
    pub have: u32,
    /// "inv" | "log" | "mark", on whose word `have` is
    pub src: &'static str,
}

/// One rung: an island, what it takes to stand on it, and what it drops for the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyStep {
    pub isl: String,
    pub name: String,
    pub req: Vec<KeyNeed>,
    /// sky.json `key`: the keys that drop HERE
    pub drops: Vec<String>,
}

impl KeyStep {
    /// Every key it takes is in hand, or it takes none.
    pub fn open(&self) -> bool {
        self.req.iter().all(|k| k.have >= 1)
    }
}

/// The island ladder, in file order (island order on the measured file). `have(name)` is the
/// host's held count and its source: keys sit on the dump's KeyRing table, in a bag or in the
/// bank, and the log's loot line is the other witness. Nothing else in this file knows a key
/// from a component, so the isles' own `req` and `key` lists are the whole rule.
pub fn key_ladder(data: &SkyData, have: &dyn Fn(&str) -> (u32, &'static str)) -> Vec<KeyStep> {
    data.isles
        .iter()
        .map(|isle| KeyStep {
            isl: isle.isl.clone(),
            name: isle.name.clone(),
            req: isle
                .req
                .iter()
                .map(|k| {
                    let (have, src) = have(k);
                    KeyNeed {
                        n: k.clone(),
                        from: data
                            .isles
                            .iter()
                            .find(|i| i.key.iter().any(|d| d == k))
                            .map(|i| i.isl.clone()),
                        have,
                        src,
                    }
                })
                .collect(),
            drops: isle.key.clone(),
        })
        .collect()
}

/* ---------------------------------------------------------------- which tests are done -- */

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completions {
    pub by_test: BTreeMap<String, u32>,
    /// Closed trades with this giver that matched no single test: what a split turn-in looks
    /// like, reported rather than dropped.
    pub orphan: u32,
}

/// One closed trade is one turn-in. A trade counts for a test when every
/// item that test asks for appears in it; where two tests could match, the one asking for more
/// items wins.
pub fn completions(data: &SkyData, led: &Ledger, code: &str) -> Completions {
    let mut out = Completions::default();
    let Some(c) = data.classes.get(code).filter(|c| !c.giver.is_empty()) else {
        return out;
    };
    for t in &c.tests {
        out.by_test.insert(t.n.clone(), 0);
    }
    for tr in &led.trades {
        if tr.npc != c.giver {
            continue;
        }
        let mut best: Option<(&Test, usize)> = None;
        for t in &c.tests {
            let need = needs_of(t);
            if need.is_empty() {
                continue;
            }
            if need
                .iter()
                .all(|n| tr.items.get(n).copied().unwrap_or(0) >= 1)
                && best.map(|(_, k)| need.len() > k).unwrap_or(true)
            {
                best = Some((t, need.len()));
            }
        }
        match best {
            Some((t, _)) => *out.by_test.entry(t.n.clone()).or_insert(0) += 1,
            None => out.orphan += 1,
        }
    }
    out
}

/// Everything one test consumes, its wind rune plus its components, one each.
pub fn needs_of(t: &Test) -> Vec<String> {
    t.rune
        .iter()
        .cloned()
        .chain(t.items.iter().map(|i| i.n.clone()))
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestState {
    pub done: u32,
    pub skip: bool,
    pub finished: bool,
    pub ready: bool,
    pub missing: Vec<String>,
}

/// The state of one test. `have(name)` is the host's held count; `skip` is a want, not a fact,
/// and suppresses ready.
pub fn test_state(t: &Test, done: u32, skip: bool, have: &dyn Fn(&str) -> u32) -> TestState {
    let missing: Vec<String> = needs_of(t).into_iter().filter(|n| have(n) < 1).collect();
    TestState {
        done,
        skip,
        finished: done > 0,
        ready: missing.is_empty() && done == 0 && !skip,
        missing,
    }
}

/* ---------------------------------------------------------------- the boss board -- */

/// One fold of the board. The isle it sits on carries its own id, name, `req` and `key` in
/// `IsleFold`, and is not copied onto every fold under it.
#[derive(Clone, Debug, Default)]
pub struct MobFold<R> {
    pub n: String,
    pub role: String,
    pub from: Vec<String>,
    /// drop -> which commons dropped it (the "Island trash" fold only)
    pub src: BTreeMap<String, Vec<String>>,
    pub rows: Vec<R>,
}

#[derive(Clone, Debug, Default)]
pub struct IsleFold<R> {
    pub isl: String,
    pub name: String,
    pub req: Vec<String>,
    pub key: Vec<String>,
    pub mobs: Vec<MobFold<R>>,
}

/// The board: a fold per NAMED mob, and ONE fold per island for everything else on it.
/// Forty-eight folds is not a board you read with a corpse on the floor. `row_for(name, mob)` is
/// the host's.
pub fn boss_board<R>(data: &SkyData, row_for: &dyn Fn(&str, &str) -> R) -> Vec<IsleFold<R>> {
    data.isles
        .iter()
        .map(|isle| {
            let mk = |n: &str,
                      role: &str,
                      drops: &[String],
                      from: Vec<String>,
                      src: BTreeMap<String, Vec<String>>| MobFold {
                n: n.to_owned(),
                role: role.to_owned(),
                from,
                src,
                rows: drops.iter().map(|d| row_for(d, n)).collect(),
            };
            let mut out = Vec::new();
            let mut drops: Vec<String> = Vec::new();
            let mut from = Vec::new();
            let mut src: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for m in &isle.mobs {
                if m.role == "common" || m.role == "trash" {
                    if !m.drops.is_empty() {
                        from.push(m.n.clone());
                    }
                    for n in &m.drops {
                        if !src.contains_key(n) {
                            drops.push(n.clone());
                        }
                        src.entry(n.clone()).or_default().push(m.n.clone());
                    }
                } else {
                    out.push(mk(
                        &m.n,
                        &m.role,
                        &m.drops,
                        vec![m.n.clone()],
                        BTreeMap::new(),
                    ));
                }
            }
            if !drops.is_empty() {
                out.push(mk("Island trash", "trash", &drops, from, src));
            }
            IsleFold {
                isl: isle.isl.clone(),
                name: isle.name.clone(),
                req: isle.req.clone(),
                key: isle.key.clone(),
                mobs: out,
            }
        })
        .collect()
}

/* ---------------------------------------------------------------- the index and disposal -- */

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Use {
    pub code: String,
    pub test: String,
    pub reward: String,
}

#[derive(Clone, Debug, Default)]
pub struct IndexEntry {
    pub n: String,
    pub isl: String,
    pub mob: String,
    /// The wiki table's NO DROP marker: None when no component row carried one.
    pub mark: Option<bool>,
    pub uses: Vec<Use>,
    pub pays: Vec<Use>,
}

/// One pass over all 95 tests. Per item name, every test that consumes it,
/// every test that pays it, where it drops, and the table's NO DROP marker.
pub fn sky_index(data: &SkyData) -> HashMap<String, IndexEntry> {
    let mut idx: HashMap<String, IndexEntry> = HashMap::new();
    for (code, c) in &data.classes {
        for t in &c.tests {
            for i in &t.items {
                let e = idx.entry(i.n.clone()).or_insert_with(|| IndexEntry {
                    n: i.n.clone(),
                    ..Default::default()
                });
                if e.isl.is_empty() && !i.isl.is_empty() {
                    e.isl = i.isl.clone();
                    e.mob = i.mob.clone();
                }
                e.mark = Some(e.mark.unwrap_or(false) || i.nodrop);
                /* a test asking for two of one piece is still one test */
                if !e.uses.iter().any(|u| u.code == *code && u.test == t.n) {
                    e.uses.push(Use {
                        code: code.clone(),
                        test: t.n.clone(),
                        reward: t.reward.clone(),
                    });
                }
            }
            if !t.reward.is_empty() {
                idx.entry(t.reward.clone())
                    .or_insert_with(|| IndexEntry {
                        n: t.reward.clone(),
                        ..Default::default()
                    })
                    .pays
                    .push(Use {
                        code: code.clone(),
                        test: t.n.clone(),
                        reward: t.reward.clone(),
                    });
            }
        }
    }
    idx
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disposal {
    /// Some(true): another player can take it. Some(false): destroy is what frees the slot.
    /// None: nothing here knows.
    pub give: Option<bool>,
    /// "item" (the window) | "table" (the wiki's marker)
    pub src: Option<&'static str>,
    /// The window and the table disagreed. The window wins; the disagreement is stated because
    /// the difference between the answers is destroying a weapon and selling it.
    pub clash: bool,
}

fn no_give(rec: &ItemRec) -> bool {
    rec.flag("no_drop") || rec.flag("no_trade")
}

/// Can anyone else take it off your hands. The item window answers when
/// there is one; the table's marker answers for the 44 components with no item page.
pub fn disposal(rec: Option<&ItemRec>, mark: Option<bool>) -> Disposal {
    match rec.filter(|r| r.window) {
        None => Disposal {
            give: mark.map(|m| !m),
            src: mark.map(|_| "table"),
            clash: false,
        },
        Some(r) => {
            let nd = no_give(r);
            Disposal {
                give: Some(!nd),
                src: Some("item"),
                clash: mark.map(|m| m != nd).unwrap_or(false),
            }
        }
    }
}

/// Holding a NO DROP or No Trade reward means the turn-in happened,
/// because the giver is the only source. Positive only; NOT holding one says nothing.
pub fn reward_proves(rec: Option<&ItemRec>) -> bool {
    rec.filter(|r| r.window).map(no_give).unwrap_or(false)
}

/* ---------------------------------------------------------------- the cleanout list -- */

/// What the host knows about a held thing, for the cleanout list.
#[derive(Clone, Debug, Default)]
pub struct HaveFull {
    pub n: u32,
    pub worn: u32,
    pub tier: u8,
    /// None when the count is the log's word alone (no dump)
    pub places: Option<Vec<Place>>,
}

#[derive(Clone, Debug)]
pub struct CleanoutRow {
    pub n: String,
    pub loc: String,
    pub sec: String,
    pub sec_idx: usize,
    pub count: u32,
    pub wt: f64,
    pub wanted_by: usize,
    pub give: Option<bool>,
    pub give_src: Option<&'static str>,
    pub clash: bool,
    pub flag: &'static str,
    pub isl: String,
    pub mob: String,
    pub have: u32,
    pub tier: u8,
    /// (code, test) pairs you skipped, all of which wanted this piece
    pub uses: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct Cleanout {
    pub rows: Vec<CleanoutRow>,
    /// pieces a skipped test wants that some other, live test does too: held back, not listed
    pub mixed: usize,
    /// pieces with a worn copy the list left out
    pub worn: usize,
}

/// An item is on the cleanout list when you are holding it and EVERY test that
/// consumes it is one you ticked skip on. One row per PLACE. Rewards and runes are never on it.
pub fn cleanout_rows(
    data: &SkyData,
    have: &dyn Fn(&str) -> HaveFull,
    skipped: &dyn Fn(&str, &str) -> bool,
) -> Cleanout {
    let mut out = Cleanout::default();
    let mut idx: Vec<IndexEntry> = sky_index(data).into_values().collect();
    idx.sort_by(|a, b| a.n.cmp(&b.n));
    for e in idx {
        if is_rune(&e.n) || !e.pays.is_empty() || e.uses.is_empty() {
            continue;
        }
        let h = have(&e.n);
        if h.n == 0 {
            continue;
        }
        let skips: Vec<&Use> = e
            .uses
            .iter()
            .filter(|u| skipped(&u.code, &u.test))
            .collect();
        if skips.is_empty() {
            continue;
        }
        if skips.len() < e.uses.len() {
            out.mixed += 1;
            continue;
        }
        let r = data.rec(&e.n);
        let d = disposal(r, e.mark);
        let flag = match r {
            Some(r) if r.flag("no_drop") => "NO DROP",
            Some(r) if r.flag("no_trade") => "No Trade",
            _ => "",
        };
        let base = |loc: &str, sec: &str, count: u32| CleanoutRow {
            n: e.n.clone(),
            loc: loc.to_owned(),
            sec: sec.to_owned(),
            sec_idx: if sec.is_empty() {
                99
            } else {
                section_index(sec)
            },
            count,
            wt: r.map(|r| r.weight()).unwrap_or(0.0),
            wanted_by: e.uses.len(),
            give: d.give,
            give_src: d.src,
            clash: d.clash,
            flag,
            isl: e.isl.clone(),
            mob: e.mob.clone(),
            have: h.n,
            tier: h.tier,
            uses: skips
                .iter()
                .map(|u| (u.code.clone(), u.test.clone()))
                .collect(),
        };
        match &h.places {
            None => {
                /* held on the log's word alone: no dump */
                if h.n > h.worn {
                    out.rows.push(base("", "", h.n - h.worn));
                }
            }
            Some(places) => {
                let keep: Vec<&Place> = places.iter().filter(|w| w.sec != "worn").collect();
                if keep.len() < places.len() {
                    out.worn += 1;
                }
                for w in keep {
                    out.rows.push(base(&w.loc, w.sec, w.count));
                }
            }
        }
    }
    out.rows.sort_by(|a, b| {
        a.sec_idx
            .cmp(&b.sec_idx)
            .then_with(|| loc_sort_key(&a.loc).cmp(&loc_sort_key(&b.loc)))
            .then_with(|| a.n.cmp(&b.n))
    });
    out
}

/// A location with its numbers zero padded, so "Bank2" sorts before "Bank10".
fn loc_sort_key(loc: &str) -> String {
    let mut out = String::new();
    let mut digits = String::new();
    for ch in loc.chars().chain(std::iter::once('\u{0}')) {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            if !digits.is_empty() {
                out.push_str(&format!("{:0>4}", digits));
                digits.clear();
            }
            if ch != '\u{0}' {
                out.push(ch);
            }
        }
    }
    out
}

/* ================================================================== 5. the arranged model ==
 * Built once per change and handed to every view. */

/// The player's own marks: a want (skip), an ordering (track), and a claim (held). Persisted by
/// this module under the platform config dir because the settings lane owns `Settings` and its
/// file, and two lanes writing one file is a corrupt file.
///
/// EVERY MARK IN HERE WAS TYPED BY HAND, over months, and nothing in the app can recreate one.
/// `load` used to answer an unparsable file with `Marks::default()` and the words "starting
/// empty", and the next click wrote that empty set over the file. The guard against that is
/// [`Settings`](crate::settings::Settings)'s, taken whole rather than rewritten: the same
/// `OnUnreadable`, the same `unreadable_bytes`, the same `keep_a_copy`, so the two files cannot
/// drift apart in what they protect. See `write_file` below for the two halves of it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Marks {
    #[serde(default)]
    pub skipped: BTreeSet<String>,
    #[serde(default)]
    pub tracked: BTreeSet<String>,
    /// item_key -> count you told the app you hold (a rune looted before /log on, a piece on
    /// your pet). A FLOOR, never a sum.
    #[serde(default)]
    pub held: BTreeMap<String, u32>,
    /// Every key this struct does not name, kept so a file written by a LATER build and saved by
    /// this one comes back whole. `Settings::extra` is the same field for the same reason.
    ///
    /// It is also what makes the file half of the guard honest. That half stands down for a file
    /// that parses, on the premise that whatever it held is already in the `Marks` about to be
    /// written, and without this the premise would be false for every key a later build adds.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
    /// Why this session is holding no marks of the owner's, when it is. `serde(skip)`: it is a
    /// fact about this run, never a key in the file, and `load` is the only thing that sets it.
    ///
    /// THIS IS THE HALF A FILE-ONLY GUARD MISSES. A file that parses is not proof that this
    /// session read it; see `write_file`.
    #[serde(skip)]
    load_problem: Option<String>,
}

pub fn mark_key(code: &str, test: &str) -> String {
    format!("{code}::{test}")
}

/// Said by the two entries that reach the platform marks file, so a test that trips one can be
/// told apart from a guard that refused for a reason about the owner's data.
const TEST_REFUSAL: &str =
    "refused: a unit test may not write the owner's marks file; use a scratch path";
const NO_CONFIG_DIR: &str = "no config dir on this platform; marks cannot be saved";

/// Why a save did not happen, and whether starting fresh is the way out of it.
///
/// THE BOOL IS NOT DECORATION. Both refusals below tell the person to use "start fresh", and a
/// screen that says that while painting no such control is lying to them. It is the screen's
/// condition for painting the button, so the sentence and the button cannot come apart.
#[derive(Clone, Debug)]
pub struct Refused {
    pub why: String,
    /// True when marks that exist are being protected, and replacing them deliberately is the way
    /// out. False for a plain failure (no config dir, a disk error, a locked file) that starting
    /// fresh cannot help with either.
    pub start_fresh_helps: bool,
}

impl Refused {
    /// The guard held: there are marks worth protecting and the person may choose to replace them.
    fn guarded(why: String) -> Refused {
        Refused {
            why,
            start_fresh_helps: true,
        }
    }
    /// Nothing was protected, the save simply could not happen. Starting fresh would fail too.
    fn plain(why: String) -> Refused {
        Refused {
            why,
            start_fresh_helps: false,
        }
    }
}

impl Marks {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("eql-grimoire").join("sky-marks.json"))
    }
    /// Read the platform file. The `Option<String>` is the same string as `load_problem` and is
    /// what the screen paints; the field is what the guard reads. Both, so neither surface can be
    /// the one that forgot.
    pub fn load() -> (Marks, Option<String>) {
        let Some(p) = Marks::path() else {
            return Marks::absent(
                "no config dir on this platform; marks cannot be read or saved".to_owned(),
            );
        };
        Marks::load_from(&p)
    }

    /// `load` on a named path. The tests take this; nothing else may, because the owner's marks
    /// live at exactly one path and it is `Marks::path`.
    fn load_from(path: &Path) -> (Marks, Option<String>) {
        match std::fs::read(path) {
            Ok(b) => {
                /* An empty or whitespace-only file is a file with nothing in it to lose, and the
                 * same answer as no file at all. Calling it broken would refuse every save for
                 * ever over nothing. `unreadable_bytes` makes the same exemption on the write
                 * side, and the two have to agree or a save is refused for a file the loader was
                 * happy with. */
                if b.iter().all(u8::is_ascii_whitespace) {
                    return (Marks::default(), None);
                }
                match serde_json::from_slice::<Marks>(&b) {
                    Ok(m) => (m, None),
                    Err(e) => Marks::absent(format!(
                        "{}: your marks are not valid JSON, so this session is holding none of \
                         them and will NOT save over that file: {e}",
                        path.display()
                    )),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Marks::default(), None),
            /* Locked, permissions, a directory in its place: "I cannot tell what is there" is
             * the one answer that must never be followed by a write. */
            Err(e) => Marks::absent(format!(
                "{}: your marks could not be read, so this session is holding none of them and \
                 will NOT save over that file: {e}",
                path.display()
            )),
        }
    }

    /// Empty marks that know they are empty because a read failed, not because there was nothing
    /// to read. The only constructor that sets `load_problem`.
    fn absent(why: String) -> (Marks, Option<String>) {
        (
            Marks {
                load_problem: Some(why.clone()),
                ..Marks::default()
            },
            Some(why),
        )
    }

    /// The choke point. Every save of the marks this program performs is this function, which is
    /// why both halves of the guard are here and not at the call sites.
    ///
    /// HALF ONE, ABOUT SELF: refuse whenever this session never managed to READ the file, whatever
    /// is on disk now. The sequence that defeats a file-only guard is the one the error message
    /// itself recommends: the file is broken, a save refuses, the owner repairs the JSON by hand
    /// WITHOUT restarting, and the next save finds a file that parses, stands down, and writes
    /// this session's empty set over every mark in it. `load` runs once, from `SkyScreen::ui`, and
    /// nothing re-reads. A session running on nothing has nothing worth writing.
    ///
    /// HALF TWO, ABOUT THE FILE: refuse to replace a file that is there and does not parse, which
    /// covers the file a DIFFERENT process broke after this session read it cleanly.
    ///
    /// `KeepACopy` is exempt from half one for the same reason the Settings screen is: the screen
    /// has said what will happen, the person chose it, and `keep_a_copy` preserves the bytes
    /// first regardless.
    ///
    /// The write itself is a sibling temp file and a rename, so a crash or a full disk mid-write
    /// leaves the previous marks whole rather than a truncated file that will not parse next
    /// launch. The old write was a bare `std::fs::write`.
    fn write_file(
        &self,
        path: &Path,
        on_unreadable: OnUnreadable,
    ) -> Result<Option<PathBuf>, Refused> {
        if on_unreadable == OnUnreadable::Refuse {
            if let Some(why) = &self.load_problem {
                return Err(Refused::guarded(format!(
                    "{why}. RESTART the app to pick up marks you have repaired, or use \"start \
                     fresh\" below to replace them deliberately"
                )));
            }
        }
        /* BRANCH ON THE INTENT FIRST. See `settings::existing_bytes`: the refusal path asks
         * whether what is there is unreadable, the deliberate path asks only what is there, and
         * answering the second with the first is how "start fresh" replaced a repaired file
         * with no copy kept. */
        let kept = match on_unreadable {
            OnUnreadable::KeepACopy => {
                match crate::settings::existing_bytes(path, "marks").map_err(Refused::plain)? {
                    None => None,
                    Some(bytes) => Some(
                        crate::settings::keep_a_copy(path, &bytes, "marks")
                            .map_err(Refused::plain)?,
                    ),
                }
            }
            OnUnreadable::Refuse => match crate::settings::unreadable_bytes::<Marks>(path, "marks")
                .map_err(Refused::plain)?
            {
                None => None,
                Some(_bytes) => match on_unreadable {
                    OnUnreadable::Refuse => {
                        return Err(Refused::guarded(format!(
                        "{}: the marks already there could not be read, so they were NOT replaced \
                         and that file is untouched. RESTART the app after repairing its JSON by \
                         hand, or use \"start fresh\" below to replace them deliberately",
                        path.display()
                    )));
                    }
                    /* Before the temp file, not after: if the write below fails, a spare copy of a
                     * file that is still there costs nothing, and the other order costs everything. */
                    OnUnreadable::KeepACopy => unreachable!("handled above"),
                },
            },
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                Refused::plain(format!(
                    "{}: cannot create the marks folder: {e}",
                    dir.display()
                ))
            })?;
        }
        let body = serde_json::to_vec_pretty(self).map_err(|e| {
            Refused::plain(format!("{}: cannot encode the marks: {e}", path.display()))
        })?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, body).map_err(|e| {
            Refused::plain(format!("{}: cannot write the marks: {e}", tmp.display()))
        })?;
        std::fs::rename(&tmp, path).map_err(|e| {
            Refused::plain(format!("{}: cannot replace the marks: {e}", path.display()))
        })?;
        Ok(kept)
    }
    pub fn skipped(&self, code: &str, test: &str) -> bool {
        self.skipped.contains(&mark_key(code, test))
    }
    pub fn tracked(&self, code: &str, test: &str) -> bool {
        self.tracked.contains(&mark_key(code, test))
    }
}

/* --------------------------------------------------------------- one file, one owner -- */

/// Everything this process knows about the ONE marks file: the marks, whether the once-per-run
/// read has happened, where an unreadable file was kept, and whatever the last read or write left
/// to say about it. Held by [`MarksCell`], never by a screen.
#[derive(Default)]
struct MarksState {
    marks: Marks,
    /// Whether the file has been read this run. The read happens once per RUN, not once per
    /// screen, which is the same statement as "one owner" seen from the read side.
    loaded: bool,
    /// The sentence the screen paints: `Marks::load_problem` after a failed read, the `Refused`
    /// reason after a refused write.
    problem: Option<String>,
    /// Where the unreadable marks were kept when the person chose to start fresh, so the screen
    /// can show them the way back to them.
    kept: Option<PathBuf>,
    /// Whether the last save was refused to protect marks that exist, which is the case where
    /// "start fresh" is the way out and must therefore be on screen. `Refused::start_fresh_helps`.
    refused: bool,
    /// Bumped by every change to anything above. A screen rebuilds its model when this moves,
    /// which is how a mark set in one window reaches the board in the other one.
    rev: u64,
}

/// THE ONE OWNER OF THE ONE MARKS FILE, shared by every `SkyScreen` in the process.
///
/// THE DEFECT THIS EXISTS TO KILL. The main window owns a `SkyScreen` and the popped-out Sky tool
/// window owns a SECOND one (`windows::Screen::new`). While each held its own `Marks`, each read
/// the file once and each wrote its whole copy back over it, so the second save wrote a set that
/// had never heard of the first save's mark. A mark typed by hand was gone, no copy was kept, and
/// NEITHER SCREEN SAID ANYTHING, because by its own lights nothing was wrong: the file parsed,
/// both sessions had read it cleanly, and `Marks::write_file`'s guard is about a file nobody
/// could read, never about a file that changed since this session read it. No guard catches a
/// lost update; only one owner prevents it.
///
/// A MERGE WOULD NOT DO. Unioning what is on disk with what is in hand resurrects every mark the
/// person has just cleared, because an absent key and a deleted key look the same on the way in.
/// The marks are a set the person edits, so there has to be exactly one of them.
///
/// SO `Default` HANDS OUT THE ONE CELL. Every field of `SkyScreen` is private, so `Default` is
/// the only way anything outside this module can make one, and there is therefore no way for a
/// second screen to be born holding a second copy of the marks. That is the point: the wiring
/// cannot be forgotten because there is no wiring to forget.
#[derive(Clone)]
pub(crate) struct MarksCell(Arc<Mutex<MarksState>>);

impl Default for MarksCell {
    /// The process-wide cell. See the type doc: one marks file, one owner of it.
    fn default() -> MarksCell {
        static CELL: OnceLock<MarksCell> = OnceLock::new();
        CELL.get_or_init(|| MarksCell(Arc::new(Mutex::new(MarksState::default()))))
            .clone()
    }
}

impl MarksCell {
    /// A panic inside one screen must not wedge every later save out of this cell. The state is
    /// plain data and is fine to keep using, exactly as the window registry treats its own.
    fn lock(&self) -> MutexGuard<'_, MarksState> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The revision a screen's model was built from. See `MarksState::rev`.
    fn rev(&self) -> u64 {
        self.lock().rev
    }

    /// The once-per-run read. Every Sky screen calls this on every pass; the FIRST one does the
    /// read and every other one sees what it read.
    fn load_once_with(&self, read: impl FnOnce() -> (Marks, Option<String>)) {
        let mut g = self.lock();
        if g.loaded {
            return;
        }
        let (m, p) = read();
        g.marks = m;
        g.problem = p;
        g.loaded = true;
        g.rev += 1;
    }

    fn load_once(&self) {
        self.load_once_with(Marks::load);
    }

    /// What a save that never reached the file has to say, and whether starting fresh is the way
    /// out of it. Both from one call, so the sentence and the button cannot come apart.
    fn report(&self, problem: Option<String>, start_fresh_helps: bool) {
        let mut g = self.lock();
        g.problem = problem;
        g.refused = start_fresh_helps;
        g.rev += 1;
    }

    /// What a step that never reached the file has to say, leaving the "start fresh" flag
    /// exactly as it was. The two entries that refuse before touching anything take this.
    fn note(&self, problem: Option<String>) {
        let mut g = self.lock();
        g.problem = problem;
        g.rev += 1;
    }

    /// `load_once` on a named path. The tests take this; nothing else may, because the owner's
    /// marks live at exactly one path and it is `Marks::path`.
    #[cfg(test)]
    fn load_once_from(&self, path: &Path) {
        self.load_once_with(|| Marks::load_from(path));
    }

    /// A cell of one test's own, so tests cannot reach each other through the shared one.
    #[cfg(test)]
    fn isolated() -> MarksCell {
        MarksCell(Arc::new(Mutex::new(MarksState::default())))
    }

    /// Whether two handles are the same cell. The wiring check in `windows` reads this, because
    /// "both screens share the marks" is a statement about identity and nothing else proves it.
    #[cfg(test)]
    pub(crate) fn is_same(&self, other: &MarksCell) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// What you hold, keyed by item_key(base name), with the dump as the base, the
/// log's arrivals and hand-ins since the dump applied on top, the log as a FLOOR under rows the
/// dump has none of, and the held marks as a floor over everything.
#[derive(Clone, Debug, Default)]
pub struct HaveMap {
    pub counts: HashMap<String, u32>,
    /// keys whose count is the log's floor, not the dump's row
    pub log_floor: HashSet<String>,
}

fn after(ts: &str, since: Option<NaiveDateTime>) -> bool {
    match log_ts(ts) {
        None => false,
        Some(t) => since.map(|s| t > s).unwrap_or(true),
    }
}

pub fn have_map(
    led: &Ledger,
    inv: Option<&Held>,
    since: Option<NaiveDateTime>,
    marks: &Marks,
) -> HaveMap {
    let mut m: HashMap<String, u32> = HashMap::new();
    let mut floor = HashSet::new();
    let add = |m: &mut HashMap<String, u32>, name: &str, q: u32| {
        *m.entry(item_key(name)).or_insert(0) += q
    };
    if let Some(inv) = inv {
        for (k, e) in &inv.items {
            *m.entry(k.clone()).or_insert(0) += e.n;
        }
        /* since the dump: loot, buys, combines and receipts arrive, closed hand-ins leave */
        for e in led.looted_at.iter().chain(&led.bought).chain(&led.received) {
            if after(&e.ts, since) {
                add(&mut m, &e.n, e.q.max(1));
            }
        }
        for e in &led.made {
            if after(&e.ts, since) {
                add(&mut m, &e.n, 1);
            }
        }
        for t in &led.trades {
            if !after(&t.ts, since) {
                continue;
            }
            for (n, q) in &t.items {
                if let Some(v) = m.get_mut(&item_key(n)) {
                    *v = v.saturating_sub(*q);
                }
            }
        }
        /* The log is a FLOOR under the dump, over the whole log, and fills only a row the dump
         * has NONE of. A render into an exaltation leaves a stone wearing the item's name, and
         * each stone is one copy the log still counts. */
        let mut seen = HashSet::new();
        let names = led
            .looted
            .keys()
            .cloned()
            .chain(led.bought.iter().map(|e| e.n.clone()))
            .chain(led.made.iter().map(|e| e.n.clone()))
            .chain(led.received.iter().map(|e| e.n.clone()));
        for name in names {
            if !seen.insert(name.clone()) {
                continue;
            }
            let k = item_key(&name);
            if m.get(&k).copied().unwrap_or(0) > 0 {
                continue;
            }
            let stones = inv.stones.get(&k).copied().unwrap_or(0);
            let f = log_floor(led, &name).saturating_sub(stones);
            if f > 0 {
                m.insert(k.clone(), f);
                floor.insert(k);
            }
        }
    }
    for (k, c) in &marks.held {
        let v = m.entry(k.clone()).or_insert(0);
        *v = (*v).max(*c);
    }
    HaveMap {
        counts: m,
        log_floor: floor,
    }
}

/// Everything the arranged model measures against.
pub struct Witness<'a> {
    pub data: &'a SkyData,
    pub led: &'a Ledger,
    pub inv: Option<&'a Held>,
    pub have: &'a HaveMap,
    pub marks: &'a Marks,
}

impl Witness<'_> {
    /// How many of a piece you hold, and on whose word ("inv" | "log" | "mark").
    pub fn sky_have(&self, name: &str) -> (u32, &'static str) {
        let key = item_key(name);
        let mark = self.marks.held.get(&key).copied();
        if self.inv.is_none() || is_rune(name) {
            let n = log_held(self.led, name);
            return match mark {
                /* WHOSE NUMBER IS IT. `n.max(c)` takes the larger of the log's count and your
                 * held mark, so the log's word covers what is on screen only while the log alone
                 * accounts for it. A mark ABOVE the log is your typing, and calling that the
                 * log's word puts the game's witness on a number the game never saw. */
                Some(c) => (n.max(c), if n > 0 && n >= c { "log" } else { "mark" }),
                None => (n, "log"),
            };
        }
        let seen = self
            .inv
            .map(|i| i.items.contains_key(&key))
            .unwrap_or(false);
        let n = self.have.counts.get(&key).copied().unwrap_or(0);
        let src = if !seen && mark.is_some() {
            "mark"
        } else if !seen && self.have.log_floor.contains(&key) {
            "log"
        } else {
            "inv"
        };
        (n, src)
    }

    fn places(&self, name: &str) -> Option<&HeldEntry> {
        self.inv.and_then(|i| i.items.get(&item_key(name)))
    }

    pub fn have_full(&self, name: &str) -> HaveFull {
        let (n, _) = self.sky_have(name);
        match self.places(name) {
            Some(e) => HaveFull {
                n,
                worn: e.worn,
                tier: e.tier,
                places: Some(e.places.clone()),
            },
            None => HaveFull {
                n,
                worn: 0,
                tier: 0,
                places: if self.inv.is_some() {
                    Some(Vec::new())
                } else {
                    None
                },
            },
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PieceModel {
    pub n: String,
    pub need: u32,
    pub have: u32,
    pub src: &'static str,
    pub isl: String,
    pub mob: String,
    pub nodrop: bool,
    pub req: Vec<String>,
    pub sold: u32,
    pub vend: u32,
    pub gone: u32,
    /// other tests that also want this piece ("Warrior: Test of Smash")
    pub star_with: Vec<String>,
    pub places: Vec<Place>,
}

#[derive(Clone, Debug, Default)]
pub struct RuneModel {
    pub n: String,
    pub have: u32,
    /// Whose word `have` is on, the same three values [`PieceModel::src`] carries and for the
    /// same reason: "log" when the log watched it arrive, "mark" when the only witness is a held
    /// mark you ticked yourself. THE ROW CANNOT WORK THIS OUT. `build_model` threw the second
    /// half of `sky_have` away here (`have: w.sky_have(r).0`) and the row named the log outright,
    /// so a rune held on your own word was reported as the game's.
    pub src: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct TestModel {
    pub full: String,
    pub short: String,
    pub say: String,
    pub reward: String,
    pub items: Vec<PieceModel>,
    pub rune: Vec<RuneModel>,
    /// this log watched you hand it over, N times
    pub done: u32,
    /// the reward is in your dump and this giver is the only source
    pub rew: bool,
    pub fin: bool,
    pub ready: bool,
    pub missing: usize,
    pub held: usize,
    pub skip: bool,
    pub track: bool,
    pub need: usize,
    pub reward_tier: Option<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct ClassModel {
    pub code: String,
    pub name: String,
    pub giver: String,
    pub tests: Vec<TestModel>,
    pub n_done: usize,
    pub n_ready: usize,
    pub n_repeat: usize,
    pub n_partial: usize,
    pub n_skip: usize,
    pub n_left: usize,
    pub n_held: usize,
    pub orphan: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Totals {
    pub tests: usize,
    pub done: usize,
    pub ready: usize,
    pub repeat: usize,
    pub partial: usize,
    pub pieces: u32,
    pub held: usize,
}

#[derive(Clone, Debug, Default)]
pub struct DropUse {
    pub cls: String,
    pub short: String,
    pub reward: String,
    pub fin: bool,
    pub skip: bool,
    pub track: bool,
}

/// One row of the boss board. `have` is measured; `need` is how many MORE this object owes.
#[derive(Clone, Debug, Default)]
pub struct DropRow {
    pub n: String,
    pub have: u32,
    pub uses: Vec<DropUse>,
    pub open: usize,
    pub quest: bool,
    pub need: u32,
    pub track: bool,
    pub places: Vec<Place>,
    pub sold: u32,
    pub vend: u32,
    pub gone: u32,
}

#[derive(Clone, Debug, Default)]
pub struct MobModel {
    pub fold: MobFold<DropRow>,
    pub need: u32,
    pub track: usize,
    pub nq: usize,
}

#[derive(Clone, Debug, Default)]
pub struct IsleModel {
    pub isl: String,
    pub name: String,
    pub req: Vec<String>,
    pub key: Vec<String>,
    pub mobs: Vec<MobModel>,
}

#[derive(Clone, Debug, Default)]
pub struct SkyModel {
    pub classes: Vec<ClassModel>,
    pub tot: Totals,
    pub isles: Vec<IsleModel>,
    pub cleanout: Cleanout,
    /// distinct pieces still to loot across the board (skyTotalNeed)
    pub to_loot: u32,
    /// the island ladder, keys counted the same way as every other piece
    pub keys: Vec<KeyStep>,
}

/// Every component the wiki puts in more than one test. Runes excluded
/// on purpose: every rune feeds several tests, so starring them would star the whole column.
fn sky_uses(data: &SkyData) -> HashMap<String, Vec<(String, String)>> {
    let mut m: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (code, c) in &data.classes {
        for t in &c.tests {
            for i in &t.items {
                m.entry(item_key(&i.n))
                    .or_default()
                    .push((code.clone(), t.n.clone()));
            }
        }
    }
    m
}

fn short_test(code: &str, n: &str) -> String {
    let prefix = format!("{} ", class_name(code));
    n.strip_prefix(prefix.as_str()).unwrap_or(n).to_owned()
}

/// The checklist, the boss board and the drop board, in one build.
pub fn build_model(w: &Witness) -> SkyModel {
    let data = w.data;
    let uses = sky_uses(data);
    let isle_of: HashMap<&str, &Isle> = data.isles.iter().map(|i| (i.isl.as_str(), i)).collect();
    let mut tot = Totals::default();
    let mut held_names: HashSet<String> = HashSet::new();
    let mut classes = Vec::new();
    let mut comp_by_code: HashMap<String, Completions> = HashMap::new();

    let mut codes: Vec<&String> = data.classes.keys().collect();
    codes.sort_by_key(|c| class_name(c));
    for code in codes {
        let c = &data.classes[code];
        let comp = completions(data, w.led, code);
        let tests: Vec<TestModel> = c
            .tests
            .iter()
            .map(|t| {
                let items: Vec<PieceModel> = t
                    .items
                    .iter()
                    .map(|i| {
                        let (have, src) = w.sky_have(&i.n);
                        let others: Vec<String> = uses
                            .get(&item_key(&i.n))
                            .map(|v| {
                                v.iter()
                                    .filter(|(_, tn)| *tn != t.n)
                                    .map(|(cd, tn)| {
                                        format!("{}: {}", class_name(cd), short_test(cd, tn))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        PieceModel {
                            n: i.n.clone(),
                            need: 1,
                            have,
                            src,
                            isl: i.isl.clone(),
                            mob: i.mob.clone(),
                            nodrop: i.nodrop,
                            req: isle_of
                                .get(i.isl.as_str())
                                .map(|x| x.req.clone())
                                .unwrap_or_default(),
                            sold: sold_count(w.led, &i.n),
                            vend: vendor_count(w.led, &i.n),
                            gone: gone_count(w.led, &i.n),
                            star_with: others,
                            places: w.places(&i.n).map(|e| e.places.clone()).unwrap_or_default(),
                        }
                    })
                    .collect();
                let rune: Vec<RuneModel> = t
                    .rune
                    .iter()
                    .map(|r| {
                        let (have, src) = w.sky_have(r);
                        RuneModel {
                            n: r.clone(),
                            have,
                            src,
                        }
                    })
                    .collect();
                let done = comp.by_test.get(&t.n).copied().unwrap_or(0);
                /* Two witnesses to one turn-in, kept apart so a row can say which one it is
                 * standing on: the log watched it, or the reward is in your dump and only this
                 * giver hands it out. The third (the achievement record) is another lane's. */
                let rew = !t.reward.is_empty()
                    && reward_proves(data.rec(&t.reward))
                    && w.places(&t.reward).is_some();
                let fin = done > 0 || rew;
                let skip = w.marks.skipped(code, &t.n);
                let st = test_state(t, if fin { done.max(1) } else { 0 }, skip, &|n| {
                    w.sky_have(n).0
                });
                /* Progress on a test means a COMPONENT of it. Runes are generic. */
                let held = items.iter().filter(|i| i.have >= 1).count();
                tot.pieces += items.iter().map(|i| i.have).sum::<u32>();
                for i in &items {
                    if i.have >= 1 {
                        held_names.insert(i.n.clone());
                    }
                }
                let reward_tier = if !t.reward.is_empty() && w.sky_have(&t.reward).0 > 0 {
                    Some(w.places(&t.reward).map(|e| e.tier).unwrap_or(0))
                } else {
                    None
                };
                TestModel {
                    full: t.n.clone(),
                    short: short_test(code, &t.n),
                    say: t.say.clone(),
                    reward: t.reward.clone(),
                    need: items.len() + rune.len(),
                    items,
                    rune,
                    done,
                    rew,
                    fin,
                    ready: st.ready,
                    missing: st.missing.len(),
                    held,
                    skip,
                    track: w.marks.tracked(code, &t.n),
                    reward_tier,
                }
            })
            .collect();
        let n_done = tests.iter().filter(|t| t.fin).count();
        let n_ready = tests.iter().filter(|t| t.ready).count();
        let n_repeat = tests
            .iter()
            .filter(|t| t.fin && t.missing == 0 && !t.skip)
            .count();
        let n_partial = tests
            .iter()
            .filter(|t| !t.ready && !t.fin && t.held > 0)
            .count();
        let n_skip = tests.iter().filter(|t| t.skip).count();
        let n_left = tests.iter().filter(|t| !t.fin && !t.skip).count();
        /* distinct components, not copies: one Efreeti Zweihander is one piece however many tests
         * it feeds */
        let n_held = tests
            .iter()
            .flat_map(|t| t.items.iter().filter(|i| i.have >= 1).map(|i| i.n.as_str()))
            .collect::<HashSet<&str>>()
            .len();
        tot.tests += tests.len();
        tot.done += n_done;
        tot.ready += n_ready;
        tot.repeat += n_repeat;
        tot.partial += n_partial;
        classes.push(ClassModel {
            code: code.clone(),
            name: class_name(code).to_owned(),
            giver: c.giver.clone(),
            tests,
            n_done,
            n_ready,
            n_repeat,
            n_partial,
            n_skip,
            n_left,
            n_held,
            orphan: comp.orphan,
        });
        comp_by_code.insert(code.clone(), comp);
    }
    tot.held = held_names.len();

    /* The same board, arranged by what dropped it. Item counts come straight off the class model,
     * so a piece can never read one number here and another under its giver. */
    struct Seen {
        have: u32,
        places: Vec<Place>,
        sold: u32,
        vend: u32,
        gone: u32,
        uses: Vec<DropUse>,
    }
    let mut idx: HashMap<String, Seen> = HashMap::new();
    for g in &classes {
        for t in &g.tests {
            for i in &t.items {
                let e = idx.entry(i.n.clone()).or_insert_with(|| Seen {
                    have: i.have,
                    places: i.places.clone(),
                    sold: i.sold,
                    vend: i.vend,
                    gone: i.gone,
                    uses: Vec::new(),
                });
                e.uses.push(DropUse {
                    cls: g.name.clone(),
                    short: t.short.clone(),
                    reward: t.reward.clone(),
                    fin: t.fin,
                    skip: t.skip,
                    track: t.track,
                });
            }
        }
    }
    let board = boss_board(data, &|n, _mob| {
        let e = idx.get(n);
        let uses = e.map(|e| e.uses.clone()).unwrap_or_default();
        let open = uses.iter().filter(|u| !u.fin && !u.skip).count();
        let have = e.map(|e| e.have).unwrap_or_else(|| w.sky_have(n).0);
        DropRow {
            n: n.to_owned(),
            have,
            open,
            quest: e.is_some(),
            need: (open as u32).saturating_sub(have),
            track: uses.iter().any(|u| u.track && !u.fin && !u.skip),
            places: e.map(|e| e.places.clone()).unwrap_or_default(),
            sold: e.map(|e| e.sold).unwrap_or_else(|| sold_count(w.led, n)),
            vend: e.map(|e| e.vend).unwrap_or_else(|| vendor_count(w.led, n)),
            gone: e.map(|e| e.gone).unwrap_or_else(|| gone_count(w.led, n)),
            uses,
        }
    });
    let mut per_need: HashMap<String, u32> = HashMap::new();
    let isles: Vec<IsleModel> = board
        .into_iter()
        .map(|isle| IsleModel {
            isl: isle.isl,
            name: isle.name,
            req: isle.req,
            key: isle.key,
            mobs: isle
                .mobs
                .into_iter()
                .map(|mut m| {
                    /* Tracked first, then what you still owe, then quest pieces, then the rest. */
                    m.rows.sort_by(|a, b| {
                        b.track
                            .cmp(&a.track)
                            .then(b.need.cmp(&a.need))
                            .then(b.open.cmp(&a.open))
                            .then(b.quest.cmp(&a.quest))
                            .then_with(|| a.n.cmp(&b.n))
                    });
                    for r in &m.rows {
                        if r.need > 0 {
                            per_need.insert(r.n.clone(), r.need);
                        }
                    }
                    let need = m.rows.iter().map(|r| r.need).sum();
                    let track = m.rows.iter().filter(|r| r.track).count();
                    let nq = m.rows.iter().filter(|r| r.quest).count();
                    MobModel {
                        fold: m,
                        need,
                        track,
                        nq,
                    }
                })
                .collect(),
        })
        .collect();

    let cleanout = cleanout_rows(data, &|n| w.have_full(n), &|code, test| {
        w.marks.skipped(code, test)
    });
    let keys = key_ladder(data, &|n| w.sky_have(n));
    SkyModel {
        classes,
        tot,
        isles,
        cleanout,
        to_loot: per_need.values().sum(),
        keys,
    }
}

/// "ready" is everything in hand (a finished test with a full set in hand again
/// is a repeat, and a repeat IS a turn-in), "held" adds the tests you have started, "all" is the
/// rest. `hide_done` hides a finished test with NOTHING to hand in.
pub fn sky_keep(t: &TestModel, show: Show, hide_done: bool) -> bool {
    let repeat = t.fin && t.missing == 0;
    if hide_done && t.fin && !repeat {
        return false;
    }
    match show {
        Show::Ready => t.ready || repeat,
        Show::Held => t.ready || t.held > 0 || t.fin,
        Show::All => true,
    }
}

/// The search box against everything a test says.
pub fn sky_match(g: &ClassModel, t: &TestModel, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let mut hay = format!("{} {} {} {} {}", g.name, g.giver, t.full, t.say, t.reward);
    for i in &t.items {
        hay.push(' ');
        hay.push_str(&i.n);
        hay.push(' ');
        hay.push_str(&i.mob);
    }
    for r in &t.rune {
        hay.push(' ');
        hay.push_str(&r.n);
    }
    hay.to_lowercase().contains(q)
}

/* ====================================================================== 6. the screen == */

/// The four arrangements of one model. The nav's three SKY rows land on the first three through
/// `SkyScreen::set_view`; the "get rid of" view is the fourth.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    /// poSky checklist: by quest giver.
    #[default]
    Giver,
    /// Islands: by island and by what dropped it.
    Boss,
    /// Keys: the island ladder.
    Keys,
    /// Get rid of: what only skipped tests still want, by place.
    Cleanout,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Show {
    Ready,
    #[default]
    Held,
    All,
}

/// The converted poSky data, remembered against the snapshot root it came from so a snapshot
/// swap (Settings changed the data root) rebuilds it and nothing else does.
struct Loaded {
    root: PathBuf,
    res: Result<Arc<SkyData>, String>,
}

/// The empty state: what the screen says before one log line has been read.
pub const WAITING: &str = "Nothing read from the log yet. Hand something in and this fills in.";
pub const WAITING_2: &str = "The game writes a log only while logging is on: /log";

/// What the bootstrap thread hands back: the fold over the file's tail and the cursor to keep
/// reading from. `problem` set means nothing else in it is meaningful.
#[derive(Default)]
struct Boot {
    stream: Stream,
    cursor: Tail,
    lines: usize,
    /// Where the tail read started; non-zero means the 40MB cap cut the file.
    start: u64,
    problem: Option<String>,
}

/// The bootstrap: at most `TAIL_CAP` bytes from the end of the log, first partial line dropped
/// (the ingest's `read_tail_capped` carries the NUL and short-read rules), folded into
/// a fresh stream. Runs on a worker because 40MB of regex is not a frame's work.
fn boot_read(path: &Path, me: &str) -> Boot {
    let size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) => {
            return Boot {
                problem: Some(format!("{}: {e}", path.display())),
                ..Default::default()
            }
        }
    };
    match read_tail_capped(path, size, TAIL_CAP) {
        Ok(t) => {
            let mut stream = Stream::new(me);
            let lines = t.text.lines().filter(|l| !l.is_empty()).count();
            stream.text(&t.text);
            Boot {
                stream,
                cursor: Tail::at(t.end),
                lines,
                start: t.start,
                problem: None,
            }
        }
        Err(e) => Boot {
            problem: Some(format!("{}: {e}", path.display())),
            ..Default::default()
        },
    }
}

/// What one live poll found.
#[derive(Debug, PartialEq, Eq)]
enum Live {
    /// this many whole lines were appended and fed (0 when nothing was)
    Fed(usize),
    /// the file is shorter than the cursor: a new log took the name, or it was truncated
    Shrank,
}

/// The live read: the bytes appended since the cursor, as whole lines, fed to the stream. The
/// cursor moves by what was read, so a line is fed exactly once however the polls fall.
fn live_read(path: &Path, cursor: &mut Tail, stream: &mut Stream) -> Result<Live, String> {
    let size = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if size < cursor.offset {
        return Ok(Live::Shrank);
    }
    if size == cursor.offset {
        return Ok(Live::Fed(0));
    }
    let lines = read_appended(path, cursor, size);
    stream.feed(lines.iter().map(String::as_str));
    Ok(Live::Fed(lines.len()))
}

/// How often the live read looks at the file. The ingest polls once a second too.
const LIVE_POLL: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct SkyScreen {
    data: Option<Loaded>,
    /// The character in the log's file name, matched against the handback line.
    me: String,
    /// The fold over the active log, once the bootstrap has handed it over.
    stream: Option<Stream>,
    /// This screen's OWN cursor on the active log. The ingest's `tail()` hands its live lines to
    /// whichever screen calls first and keeps the bootstrap for itself, so a turn-in from before
    /// launch would never reach this ledger through it. Same file, same reader rules, own cursor.
    cursor: Option<Tail>,
    log_path: Option<PathBuf>,
    boot: Option<mpsc::Receiver<Boot>>,
    log_problem: Option<String>,
    last_poll: Option<Instant>,
    /// Non-zero when the 40MB cap cut the file: turn-ins older than that are not counted.
    tail_start: u64,
    inv: Option<Held>,
    inv_path: Option<PathBuf>,
    inv_time: Option<DateTime<Utc>>,
    inv_key: Option<(String, usize)>,
    /// NOT THIS SCREEN'S MARKS. See [`MarksCell`]: there is one marks file, one cell holding it,
    /// and every `SkyScreen` in the process is a view of that cell rather than a copy of it.
    marks: MarksCell,
    /// The cell revision this screen last rebuilt its model from. A mark set in the OTHER Sky
    /// window changes the cell and not this screen, so without this the board here would go on
    /// drawing the marks as they stood when this window last touched them.
    marks_seen: u64,
    model: Option<Arc<SkyModel>>,
    /// bumps on every change that should rebuild the model
    gen: u64,
    model_gen: u64,
    view: View,
    show: Show,
    hide_done: bool,
    only_need: bool,
    search: String,
    fed_lines: usize,
    /// An item name clicked this frame, handed to the App as `Ask::ShowItem` after the draw.
    want_item: Option<String>,
}

const ROW_H: f32 = 20.0;

/// The five colours are for states only, and these are states.
fn status_of(t: &TestModel) -> (&'static str, Color32) {
    if t.skip {
        ("skipped", IDLE)
    } else if t.fin && t.missing == 0 {
        ("repeat", SETTLED)
    } else if t.fin {
        ("done", SETTLED)
    } else if t.ready {
        ("ready", YOU)
    } else {
        ("missing", IDLE)
    }
}

fn mono(s: impl Into<String>) -> RichText {
    RichText::new(s).font(FontId::monospace(11.5))
}

fn body(s: impl Into<String>) -> RichText {
    RichText::new(s).font(FontId::proportional(12.5))
}

fn caps(ui: &mut Ui, s: &str, size: f32, col: Color32) {
    ui.label(
        RichText::new(s)
            .font(crate::fonts::display(size))
            .color(col),
    );
}

/// A bordered state chip: a leading square in the state colour (hollow for idle, the console's
/// ring), then the word in the text colour inside a hairline. THE COLOUR IS ON THE SQUARE AND
/// NEVER ON THE WORD: the vocabulary puts every state colour on a leading square, and a word set
/// in gold or red would be the state colour spent as decoration.
fn chip(ui: &mut Ui, s: &str, col: Color32) -> egui::Response {
    let f = FontId::proportional(10.5);
    let g = ui.painter().layout_no_wrap(s.to_owned(), f.clone(), TEXT_2);
    let sq_w = 6.0;
    let size = Vec2::new(g.rect.width() + 10.0 + sq_w + 5.0, ROW_H - 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::ZERO,
        Stroke::new(1.0, RULE),
        egui::StrokeKind::Middle,
    );
    let sq = Rect::from_center_size(
        Pos2::new(rect.left() + 5.0 + sq_w * 0.5, rect.center().y),
        Vec2::splat(sq_w),
    );
    if col == IDLE {
        ui.painter().rect_stroke(
            sq,
            egui::CornerRadius::ZERO,
            Stroke::new(1.0, IDLE),
            egui::StrokeKind::Middle,
        );
    } else {
        ui.painter().rect_filled(sq, egui::CornerRadius::ZERO, col);
    }
    ui.painter().text(
        Pos2::new(sq.right() + 5.0, rect.center().y),
        Align2::LEFT_CENTER,
        s,
        f,
        TEXT_2,
    );
    resp
}

/// A count in Plex Mono, right aligned in a fixed column, because coin and count columns line up
/// or the eye cannot subtract them.
fn mono_right(ui: &mut Ui, s: &str, w: f32, col: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, ROW_H), Sense::hover());
    ui.painter().text(
        Pos2::new(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        s,
        FontId::monospace(11.5),
        col,
    );
    resp
}

/// The leading square: filled in the state colour, or a hollow ring for nothing happening.
fn square(ui: &mut Ui, on: bool, col: Color32, clickable: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(14.0, ROW_H),
        if clickable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let sq = Rect::from_center_size(rect.center(), Vec2::splat(7.0));
    if on {
        ui.painter().rect_filled(sq, egui::CornerRadius::ZERO, col);
    } else {
        ui.painter().rect_stroke(
            sq,
            egui::CornerRadius::ZERO,
            Stroke::new(1.0, IDLE),
            egui::StrokeKind::Middle,
        );
    }
    resp
}

fn age_text(t: DateTime<Utc>) -> String {
    let secs = (Utc::now() - t).num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

impl SkyScreen {
    /// Which arrangement to draw. The integrator routes the nav's `Islands` row here with
    /// `View::Boss` and `Keys` with `View::Keys`; the checklist row is the default.
    pub fn set_view(&mut self, v: View) {
        self.view = v;
    }

    /// WHICH VIEW IS SHOWING, read only.
    ///
    /// ITS CALLER IS A TEST AND IT IS STILL NOT `#[cfg(test)]`, because the screens are in the
    /// LIB and the guard that reads this is in the BIN: the lib is compiled without `cfg(test)`
    /// for the bin`s test target, so a gate here would make the method invisible to the only
    /// thing that calls it. Measured, not assumed; that gate was written first and did not link.
    ///
    /// READ ONLY, AND DELIBERATELY NOT PART OF THE NAVIGATION. The App owns which section is
    /// selected and this screen follows it, so a production reader here would be a second answer
    /// to one question. `main::every_section_of_every_destination_reaches_its_view` uses it to
    /// prove the screen really moved when a section was pressed.
    pub fn showing(&self) -> View {
        self.view
    }

    fn sync_data(&mut self, cx: &crate::screens::Cx) {
        let Some(snap) = cx.data else {
            if self.data.is_some() {
                self.data = None;
                self.gen += 1;
            }
            return;
        };
        let root = snap.report().root;
        if self.data.as_ref().map(|d| d.root == root).unwrap_or(false) {
            return;
        }
        let d = SkyData::from_snapshot(&snap.sky);
        /* The data lane refuses a sky.json with no items or no islands; a file with no class
         * tests gets through it and would draw an empty checklist here that reads as "all done".
         * Say what is missing instead. */
        let res = if d.classes.is_empty() {
            Err(format!(
                "{}: sky.json has no class tests in it, so there is no checklist to draw",
                root.join("sky.json").display()
            ))
        } else if d.isles.is_empty() {
            Err(format!(
                "{}: sky.json has no isles in it, so there is no island board to draw",
                root.join("sky.json").display()
            ))
        } else {
            Ok(Arc::new(d))
        };
        self.data = Some(Loaded { root, res });
        self.gen += 1;
    }

    /// Start reading `path` from its tail on a worker. Whatever was known about the previous
    /// file is dropped first: a ledger is one file's word.
    fn start_boot(&mut self, path: PathBuf) {
        self.stream = None;
        self.cursor = None;
        self.log_problem = None;
        self.fed_lines = 0;
        self.tail_start = 0;
        self.gen += 1;
        let me = self.me.clone();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("grimoire-sky-log".into())
            .spawn(move || {
                let _ = tx.send(boot_read(&path, &me));
            });
        match spawned {
            Ok(_) => self.boot = Some(rx),
            Err(e) => {
                self.log_problem = Some(format!("could not start the log reader thread: {e}"))
            }
        }
    }

    fn sync_ingest(&mut self, cx: &mut crate::screens::Cx) {
        /* The log the ingest is tailing (the most recently written eqlog_<Character>_<server>.txt,
         * its choice, so this screen and the Parser never read two different characters). The
         * character name is what the handback line is matched against. */
        static ME: OnceLock<Regex> = OnceLock::new();
        let me_rx = ME.get_or_init(|| Regex::new(r"^eqlog_([^_]+)_").expect("me regex"));
        let mut active: Option<(PathBuf, String)> = None;
        for s in cx.ingest.sources() {
            if s.kind != SourceKind::Log || s.last_read.is_none() {
                continue;
            }
            let file = s.path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            if let Some(m) = me_rx.captures(file) {
                active = Some((s.path.clone(), m[1].to_owned()));
                break;
            }
        }
        let active_path = active.as_ref().map(|(p, _)| p.clone());
        if active_path != self.log_path {
            self.log_path = active_path.clone();
            self.me = active.map(|(_, me)| me).unwrap_or_default();
            self.boot = None;
            match active_path {
                Some(p) => self.start_boot(p),
                None => {
                    self.stream = None;
                    self.cursor = None;
                    self.log_problem = None;
                    self.fed_lines = 0;
                    self.tail_start = 0;
                    self.gen += 1;
                }
            }
        }

        /* adopt a finished bootstrap */
        if let Some(rx) = &self.boot {
            match rx.try_recv() {
                Ok(b) => {
                    self.boot = None;
                    match b.problem {
                        Some(p) => self.log_problem = Some(p),
                        None => {
                            self.stream = Some(b.stream);
                            self.cursor = Some(b.cursor);
                            self.fed_lines = b.lines;
                            self.tail_start = b.start;
                        }
                    }
                    self.gen += 1;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.boot = None;
                    self.log_problem =
                        Some("the log reader thread ended without a result".to_owned());
                }
            }
        }

        /* the live read, once a second */
        let mut restart = false;
        if let (Some(p), Some(cur), Some(s)) = (&self.log_path, &mut self.cursor, &mut self.stream)
        {
            if self
                .last_poll
                .map(|t| t.elapsed() >= LIVE_POLL)
                .unwrap_or(true)
            {
                self.last_poll = Some(Instant::now());
                let before = s.led.n;
                match live_read(p, cur, s) {
                    Ok(Live::Fed(n)) => {
                        self.fed_lines += n;
                        self.log_problem = None;
                    }
                    Ok(Live::Shrank) => restart = true,
                    Err(e) => self.log_problem = Some(e),
                }
                if s.led.n != before {
                    self.gen += 1;
                }
            }
        }
        if restart {
            if let Some(p) = self.log_path.clone() {
                self.start_boot(p);
            }
        }

        match cx.ingest.inventory() {
            Some(dump) => {
                let rows = dump_rows(dump);
                /* The dump's own mtime is the "since" the log arrivals are measured against; the
                 * read time stands in when the file system gave none. */
                let time = dump.modified.unwrap_or(dump.read_at);
                let key = (
                    format!("{}@{}", dump.path.display(), time.timestamp()),
                    rows.len(),
                );
                if self.inv_key.as_ref() != Some(&key) {
                    self.inv = Some(held_of_rows(&rows));
                    self.inv_key = Some(key);
                    self.gen += 1;
                }
                self.inv_path = Some(dump.path.clone());
                self.inv_time = Some(time);
            }
            None => {
                if self.inv.is_some() {
                    self.inv = None;
                    self.inv_key = None;
                    self.inv_path = None;
                    self.inv_time = None;
                    self.gen += 1;
                }
            }
        }
    }

    fn rebuild(&mut self) {
        if self.model_gen == self.gen && self.model.is_some() {
            return;
        }
        self.model_gen = self.gen;
        let Some(data) = self.data.as_ref().and_then(|d| d.res.as_ref().ok()) else {
            self.model = None;
            return;
        };
        let empty = Ledger::default();
        let led = self.stream.as_ref().map(|s| &s.led).unwrap_or(&empty);
        let since = self
            .inv_time
            .map(|t| t.with_timezone(&chrono::Local).naive_local());
        let model = {
            let g = self.marks.lock();
            let have = have_map(led, self.inv.as_ref(), since, &g.marks);
            let w = Witness {
                data,
                led,
                inv: self.inv.as_ref(),
                have: &have,
                marks: &g.marks,
            };
            Arc::new(build_model(&w))
        };
        self.model = Some(model);
    }

    /// Take up the cell's revision and say whether it had moved since this screen last drew.
    /// A mark set in the OTHER Sky window moves the cell and touches nothing here, so this is the
    /// whole of how it reaches this screen's model.
    fn marks_changed(&mut self) -> bool {
        let rev = self.marks.rev();
        let moved = self.marks_seen != rev;
        self.marks_seen = rev;
        moved
    }

    /// The cell this screen is a view of. The wiring check in `windows` reads it, to prove that
    /// the popped-out Sky window and the main window look at the same marks rather than at two
    /// copies of them, which is the whole of the defect this shape exists to kill.
    #[cfg(test)]
    pub(crate) fn marks_cell(&self) -> MarksCell {
        self.marks.clone()
    }

    /// Every mark click in this screen ends here. The platform path half: the `cfg(test)` refusal
    /// is here so no test can reach the owner's marks file through a click.
    fn save_marks(&self) {
        if cfg!(test) {
            self.marks.report(Some(TEST_REFUSAL.to_owned()), false);
            return;
        }
        match Marks::path() {
            Some(p) => self.save_marks_to(&p),
            None => self.marks.report(Some(NO_CONFIG_DIR.to_owned()), false),
        }
    }

    /// `save_marks` on a named path. THE REFUSAL AND THE WAY OUT OF IT ARE BOTH SET HERE, from the
    /// one `Refused` the guard returned, so the sentence on screen and the button under it cannot
    /// disagree about whether starting fresh would help.
    fn save_marks_to(&self, path: &Path) {
        let mut g = self.marks.lock();
        let out = g.marks.write_file(path, OnUnreadable::Refuse);
        match out {
            Ok(_) => {
                g.problem = None;
                g.refused = false;
            }
            Err(r) => {
                g.problem = Some(r.why);
                g.refused = r.start_fresh_helps;
            }
        }
        g.rev += 1;
    }

    /// THE ONE WRITER ALLOWED TO REPLACE MARKS NOBODY COULD READ. The platform path half: the
    /// `cfg(test)` refusal is here so no test can reach the owner's file through the button.
    fn start_fresh(&self) {
        if cfg!(test) {
            self.marks.note(Some(TEST_REFUSAL.to_owned()));
            return;
        }
        match Marks::path() {
            Some(p) => self.start_fresh_at(&p),
            None => self.marks.note(Some(NO_CONFIG_DIR.to_owned())),
        }
    }

    /// `start_fresh` on a named path, which is all of it: keep the unreadable bytes, write, and
    /// let ordinary saves work again. The reason this replacement is allowed is painted right
    /// beside its button in `sources_line`: it says the old file is kept first and where it went,
    /// and it only ever runs because a person clicked it.
    ///
    /// CLEARING `load_problem` IS THE POINT OF THE WHOLE THING. This session's marks ARE the file
    /// from here on, so the next ordinary click may save again. Leaving it set would wedge the
    /// screen out of saving until a restart, which is the refusal outliving its reason.
    fn start_fresh_at(&self, path: &Path) {
        let mut g = self.marks.lock();
        let out = g.marks.write_file(path, OnUnreadable::KeepACopy);
        match out {
            Ok(kept) => {
                if kept.is_some() {
                    g.kept = kept;
                }
                g.marks.load_problem = None;
                g.problem = None;
                g.refused = false;
            }
            Err(r) => {
                g.problem = Some(r.why);
                g.refused = r.start_fresh_helps;
            }
        }
        g.rev += 1;
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut crate::screens::Cx) {
        /* ONE FILE, ONE READ, AND ONE COPY OF IT IN THIS PROCESS. The first Sky screen to draw
         * does the read and every other one sees what it read. `marks_seen` is the other half of
         * that: a mark set in the other Sky window moves the cell's revision and nothing else, so
         * without this line the board here would keep drawing the marks as they were. */
        self.marks.load_once();
        if self.marks_changed() {
            self.gen += 1;
        }
        self.sync_data(cx);
        self.sync_ingest(cx);
        self.rebuild();
        /* the tail is polled by drawing, so keep drawing while this screen is up */
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));

        ui.horizontal_wrapped(|ui| {
            caps(ui, "PLANE OF SKY", 16.0, GOLD);
            ui.add_space(12.0);
            /* THE VIEW ROW ONLY WHERE NOTHING ELSE OFFERS IT. See `Cx::railed`: in the main
             * window the inner rail lists these same four, and in the pop-out Sky window this is
             * the only way between them. The heading beside it stays either way. */
            if !cx.railed {
                for (v, label) in [
                    (View::Giver, "by giver"),
                    (View::Boss, "by island"),
                    (View::Keys, "keys"),
                    (View::Cleanout, "get rid of"),
                ] {
                    if ui.selectable_label(self.view == v, body(label)).clicked() {
                        self.view = v;
                    }
                }
            }
        });
        ui.add_space(4.0);
        self.sources_line(ui);

        /* The snapshot's own sky.json, for the item windows the boss view names under a held
         * count. `cx.data` is a shared borrow that outlives this frame's closures. */
        let sky: Option<&crate::data::Sky> = cx.data.map(|d| &d.sky);
        let Some(data) = self
            .data
            .as_ref()
            .and_then(|d| d.res.as_ref().ok())
            .cloned()
        else {
            self.no_data(ui, cx);
            return;
        };
        if self.stream.is_none() && self.inv.is_none() {
            ui.add_space(14.0);
            match cx.ingest.log_dir_problem() {
                /* No Logs folder at all: telling a person to turn on /log would send them to fix
                 * the wrong thing. The ingest's own words name the fix (Settings). */
                Some(p) => {
                    ui.label(
                        body("Nothing can be counted yet: there is no Logs folder to read.")
                            .color(TEXT),
                    );
                    ui.label(body(p).color(TEXT_2));
                }
                None => {
                    ui.label(body(WAITING).color(TEXT));
                    ui.label(body(WAITING_2).color(TEXT_2));
                    ui.label(mono("No EQ log (eqlog_<Character>_<server>.txt) and no inventory dump in the ingest sources yet.").color(TEXT_3));
                }
            }
            /* The ladder and the board are the data's own and stand without a witness; only the
             * held counts wait. Drawn under the line above so the wait is never mistaken for
             * missing data. */
            if matches!(self.view, View::Keys | View::Boss) {
                if let Some(model) = self.model.clone() {
                    ui.add_space(10.0);
                    egui::ScrollArea::vertical()
                        .id_salt("sky-body-cold")
                        .auto_shrink([false, false])
                        .show(ui, |ui| match self.view {
                            View::Keys => self.keys_view(ui, &data, &model),
                            _ => self.boss_view(ui, &model, sky),
                        });
                }
            }
            return;
        }
        let Some(model) = self.model.clone() else {
            return;
        };

        self.banner(ui);
        ui.add_space(6.0);
        self.controls(ui);
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("sky-body")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.view {
                View::Giver => self.giver_view(ui, &data, &model),
                View::Boss => self.boss_view(ui, &model, sky),
                View::Keys => self.keys_view(ui, &data, &model),
                View::Cleanout => self.cleanout_view(ui, &data, &model),
            });
        if let Some(n) = self.want_item.take() {
            cx.ask = crate::screens::Ask::ShowItem(n);
        }
    }

    fn sources_line(&self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| match &self.data {
            Some(Loaded { root, res: Ok(d) }) => {
                ui.label(mono(format!("{}", root.join("sky.json").display())).color(TEXT_3));
                ui.label(
                    mono(format!(
                        "{} classes, {} tests, {} islands, {} item windows",
                        d.classes.len(),
                        d.test_count(),
                        d.isles.len(),
                        d.items.len()
                    ))
                    .color(TEXT_3),
                );
            }
            Some(Loaded { res: Err(e), .. }) => {
                ui.label(mono(e.clone()).color(WRONG));
            }
            None => {}
        });
        ui.horizontal_wrapped(|ui| {
            match (&self.log_path, &self.stream) {
                (Some(p), Some(s)) => {
                    ui.label(mono(format!("log {}", p.display())).color(TEXT_3));
                    ui.label(
                        mono(format!(
                            "{} of {} tail lines were ledger lines",
                            s.led.n, self.fed_lines
                        ))
                        .color(TEXT_3),
                    );
                    if let (Some(a), Some(b)) = (&s.led.first, &s.led.last) {
                        ui.label(mono(format!("{a} to {b}")).color(TEXT_3));
                    }
                    if self.tail_start > 0 {
                        ui.label(
                            mono(format!(
                                "read from byte {} ({} cap): older turn-ins are not counted",
                                self.tail_start,
                                crate::ingest::tail_cap_text()
                            ))
                            .color(TEXT_3),
                        );
                    }
                }
                (Some(p), None) if self.boot.is_some() => {
                    ui.label(mono(format!("log {}", p.display())).color(TEXT_3));
                    ui.label(mono("reading its tail").color(TEXT_3));
                }
                (Some(p), None) => {
                    ui.label(mono(format!("log {}", p.display())).color(TEXT_3));
                }
                _ => {
                    ui.label(mono("no EQ log being tailed by the ingest yet").color(TEXT_3));
                }
            }
            if let Some(e) = &self.log_problem {
                ui.label(mono(e.clone()).color(WRONG));
            }
            match (&self.inv_path, &self.inv) {
                (Some(p), Some(h)) => {
                    ui.label(mono(format!("dump {}", p.display())).color(TEXT_3));
                    ui.label(mono(format!("{} rows", h.rows)).color(TEXT_3));
                    if let Some(t) = self.inv_time {
                        ui.label(mono(format!("read {} ago", age_text(t))).color(TEXT_3));
                    }
                }
                _ => {
                    ui.label(mono("no inventory dump").color(TEXT_3));
                }
            }
        });
        /* READ THE CELL ONCE, THEN PAINT FROM WHAT WAS READ. The "start fresh" button below
         * reaches back into the same cell, so nothing may still be holding its lock by the time
         * it is clicked. */
        let (problem, way_out, kept) = {
            let g = self.marks.lock();
            (
                g.problem.clone(),
                g.marks.load_problem.is_some() || g.refused,
                g.kept.clone(),
            )
        };
        if let Some(p) = &problem {
            ui.label(mono(format!("marks: {p}")).color(WRONG));
        }
        /* THE REFUSAL HAS TO REACH A PIXEL. A guard whose refusal reaches only a log line leaves
         * the owner believing their marks saved, which is the same loss with an extra step. The
         * line above is the reason; these are what it means and the way out of it.
         *
         * BOTH CONDITIONS, because there are two ways to be refused and both messages end in the
         * words "start fresh". `load_problem` is the session half, and it is true from the moment
         * the screen loads, before any click. `MarksState::refused` is the file half, which is
         * only known once a save has tried: this session read the file cleanly and something broke
         * it afterwards. Painting the sentence without the button in that second case would point
         * a person at a control that is not there. */
        if way_out {
            ui.label(
                body(
                    "Nothing you mark is being saved, so the marks already in that file are still \
                     there, every one of them.",
                )
                .color(TEXT_2),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(body("Repair that file by hand and RESTART, or").color(TEXT_2));
                if ui
                    .button(body("start fresh").color(TEXT))
                    .on_hover_text(
                        "Writes the marks this session is holding, which are none of yours, over \
                         that file. The file that could not be read is copied beside it first and \
                         this screen says where.",
                    )
                    .clicked()
                {
                    self.start_fresh();
                }
            });
        }
        if let Some(k) = &kept {
            ui.label(
                mono(format!(
                    "the marks that could not be read were kept as {}",
                    k.display()
                ))
                .color(TEXT_2),
            );
        }
    }

    fn no_data(&self, ui: &mut Ui, cx: &crate::screens::Cx) {
        ui.add_space(14.0);
        match (&self.data, cx.data_err) {
            (Some(Loaded { res: Err(e), .. }), _) => {
                ui.label(
                    body("The snapshot loaded, but its Plane of Sky part cannot be drawn.")
                        .color(TEXT),
                );
                ui.label(mono(e.clone()).color(WRONG));
            }
            (_, Some(e)) => {
                ui.label(
                    body("The data snapshot is absent, so there is no Plane of Sky data to draw.")
                        .color(TEXT),
                );
                ui.label(mono(e.to_owned()).color(WRONG));
            }
            _ => {
                ui.label(
                    body("The data snapshot is absent, so there is no Plane of Sky data to draw.")
                        .color(TEXT),
                );
            }
        }
        /* The loader's own probe list, in its order, so these words cannot drift from what
         * `Snapshot::locate` actually tried. */
        let tried: Vec<String> = crate::data::candidates()
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        ui.label(body(format!("Put sky.json (with the other four snapshot files and atlas-wiki/) at one of: {}; or name the folder on the Settings screen.", tried.join("; "))).color(TEXT_2));
    }

    /// What this reading can and cannot have seen. Both halves are real limits and neither is
    /// recoverable, so they are stated rather than left to be inferred.
    fn banner(&self, ui: &mut Ui) {
        let mut bits: Vec<String> = Vec::new();
        match &self.inv {
            None => bits.push("No inventory dump yet. Counts come from the log alone. Type /out inventory in game to get bag and bank locations.".to_owned()),
            Some(_) => {
                if let Some(t) = self.inv_time {
                    if (Utc::now() - t).num_minutes() > 10 {
                        bits.push(format!("Inventory dump was read {} ago. /out inventory to refresh.", age_text(t)));
                    }
                }
            }
        }
        match &self.stream {
            None if self.boot.is_some() => bits.push("Reading the log's tail. Turn-ins, runes and losses fill in when it is done.".to_owned()),
            None => bits.push("No EQ log being tailed by the ingest. Turn-ins, runes and losses all come from the log.".to_owned()),
            Some(s) if !s.led.saw_sky => bits.push("No Plane of Sky zone-in in this log. Turn-ins from before it are not counted.".to_owned()),
            _ => {}
        }
        bits.push("No achievement record is read on this screen, so a test finished before this log with no reward in your dump shows as ready, not done.".to_owned());
        for b in bits {
            ui.label(body(b).color(TEXT_2));
        }
    }

    fn controls(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            if self.view == View::Giver {
                ui.label(body("show").color(TEXT_3));
                for (s, label) in [
                    (Show::Ready, "ready"),
                    (Show::Held, "held a piece"),
                    (Show::All, "all"),
                ] {
                    if ui.selectable_label(self.show == s, body(label)).clicked() {
                        self.show = s;
                    }
                }
                ui.checkbox(
                    &mut self.hide_done,
                    body("hide finished with nothing to hand in"),
                );
            }
            if self.view == View::Boss {
                ui.checkbox(&mut self.only_need, body("only what I need"));
            }
            if matches!(self.view, View::Giver | View::Boss) {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("search")
                        .desired_width(180.0)
                        .font(FontId::monospace(11.5)),
                );
            }
        });
    }

    /* ------------------------------------------------------------ keys -- */

    /// The island ladder: what each island takes, whether you hold it, and where sky.json says it
    /// drops. Keys are items like any other to the dump (KeyRing table, bags, bank) and to the
    /// log's loot line, so the counts are the same `sky_have` the checklist runs on.
    fn keys_view(&mut self, ui: &mut Ui, data: &Arc<SkyData>, m: &Arc<SkyModel>) {
        let open = m.keys.iter().filter(|k| k.open()).count();
        ui.label(
            mono(format!(
                "{} of {} islands open on what you hold",
                open,
                m.keys.len()
            ))
            .color(TEXT_2),
        );
        ui.label(body("Read from your inventory dump (key ring, bags, bank) and the log's loot lines. The achievement record is not read here, so a key you used up and no longer carry shows as not held.").color(TEXT_2));
        ui.add_space(8.0);
        for step in &m.keys {
            ui.horizontal_wrapped(|ui| {
                square(ui, step.open(), SETTLED, false).on_hover_text(if step.open() {
                    if step.req.is_empty() {
                        "takes no key"
                    } else {
                        "every key it takes is in hand"
                    }
                } else {
                    "a key it takes is not in hand"
                });
                caps(
                    ui,
                    &format!("{}  I{}", step.name.to_uppercase(), step.isl),
                    11.0,
                    if step.open() { GOLD } else { GOLD_DIM },
                );
            });
            ui.indent(("sky-key", &step.isl), |ui| {
                if step.req.is_empty() {
                    ui.label(body("takes no key").color(TEXT_3));
                }
                for k in &step.req {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(body("takes").color(TEXT_3));
                        if ui.add(egui::Label::new(body(&k.n).color(if k.have >= 1 { TEXT } else { TEXT_2 })).sense(Sense::click())).on_hover_text("Open on the Items screen").clicked() {
                            self.want_item = Some(k.n.clone());
                        }
                        let src_note = match k.src {
                            "inv" => "from your inventory dump",
                            "log" => "from the log; a dump cannot see this one",
                            "mark" => "you marked this one held",
                            _ => "",
                        };
                        mono_right(ui, &format!("{}/1", k.have), 40.0, if k.have >= 1 { TEXT } else { TEXT_3 }).on_hover_text(src_note);
                        match &k.from {
                            Some(isl) => {
                                ui.label(body(format!("drops on {}", data.isle_name(isl))).color(TEXT_3));
                            }
                            None => {
                                ui.label(body("not on any island's drop list in sky.json").color(TEXT_3)).on_hover_text("No isle in the file names this key under its drops. Where it comes from is not stated here.");
                            }
                        }
                    });
                }
                if !step.drops.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(body("drops").color(TEXT_3));
                        for d in &step.drops {
                            if ui.add(egui::Label::new(body(d).color(TEXT_2)).sense(Sense::click())).on_hover_text("Open on the Items screen").clicked() {
                                self.want_item = Some(d.clone());
                            }
                        }
                    });
                }
            });
            ui.add_space(4.0);
        }
    }

    /* ------------------------------------------------------------ by giver -- */

    fn giver_view(&mut self, ui: &mut Ui, data: &Arc<SkyData>, m: &Arc<SkyModel>) {
        let q = self.search.trim().to_lowercase();
        let mut shown = 0usize;
        ui.label(
            mono(format!(
                "{} ready, {} repeat, {}/{} done, {} started, {} distinct pieces held",
                m.tot.ready, m.tot.repeat, m.tot.done, m.tot.tests, m.tot.partial, m.tot.held
            ))
            .color(TEXT_2),
        );
        ui.add_space(6.0);
        let mut toggles: Vec<(String, String, MarkOp)> = Vec::new();
        for g in &m.classes {
            let rows: Vec<&TestModel> = g
                .tests
                .iter()
                .filter(|t| sky_keep(t, self.show, self.hide_done) && sky_match(g, t, &q))
                .collect();
            if rows.is_empty() {
                continue;
            }
            shown += rows.len();
            let mut tally: Vec<String> = Vec::new();
            if g.n_ready > 0 {
                tally.push(format!("{} ready", g.n_ready));
            }
            if g.n_repeat > 0 {
                tally.push(format!("{} repeat", g.n_repeat));
            }
            if g.n_done > 0 {
                tally.push(format!("{} done", g.n_done));
            }
            if g.n_partial > 0 {
                tally.push(format!("{} started", g.n_partial));
            }
            if g.n_held > 0 {
                tally.push(format!("{} pieces held", g.n_held));
            }
            tally.push(format!("{} left of {}", g.n_left, g.tests.len()));
            let head = format!("{}    {}    {}", g.name, g.giver, tally.join(", "));
            egui::CollapsingHeader::new(RichText::new(head).font(FontId::proportional(12.5)).color(GOLD))
                .id_salt(("sky-class", &g.code))
                .icon(crate::chrome::fold_icon)
                .default_open(g.n_ready > 0)
                .show(ui, |ui| {
                    if g.orphan > 0 {
                        ui.label(
                            body(format!(
                                "{} completed trade{} with {} matched no single test. That is what a turn-in split across two trades looks like.",
                                g.orphan,
                                if g.orphan == 1 { "" } else { "s" },
                                g.giver
                            ))
                            .color(TEXT_2),
                        );
                    }
                    for t in rows {
                        self.test_row(ui, data, g, t, &mut toggles);
                    }
                });
        }
        if shown == 0 {
            ui.label(body("Nothing matches. Widen show, or clear the search.").color(TEXT_2));
        }
        for (code, test, op) in toggles {
            self.apply(&code, &test, op);
        }
    }

    fn test_row(
        &self,
        ui: &mut Ui,
        data: &SkyData,
        g: &ClassModel,
        t: &TestModel,
        toggles: &mut Vec<(String, String, MarkOp)>,
    ) {
        let (word, col) = status_of(t);
        ui.horizontal_wrapped(|ui| {
            /* the star: an ordering and a mark, drawn in the metal, never a state colour */
            let star = if t.track { "\u{2605}" } else { "\u{2606}" };
            if ui.add(egui::Button::new(RichText::new(star).color(if t.track { GOLD } else { GOLD_DEEP })).frame(false)).on_hover_text("Track this test: it leads its giver's fold and the boss board").clicked() {
                toggles.push((g.code.clone(), t.full.clone(), MarkOp::Track));
            }
            let name = if t.skip { body(&t.short).strikethrough().color(TEXT_2) } else { body(&t.short).color(TEXT) };
            ui.label(name);
            if !t.say.is_empty() {
                ui.label(mono(format!("say {}", t.say)).color(TEXT_3)).on_hover_text(format!("Hail {} and say this", g.giver));
            }
            let why = if t.done > 0 {
                format!("Your log shows {} turn-in{} of this test", t.done, if t.done == 1 { "" } else { "s" })
            } else if t.rew {
                format!("{} is in your inventory dump, and this test is the only place it comes from", t.reward)
            } else if t.ready {
                "Everything is in hand. Nothing here has seen a turn-in of this test, and without an achievement record that is not the same as never.".to_owned()
            } else if t.skip {
                "You ticked skip: its pieces stop counting as needed".to_owned()
            } else {
                format!("missing {} of {}", t.missing, t.need)
            };
            let label = if word == "missing" { format!("missing {}", t.missing) } else { word.to_owned() };
            chip(ui, &label, col).on_hover_text(why);
            if t.done > 0 {
                ui.label(mono(format!("\u{2713}{}", t.done)).color(TEXT_2));
            }
            ui.label(body("\u{2192}").color(TEXT_2));
            if ui.add(egui::Label::new(body(&t.reward).color(TEXT_2)).sense(Sense::click())).on_hover_text("Open on the Items screen").clicked() {
                toggles.push((t.reward.clone(), String::new(), MarkOp::Show));
            }
            if let Some(tier) = t.reward_tier {
                ui.label(mono(format!("held +{tier}")).color(TEXT_3)).on_hover_text("The copy in your dump, at its upgrade tier. A repeat hands you a +0 duplicate to merge into it.");
            }
            if ui.small_button(body(if t.skip { "un-skip" } else { "skip" })).on_hover_text(if t.skip { "Count this one again" } else { "Never doing this one: its pieces stop counting as needed" }).clicked() {
                toggles.push((g.code.clone(), t.full.clone(), MarkOp::Skip));
            }
        });
        ui.indent(("sky-test", &g.code, &t.full), |ui| {
            for r in &t.rune {
                rune_row(ui, r, toggles);
            }
            let wanted = !t.fin && !t.skip;
            for i in &t.items {
                self.piece_row(ui, data, i, wanted, toggles);
            }
        });
    }

    fn piece_row(
        &self,
        ui: &mut Ui,
        data: &SkyData,
        i: &PieceModel,
        wanted: bool,
        toggles: &mut Vec<(String, String, MarkOp)>,
    ) {
        let has = i.have >= i.need;
        ui.horizontal_wrapped(|ui| {
            let tip = match i.src {
                "mark" => "Marked as held. Click to clear",
                _ if has => "held",
                _ => "Mark as held (bought it, or it is on your pet)",
            };
            if square(ui, has, SETTLED, true).on_hover_text(tip).clicked() {
                toggles.push((i.n.clone(), String::new(), MarkOp::Held));
            }
            if ui.add(egui::Label::new(body(&i.n).color(if has { TEXT } else { TEXT_2 })).sense(Sense::click())).on_hover_text("Open on the Items screen").clicked() {
                toggles.push((i.n.clone(), String::new(), MarkOp::Show));
            }
            if !i.star_with.is_empty() {
                ui.label(RichText::new("\u{2605}").color(GOLD_DIM)).on_hover_text(format!("Also wanted by {}", i.star_with.join("; ")));
            }
            if i.nodrop {
                ui.label(mono("NO DROP").color(TEXT_3)).on_hover_text("The wiki's Plane of Sky table marks it NO DROP: only your own loot can fill this one, nobody can hand it to you.");
            }
            let src_note = match i.src {
                "inv" => "from your inventory dump",
                "log" => "from the log; a dump cannot see this one",
                "mark" => "you marked this one held",
                _ => "",
            };
            mono_right(ui, &format!("{}/{}", i.have, i.need), 40.0, if has { TEXT } else { TEXT_3 }).on_hover_text(src_note);
            for p in &i.places {
                let b = loc_badge(&p.loc);
                let txt = if p.count > 1 { format!("{} x{}", b.text(), p.count) } else { b.text() };
                ui.label(mono(txt).color(TEXT_2)).on_hover_text(p.loc.clone());
            }
            /* The three ways a piece leaves without reaching a bank. Loud (WRONG) only when a
             * live test still wants the piece and you hold none: that is a filter to fix. */
            let hard = wanted && i.have == 0;
            if i.sold > 0 {
                chip(ui, &format!("autosold {}x", i.sold), if hard { WRONG } else { IDLE })
                    .on_hover_text(format!("Your loot filter sold {} of these on pickup.{}", i.sold, if hard { " You are holding none and a test here still wants it." } else { "" }));
            }
            if i.vend > 0 {
                chip(ui, &format!("vendored {}x", i.vend), IDLE).on_hover_text("A merchant line carries no quantity, so this counts sales, not pieces.");
            }
            if i.gone > 0 {
                chip(ui, &format!("destroyed {}x", i.gone), if hard { WRONG } else { IDLE }).on_hover_text(format!("You destroyed {}.", i.gone));
            }
            if !has && !i.isl.is_empty() {
                let mut wh = data.isle_name(&i.isl);
                if !i.mob.is_empty() {
                    wh.push_str(", ");
                    wh.push_str(&i.mob);
                }
                ui.label(body(wh).color(TEXT_3));
                if !i.req.is_empty() {
                    ui.label(body(format!("needs {}", i.req.join(", "))).color(TEXT_3)).on_hover_text(format!("Isle {} cannot be reached without it", i.isl));
                }
            }
        });
    }

    /* ------------------------------------------------------------ by boss -- */

    fn boss_view(&mut self, ui: &mut Ui, m: &Arc<SkyModel>, sky: Option<&crate::data::Sky>) {
        let q = self.search.trim().to_lowercase();
        ui.label(
            mono(format!(
                "{} pieces to loot, {}/{} tests done",
                m.to_loot, m.tot.done, m.tot.tests
            ))
            .color(TEXT_2),
        );
        ui.add_space(6.0);
        let mut shown = 0usize;
        for isle in &m.isles {
            let mut head = format!("{}  I{}", isle.name.to_uppercase(), isle.isl);
            if !isle.req.is_empty() {
                head.push_str(&format!("    {} to reach", isle.req.join(", ")));
            }
            if !isle.key.is_empty() {
                head.push_str(&format!("    drops {}", isle.key.join(", ")));
            }
            let mut any = false;
            for mob in &isle.mobs {
                let rows: Vec<&DropRow> = mob
                    .fold
                    .rows
                    .iter()
                    .filter(|r| {
                        let hit = q.is_empty() || {
                            let mut hay = format!("{} {} {}", r.n, mob.fold.n, isle.name);
                            for u in &r.uses {
                                hay.push(' ');
                                hay.push_str(&u.short);
                                hay.push(' ');
                                hay.push_str(&u.reward);
                            }
                            hay.to_lowercase().contains(&q)
                        };
                        hit && (!self.only_need || r.need > 0)
                    })
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                if !any {
                    ui.add_space(8.0);
                    caps(ui, &head, 11.0, GOLD_DIM);
                    any = true;
                }
                shown += rows.len();
                let mut tally: Vec<String> = Vec::new();
                if mob.track > 0 {
                    tally.push(format!("\u{2605}{}", mob.track));
                }
                if mob.need > 0 {
                    tally.push(format!("{} to loot", mob.need));
                }
                if mob.nq > 0 {
                    tally.push(format!("{} quest pieces", mob.nq));
                }
                tally.push(format!("{} drops", mob.fold.rows.len()));
                let title = format!(
                    "{}    {}    {}",
                    mob.fold.n,
                    mob.fold.role,
                    tally.join(", ")
                );
                egui::CollapsingHeader::new(
                    RichText::new(title)
                        .font(FontId::proportional(12.5))
                        .color(if mob.need > 0 { GOLD } else { TEXT_2 }),
                )
                .id_salt(("sky-mob", &isle.isl, &mob.fold.n))
                .icon(crate::chrome::fold_icon)
                .default_open(mob.need > 0 || mob.track > 0)
                .show(ui, |ui| {
                    if mob.fold.role == "trash" && !mob.fold.from.is_empty() {
                        ui.label(body(mob.fold.from.join(", ")).color(TEXT_3));
                    }
                    for r in rows {
                        /* two witnesses under the held count (the log and the dump, through
                         * `held` and `reconcile`), and the item window's own drop table beside
                         * them, straight from sky.json through the snapshot's item index */
                        let mut witness = self.witness_words(&r.n).unwrap_or_default();
                        if let Some(it) = sky.and_then(|s| s.item(&r.n)) {
                            let rows = it.drop_rows();
                            if !rows.is_empty() {
                                let from: Vec<String> = rows
                                    .iter()
                                    .take(4)
                                    .map(|d| {
                                        if d.zone.is_empty() {
                                            d.mob.to_owned()
                                        } else {
                                            format!("{} ({})", d.mob, d.zone)
                                        }
                                    })
                                    .collect();
                                witness.push_str(&format!(
                                    " sky.json lists it from {}{}.",
                                    from.join(", "),
                                    if rows.len() > 4 {
                                        format!(" and {} more", rows.len() - 4)
                                    } else {
                                        String::new()
                                    }
                                ));
                            }
                        }
                        let witness = witness.trim().to_owned();
                        drop_row(
                            ui,
                            r,
                            &mob.fold.src,
                            &mut self.want_item,
                            if witness.is_empty() {
                                None
                            } else {
                                Some(witness.as_str())
                            },
                        );
                    }
                });
            }
        }
        if shown == 0 {
            ui.label(
                body("Nothing matches. Clear the search, or untick only what I need.")
                    .color(TEXT_2),
            );
        }
    }

    /* ------------------------------------------------------------ get rid of -- */

    fn cleanout_view(&mut self, ui: &mut Ui, data: &Arc<SkyData>, m: &Arc<SkyModel>) {
        let c = &m.cleanout;
        let skips = m.classes.iter().map(|g| g.n_skip).sum::<usize>();
        ui.label(body("What you have ticked skip on and are still carrying, by place, and whether another player can take it.").color(TEXT_2));
        if skips == 0 {
            ui.add_space(8.0);
            ui.label(body("No test is ticked skip. Tick skip on a test you will not run again (by giver view) and its pieces show here by place.").color(TEXT));
            return;
        }
        let n: u32 = c.rows.iter().map(|r| r.count).sum();
        let wt: f64 = c.rows.iter().map(|r| r.wt * r.count as f64).sum();
        let give = c.rows.iter().filter(|r| r.give == Some(true)).count();
        let places: HashSet<&str> = c.rows.iter().map(|r| r.sec.as_str()).collect();
        ui.label(mono(format!("{} tests skipped, {} pieces in {} rows across {} places, {:.1} weight, {} rows another player can take", skips, n, c.rows.len(), places.len(), wt, give)).color(TEXT_2));
        let mut notes: Vec<String> = Vec::new();
        if c.mixed > 0 {
            notes.push(format!(
                "{} more piece{} still wanted by a test you have not skipped.",
                c.mixed,
                if c.mixed == 1 { " is" } else { "s are" }
            ));
        }
        if c.worn > 0 {
            notes.push(format!(
                "{} {} on your character.",
                c.worn,
                if c.worn == 1 { "is" } else { "are" }
            ));
        }
        for nt in notes {
            ui.label(body(nt).color(TEXT_2));
        }
        if c.rows.is_empty() {
            ui.add_space(8.0);
            ui.label(
                body("Nothing to clear out: you hold no piece that only skipped tests want.")
                    .color(TEXT),
            );
            return;
        }
        ui.add_space(8.0);
        egui::Grid::new("sky-cleanout").num_columns(7).spacing([14.0, 4.0]).striped(true).show(ui, |ui| {
            for h in ["WHERE", "ITEM", "COUNT", "WEIGHT", "GET RID OF IT BY", "SKIPPED FOR", "DROPS"] {
                caps(ui, h, 9.5, GOLD_DIM);
            }
            ui.end_row();
            for r in &c.rows {
                if r.loc.is_empty() {
                    ui.label(body("your log, not the dump").color(TEXT_3));
                } else {
                    ui.label(mono(loc_badge(&r.loc).text()).color(TEXT)).on_hover_text(r.loc.clone());
                }
                ui.horizontal(|ui| {
                    if ui.add(egui::Label::new(body(&r.n).color(TEXT)).sense(Sense::click())).on_hover_text("Open on the Items screen").clicked() {
                        self.want_item = Some(r.n.clone());
                    }
                    if r.tier > 0 {
                        ui.label(mono(format!("+{}", r.tier)).color(TEXT_2));
                    }
                });
                mono_right(ui, &r.count.to_string(), 40.0, TEXT).on_hover_text(format!("{} here; {} held in all", r.count, r.have));
                if r.wt > 0.0 {
                    mono_right(ui, &format!("{:.1}", r.wt * r.count as f64), 50.0, TEXT_2).on_hover_text(format!("{} each", r.wt));
                } else {
                    ui.label("");
                }
                let who = match r.give_src {
                    Some("item") => "Its own item window says so.",
                    Some("table") => "The wiki's Plane of Sky table says so; the item has no window of its own.",
                    _ => "",
                };
                ui.horizontal(|ui| match r.give {
                    None => {
                        ui.label(body("not known").color(TEXT_3)).on_hover_text("no item page on the wiki and no marker in its table, so nothing here knows whether it is NO DROP");
                    }
                    Some(true) => {
                        ui.label(body("sell to a player").color(TEXT)).on_hover_text(format!("Not NO DROP, so another player can take it. {who} {} class test{} use it.", r.wanted_by, if r.wanted_by == 1 { "" } else { "s" }));
                        if r.clash {
                            chip(ui, "wiki disagrees", IDLE).on_hover_text("the item window and the wiki's Plane of Sky table disagree; the window is what the game shows you");
                        }
                    }
                    Some(false) => {
                        ui.label(body("destroy").color(TEXT)).on_hover_text(format!("{}: nobody else can take it, so destroying it is what frees the slot. {who}", if r.flag.is_empty() { "NO DROP" } else { r.flag }));
                        if r.clash {
                            chip(ui, "wiki disagrees", IDLE).on_hover_text("the item window and the wiki's Plane of Sky table disagree; the window is what the game shows you");
                        }
                    }
                });
                ui.label(body(r.uses.iter().map(|(c, t)| format!("{} {}", c, short_test(c, t))).collect::<Vec<_>>().join(", ")).color(TEXT_2));
                /* where it came from, so throwing it away is a decision about the trip back */
                if r.isl.is_empty() {
                    ui.label(body("no island in the table").color(TEXT_3));
                } else {
                    let mut wh = data.isle_name(&r.isl);
                    if !r.mob.is_empty() {
                        wh.push_str(", ");
                        wh.push_str(&r.mob);
                    }
                    ui.label(body(wh).color(TEXT_3));
                }
                ui.end_row();
            }
        });
    }

    fn apply(&mut self, a: &str, b: &str, op: MarkOp) {
        if let MarkOp::Show = op {
            /* not a mark: nothing to save, and the model does not change */
            self.want_item = Some(a.to_owned());
            return;
        }
        /* The guard is dropped before the save, which takes the same lock. */
        {
            let mut g = self.marks.lock();
            let m = &mut g.marks;
            match op {
                MarkOp::Skip => {
                    let k = mark_key(a, b);
                    if !m.skipped.remove(&k) {
                        m.skipped.insert(k);
                    }
                }
                MarkOp::Track => {
                    let k = mark_key(a, b);
                    if !m.tracked.remove(&k) {
                        m.tracked.insert(k);
                    }
                }
                MarkOp::Held => {
                    let k = item_key(a);
                    if m.held.remove(&k).is_none() {
                        m.held.insert(k, 1);
                    }
                }
                MarkOp::Show => unreachable!("answered above, before the lock"),
            }
            g.rev += 1;
        }
        self.save_marks();
    }
}

impl SkyScreen {
    /// `held` and `reconcile`, in words, for the hover on a held count: which witness
    /// the number came from and what the other one says. None before any log has been read.
    fn witness_words(&self, name: &str) -> Option<String> {
        let led = &self.stream.as_ref()?.led;
        let inv = self.inv.as_ref();
        let h = held(led, inv, name);
        let r = reconcile(led, inv, name);
        let mut out = match r.inv {
            Some(n) => format!(
                "The dump says {n} and the log says {}; the count above is the {}'s word",
                r.log,
                if h.src == "inv" { "dump" } else { "log" }
            ),
            None if inv.is_some() => format!(
                "The dump cannot see this one (a rune); the log says {}",
                r.log
            ),
            None => format!("No dump yet; the log says {}", r.log),
        };
        if r.gap != 0 {
            out.push_str(&format!(" (dump minus log: {:+})", r.gap));
        }
        if h.worn > 0 {
            out.push_str(&format!("; {} worn", h.worn));
        }
        if h.tier > 0 {
            out.push_str(&format!("; held at +{}", h.tier));
        }
        out.push('.');
        Some(out)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MarkOp {
    Skip,
    Track,
    Held,
    /// Not a mark: a click on an item name, routed to the Items screen after the draw.
    Show,
}

/// One rune row: the square that marks it held, its name, and the count.
///
/// A wind rune goes to the currency tab and `/outputfile inventory` cannot export one, so the log
/// is the only witness THE GAME offers for a rune. It is not the only witness this row has: the
/// square below sets a held mark, and a rune held that way is held on the owner's word alone.
///
/// BOTH HOVERS HERE NAMED THE LOG WHICHEVER IT WAS, and that is a way to lose a mark. The square
/// looks identical either way, so "held, on the log's word" over your own mark tells a person the
/// game already knows this, and hides that clicking it clears something nothing in the app can
/// put back. `piece_row` two functions down has worded its hovers off [`PieceModel::src`] since
/// the model was written; this row now does the same off [`RuneModel::src`], in the same words.
fn rune_row(ui: &mut Ui, r: &RuneModel, toggles: &mut Vec<(String, String, MarkOp)>) {
    ui.horizontal(|ui| {
        let has = r.have >= 1;
        let by_mark = r.src == "mark";
        let tip = if !has {
            "Mark as held: the log is the only witness for a rune"
        } else if by_mark {
            "Marked as held. Click to clear"
        } else {
            "held, on the log's word"
        };
        if square(ui, has, SETTLED, true).on_hover_text(tip).clicked() {
            toggles.push((r.n.clone(), String::new(), MarkOp::Held));
        }
        ui.label(body(&r.n).color(if has { TEXT } else { TEXT_2 }));
        let note = if by_mark {
            "you marked this one held; a dump cannot see the currency tab"
        } else {
            "from the log; a dump cannot see the currency tab"
        };
        mono_right(
            ui,
            &format!("{}/1", r.have),
            40.0,
            if has { TEXT } else { TEXT_3 },
        )
        .on_hover_text(note);
    });
}

fn drop_row(
    ui: &mut Ui,
    r: &DropRow,
    src: &BTreeMap<String, Vec<String>>,
    want_item: &mut Option<String>,
    witness: Option<&str>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(if r.track { "\u{2605}" } else { " " }).color(GOLD));
        let dead = r.quest && r.open == 0;
        if ui
            .add(
                egui::Label::new(body(&r.n).color(if !r.quest || dead { TEXT_3 } else { TEXT }))
                    .sense(Sense::click()),
            )
            .on_hover_text("Open on the Items screen")
            .clicked()
        {
            *want_item = Some(r.n.clone());
        }
        for p in &r.places {
            let b = loc_badge(&p.loc);
            let txt = if p.count > 1 {
                format!("{} x{}", b.text(), p.count)
            } else {
                b.text()
            };
            ui.label(mono(txt).color(TEXT_2))
                .on_hover_text(p.loc.clone());
        }
        let hard = r.open > 0 && r.have == 0;
        if r.sold > 0 {
            chip(
                ui,
                &format!("autosold {}x", r.sold),
                if hard { WRONG } else { IDLE },
            );
        }
        if r.vend > 0 {
            chip(ui, &format!("vendored {}x", r.vend), IDLE);
        }
        if r.gone > 0 {
            chip(
                ui,
                &format!("destroyed {}x", r.gone),
                if hard { WRONG } else { IDLE },
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            mono_right(
                ui,
                &(if r.quest {
                    if r.need > 0 {
                        r.need.to_string()
                    } else {
                        "\u{00b7}".to_owned()
                    }
                } else {
                    String::new()
                }),
                40.0,
                if r.need > 0 { TEXT } else { TEXT_3 },
            )
            .on_hover_text("How many more you must loot to finish every test that still wants it");
            mono_right(
                ui,
                &(if r.have > 0 {
                    r.have.to_string()
                } else {
                    "\u{00b7}".to_owned()
                }),
                40.0,
                TEXT,
            )
            .on_hover_text(match witness {
                Some(w) => format!("How many you are holding. {w}"),
                None => "How many you are holding".to_owned(),
            });
            let buys = if let Some(u) = r.uses.first() {
                let mut s = if u.reward.is_empty() {
                    u.short.clone()
                } else {
                    u.reward.clone()
                };
                if u.fin {
                    s.push_str(" \u{2713}");
                }
                if r.uses.len() > 1 {
                    s.push_str(&format!(", {} tests", r.uses.len()));
                }
                s
            } else {
                match src.get(&r.n) {
                    Some(who) if !who.is_empty() => who[0].clone(),
                    _ => "no test wants it".to_owned(),
                }
            };
            let tip = r
                .uses
                .iter()
                .map(|u| {
                    format!(
                        "{}, {}{}{}",
                        u.cls,
                        u.short,
                        if u.reward.is_empty() {
                            String::new()
                        } else {
                            format!(" -> {}", u.reward)
                        },
                        if u.skip {
                            " (skipped)"
                        } else if u.fin {
                            " (done)"
                        } else {
                            ""
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            ui.label(body(buys).color(if r.quest { TEXT_2 } else { TEXT_3 }))
                .on_hover_text(tip);
        });
    });
}

/// The one place this screen touches the dump's shape. The lane contract names
/// `Ingest::inventory() -> Option<&InventoryDump>` and stops there, so the row fields are read here
/// and nowhere else: the integrator reconciles this function alone if the ingest lane's shape
/// moves. Everything downstream (base name, tier, section, worn, badge) is derived from location,
/// name and count, which is all `/outputfile inventory` carries that this screen needs.
/// `rows()` is the ingest lane's non-empty rows; the "Empty" placeholders it keeps for the
/// exaltation socket read are not places a Sky piece can be.
fn dump_rows(d: &crate::ingest::InventoryDump) -> Vec<DumpRow> {
    d.rows()
        .map(|r| DumpRow {
            loc: r.loc.clone(),
            name: r.name.clone(),
            count: r.count,
        })
        .collect()
}

/* ======================================================================= tests == */

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "[Tue Aug 11 20:15:03 2026]";

    /// The real sky.json through the data lane's loader, converted the way the screen converts
    /// it. `testdata::snapshot()` FAILS LOUDLY when the files are absent; GRIMOIRE_NO_DATA=1 is
    /// the only way to skip and it prints SKIPPED ON PURPOSE.
    fn real_sky() -> Option<SkyData> {
        crate::data::testdata::snapshot().map(|s| SkyData::from_snapshot(&s.sky))
    }

    fn rec(fl: &[&str], sb: bool) -> ItemRec {
        ItemRec {
            fl: fl.iter().map(|s| (*s).to_owned()).collect(),
            window: !fl.is_empty() || sb,
            wt: 0.0,
        }
    }

    fn fold(lines: &[String], me: &str) -> Ledger {
        let mut s = Stream::new(me);
        s.feed(lines.iter().map(String::as_str));
        s.led
    }

    fn ln(s: &str) -> String {
        format!("{T} {s}")
    }

    /* rule 1: kept_it / was_sold. A loot line does not mean you kept it. */
    #[test]
    fn autosold_loot_is_not_held() {
        let led = fold(
            &[
                ln("You looted a Bone Chips from a dry bone skeleton's corpse and sold it for 1 silver and 1 copper."),
                ln("You looted a Bone Chips from a dry bone skeleton's corpse and sold it for free."),
                ln("You looted a Bone Chips from a dry bone skeleton's corpse."),
            ],
            "",
        );
        assert_eq!(
            led.looted.get("Bone Chips"),
            Some(&1),
            "only the plain line is kept"
        );
        assert_eq!(
            led.sold.get("Bone Chips"),
            Some(&2),
            "both autosell tails, incl. for free"
        );
        assert_eq!(log_held(&led, "Bone Chips"), 1);
        assert_eq!(led.n, 3);
        assert!(!kept_it(" and sold it for 3 gold."));
        assert!(!kept_it(" to create an Efreeti Standard."));
        assert!(kept_it(" and stored it in your tradeskill depot"));
    }

    /* rule 2: the currency tail is the only way a wind rune ever appears, and both loot phrasings
     * parse (the --You have looted-- form and the plain one). */
    #[test]
    fn rune_in_currency_and_both_loot_phrasings() {
        let led = fold(
            &[
                ln("You looted a Wind Rune Neza from Protector of Sky's corpse and stored it in your currency"),
                ln("--You have looted a Fine Velvet Cloak from Noble Dojorn's corpse.--"),
                ln("You looted 2 Djinni War Blade from Sister of the Spire's corpse."),
                /* a single digit day is space padded by the client */
                "[Mon Aug  3 09:00:00 2026] You looted a Sphinx Claw from Sister of the Spire's corpse.".to_owned(),
            ],
            "",
        );
        assert_eq!(led.looted.get("Wind Rune Neza"), Some(&1));
        assert!(is_rune("Wind Rune Neza"));
        assert_eq!(led.looted.get("Fine Velvet Cloak"), Some(&1));
        assert_eq!(
            led.looted.get("Djinni War Blade"),
            Some(&2),
            "a count on the line is the quantity"
        );
        assert_eq!(led.looted.get("Sphinx Claw"), Some(&1), "space padded day");
        assert_eq!(led.first.as_deref(), Some("Tue Aug 11 20:15:03 2026"));
        assert_eq!(led.last.as_deref(), Some("Mon Aug  3 09:00:00 2026"));
    }

    /* rule 3: qty and baseOf. "1,200" is 1200; tiers and stars come off the name. */
    #[test]
    fn qty_and_base_name() {
        assert_eq!(qty(Some("1,200")), 1200);
        assert_eq!(qty(None), 1);
        assert_eq!(qty(Some("0")), 1);
        assert_eq!(qty(Some("x")), 1);
        assert_eq!(
            base_name("Efreeti Magi Staff +1"),
            ("Efreeti Magi Staff".to_owned(), 1)
        );
        assert_eq!(base_name("Backpack*"), ("Backpack".to_owned(), 0));
        assert_eq!(
            base_name("Dark Cloak of the Sky +12"),
            ("Dark Cloak of the Sky".to_owned(), 10),
            "tier is capped at 10"
        );
        assert_eq!(base_of("Giant Snake Fang +4"), "Giant Snake Fang");
        let led = fold(
            &[ln(
                "You looted a Efreeti Standard +1 from Noble Dojorn's corpse.",
            )],
            "",
        );
        assert_eq!(
            led.looted.get("Efreeti Standard"),
            Some(&1),
            "the log names items at their tier; tracked by base"
        );
    }

    /* rule 4: an offer proves nothing until "You complete the trade with X". */
    #[test]
    fn offer_is_pending_until_the_trade_closes() {
        let open = fold(
            &[ln("You offered 1 Fine Velvet Cloak to Ranger Spirit.")],
            "",
        );
        assert!(
            open.delivered.is_empty(),
            "an offer alone is not a delivery"
        );
        assert!(open.trades.is_empty());
        let closed = fold(
            &[
                ln("You offered 1 Fine Velvet Cloak to Ranger Spirit."),
                ln("You complete the trade with Ranger Spirit."),
            ],
            "",
        );
        assert_eq!(closed.delivered.get("Fine Velvet Cloak"), Some(&1));
        assert_eq!(closed.trades.len(), 1);
        assert_eq!(closed.trades[0].npc, "Ranger Spirit");
        /* a different NPC closing does not close this one */
        let other = fold(
            &[
                ln("You offered 1 Fine Velvet Cloak to Ranger Spirit."),
                ln("You complete the trade with Torgon Blademaster."),
            ],
            "",
        );
        assert!(other.delivered.is_empty());
    }

    /* rule 5: the handback. "I have no need for this, <me>" against an open offer means nothing
     * was handed over; the same line naming someone else is that player's trade, not yours. */
    #[test]
    fn handback_cancels_only_your_own_offer() {
        let mine = fold(
            &[
                ln("You offered 1 Fine Velvet Cloak to Josin Faithbringer."),
                ln("Josin Faithbringer says, 'I have no need for this, Testchar. You can have it back.'"),
                ln("You complete the trade with Josin Faithbringer."),
            ],
            "Testchar",
        );
        assert!(
            mine.delivered.is_empty(),
            "every offer refused: the whole trade came back"
        );
        assert!(mine.trades.is_empty());
        let theirs = fold(
            &[
                ln("You offered 1 Fine Velvet Cloak to Josin Faithbringer."),
                ln("Josin Faithbringer says, 'I have no need for this, Somebodyelse. You can have it back.'"),
                ln("You complete the trade with Josin Faithbringer."),
            ],
            "Testchar",
        );
        assert_eq!(
            theirs.delivered.get("Fine Velvet Cloak"),
            Some(&1),
            "another player's handback is not yours"
        );
        /* partial refusal: two offered, one refused, the trade still counts */
        let partial = fold(
            &[
                ln("You offered 1 Fine Velvet Cloak to Josin Faithbringer."),
                ln("You offered 1 Efreeti Standard to Josin Faithbringer."),
                ln("Josin Faithbringer says, 'I have no need for this, Testchar. You can have it back.'"),
                ln("You complete the trade with Josin Faithbringer."),
            ],
            "Testchar",
        );
        assert_eq!(partial.trades.len(), 1);
    }

    /* rule 6: one trade window at a time, so a cancel empties everything pending. */
    #[test]
    fn cancel_empties_pending() {
        let led = fold(
            &[
                ln("You offered 1 Fine Velvet Cloak to Ranger Spirit."),
                ln("You have cancelled the trade."),
                ln("You complete the trade with Ranger Spirit."),
            ],
            "",
        );
        assert!(led.delivered.is_empty());
        assert!(led.trades.is_empty());
    }

    /* rule 7: destroyed comes off held exactly; a hand sale makes the floor 0 but not held. */
    #[test]
    fn destroyed_and_vendored() {
        let led = fold(
            &[
                ln("You looted a Bone Chips from a dry bone skeleton's corpse."),
                ln("You looted a Bone Chips from a dry bone skeleton's corpse."),
                ln("You successfully destroyed 1 Bone Chips."),
            ],
            "",
        );
        assert_eq!(gone_count(&led, "Bone Chips"), 1);
        assert_eq!(log_held(&led, "Bone Chips"), 1);
        assert_eq!(log_floor(&led, "Bone Chips"), 1);
        let sold = fold(
            &[
                ln("You looted a Jade from a dry bone skeleton's corpse."),
                ln("You receive 3 gold from Merchant Bob for the Jade(s)."),
            ],
            "",
        );
        assert_eq!(vendor_count(&sold, "Jade"), 1);
        assert_eq!(
            log_held(&sold, "Jade"),
            1,
            "a merchant line carries no quantity, so held is not guessed down"
        );
        assert_eq!(log_floor(&sold, "Jade"), 0, "but nothing may floor on it");
    }

    /* rule 8: coin rides the trade lines and is not an item. */
    #[test]
    fn coin_is_not_an_item() {
        let led = fold(
            &[
                ln("You offered 500 Platinum to Franchise."),
                ln("Yarrow has offered you 3,500 platinum."),
                ln("You complete the trade with Franchise."),
            ],
            "",
        );
        assert!(led.trades.is_empty());
        assert!(led.received.is_empty());
        assert!(led.delivered.is_empty());
    }

    /* rule 9: what arrives by player trade, parcel, purchase or combine is held. */
    #[test]
    fn receipts_buys_and_combines_arrive() {
        let led = fold(
            &[
                ln("Franchise has offered you 1 Trueshot Longbow."),
                ln("You complete the trade with Franchise."),
                ln("Verrin hands you the Adamantite Band +1 that was sent from Dorne."),
                ln("You purchased 4 Drom's Champagne from Maleth Ambersong for  1 platinum 2 gold 5 silver 6 copper."),
                ln("You have fashioned the items together to create something new: Efreeti Standard."),
            ],
            "",
        );
        assert_eq!(led.received.len(), 2);
        assert_eq!(led.received[0].via, "trade");
        assert_eq!(led.received[0].from, "Franchise");
        assert_eq!(
            led.received[1].n, "Adamantite Band",
            "parcel names come in at their tier"
        );
        assert_eq!(led.received[1].via, "parcel");
        assert_eq!(log_held(&led, "Trueshot Longbow"), 1);
        assert_eq!(led.bought[0].q, 4);
        assert_eq!(log_held(&led, "Drom's Champagne"), 4);
        assert_eq!(log_held(&led, "Efreeti Standard"), 1);
    }

    /* rule 10: "The Plane of Sky" is the zone; anything else is not. */
    #[test]
    fn saw_sky_only_for_the_plane() {
        assert!(fold(&[ln("You have entered The Plane of Sky.")], "").saw_sky);
        assert!(fold(&[ln("You have entered Plane of Sky.")], "").saw_sky);
        assert!(!fold(&[ln("You have entered Plane of Knowledge.")], "").saw_sky);
        assert!(
            !Stream::new("").line("You have entered The Plane of Sky."),
            "a line without the stamp is not a log line"
        );
    }

    /* rule 11: the dump. Header rows skipped by shape, Empty skipped, storage rows are three
     * columns, exaltation stones are not copies, -SlotN rows are not worn, tier is the max. */
    #[test]
    fn dump_rows_and_held() {
        let text = "Location\tName\tID\tCount\tSlots\r\n\
                    Charm\tA Rabbit Foot\t1001\t1\t0\r\n\
                    Primary\tEfreeti Zweihander +2\t1002\t1\t0\r\n\
                    Primary-Slot1\tEfreeti Zweihander +2\t1002\t1\t0\r\n\
                    General 1\tLarge Bag\t1100\t1\t10\r\n\
                    General 1-Slot2\tDjinni War Blade\t2000\t1\t0\r\n\
                    General 1-Slot3\tEmpty\t0\t0\t0\r\n\
                    Bank5\tDjinni War Blade\t2000\t2\t0\r\n\
                    Equipment\tEfreeti Zweihander\t1002\r\n\
                    Augmentation\tShining Metallic Robes (Exaltation)\t3000\r\n\
                    Location\tName\tID\r\n\
                    KeyRing\tKey of Swords\t4000\r\n";
        let rows = parse_dump_text(text);
        assert_eq!(rows.len(), 9, "two headers and one Empty skipped");
        assert_eq!(
            rows.iter().find(|r| r.loc == "Equipment").map(|r| r.count),
            Some(1),
            "storage rows have no count column"
        );
        let h = held_of_rows(&rows);
        let blade = &h.items[&item_key("Djinni War Blade")];
        assert_eq!(blade.n, 3);
        assert_eq!(blade.places.len(), 2);
        assert_eq!(blade.places[0].sec, "bags");
        assert_eq!(blade.places[1].sec, "bank");
        let zw = &h.items[&item_key("Efreeti Zweihander")];
        assert_eq!(
            zw.n, 3,
            "worn, socketed row and storage copy all count as copies"
        );
        assert_eq!(zw.worn, 1, "the -Slot1 row is not the worn item");
        assert_eq!(zw.tier, 2);
        assert_eq!(
            zw.places
                .iter()
                .find(|p| p.loc == "Equipment")
                .map(|p| p.sec),
            Some("storage")
        );
        assert!(
            !h.items.contains_key(&item_key("Shining Metallic Robes")),
            "a stone is not the item"
        );
        assert_eq!(h.stones.get(&item_key("Shining Metallic Robes")), Some(&1));
        assert_eq!(h.items[&item_key("Key of Swords")].places[0].sec, "keyring");
    }

    /* rule 12: where is it. Bags and bank get numbers, everywhere else a word. */
    #[test]
    fn badges_and_sections() {
        assert_eq!(
            loc_badge("General 1-Slot2"),
            Badge::Bag { n: 1, sub: Some(2) }
        );
        assert_eq!(loc_badge("Bank5"), Badge::Bank { n: 5, sub: None });
        assert_eq!(
            loc_badge("Bank 12-Slot3"),
            Badge::Bank {
                n: 12,
                sub: Some(3)
            }
        );
        assert_eq!(loc_badge("Equipment"), Badge::Word("storage".into()));
        assert_eq!(loc_badge("SharedBank1"), Badge::Word("shared bank".into()));
        assert_eq!(loc_badge("Primary-Slot1"), Badge::Word("worn".into()));
        assert_eq!(loc_badge("Cursor"), Badge::Word("cursor".into()));
        assert_eq!(loc_badge("General 1-Slot2").text(), "bag 1\u{00b7}2");
        assert_eq!(root_loc("General 8-Slot6-Slot7"), "General 8");
        assert_eq!(loc_section("General 8-Slot6-Slot7"), "bags");
        assert_eq!(loc_section("Personal-Depot3"), "depot");
        assert_eq!(loc_section("dragonhoard2"), "hoard");
        assert_eq!(loc_section("Somewhere"), "other");
        assert!(section_index("bags") < section_index("bank"));
        assert!(loc_sort_key("Bank2") < loc_sort_key("Bank10"));
    }

    /* rule 13: held(). The dump is the authority except for runes and for rows it has none of. */
    #[test]
    fn held_prefers_dump_except_runes_and_missing_rows() {
        let led = fold(
            &[
                ln("You looted a Wind Rune Neza from Protector of Sky's corpse and stored it in your currency"),
                ln("You looted a Trueshot Longbow from a dry bone skeleton's corpse."),
                ln("You looted a Djinni War Blade from Sister of the Spire's corpse."),
                ln("You looted a Jade from a dry bone skeleton's corpse."),
                ln("You receive 3 gold from Merchant Bob for the Jade(s)."),
            ],
            "",
        );
        let inv = held_of_rows(&parse_dump_text(
            "Location\tName\tID\tCount\tSlots\nBank5\tDjinni War Blade\t2000\t2\t0\n",
        ));
        assert_eq!(
            held(&led, None, "Djinni War Blade").n,
            1,
            "no dump: the log"
        );
        let d = held(&led, Some(&inv), "Djinni War Blade");
        assert_eq!((d.n, d.src), (2, "inv"), "the dump wins where it has a row");
        assert_eq!(d.places[0].loc, "Bank5");
        let r = held(&led, Some(&inv), "Wind Rune Neza");
        assert_eq!((r.n, r.src), (1, "log"), "a rune is the log's alone");
        let f = held(&led, Some(&inv), "Trueshot Longbow");
        assert_eq!(
            (f.n, f.src),
            (1, "log"),
            "a row the dump dropped is floored by the log"
        );
        let j = held(&led, Some(&inv), "Jade");
        assert_eq!((j.n, j.src), (0, "inv"), "ever hand-sold: no floor");
        let rc = reconcile(&led, Some(&inv), "Djinni War Blade");
        assert_eq!((rc.log, rc.inv, rc.gap), (1, Some(2), 1));
        assert_eq!(reconcile(&led, Some(&inv), "Wind Rune Neza").inv, None);
    }

    /* rule 14: testState. Ready needs everything; skip suppresses; done finishes. */
    #[test]
    fn test_state_rules() {
        let t = Test {
            n: "Warrior Test of Skill".into(),
            say: "skill".into(),
            reward: "Azure Ruby Ring".into(),
            rune: vec!["Wind Rune Neza".into()],
            items: vec![Comp {
                n: "Azure Ring".into(),
                isl: "3".into(),
                mob: "Gorgalosk".into(),
                nodrop: true,
            }],
        };
        assert_eq!(
            needs_of(&t),
            vec!["Wind Rune Neza".to_owned(), "Azure Ring".to_owned()]
        );
        let all = |_: &str| 1u32;
        let none = |_: &str| 0u32;
        assert!(test_state(&t, 0, false, &all).ready);
        assert!(
            !test_state(&t, 0, true, &all).ready,
            "skip is a want and suppresses ready"
        );
        let d = test_state(&t, 1, false, &all);
        assert!(d.finished && !d.ready);
        let m = test_state(&t, 0, false, &none);
        assert_eq!(m.missing.len(), 2);
        let one = |n: &str| u32::from(n == "Azure Ring");
        assert_eq!(
            test_state(&t, 0, false, &one).missing,
            vec!["Wind Rune Neza".to_owned()]
        );
    }

    /* rule 15: disposal and rewardProves. The window wins; the table answers when there is no
     * window; a clash is stated. */
    #[test]
    fn disposal_and_reward_proves() {
        let nd = rec(&["lore", "magic", "no_drop"], false);
        let ok = rec(&["lore", "magic"], true);
        let blank = ItemRec::default();
        assert_eq!(
            disposal(Some(&nd), None),
            Disposal {
                give: Some(false),
                src: Some("item"),
                clash: false
            }
        );
        assert_eq!(
            disposal(Some(&nd), Some(false)),
            Disposal {
                give: Some(false),
                src: Some("item"),
                clash: true
            }
        );
        assert_eq!(
            disposal(Some(&ok), Some(false)),
            Disposal {
                give: Some(true),
                src: Some("item"),
                clash: false
            }
        );
        assert_eq!(
            disposal(None, Some(true)),
            Disposal {
                give: Some(false),
                src: Some("table"),
                clash: false
            }
        );
        assert_eq!(
            disposal(Some(&blank), None),
            Disposal {
                give: None,
                src: None,
                clash: false
            },
            "no window and no marker: not known"
        );
        assert!(reward_proves(Some(&nd)));
        assert!(
            !reward_proves(Some(&ok)),
            "a tradeable reward proves nothing"
        );
        assert!(!reward_proves(None));
        assert!(!reward_proves(Some(&blank)));
    }

    /* rule 16: completions against the REAL tests. A closed trade with the giver counts for the
     * test whose every piece it carries; more pieces wins; a match on nothing is an orphan. */
    #[test]
    fn completions_on_real_data() {
        let Some(d) = real_sky() else { return };
        let war = &d.classes["WAR"];
        assert_eq!(war.giver, "Torgon Blademaster");
        let led = fold(
            &[
                ln("You have entered The Plane of Sky."),
                ln("You offered 1 Djinni War Blade to Torgon Blademaster."),
                ln("You offered 1 Gem of Invigoration to Torgon Blademaster."),
                ln("You offered 1 Wind Rune Jaka to Torgon Blademaster."),
                ln("You complete the trade with Torgon Blademaster."),
                ln("You offered 1 Wind Rune Jaka to Torgon Blademaster."),
                ln("You complete the trade with Torgon Blademaster."),
                ln("You offered 1 Djinni War Blade to Stragen The Hewer."),
                ln("You offered 1 Efreeti Standard to Stragen The Hewer."),
                ln("You offered 1 Wind Rune Jaka to Stragen The Hewer."),
                ln("You complete the trade with Stragen The Hewer."),
            ],
            "",
        );
        let c = completions(&d, &led, "WAR");
        assert_eq!(c.by_test["Warrior Test of Smash"], 1);
        assert_eq!(
            c.orphan, 1,
            "a rune on its own matches no test and is reported"
        );
        assert_eq!(c.by_test.values().sum::<u32>(), 1);
        let b = completions(&d, &led, "BER");
        assert_eq!(
            b.by_test["Berserker Test of Sharpness"], 1,
            "the same blade and rune, at the other giver, is the other class's test"
        );
        assert_eq!(completions(&d, &led, "NOPE"), Completions::default());
    }

    /* rule 17: the boss board on the real isles. Named mobs fold alone, commons fold together. */
    #[test]
    fn boss_board_on_real_data() {
        let Some(d) = real_sky() else { return };
        let board = boss_board(&d, &|n, mob| format!("{n}@{mob}"));
        assert_eq!(board.len(), 9);
        let fairy = board
            .iter()
            .find(|i| i.name == "Fairy Island")
            .expect("isle 1");
        let names: Vec<&str> = fairy.mobs.iter().map(|m| m.n.as_str()).collect();
        assert!(
            names.contains(&"Thunder Spirit Princess"),
            "the boss folds alone"
        );
        assert!(!names.contains(&"A Thunder Spirit"), "a common does not");
        let trash = fairy
            .mobs
            .iter()
            .find(|m| m.n == "Island trash")
            .expect("trash fold");
        assert_eq!(trash.rows.len(), 4);
        assert_eq!(
            trash.src["Belt of Virtue"],
            vec!["A Thunder Spirit".to_owned()]
        );
        assert_eq!(trash.rows[0], "Belt of Transience@Island trash");
        let key = fairy
            .mobs
            .iter()
            .find(|m| m.n == "Key Master")
            .expect("role other folds alone");
        assert!(key.rows.is_empty());
        let drake = board.iter().find(|i| i.isl == "7").expect("isle 7");
        assert_eq!(drake.req, vec!["Key of Scale".to_owned()]);
    }

    /* rule 18: skyIndex and the star. A component two classes want carries both uses. */
    #[test]
    fn index_on_real_data() {
        let Some(d) = real_sky() else { return };
        assert_eq!(d.classes.len(), 16);
        assert_eq!(d.test_count(), 95);
        assert_eq!(d.isles.len(), 9);
        assert_eq!(d.islands.len(), 9);
        assert_eq!(
            d.items.len(),
            3057,
            "every item window came through the conversion"
        );
        assert_eq!(
            d.items.values().filter(|r| !r.window).count(),
            138,
            "the windowless items, as measured on the file"
        );
        assert!(d
            .rec("Djinni War Blade")
            .is_some_and(|r| r.window && r.flag("no_drop")));
        assert_eq!(d.isle_name("7"), "Drake Island (I7)");
        assert_eq!(d.isle_name("1.5"), "Noble Island (I1.5)");
        assert_eq!(d.isle_name("9"), "Isle 9");
        let idx = sky_index(&d);
        let blade = &idx["Djinni War Blade"];
        let mut codes: Vec<&str> = blade.uses.iter().map(|u| u.code.as_str()).collect();
        codes.sort_unstable();
        assert_eq!(codes, vec!["BER", "WAR"]);
        assert_eq!(blade.mark, Some(true));
        assert_eq!(blade.isl, "7");
        assert_eq!(idx["Skycleaver"].pays.len(), 1);
        assert!(
            idx["Skycleaver"].uses.is_empty(),
            "a reward is not a component"
        );
        assert!(reward_proves(d.rec("Efreeti Great Staff")));
        assert!(
            !reward_proves(d.rec("Sphinx Heart Amulet")),
            "one of the two tradeable rewards"
        );
        assert!(
            d.rec("Azarack Skin Wristwraps").is_none(),
            "the one reward with no item window"
        );
        let uses = sky_uses(&d);
        assert_eq!(uses[&item_key("Djinni War Blade")].len(), 2);
    }

    /* rule 19: the cleanout list keys on the skip mark and nothing else, one row per place. */
    #[test]
    fn cleanout_on_real_data() {
        let Some(d) = real_sky() else { return };
        let inv = held_of_rows(&parse_dump_text(
            "Location\tName\tID\tCount\tSlots\nBank5\tDjinni War Blade\t2000\t1\t0\nGeneral 2-Slot1\tDjinni War Blade\t2000\t1\t0\nPrimary\tEfreeti Standard\t2001\t1\t0\n",
        ));
        let led = Ledger::default();
        let have = |n: &str| {
            let h = held(&led, Some(&inv), n);
            HaveFull {
                n: h.n,
                worn: h.worn,
                tier: h.tier,
                places: Some(h.places),
            }
        };
        let one = |code: &str, _t: &str| code == "WAR";
        let c = cleanout_rows(&d, &have, &one);
        assert!(
            c.rows.is_empty(),
            "the blade is still wanted by the Berserker test"
        );
        assert_eq!(c.mixed, 1);
        let both = |code: &str, _t: &str| code == "WAR" || code == "BER";
        let c = cleanout_rows(&d, &have, &both);
        let blades: Vec<&CleanoutRow> = c
            .rows
            .iter()
            .filter(|r| r.n == "Djinni War Blade")
            .collect();
        assert_eq!(blades.len(), 2, "one row per place");
        assert_eq!(blades[0].sec, "bags", "bags come before bank");
        assert_eq!(blades[1].loc, "Bank5");
        assert_eq!(blades[0].give, Some(false), "NO DROP in its own window");
        assert_eq!(blades[0].flag, "NO DROP");
        assert_eq!(blades[0].uses.len(), 2);
        assert!(
            !c.rows.iter().any(|r| r.n == "Efreeti Standard"),
            "the worn copy is left out"
        );
        let skipped_cleric = |code: &str, _t: &str| code == "WAR" || code == "BER" || code == "CLR";
        let c = cleanout_rows(&d, &have, &skipped_cleric);
        assert!(!c.rows.iter().any(|r| r.n == "Efreeti Standard"));
        assert_eq!(c.worn, 1, "and counted as worn");
    }

    /* rule 20: the arranged model on the real data: a turn-in
     * reads done, a full set reads ready, the wrong class's pieces handed back count nothing,
     * a rune looted before the log is only a mark. */
    #[test]
    fn model_on_real_data() {
        let Some(d) = real_sky() else { return };
        let ber = &d.classes["BER"].tests[0];
        let mut lines = vec![ln("You have entered The Plane of Sky.")];
        for i in &ber.items {
            lines.push(ln(&format!(
                "You looted a {} from Sister of the Spire's corpse.",
                i.n
            )));
        }
        lines.push(ln(&format!(
            "You looted a {} from Protector of Sky's corpse and stored it in your currency",
            ber.rune[0]
        )));
        for i in &ber.items {
            lines.push(ln(&format!("You offered 1 {} to Stragen The Hewer.", i.n)));
        }
        lines.push(ln(&format!(
            "You offered 1 {} to Stragen The Hewer.",
            ber.rune[0]
        )));
        lines.push(ln("You complete the trade with Stragen The Hewer."));
        /* the wrong class's piece walked to the cleric: handed back, trade closes, nothing counts */
        let mnk = &d.classes["MNK"].tests[0];
        lines.push(ln(&format!(
            "You looted a {} from a spiroc guardian's corpse.",
            mnk.items[0].n
        )));
        lines.push(ln(&format!(
            "You offered 1 {} to Josin Faithbringer.",
            mnk.items[0].n
        )));
        lines.push(ln(
            "Josin Faithbringer says, 'I have no need for this, Testchar. You can have it back.'",
        ));
        lines.push(ln("You complete the trade with Josin Faithbringer."));
        let led = fold(&lines, "Testchar");
        let marks = Marks::default();
        let have = have_map(&led, None, None, &marks);
        let w = Witness {
            data: &d,
            led: &led,
            inv: None,
            have: &have,
            marks: &marks,
        };
        let m = build_model(&w);
        assert_eq!(m.tot.tests, 95);
        let g = m.classes.iter().find(|g| g.code == "BER").unwrap();
        let t = &g.tests[0];
        assert_eq!(t.done, 1);
        assert!(t.fin && !t.ready);
        assert_eq!(
            t.items[0].have, 0,
            "handed over: the log says you no longer hold it"
        );
        let clr = m.classes.iter().find(|g| g.code == "CLR").unwrap();
        assert_eq!(
            clr.orphan, 0,
            "a handback is not a turn-in and not an orphan"
        );
        assert!(clr.tests.iter().all(|t| t.done == 0));
        let mk = m.classes.iter().find(|g| g.code == "MNK").unwrap();
        assert_eq!(mk.tests[0].items[0].have, 1, "and you still hold the piece");
        assert_eq!(mk.tests[0].items[0].src, "log");
        /* a mark is a floor: a rune looted before /log on */
        let mut marks2 = Marks::default();
        marks2.held.insert(item_key(&mnk.rune[0]), 1);
        let have2 = have_map(&led, None, None, &marks2);
        let w2 = Witness {
            data: &d,
            led: &led,
            inv: None,
            have: &have2,
            marks: &marks2,
        };
        assert_eq!(w2.sky_have(&mnk.rune[0]), (1, "mark"));
        let m2 = build_model(&w2);
        let mk2 = m2.classes.iter().find(|g| g.code == "MNK").unwrap();
        assert_eq!(mk2.tests[0].rune[0].have, 1);
        /* THE WIRING, one line under the assertion that already knew the answer. `sky_have`
         * returned "mark" above and `build_model` dropped it on the floor, so the rune row had
         * nothing to say and said the log. */
        assert_eq!(
            mk2.tests[0].rune[0].src, "mark",
            "the model threw away whose word the rune count is on"
        );
        /* the boss board bills a piece once, however many mobs drop it */
        assert!(m.to_loot > 0);
        let blade_rows: usize = m
            .isles
            .iter()
            .flat_map(|i| &i.mobs)
            .flat_map(|mo| &mo.fold.rows)
            .filter(|r| r.n == "Djinni War Blade")
            .count();
        assert!(blade_rows >= 1);
        let blade = m
            .isles
            .iter()
            .flat_map(|i| &i.mobs)
            .flat_map(|mo| &mo.fold.rows)
            .find(|r| r.n == "Djinni War Blade")
            .unwrap();
        assert_eq!(blade.open, 1, "BER done, WAR still wants it");
        assert_eq!(blade.need, 1);
        assert!(sky_keep(t, Show::Held, false));
        assert!(
            !sky_keep(t, Show::Ready, true),
            "finished with nothing in hand is hidden"
        );
        assert_eq!(m.keys.len(), 9, "the ladder rides on the model");
        assert!(
            m.keys[0].open() && !m.keys[1].open(),
            "Fairy takes nothing; Noble takes a key nothing here holds"
        );
    }

    /* rule 21: the have map with a dump. Post-dump loot lands on top; a pre-dump trade does not
     * come off; a row the dump dropped is floored, minus its stones. */
    #[test]
    fn have_map_with_dump_time() {
        let led = fold(
            &[
                "[Tue Aug 11 10:00:00 2026] You looted a Djinni War Blade from Sister of the Spire's corpse.".to_owned(),
                "[Tue Aug 11 10:05:00 2026] You offered 1 Djinni War Blade to Torgon Blademaster.".to_owned(),
                "[Tue Aug 11 10:05:00 2026] You complete the trade with Torgon Blademaster.".to_owned(),
                "[Tue Aug 11 12:00:00 2026] You looted a Djinni War Blade from Sister of the Spire's corpse.".to_owned(),
                "[Tue Aug 11 12:10:00 2026] Franchise has offered you 1 Trueshot Longbow.".to_owned(),
                "[Tue Aug 11 12:10:00 2026] You complete the trade with Franchise.".to_owned(),
                /* looted BEFORE the dump, and the dump shows the stone it was rendered into:
                 * that is the only order in which a stone can exist */
                "[Tue Aug 11 09:30:00 2026] You looted a Shining Metallic Robes from a dry bone skeleton's corpse.".to_owned(),
            ],
            "",
        );
        let inv = held_of_rows(&parse_dump_text(
            "Location\tName\tID\tCount\tSlots\nBank5\tDjinni War Blade\t2000\t1\t0\nAugmentation\tShining Metallic Robes (Exaltation)\t3000\n",
        ));
        let since = log_ts("Tue Aug 11 11:00:00 2026");
        let marks = Marks::default();
        let h = have_map(&led, Some(&inv), since, &marks);
        assert_eq!(
            h.counts[&item_key("Djinni War Blade")],
            2,
            "dump 1 + loot after the dump; the pre-dump hand-in is already in the dump"
        );
        assert_eq!(
            h.counts[&item_key("Trueshot Longbow")],
            1,
            "received after the dump"
        );
        assert!(
            !h.log_floor.contains(&item_key("Trueshot Longbow")),
            "an arrival after the dump is not a floor"
        );
        assert!(
            !h.counts.contains_key(&item_key("Shining Metallic Robes")),
            "the stone eats the log's copy"
        );
        let empty = SkyData::default();
        let w = Witness {
            data: &empty,
            led: &led,
            inv: Some(&inv),
            have: &h,
            marks: &marks,
        };
        assert_eq!(w.sky_have("Djinni War Blade"), (2, "inv"));
        assert_eq!(
            w.sky_have("Trueshot Longbow"),
            (1, "inv"),
            "the dump exists and the count is arithmetic on it"
        );
        let old = have_map(&led, Some(&inv), log_ts("Tue Aug 11 13:00:00 2026"), &marks);
        assert!(
            old.log_floor.contains(&item_key("Trueshot Longbow")),
            "with everything before the dump, the row the dump dropped is the log's floor"
        );
        assert_eq!(old.counts[&item_key("Trueshot Longbow")], 1);
    }

    /* the stamp: a wrong weekday still parses, a
     * space padded day parses, and nonsense is None rather than a panic */
    #[test]
    fn log_stamp_parses_like_the_client_writes_it() {
        let t = log_ts("Tue Aug 11 20:15:03 2026").expect("a real stamp");
        assert_eq!(
            t.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-08-11 20:15:03"
        );
        assert_eq!(
            log_ts("Mon Aug 11 20:15:03 2026"),
            log_ts("Tue Aug 11 20:15:03 2026"),
            "the weekday carries nothing"
        );
        assert_eq!(
            log_ts("Mon Aug  3 09:00:00 2026").map(|t| t.format("%d").to_string()),
            Some("03".to_owned())
        );
        assert_eq!(log_ts("not a stamp"), None);
        assert!(
            !after("not a stamp", None),
            "an unreadable stamp never counts as after the dump"
        );
        assert!(after("Tue Aug 11 20:15:03 2026", None));
        assert!(!after(
            "Tue Aug 11 20:15:03 2026",
            log_ts("Tue Aug 11 21:00:00 2026")
        ));
    }

    #[test]
    fn item_key_rules() {
        assert_eq!(item_key("Ghoul's  Heart "), "ghouls heart");
        assert_ne!(
            item_key("A Sapphire"),
            item_key("Sapphire"),
            "articles are identity"
        );
        assert_eq!(
            mark_key("WAR", "Warrior Test of Skill"),
            "WAR::Warrior Test of Skill"
        );
        assert_eq!(
            short_test("SHD", "Shadow Knight Test of Fear"),
            "Test of Fear"
        );
        assert_eq!(class_name("SHD"), "Shadow Knight");
    }

    /* rule 22: the conversion keeps the item-window rule. Flags alone are a window, a stat
     * block alone is a window, neither is none (and then the table's marker answers). A
     * component's missing island reads as "", the way the file spells it. */
    #[test]
    fn conversion_keeps_the_window_rule() {
        use crate::data::sky::{SkyItem, SkyTestItem};
        let flags = SkyItem {
            name: "Djinni War Blade".into(),
            key: "djinni war blade".into(),
            fl: vec!["no_drop".into()],
            wt: Some(2.5),
            ..Default::default()
        };
        let block = SkyItem {
            name: "Plain Thing".into(),
            key: "plain thing".into(),
            sb: vec!["WT: 1.0".into()],
            ..Default::default()
        };
        let ghost = SkyItem {
            name: "Ghost".into(),
            key: "ghost".into(),
            ..Default::default()
        };
        let a = item_rec(&flags);
        assert!(a.window && a.flag("no_drop"));
        assert_eq!(a.weight(), 2.5);
        assert!(item_rec(&block).window, "a stat block alone is a window");
        assert!(!item_rec(&ghost).window, "neither: no window");
        assert_eq!(item_rec(&ghost).weight(), 0.0);
        assert_eq!(
            disposal(Some(&item_rec(&ghost)), Some(true)).src,
            Some("table")
        );
        assert_eq!(
            disposal(Some(&item_rec(&block)), Some(true)),
            Disposal {
                give: Some(true),
                src: Some("item"),
                clash: true
            }
        );
        let c = comp_of(&SkyTestItem {
            name: "Azure Ring".into(),
            isl: Some("3".into()),
            mob: None,
            nodrop: true,
            ..Default::default()
        });
        assert_eq!(
            (c.n.as_str(), c.isl.as_str(), c.mob.as_str(), c.nodrop),
            ("Azure Ring", "3", "", true)
        );
    }

    /* rule 23: the island ladder. Each isle's `req` is what it takes; the isle whose `key` list
     * names it is where it drops; a key no isle lists is said to be unlisted, not guessed. Keys
     * are counted off the dump (key ring or bag) like any other piece. */
    #[test]
    fn key_ladder_on_real_data() {
        let Some(d) = real_sky() else { return };
        let inv = held_of_rows(&parse_dump_text(
            "Location\tName\tID\tCount\tSlots\nKeyRing\tKey of Misfortune\t5000\nGeneral 3-Slot1\tKey of Scale\t5001\t1\t0\n",
        ));
        let led = Ledger::default();
        let have = |n: &str| {
            let h = held(&led, Some(&inv), n);
            (h.n, h.src)
        };
        let steps = key_ladder(&d, &have);
        let ids: Vec<&str> = steps.iter().map(|s| s.isl.as_str()).collect();
        assert_eq!(ids, ["1", "1.5", "2", "3", "4", "5", "6", "7", "8"]);
        assert!(
            steps[0].req.is_empty() && steps[0].open(),
            "Fairy Island takes nothing"
        );
        assert_eq!(steps[1].req[0].n, "Key of Swords");
        assert_eq!(
            steps[1].req[0].from, None,
            "no isle in the file lists Key of Swords as a drop"
        );
        assert!(!steps[1].open());
        let harpy = steps.iter().find(|s| s.isl == "3").expect("isle 3");
        assert_eq!(
            harpy.req,
            vec![KeyNeed {
                n: "Key of Misfortune".into(),
                from: Some("2".into()),
                have: 1,
                src: "inv"
            }]
        );
        assert!(harpy.open(), "a key on the key ring counts");
        assert_eq!(harpy.drops, vec!["Animal Figurine".to_owned()]);
        let drake = steps.iter().find(|s| s.isl == "7").expect("isle 7");
        assert!(drake.open(), "a key in a bag counts");
        assert_eq!(drake.req[0].from.as_deref(), Some("6"));
        let bee = steps.iter().find(|s| s.isl == "6").expect("isle 6");
        assert_eq!((bee.open(), bee.req[0].have), (false, 0));
        for k in ["Key of Swords", "Veeshan's Key", "Key of Misfortune"] {
            assert!(
                d.rec(k).is_none(),
                "{k} has no item window in sky.json, as measured"
            );
        }
    }

    /* rule 24: the screen's own cursor. A turn-in already in the file when the app starts is
     * counted by the bootstrap; one appended after it is counted by the live read; nothing is
     * counted twice however the polls fall; a file that shrinks under the cursor is noticed. */
    #[test]
    fn bootstrap_then_live_counts_each_line_once() {
        let dir = std::env::temp_dir().join("grimoire-sky-cursor-test");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("eqlog_Testchar_legends.txt");
        let early = format!(
            "{}\r\n{}\r\n{}\r\n",
            ln("You have entered The Plane of Sky."),
            ln("You offered 1 Fine Velvet Cloak to Ranger Spirit."),
            ln("You complete the trade with Ranger Spirit.")
        );
        std::fs::write(&p, &early).unwrap();
        let b = boot_read(&p, "Testchar");
        assert_eq!(b.problem, None);
        assert_eq!(b.lines, 3);
        assert_eq!(b.start, 0, "no cap cut on a three line file");
        assert_eq!(
            b.cursor.offset,
            early.len() as u64,
            "the cursor sits where the bootstrap stopped"
        );
        let (mut cur, mut s) = (b.cursor, b.stream);
        assert_eq!(
            s.led.trades.len(),
            1,
            "the turn-in from before launch counts"
        );
        assert!(s.led.saw_sky);
        /* nothing appended: nothing fed, nothing double counted */
        assert_eq!(live_read(&p, &mut cur, &mut s), Ok(Live::Fed(0)));
        assert_eq!(s.led.n, 3);
        /* a partial line waits; its completion is fed once */
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        use std::io::Write;
        write!(f, "{}", ln("You offered 1 Efreeti Standard to Ranger Spir")).unwrap();
        assert_eq!(live_read(&p, &mut cur, &mut s), Ok(Live::Fed(0)));
        writeln!(f, "it.\r").unwrap();
        writeln!(f, "{}\r", ln("You complete the trade with Ranger Spirit.")).unwrap();
        drop(f);
        assert_eq!(live_read(&p, &mut cur, &mut s), Ok(Live::Fed(2)));
        assert_eq!(s.led.trades.len(), 2);
        assert_eq!(s.led.delivered.get("Efreeti Standard"), Some(&1));
        assert_eq!(
            live_read(&p, &mut cur, &mut s),
            Ok(Live::Fed(0)),
            "the same bytes are never read twice"
        );
        assert_eq!(s.led.trades.len(), 2);
        /* the file shrinks: a new log took the name */
        std::fs::write(&p, &early).unwrap();
        assert_eq!(live_read(&p, &mut cur, &mut s), Ok(Live::Shrank));
        /* a missing file is an error with the path in it, not a panic */
        let gone = dir.join("eqlog_Nobody_legends.txt");
        let _ = std::fs::remove_file(&gone);
        assert!(boot_read(&gone, "Nobody")
            .problem
            .as_deref()
            .unwrap_or("")
            .contains("eqlog_Nobody"));
        assert!(live_read(&gone, &mut cur, &mut s)
            .unwrap_err()
            .contains("eqlog_Nobody"));
        /* the cap cuts the first partial line and says where it started */
        let capped = read_tail_capped(&p, early.len() as u64, 40).unwrap();
        assert!(
            capped.start > 0 && !capped.text.starts_with('['),
            "the first, partial line is dropped"
        );
        let _ = std::fs::remove_file(&p);
    }

    /* The screen draws headless: no snapshot says the data is absent; the real snapshot with no
     * log and no dump says WAITING; every view paints without a panic and clicks nothing. Running
     * `ui` here is also what keeps the dead-code lint honest about the screen's own fields. */
    #[test]
    fn screen_draws_headless_without_panicking() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let mut settings = crate::settings::Settings::default();
        let log_dir = std::env::temp_dir().join("grimoire-sky-smoke-logs");
        let _ = std::fs::create_dir_all(&log_dir);
        settings.log_dir = Some(log_dir);
        let snap = crate::data::testdata::snapshot();
        settings.data_root = snap.map(|s| s.report().root);
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked("Broken_Stoic"),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        /* A cell of this test's own, already read, so nothing here goes near the owner's file
         * and no other test can see what this one marks. */
        let marks = MarksCell::isolated();
        marks.lock().loaded = true;
        let mut screen = SkyScreen {
            marks,
            ..Default::default()
        };
        let mut passes = 0;
        for (data, err, view) in [
            (None, Some("no snapshot: smoke test"), View::Giver),
            (snap, None, View::Giver),
            (snap, None, View::Boss),
            (snap, None, View::Keys),
            (snap, None, View::Cleanout),
        ] {
            screen.set_view(view);
            let mut cx = crate::screens::Cx {
                data,
                railed: false,
                data_err: err,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                chat: crate::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                player: Default::default(),
                stage: None,
                demand: None,
                ask: Default::default(),
            };
            let out = ctx.run_ui(egui::RawInput::default(), |ui| screen.ui(ui, &mut cx));
            assert!(!out.shapes.is_empty(), "nothing was painted for {view:?}");
            assert_eq!(cx.ask, crate::screens::Ask::None, "nothing was clicked");
            /* headless: there is no renderer to hand the font atlas to, and epaint panics on a
             * dropped delta unless told the drop is deliberate */
            out.drop_without_applying_deltas();
            passes += 1;
        }
        assert_eq!(passes, 5);
        if snap.is_some() {
            assert!(
                matches!(screen.data, Some(Loaded { res: Ok(_), .. })),
                "the snapshot's sky part converted"
            );
            assert!(
                screen.model.is_some(),
                "the model builds with no witness at all"
            );
            assert!(
                screen.stream.is_none() && screen.inv.is_none(),
                "an empty log folder gives no witness"
            );
        }
    }

    /* ------------------------------------------------------ the marks file is not disposable --
     *
     * `Marks::load` used to answer an unparsable file with `Marks::default()` and the words
     * "starting empty", and the next mark click wrote that empty set over the file, atomically
     * enough to be final. Every skip, track and held count in there was typed by hand and nothing
     * in this app can recreate one.
     *
     * EVERY ASSERTION BELOW IS ON THE BYTES ON DISK, not on a return value: a guard that reports
     * an error after the file is already gone is not a guard.
     *
     * None of these go near the owner's file. `Marks::save` and `SkyScreen::start_fresh` are the
     * only two things that know `Marks::path`, and both refuse outright under cfg(test). */

    /// A scratch marks file of this test's own, named for the test and this process.
    fn marks_scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grimoire-sky-marks-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir.join("sky-marks.json")
    }

    /// Marks worth losing: one of each kind, all three hand made.
    const REAL_MARKS: &str = r#"{"skipped":["ROG::Test of Wind"],"tracked":["WAR::Test of Sky"],"held":{"efreetistandard":3}}"#;
    /// The classic hand edit: a quoted number where a number belongs. It does not parse, and every
    /// mark above is still legible in it.
    const HAND_BROKEN: &str = r#"{"skipped":["ROG::Test of Wind"],"tracked":["WAR::Test of Sky"],"held":{"efreetistandard":"3"}}"#;

    fn still_has_the_marks(text: &str, why: &str) {
        for want in ["ROG::Test of Wind", "WAR::Test of Sky", "efreetistandard"] {
            assert!(
                text.contains(want),
                "{why}: {want:?} is gone. On disk now: {text}"
            );
        }
    }

    /// TWO SKY SCREENS, ONE MARKS FILE, AND NEITHER MAY EAT THE OTHER'S MARKS.
    ///
    /// The main window owns a `SkyScreen` and the popped-out Sky tool window owns a SECOND one
    /// (`windows::Screen::new`). While each held its own `Marks`, this sequence lost a hand typed
    /// mark in silence: both read the file, the first marked something and saved, the second
    /// marked something else and wrote its own whole copy over the top. On disk afterwards there
    /// was no sign of the first mark, no copy was kept beside the file, and neither screen had
    /// anything to say, because by the guard's lights nothing was wrong. Measured on the bytes,
    /// before the fix: has_A=false has_B=true kept=[] and both screens reporting no problem.
    ///
    /// NO GUARD CAN CATCH THIS. `write_file` refuses for a session that never read the file and
    /// for a file nobody can read; a lost update is neither. One owner prevents it, so the test
    /// is about ownership.
    ///
    /// BOTH SCREENS ARE BORN THE WAY PRODUCTION BORNS THEM, by `Default`, which is the only way
    /// anything outside this module can make one. That is the point: this is not asking whether
    /// two screens handed the same cell agree, it is asking whether two screens made the ordinary
    /// way ARE one cell.
    ///
    /// THIS IS THE ONE TEST IN THIS BINARY THAT USES THE PROCESS-WIDE CELL, deliberately, because
    /// the sharing is the thing under test; every other test here takes `MarksCell::isolated`. It
    /// still goes nowhere near `Marks::path`: every read and every write below names a scratch
    /// file of this test's own.
    #[test]
    fn two_sky_screens_do_not_eat_each_others_marks() {
        let p = marks_scratch("two-screens");
        std::fs::write(&p, REAL_MARKS).expect("seed");

        let a = SkyScreen::default();
        let mut b = SkyScreen::default();
        /* Both draw, so both run the once-per-run read that `ui` runs. The second is a no-op. */
        a.marks.load_once_from(&p);
        b.marks.load_once_from(&p);
        assert_eq!(
            a.marks.lock().marks.held.get("efreetistandard"),
            Some(&3),
            "the seeded marks were never read, so the rest proves nothing"
        );
        b.marks_changed();

        {
            let mut g = a.marks.lock();
            g.marks.tracked.insert(mark_key("CLR", "Test of Patience"));
            g.rev += 1;
        }
        a.save_marks_to(&p);

        /* The second window is looking at the first window's mark before it has saved anything:
         * one file, one set of marks, two views of it. */
        assert!(
            b.marks
                .lock()
                .marks
                .tracked
                .contains(&mark_key("CLR", "Test of Patience")),
            "the second Sky window is drawing a different set of marks from the first"
        );
        assert!(
            b.marks_changed(),
            "the second window has no way to notice that the marks moved, so its board would go \
             on drawing the old ones"
        );
        assert!(!b.marks_changed(), "and it notices once, not every pass");

        {
            let mut g = b.marks.lock();
            g.marks.skipped.insert(mark_key("MNK", "Test of Balance"));
            g.rev += 1;
        }
        b.save_marks_to(&p);

        let after = std::fs::read_to_string(&p).expect("the file is still there");
        assert!(
            after.contains("CLR::Test of Patience"),
            "the second window's save ate the first window's mark: {after}"
        );
        assert!(
            after.contains("MNK::Test of Balance"),
            "the second window's own mark never reached the file: {after}"
        );
        still_has_the_marks(&after, "the marks already in the file were eaten");

        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// HALF TWO OF THE GUARD, WHICH IS THE HALF THAT SHIPPED BROKEN IN THE SETTINGS FILE.
    ///
    /// A file guard alone asks "does the file on disk parse", and this is the sequence that walks
    /// straight through it, recommended by the error message itself:
    ///   1. the marks file is malformed, so `load` hands the screen empty marks with
    ///      `load_problem` set. `load` runs ONCE, from `ui`, and nothing ever re-reads.
    ///   2. a mark click saves, the guard refuses, the file is untouched. Good.
    ///   3. the owner repairs the JSON by hand, as told, WITHOUT restarting. The file is whole.
    ///   4. the next click saves; a file-only guard sees a file that parses, stands down, and
    ///      writes this session's EMPTY set over every mark in it.
    #[test]
    fn a_marks_file_repaired_by_hand_is_not_eaten_by_a_session_that_never_read_it() {
        let p = marks_scratch("repaired");

        /* 1. */
        std::fs::write(&p, HAND_BROKEN).expect("seed");
        let (session, problem) = Marks::load_from(&p);
        assert!(problem.is_some(), "a quoted count is not valid marks JSON");
        assert!(
            session.skipped.is_empty() && session.tracked.is_empty() && session.held.is_empty(),
            "the session is holding nothing, which is what makes a save so dangerous"
        );

        /* 2. */
        session
            .write_file(&p, OnUnreadable::Refuse)
            .expect_err("a broken marks file must not be replaced by an ordinary save");
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            HAND_BROKEN,
            "the refused save touched the file anyway"
        );

        /* 3. the owner repairs it by hand and does NOT restart */
        std::fs::write(&p, REAL_MARKS).expect("repair");
        let (repaired, none) = Marks::load_from(&p);
        assert!(
            none.is_none() && repaired.held.get("efreetistandard") == Some(&3),
            "the repaired file must be valid, or step 4 proves nothing"
        );

        /* 4. the SAME session object from step 1 saves again */
        let out = session.write_file(&p, OnUnreadable::Refuse);

        let after = std::fs::read_to_string(&p).expect("the file is still there");
        still_has_the_marks(
            &after,
            "a session that never read the marks ate the repaired file",
        );
        assert_eq!(
            after, REAL_MARKS,
            "the repaired file was rewritten at all; a session holding none of the owner's marks \
             has nothing to say about that file"
        );
        out.expect_err("the save must report that it refused, not silently succeed");

        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// HALF ONE: the file on disk cannot be read, so it is not replaced. This is the case where a
    /// different process broke the file after this session read it cleanly, so `load_problem` is
    /// NOT set and half two would let the write through.
    #[test]
    fn an_unreadable_marks_file_is_never_replaced_by_an_empty_set() {
        let p = marks_scratch("unreadable");
        std::fs::write(&p, HAND_BROKEN).expect("seed");

        /* A session that read the file cleanly earlier: no load_problem at all. */
        let session = Marks::default();
        let refused = session
            .write_file(&p, OnUnreadable::Refuse)
            .expect_err("marks nobody could read must not be replaced by an empty set");
        assert!(
            refused.why.contains(&p.display().to_string()),
            "the refusal must name the file: {refused:?}"
        );
        assert!(
            refused.start_fresh_helps,
            "these are marks worth protecting, so the screen must offer the way out: {refused:?}"
        );

        still_has_the_marks(
            &std::fs::read_to_string(&p).expect("the file is still there"),
            "the owner's marks were destroyed by a save",
        );
        assert!(
            !p.with_extension("json.tmp").exists(),
            "a refused save left its temp file behind"
        );

        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// AN EMPTY FILE IS NOT A BROKEN ONE. There are no bytes in it to lose, and treating it as
    /// broken would wedge the screen out of ever saving over nothing at all. Both sides have to
    /// agree about this, the loader and the write guard, or a save is refused for a file the
    /// loader was perfectly happy with.
    ///
    /// The saved file is read back through `load_from` as well, which is what proves
    /// `load_problem` is a fact about the run and never a key in the file.
    #[test]
    fn an_empty_marks_file_is_not_treated_as_broken() {
        for (tag, seed) in [("empty", ""), ("blank", "  \r\n\t ")] {
            let p = marks_scratch(tag);
            std::fs::write(&p, seed).expect("seed");

            let (m, problem) = Marks::load_from(&p);
            assert!(
                problem.is_none() && m.load_problem.is_none(),
                "an empty marks file is the same answer as no marks file: {problem:?}"
            );

            let mut session = m;
            session.tracked.insert(mark_key("WAR", "Test of Sky"));
            session
                .write_file(&p, OnUnreadable::Refuse)
                .expect("an empty file has nothing to lose and must not refuse the save");

            let text = std::fs::read_to_string(&p).expect("written");
            assert!(
                text.contains("WAR::Test of Sky"),
                "the mark did not reach the file: {text}"
            );
            assert!(
                !text.contains("load_problem"),
                "load_problem is a fact about this run, never a key in the file: {text}"
            );
            let (back, none) = Marks::load_from(&p);
            assert!(none.is_none() && back.tracked.len() == 1);
            assert!(
                !p.with_extension("json.tmp").exists(),
                "the temp file was left behind"
            );

            let _ = std::fs::remove_dir_all(p.parent().unwrap());
        }
    }

    /// A KEY THIS BUILD DOES NOT NAME SURVIVES A SAVE, which is what the file half of the guard
    /// rests on. That half stands down for a file that parses, on the premise that whatever it
    /// held is already in the `Marks` about to be written. Without `extra` the premise is false
    /// the day a later build writes a fourth key and this one is run again over it, and the loss
    /// would be silent, because a file that parses is never even kept beside itself.
    #[test]
    fn a_key_this_build_does_not_name_survives_a_save() {
        let p = marks_scratch("extra");
        std::fs::write(
            &p,
            r#"{"skipped":["ROG::Test of Wind"],"tracked":[],"held":{},"pinned":["WAR::Test of Sky"]}"#,
        )
        .expect("seed");

        let (mut session, problem) = Marks::load_from(&p);
        assert!(problem.is_none(), "a key from a later build is not a break");
        assert!(
            session.extra.contains_key("pinned"),
            "the unnamed key was dropped on the way in: {:?}",
            session.extra
        );
        session.tracked.insert(mark_key("WAR", "Test of Sky"));
        session
            .write_file(&p, OnUnreadable::Refuse)
            .expect("a file that parses is replaced as before");

        let after = std::fs::read_to_string(&p).expect("written");
        assert!(
            after.contains("\"pinned\"") && after.contains("WAR::Test of Sky"),
            "a key this build does not name was eaten by a save: {after}"
        );

        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// THE WRITE IS ATOMIC: a temp file and a rename, not a bare write over the real file.
    ///
    /// Proved by making the temp file impossible to create (a directory sits on its name) and
    /// showing the marks already there are untouched. A bare `std::fs::write` to the real path,
    /// which is what this used to be, would sail past that and truncate the file first.
    #[test]
    fn the_write_goes_through_a_temp_file_and_a_rename() {
        let p = marks_scratch("atomic");
        std::fs::write(&p, REAL_MARKS).expect("seed");
        std::fs::create_dir_all(p.with_extension("json.tmp")).expect("block the temp name");

        let mut session = Marks::load_from(&p).0;
        session.skipped.clear();
        session.tracked.clear();
        session.held.clear();
        let out = session.write_file(&p, OnUnreadable::Refuse);

        assert!(
            out.is_err(),
            "a write that could not reach its temp file reported success"
        );
        still_has_the_marks(
            &std::fs::read_to_string(&p).expect("the file is still there"),
            "a torn write truncated the marks file",
        );

        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// A REFUSED SAVE LEAVES THE SCREEN ABLE TO SAY SO, AND TO OFFER THE WAY OUT.
    ///
    /// This is the click path: `apply` toggles a mark and calls `save_marks`, which is this on the
    /// platform file. The refusal message ends in the words "start fresh", and the button that
    /// says it is painted on `MarksState::refused` alone in this case, because a session that read
    /// the file cleanly has no `load_problem` to paint on. If this flag did not come from the same
    /// `Refused` the guard returned, the screen would name a control it does not draw.
    #[test]
    fn a_refused_save_tells_the_screen_that_starting_fresh_is_the_way_out() {
        let p = marks_scratch("refused");
        std::fs::write(&p, HAND_BROKEN).expect("seed");

        /* a session that read the file cleanly, before something else broke it */
        let marks = MarksCell::isolated();
        marks.lock().loaded = true;
        let screen = SkyScreen {
            marks,
            ..Default::default()
        };
        screen
            .marks
            .lock()
            .marks
            .tracked
            .insert(mark_key("WAR", "Test of Sky"));
        screen.save_marks_to(&p);

        assert!(
            screen.marks.lock().refused,
            "the screen was not told that starting fresh is the way out of this refusal"
        );
        let why = screen
            .marks
            .lock()
            .problem
            .clone()
            .expect("a refusal to paint");
        assert!(
            why.contains("start fresh"),
            "the refusal names a control: {why}"
        );
        still_has_the_marks(
            &std::fs::read_to_string(&p).expect("the file is still there"),
            "a click destroyed the marks it could not read",
        );

        /* and a save that works clears both, so neither outlives its reason */
        std::fs::write(&p, REAL_MARKS).expect("repair from another process");
        screen.save_marks_to(&p);
        let g = screen.marks.lock();
        assert!(
            g.problem.is_none() && !g.refused,
            "a save that worked left the refusal on screen: {:?}",
            g.problem
        );
        drop(g);

        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// THE DELIBERATE WAY TO START FRESH, END TO END, THROUGH THE SCREEN'S OWN HANDLER.
    ///
    /// The person is allowed to replace marks nobody can read. What they are not allowed to do is
    /// lose them by accident, so the old bytes are kept beside the file FIRST and the screen is
    /// told where they went. Afterwards this session's marks are the file, so ordinary saves work
    /// again: a refusal that outlives its reason is a screen that never saves again.
    ///
    /// The second half proves two failures cannot pick the same name: `keep_a_copy` stamps to the
    /// second and both of these land inside one.
    #[test]
    fn starting_fresh_keeps_the_old_marks_and_never_clobbers_a_kept_copy() {
        let p = marks_scratch("fresh");
        std::fs::write(&p, HAND_BROKEN).expect("seed");

        let (marks, problem) = Marks::load_from(&p);
        let cell = MarksCell::isolated();
        {
            let mut g = cell.lock();
            g.marks = marks;
            g.problem = problem;
            g.loaded = true;
        }
        let screen = SkyScreen {
            marks: cell,
            ..Default::default()
        };
        screen
            .marks
            .lock()
            .marks
            .skipped
            .insert(mark_key("ROG", "Test of Wind"));

        screen.start_fresh_at(&p);
        let kept = screen
            .marks
            .lock()
            .kept
            .clone()
            .expect("the unreadable marks must be kept, not dropped");
        assert_eq!(
            std::fs::read_to_string(&kept).expect("the kept copy is a real file"),
            HAND_BROKEN,
            "the kept copy is not the file that was there"
        );
        let g = screen.marks.lock();
        assert!(
            g.problem.is_none() && g.marks.load_problem.is_none(),
            "the refusal outlived its reason: {:?}",
            g.problem
        );
        drop(g);
        let after = std::fs::read_to_string(&p).expect("the file was written");
        assert!(
            after.contains("ROG::Test of Wind") && !after.contains("efreetistandard"),
            "the fresh file is not what this session is holding: {after}"
        );

        /* an ordinary save works again, and does not refuse on the file it just wrote */
        {
            let mut g = screen.marks.lock();
            g.marks.tracked.insert(mark_key("WAR", "Test of Sky"));
            g.marks
                .write_file(&p, OnUnreadable::Refuse)
                .expect("after starting fresh, ordinary saves must work again");
        }

        /* a SECOND unreadable file, inside the same second, must not overwrite the first copy */
        std::fs::write(&p, HAND_BROKEN.replace("Wind", "Rain")).expect("break it again");
        screen.start_fresh_at(&p);
        let kept2 = screen
            .marks
            .lock()
            .kept
            .clone()
            .expect("the second copy is kept too");
        assert_ne!(kept, kept2, "the second copy took the first copy's name");
        assert_eq!(
            std::fs::read_to_string(&kept).unwrap(),
            HAND_BROKEN,
            "the first kept copy was written over by the second"
        );
        assert!(std::fs::read_to_string(&kept2).unwrap().contains("Rain"));

        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// THE REFUSAL REACHES A PIXEL ON THE SKY SCREEN, PROVED BY PAINTING IT.
    ///
    /// This is the reachability check. A guard whose refusal reaches only a log line leaves the
    /// owner believing their marks saved, which is the same loss with an extra step, so a test
    /// that asserted the field rather than the paint would prove nothing. It runs a real frame
    /// through `SkyScreen::ui` and reads the strings out of the shapes egui produced.
    ///
    /// BOTH REFUSALS ARE PAINTED, because both of their messages end in the words "start fresh"
    /// and a screen that says that while painting no such control has sent the owner looking for
    /// something that is not there. The session half is known before any click; the file half is
    /// only known once a save has tried, which is a different field and was very nearly a
    /// different answer.
    #[test]
    fn the_sky_screen_paints_the_marks_refusal_and_the_way_out() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let why = "C:/x/sky-marks.json: your marks are not valid JSON";
        /* the session half (load could not read it), then the file half (a save was refused for a
         * file this session HAD read cleanly, so `load_problem` is None) */
        for (case, marks, refused) in [
            (
                "the session never read the file",
                Marks {
                    load_problem: Some(why.to_owned()),
                    ..Marks::default()
                },
                false,
            ),
            ("a save was refused", Marks::default(), true),
        ] {
            let mut settings = crate::settings::Settings::default();
            let mut ingest = crate::ingest::Ingest::new(&settings);
            let live = crate::watcher::Status {
                twitch: crate::watcher::Channel::unchecked("Broken_Stoic"),
                youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
            };
            let cell = MarksCell::isolated();
            {
                let mut g = cell.lock();
                g.marks = marks;
                g.loaded = true;
                g.problem = Some(why.to_owned());
                g.refused = refused;
                g.kept = Some(PathBuf::from(
                    "C:/x/sky-marks.json.unreadable-20260902-101500",
                ));
            }
            let mut screen = SkyScreen {
                marks: cell,
                ..Default::default()
            };
            let mut cx = crate::screens::Cx {
                data: None,
                railed: false,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                chat: crate::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                player: Default::default(),
                stage: None,
                demand: None,
                ask: Default::default(),
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 1400.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();

            fn flatten(sh: egui::Shape, into: &mut Vec<egui::Shape>) {
                match sh {
                    egui::Shape::Vec(v) => {
                        for x in v {
                            flatten(x, into);
                        }
                    }
                    other => into.push(other),
                }
            }
            let mut flat = Vec::new();
            for cs in shapes {
                flatten(cs.shape, &mut flat);
            }
            let words: Vec<String> = flat
                .iter()
                .filter_map(|sh| match sh {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
            let joined = words.join("\u{1F}");
            for want in [
                "your marks are not valid JSON",
                "Nothing you mark is being saved",
                "RESTART",
                "start fresh",
                "sky-marks.json.unreadable-20260902-101500",
            ] {
                assert!(
                    joined.contains(want),
                    "with {case}, the Sky screen never says {want:?}. It painted: {words:?}"
                );
            }
            assert_eq!(cx.ask, crate::screens::Ask::None, "nothing was clicked");
        }
    }

    /* ============================================ whose word is a held rune on ==
     *
     * A wind rune goes to the currency tab and /outputfile inventory cannot export one, so the
     * log is the only witness THE GAME has for a rune. It is not the only witness the screen has:
     * the square on a rune row sets a held mark, and a rune held that way is held on the owner's
     * own word. Both hovers on that row named the log whichever it was.
     *
     * THAT IS A WAY TO LOSE A MARK, which is why it is tested here rather than filed as a wording
     * nit. The square looks identical for both witnesses, so a hover saying "held, on the log's
     * word" over your own mark tells a person the game already knows this, and hides that the
     * click clears something nothing in this app can put back: MarkOp::Held removes the entry
     * outright, and marking it again writes 1, whatever the number was.
     *
     * PieceModel has carried src since the model was written and piece_row words its hovers from
     * it. The rune path threw it away at build_model and hard coded the log at the row.
     */

    /// `sky_have` names the witness THE NUMBER ON SCREEN came from, not the witness the item kind
    /// usually has. Three cases, and the third is the one the old expression got wrong.
    #[test]
    fn a_rune_held_by_your_own_mark_is_held_on_your_word() {
        let data = SkyData::default();
        let have = HaveMap::default();
        let rune = "Wind Rune Neza";
        assert!(is_rune(rune), "the fixture stopped being a rune");

        /* nothing in the log, one held mark: your word */
        let quiet = Ledger::default();
        let mut mine = Marks::default();
        mine.held.insert(item_key(rune), 1);
        let w = Witness {
            data: &data,
            led: &quiet,
            inv: None,
            have: &have,
            marks: &mine,
        };
        assert_eq!(
            w.sky_have(rune),
            (1, "mark"),
            "a rune the log never saw is held on your word alone"
        );

        /* the log watched two arrive and covers the one you marked: the log's word */
        let mut watched = Ledger::default();
        watched.looted.insert(rune.to_owned(), 2);
        let w2 = Witness {
            data: &data,
            led: &watched,
            inv: None,
            have: &have,
            marks: &mine,
        };
        assert_eq!(
            w2.sky_have(rune),
            (2, "log"),
            "the log accounts for the whole count, so it is the log's word"
        );

        /* a hand typed floor ABOVE the log: the number is yours, so the word is */
        let mut typed = Marks::default();
        typed.held.insert(item_key(rune), 5);
        let w3 = Witness {
            data: &data,
            led: &watched,
            inv: None,
            have: &have,
            marks: &typed,
        };
        assert_eq!(
            w3.sky_have(rune),
            (5, "mark"),
            "the 5 on screen is your held mark; the log only ever saw 2"
        );
    }

    /// THE ROW SAYS WHAT THE MODEL HANDED IT, PROVED BY HOVERING IT IN A REAL FRAME.
    ///
    /// A `src` on the model is worth nothing if the row still hard codes the log, and code with no
    /// caller is this repo's own recurring defect. So this draws `rune_row` for real, puts the
    /// pointer on the square and then on the count, and reads the tooltip egui painted.
    #[test]
    fn the_rune_row_never_calls_your_own_mark_the_logs_word() {
        fn flat(sh: egui::Shape, into: &mut Vec<egui::Shape>) {
            match sh {
                egui::Shape::Vec(v) => {
                    for x in v {
                        flat(x, into);
                    }
                }
                other => into.push(other),
            }
        }
        fn shapes(out: &mut egui::FullOutput) -> Vec<egui::Shape> {
            let mut f = Vec::new();
            for cs in std::mem::take(&mut out.shapes) {
                flat(cs.shape, &mut f);
            }
            f
        }
        fn words(sh: &[egui::Shape]) -> String {
            sh.iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\u{1F}")
        }
        fn base() -> egui::RawInput {
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(400.0, 120.0),
                )),
                ..Default::default()
            }
        }

        for (src, on_square, not_on_square, on_count, not_on_count) in [
            (
                "mark",
                "Marked as held. Click to clear",
                "log",
                "you marked this one held",
                "from the log",
            ),
            (
                "log",
                "held, on the log's word",
                "Marked as held",
                "from the log",
                "you marked",
            ),
        ] {
            let ctx = egui::Context::default();
            crate::fonts::install(&ctx);
            crate::theme::install(&ctx);
            /* the tooltip is the thing under test, so it may not wait half a second for us */
            ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
            let r = RuneModel {
                n: "Wind Rune Neza".to_owned(),
                have: 1,
                src,
            };
            let draw = |ui: &mut Ui| {
                let mut toggles = Vec::new();
                rune_row(ui, &r, &mut toggles);
                assert!(toggles.is_empty(), "nothing was clicked");
            };

            /* a first pass to learn where the two hoverable things landed */
            let mut out = ctx.run_ui(base(), draw);
            let laid = shapes(&mut out);
            out.drop_without_applying_deltas();
            let square_at = laid
                .iter()
                .find_map(|s| match s {
                    egui::Shape::Rect(rs) if (rs.rect.width() - 7.0).abs() < 0.6 => {
                        Some(rs.rect.center())
                    }
                    _ => None,
                })
                .expect("the rune row painted no held square");
            let count_at = laid
                .iter()
                .find_map(|s| match s {
                    egui::Shape::Text(t) if t.galley.text() == "1/1" => {
                        Some(egui::Rect::from_min_size(t.pos, t.galley.size()).center())
                    }
                    _ => None,
                })
                .expect("the rune row painted no count");

            /* egui's clock only moves forwards, and a tooltip is not due on the first frame
             * the pointer arrives: it wants the widget hovered, laid out and still. Measured on
             * this ctx, the third pass is the first one that paints it. */
            let mut clock = 1.0f64;
            for (what, at, want, forbid) in [
                ("the square", square_at, on_square, not_on_square),
                ("the count", count_at, on_count, not_on_count),
            ] {
                let mut said = String::new();
                for _ in 0..4 {
                    clock += 1.0;
                    let mut input = base();
                    input.time = Some(clock);
                    input.events.push(egui::Event::PointerMoved(at));
                    let mut out = ctx.run_ui(input, draw);
                    said = words(&shapes(&mut out));
                    out.drop_without_applying_deltas();
                }
                assert!(
                    said.contains(want),
                    "src={src:?}: hovering {what} never said {want:?}. It said: {said:?}"
                );
                assert!(
                    !said.contains(forbid),
                    "src={src:?}: hovering {what} said {forbid:?}, which is the wrong witness. It \
                     said: {said:?}"
                );
            }
        }
    }
}
