//! One ingest for inventory dumps, hunting logs and the EQ log tail. Decision D7.
//!
//! THREE SOURCES, ONE MODULE, AND WHY.
//! The game writes three kinds of file about one character: the log it appends to while logging
//! is on (`eqlog_<character>_<server>.txt`), and the two dumps `/outputfile` writes on demand
//! (`<character>_<server>-Inventory.txt` and `-Achievements.txt`). They are three files saying
//! the same kind of thing about the same character, so here they are one struct with one
//! "sources" report: what files, when last read, how many records, and one Re-read.
//!
//! WHAT EACH PART READS.
//!   The grammar.       The timestamp every log line opens with, the seven line shapes kills,
//!                      loot, experience and zoning print, and the two zone filters.
//!   The KillReader.    Kill and experience lines: an experience line within two seconds either
//!                      way of a kill someone else landed makes it a group kill, candidates drain
//!                      in log order behind one forward pointer, the three credits (blow, xp,
//!                      wit), and the zone a "You have entered" line names, tier suffix folded.
//!   The merge.         Kills into the tracker's buckets: each file's high-water mark gates a
//!                      re-read, known, unmatched and NPC-pet names are kept apart, and a kill
//!                      before any zone line lands in the one zone that lists its name.
//!   The credit rules.  What the kill tracker draws: the name keys, the cross-zone pool, which
//!                      roster rows a kill ticks, ignored zones, levels and roster order.
//!   The tail reader.   A read never pads a short read, a capped tail drops its first partial
//!                      line, a 40MB cap, a cursor that moves by bytes actually read. Each of
//!                      those three is a test below.
//!   The dump reader.   The inventory dump's tab-separated rows, their location chains and
//!                      sections, and each worn item paired with its own row.
//!
//! WHAT IS NOT HERE, SAID PLAINLY.
//!   Cross-run persistence of the kill tracker. Re-reads are gated with a per-file high-water
//!   mark and that gate is tested, but the buckets live in memory for this run and are rebuilt
//!   from the 40MB tail on every launch. The screen says which file and how much of it that is.
//!
//! TIME. The log prints local wall-clock time with no zone. Every `ts` in this module is that
//! wall clock read AS IF it were UTC, so formatting it as UTC gives the wall clock back.
//!
//! THREADS. The 40MB bootstrap read and parse happens on a worker thread (`scan`). The UI
//! thread only ever reads the few KB appended since the last poll, and a dump of a few KB, and
//! only once a second. Nothing here touches the network.

use crate::fights::{fold_text, fold_text_after, FightRow};
use chrono::{DateTime, Utc};
use grimoire_parse::group::Party;
use regex::Regex;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime};

/* ================================================================== grammar == */

/// The timestamp every log line opens with: `[Mon Jul 13 12:33:10 2026] `.
const TS: &str = r"^\[(?P<dow>\w{3}) (?P<mon>\w{3}) (?P<day>[ \d]\d) (?P<h>\d{2}):(?P<mi>\d{2}):(?P<s>\d{2}) (?P<y>\d{4})\] ";

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

struct Rx {
    slain_you: Regex,
    slain_by: Regex,
    died: Regex,
    xp: Regex,
    zone: Regex,
    loot: Regex,
    loot_manual: Regex,
    zone_skip: Regex,
    zone_tier: Regex,
}

/// The seven line patterns plus the two zone filters, compiled once. Every capture group is
/// named for the part of the line it holds.
static RX: LazyLock<Rx> = LazyLock::new(|| {
    let rx =
        |tail: &str| Regex::new(&format!("{TS}{tail}")).expect("the log grammar is a constant");
    Rx {
        slain_you: rx(r"You have slain (?P<n>.+?)!$"),
        slain_by: rx(r"(?P<n>.+?) has been slain by (?P<by>.+?)!$"),
        died: rx(r"(?P<n>.+?) died\.$"),
        xp: rx(r"You gain (?:party |group )?experience! \((?P<pct>\d+(?:\.\d+)?)%\)$"),
        zone: rx(r"You have entered (?P<z>.+?)\.$"),
        /* The groups are the line's own parts: an optional printed quantity, the item, the mob
         * whose corpse it came off, and an optional tail saying what became of it. No tail at
         * all is the plain "...corpse." line, which means you kept it.
         *
         * THE TAIL BRANCHES ARE ORDERED BY HOW OFTEN THEY ACTUALLY OCCUR, counted over the two
         * real logs in this repository (web/fixtures/eqlog-tail-200k.txt and
         * tests/fixtures/princess-night.txt, 64 loot lines between them): sold for free 18,
         * sold for coin 10, consumed by a combine 9, kept outright 1. The tradeskill depot tail
         * is last because neither log contains one; it is carried on the client's wording alone
         * and nothing here has seen it fire.
         *
         * The free-sale branch MUST stay ahead of the coin branch whatever the counts say: the
         * coin branch's lazy group would otherwise swallow the word "free" and report a sale
         * price of "free". */
        loot: rx(
            r"You looted (?:an? |(?P<qty>\d+) )?(?P<item>.+?) from (?P<mob>.+?)'s corpse(?:\.| (?P<tail>and sold it for free\.|and sold it for (?P<coin>.+?)\.|to create an? .+|and stored it in your tradeskill depot))?$",
        ),
        /* Looting by hand out of the corpse window prints a second, different line: wrapped in
         * "--" at both ends, always articled even where the auto-loot line is not, and never
         * carrying a sale or depot tail, because nothing automatic happened to it. It is the
         * commonest loot line in tests/fixtures/princess-night.txt (25 of that file's 52). */
        loot_manual: rx(
            r"--You have looted (?:an? |(?P<qty>\d+) )?(?P<item>.+?) from (?P<mob>.+?)'s corpse\.--$",
        ),
        /* NOT EVERY "You have entered" IS A ZONE. The client uses the same sentence for arriving
         * somewhere that is not a place on the roster. These three are carried WITHOUT LOCAL
         * EVIDENCE: neither log in this repository, and no zone in kills-data.json, contains any
         * of them, so nothing here can confirm the list is right or complete. It is left in
         * because filtering a non-zone wrongly costs a zone row, while failing to filter one
         * invents a zone; it is flagged here so the next person with a long log checks it
         * rather than trusting it. */
        zone_skip: Regex::new(r"^(an area|an Arena|the Drunken Monkey)").expect("constant"),
        /* A zone can be entered at an upgrade tier, and the tier is part of the printed name but
         * not part of the zone's identity, so it is folded off before the roster lookup.
         * Measured in tests/fixtures/princess-night.txt: of the 5 distinct zones entered, 3 are
         * tier suffixed ("The Plane of Sky 1 (Awakened)", "2 (Adaptive)", "3 (Fused)"), all of
         * the same base zone, which is exactly the case that would otherwise split one zone's
         * kills across four rosters. The fourth word, Refined, has not been seen locally. */
        zone_tier: Regex::new(r"^(.*) [1-4] \((?:Awakened|Adaptive|Fused|Refined)\)$")
            .expect("constant"),
    }
});

/// Seconds for a matched timestamp: the local wall clock read as UTC (see the module note). A
/// month that is not one of the twelve names (an unknown three-letter word) makes the line
/// unrecognised rather than a candidate with no time.
fn to_sec(c: &regex::Captures) -> Option<i64> {
    let mon = MONTHS.iter().position(|m| *m == &c["mon"])? as u32 + 1;
    let day: u32 = c["day"].trim().parse().ok()?;
    let y: i32 = c["y"].parse().ok()?;
    let h: u32 = c["h"].parse().ok()?;
    let mi: u32 = c["mi"].parse().ok()?;
    let s: u32 = c["s"].parse().ok()?;
    let d = chrono::NaiveDate::from_ymd_opt(y, mon, day)?.and_hms_opt(h, mi, s)?;
    Some(d.and_utc().timestamp())
}

/// Wall-clock `HH:MM:SS` for a log `ts`, which is why it formats as UTC (see the module note).
pub fn hhmmss(ts: i64) -> String {
    match DateTime::<Utc>::from_timestamp(ts, 0) {
        Some(t) => t.format("%H:%M:%S").to_string(),
        None => String::from("??:??:??"),
    }
}

/* ==================================================================== names == */

/// Matching key for a mob name: lowercase, backticks and apostrophes out, whitespace collapsed,
/// ONE leading article off.
///
/// EVERY RULE HERE ANSWERS A DISAGREEMENT BETWEEN THE TWO SIDES OF THE JOIN, and the sizes are
/// measured over the 6,833 mob rows in kills-data.json:
///
///   * 2,399 rows carry a leading article, and they are not consistent about its case
///     ("A blade storm" beside "a gorgalask"), so the fold happens after lowercasing and only
///     one article comes off: a name whose second word is also an article is not one this rule
///     may keep eating.
///   * 349 rows carry a backtick or an apostrophe inside the name ("King Ak`Anon",
///     "a Teir`Dal priest"). The game and the wiki do not agree on that character, so it is
///     removed from both sides rather than translated on either.
///
/// Items must NOT use this key: an article is part of an item's name, and the wiki keeps
/// "Sapphire" and "A Sapphire" deliberately apart.
pub fn norm_name(n: &str) -> String {
    let lowered = n.to_lowercase();
    let stripped: String = lowered
        .chars()
        .filter(|c| *c != '`' && *c != '\'')
        .collect();
    let s = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    for art in ["a ", "an ", "the "] {
        if let Some(rest) = s.strip_prefix(art) {
            return rest.to_string();
        }
    }
    s
}

/// Mob-only key: the roster disambiguates mobs that share a name with a parenthetical the game
/// never prints, so mob matching drops every (...) group before the rest of the fold.
///
/// It is a small rule with a narrow target: 16 of the 6,833 rows in kills-data.json carry one,
/// and they are the reason it exists rather than a general clean-up. They come in three kinds,
/// all of which would otherwise never match a log line: the same mob split by the race wearing
/// it ("a shadowknight (Ogre)"), by where it spawns ("a bottomless devourer (Howling Stones)"),
/// and by sex ("a fetid fiend (male/female)").
///
/// This is exactly why items keep plain `norm_name`: a parenthetical on an item name is part of
/// the item's identity, not a note about it.
pub fn norm_mob(n: &str) -> String {
    static PAREN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\s*\([^)]*\)").expect("constant"));
    norm_name(&PAREN.replace_all(n, " "))
}

/* =================================================================== roster == */

/// One mob row of `kills-data.json` zones[key].mobs. Every field the file was measured to carry
/// is typed; anything else survives in `extra` rather than being dropped.
#[derive(Clone, Debug, Deserialize)]
pub struct MobRow {
    pub n: String,
    #[serde(default)]
    pub named: bool,
    /// Precomputed numeric level, when the pipeline had one.
    #[serde(default)]
    pub lv: Option<f64>,
    /// The raw wiki level string: "59-61", "~53", "?".
    #[serde(default)]
    pub lvl: Option<String>,
    /// Wiki page title.
    #[serde(default)]
    pub t: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// One zone of `kills-data.json`: `{city, mobs, name}`.
#[derive(Clone, Debug, Deserialize)]
pub struct ZoneRoster {
    pub name: String,
    #[serde(default)]
    pub city: bool,
    #[serde(default)]
    pub mobs: Vec<MobRow>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct KillsData {
    #[serde(default)]
    zones: BTreeMap<String, ZoneRoster>,
}

/// The mob roster and the two indexes built from it: zone name (lowercased) to zone key, and
/// `norm_mob` name to every zone key that lists it.
#[derive(Debug)]
pub struct Roster {
    pub path: PathBuf,
    pub zones: BTreeMap<String, ZoneRoster>,
    name2key: Arc<HashMap<String, String>>,
    name_zones: HashMap<String, BTreeSet<String>>,
}

impl Roster {
    /// Read `kills-data.json` under `root`. Errors carry the path and the reason.
    pub fn load(root: &Path) -> Result<Roster, String> {
        let path = root.join("kills-data.json");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let kd: KillsData =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut name2key = HashMap::new();
        let mut name_zones: HashMap<String, BTreeSet<String>> = HashMap::new();
        for (key, z) in &kd.zones {
            name2key.insert(z.name.to_lowercase(), key.clone());
            for row in &z.mobs {
                name_zones
                    .entry(norm_mob(&row.n))
                    .or_default()
                    .insert(key.clone());
            }
        }
        Ok(Roster {
            path,
            zones: kd.zones,
            name2key: Arc::new(name2key),
            name_zones,
        })
    }

    pub fn name2key(&self) -> Arc<HashMap<String, String>> {
        Arc::clone(&self.name2key)
    }

    pub fn name_zones(&self) -> &HashMap<String, BTreeSet<String>> {
        &self.name_zones
    }
}

/* =============================================================== kill stream == */

/// Who gets the kill. `blow`: your killing blow. `xp`: someone else's blow with an xp line
/// within two seconds either way (a group or pet kill). `wit`: witnessed, no xp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Credit {
    Blow,
    Xp,
    Wit,
}

impl Credit {
    /// How the feed says it, in the plainest words that keep the three apart. `Blow` is the only
    /// one the log states outright, so it gets the unqualified word; `Xp` is a kill you were
    /// paid for without landing it, which is what being in a group means; `Wit` is a death that
    /// happened in front of you and earned you nothing, and the word has to make that obvious or
    /// a reader counts it as his own.
    pub fn label(self) -> &'static str {
        match self {
            Credit::Blow => "kill",
            Credit::Xp => "group kill",
            Credit::Wit => "witnessed",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KillEvent {
    pub ts: i64,
    /// An atlas key, or "?" before the first "You have entered" resolves.
    pub zone: String,
    /// `norm_mob` of the printed name.
    pub name: String,
    pub credit: Credit,
}

/// What happened to a looted item, from the loot line's tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Kept,
    Depot,
    SoldFree,
    Sold,
    Created,
}

impl Disposition {
    /// The feed's suffix, taken from what the loot line's own tail says happened.
    ///
    /// `Kept` is silent on purpose: the plain "...corpse." line is the ordinary case and the
    /// commonest thing in the feed, and a suffix on every row would be noise on all of them. The
    /// other four each mark an item that is NOT in your bags despite a loot line saying you
    /// looted it, which is the single thing this column exists to tell you: the depot tail says
    /// it was stored, the two sale tails say it went to a merchant (and which of them says
    /// whether it was worth anything), and the combine tail says it was consumed making
    /// something else.
    pub fn label(self) -> &'static str {
        match self {
            Disposition::Kept => "",
            Disposition::Depot => "to depot",
            Disposition::SoldFree => "sold (worthless)",
            Disposition::Sold => "auto-sold",
            Disposition::Created => "crafted",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LootEvent {
    pub ts: i64,
    pub zone: String,
    pub qty: u32,
    pub item: String,
    pub mob: String,
    pub disp: Disposition,
    /// The coin string the client printed, only when `disp` is `Sold`.
    pub sold_for: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct XpEvent {
    pub ts: i64,
    /// Percent of the current level, as the line prints it.
    pub pct: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Kill(KillEvent),
    Loot(LootEvent),
    Xp(XpEvent),
}

struct Cand {
    ts: i64,
    zone: String,
    name: String,
    credit: Option<Credit>,
}

/// Stateful line-at-a-time parser for live tailing.
///
/// One of the three credits is free and two have to be waited for. "You have slain" names you as
/// the killer on the line itself. "has been slain by" and "died." do not say whether the kill was
/// anything to do with you, and the only witness the log offers is whether an experience line
/// lands beside it, which may be a line that has not been written yet.
///
/// So a candidate is held until the stream has seen ANY timestamp later than its window, or
/// until `flush()` declares the log over, and candidates are released strictly in the order they
/// were created. That ordering rule is the one that matters: a decided candidate waits behind an
/// older undecided one, so what this reader emits is always in log order and never in the order
/// the answers happened to arrive.
///
/// Holding them in creation order is safe because the file's own timestamps do not go backwards
/// within a session; where they do (a log rolled over, two files concatenated), the worst case is
/// a candidate resolved as witnessed that a later line would have called a group kill, which is
/// the conservative direction: it under-claims rather than over-claims a kill.
pub struct KillReader {
    name2key: Arc<HashMap<String, String>>,
    /// Current zone key, "?" until a "You have entered" line resolves against the roster.
    pub zone: String,
    /// The file's high-water mark: advanced ONLY by the five kill-relevant patterns. A loot
    /// line must not move it, or a re-dropped log could silently skip a kill sharing its second.
    pub last_ts: i64,
    /// Lines fed that were long enough to carry a timestamp.
    pub lines: usize,
    pending: Vec<Cand>,
    head: usize,
    xp_ts: Vec<i64>,
    xp_i: usize,
    /// Latest ts from ANY recognised line, loot included. What "resolvable yet?" checks.
    seen_ts: i64,
}

impl KillReader {
    pub fn new(name2key: Arc<HashMap<String, String>>) -> KillReader {
        KillReader {
            name2key,
            zone: String::from("?"),
            last_ts: 0,
            lines: 0,
            pending: Vec::new(),
            head: 0,
            xp_ts: Vec::new(),
            xp_i: 0,
            seen_ts: 0,
        }
    }

    /// Feed one line (no trailing newline). Returns every event that became decidable.
    pub fn feed(&mut self, raw: &str) -> Vec<Event> {
        let mut out = Vec::new();
        if raw.len() < 28 {
            return out;
        }
        self.lines += 1;
        let rx = &*RX;
        if let Some(m) = rx.zone.captures(raw) {
            let z = &m["z"];
            if !rx.zone_skip.is_match(z) {
                let folded = rx
                    .zone_tier
                    .captures(z)
                    .map(|t| t[1].to_string())
                    .unwrap_or_else(|| z.to_string());
                self.zone = self
                    .name2key
                    .get(&folded.to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| String::from("?"));
                if let Some(ts) = to_sec(&m) {
                    self.seen_ts = ts;
                    self.last_ts = ts;
                }
            }
        } else if let Some(m) = rx.slain_you.captures(raw) {
            if let Some(ts) = to_sec(&m) {
                self.seen_ts = ts;
                self.last_ts = ts;
                self.pending.push(Cand {
                    ts,
                    zone: self.zone.clone(),
                    name: norm_mob(&m["n"]),
                    credit: Some(Credit::Blow),
                });
            }
        } else if let Some(m) = rx.slain_by.captures(raw) {
            if let Some(ts) = to_sec(&m) {
                self.seen_ts = ts;
                self.last_ts = ts;
                self.pending.push(Cand {
                    ts,
                    zone: self.zone.clone(),
                    name: norm_mob(&m["n"]),
                    credit: None,
                });
            }
        } else if let Some(m) = rx.died.captures(raw) {
            if &m["n"] != "You" {
                if let Some(ts) = to_sec(&m) {
                    self.seen_ts = ts;
                    self.last_ts = ts;
                    self.pending.push(Cand {
                        ts,
                        zone: self.zone.clone(),
                        name: norm_mob(&m["n"]),
                        credit: None,
                    });
                }
            }
        } else if let Some(m) = rx.xp.captures(raw) {
            if let Some(ts) = to_sec(&m) {
                self.seen_ts = ts;
                self.last_ts = ts;
                self.xp_ts.push(ts);
                let pct = m["pct"].parse::<f64>().unwrap_or(0.0);
                out.push(Event::Xp(XpEvent { ts, pct }));
            }
        } else if let Some(m) = rx.loot.captures(raw) {
            if let Some(ts) = to_sec(&m) {
                self.seen_ts = ts;
                let (disp, sold_for) = loot_disp(&m);
                out.push(Event::Loot(LootEvent {
                    ts,
                    zone: self.zone.clone(),
                    qty: m
                        .name("qty")
                        .and_then(|q| q.as_str().parse().ok())
                        .unwrap_or(1),
                    item: m["item"].to_string(),
                    mob: m["mob"].to_string(),
                    disp,
                    sold_for,
                }));
            }
        } else if let Some(m) = rx.loot_manual.captures(raw) {
            if let Some(ts) = to_sec(&m) {
                self.seen_ts = ts;
                out.push(Event::Loot(LootEvent {
                    ts,
                    zone: self.zone.clone(),
                    qty: m
                        .name("qty")
                        .and_then(|q| q.as_str().parse().ok())
                        .unwrap_or(1),
                    item: m["item"].to_string(),
                    mob: m["mob"].to_string(),
                    disp: Disposition::Kept,
                    sold_for: None,
                }));
            }
        }
        self.drain(&mut out, false);
        out
    }

    /// Resolve every remaining candidate as if the log ended here.
    pub fn flush(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        self.drain(&mut out, true);
        out
    }

    fn drain(&mut self, out: &mut Vec<Event>, final_: bool) {
        while self.head < self.pending.len() {
            let c = &mut self.pending[self.head];
            if c.credit.is_none() {
                /* Not decidable yet: nothing later than this candidate's window has been read,
                 * so the experience line that would settle it may still be coming. Nothing
                 * behind it can be released either, because releasing out of order is the one
                 * thing this queue exists to prevent. */
                if !final_ && self.seen_ts <= c.ts + 2 {
                    break;
                }
                while self.xp_i < self.xp_ts.len() && self.xp_ts[self.xp_i] < c.ts - 2 {
                    self.xp_i += 1;
                }
                let hit = self.xp_i < self.xp_ts.len() && self.xp_ts[self.xp_i] <= c.ts + 2;
                c.credit = Some(if hit { Credit::Xp } else { Credit::Wit });
            }
            out.push(Event::Kill(KillEvent {
                ts: c.ts,
                zone: c.zone.clone(),
                name: c.name.clone(),
                credit: c.credit.expect("set just above or at creation"),
            }));
            self.head += 1;
        }
        /* A stream that tails for days would otherwise grow without bound. Emitted candidates
         * and xp timestamps the forward pointer has passed are dead, so they are dropped once
         * there are enough of them to be worth the copy. The pointer logic is unchanged: both
         * indexes shift by exactly what was removed. */
        if self.head > 4096 {
            self.pending.drain(..self.head);
            self.head = 0;
        }
        if self.xp_i > 4096 {
            self.xp_ts.drain(..self.xp_i);
            self.xp_i = 0;
        }
    }
}

/// A loot line's disposition: the tail group decides.
fn loot_disp(m: &regex::Captures) -> (Disposition, Option<String>) {
    let Some(tail) = m.name("tail") else {
        return (Disposition::Kept, None);
    };
    let tail = tail.as_str();
    if tail.contains("tradeskill depot") {
        return (Disposition::Depot, None);
    }
    if tail.contains("sold it for free") {
        return (Disposition::SoldFree, None);
    }
    if let Some(coin) = m.name("coin") {
        return (Disposition::Sold, Some(coin.as_str().to_string()));
    }
    (Disposition::Created, None)
}

/// What `parse_log` hands back: every event in log order with the kills flushed, and the stream
/// that read them, which carries the high-water mark (`last_ts`), the line count and the last
/// zone seen. The bootstrap seeds its live stream from that.
pub struct ParsedLog {
    pub events: Vec<Event>,
    pub stream: KillReader,
}

/// Batch parse: a whole text through one stream, CRLF tolerant, kills
/// flushed at the end. The bootstrap runs the tail of the active log through this.
pub fn parse_log(text: &str, name2key: Arc<HashMap<String, String>>) -> ParsedLog {
    let mut stream = KillReader::new(name2key);
    let mut events = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        events.extend(stream.feed(raw));
    }
    events.extend(stream.flush());
    ParsedLog { events, stream }
}

/* ============================================================ tracker state == */

/// One bucket entry: how many, and the earliest time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub c: u32,
    pub t: i64,
}

/// The kill tracker's settings, and what each is before a reader changes it.
///
/// # THESE THREE WERE PER WINDOW AND PER RUN, AND THE SERDE HERE IS WHY THEY ARE NOT
///
/// `TrackerState::settings` lives on the `Ingest`, and there is one `Ingest` per window, so
/// ticking "count witnessed kills" in a pop-out moved that window's completion percentage and
/// left the main window's where it was: two headline percentages for one roster, six inches
/// apart, with nothing on screen saying why. Nothing wrote them anywhere either, so all three
/// were back at [`TrackerSettings::default`] on the next launch. They live on
/// [`crate::settings::Settings::tracker`] now, one value for the process, saved with everything
/// else; `Ingest::new` and `Ingest::reconfigure` seed this copy from it.
///
/// # `#[serde(default)]` IS ON THE CONTAINER AND IT MUST NOT BE MOVED ONTO THE FIELDS
///
/// A container default takes any absent key from `TrackerSettings::default()`, which is the
/// shipped behaviour: `ignore_cities` is TRUE. Field-level `#[serde(default)]` would take each
/// absent bool from `bool::default()` instead, which is FALSE, so the first build to read an
/// older settings file would silently start counting every city kill for every reader who has
/// one. The two spellings look identical in a diff and differ on the one field whose default is
/// not the zero value.
///
/// EVERY SETTINGS FILE ON DISK PREDATES THIS FIELD, so the container `#[serde(default)]` on
/// `Settings` supplies the whole struct when the `tracker` key is missing, and this one supplies
/// each field when a later build adds one.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TrackerSettings {
    pub ignore_cities: bool,
    pub ignored_zones: Vec<String>,
    pub unignored_zones: Vec<String>,
    pub generic_everywhere: bool,
    pub witnessed: bool,
}

impl Default for TrackerSettings {
    fn default() -> Self {
        TrackerSettings {
            ignore_cities: true,
            ignored_zones: Vec::new(),
            unignored_zones: Vec::new(),
            generic_everywhere: false,
            witnessed: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileMark {
    /// The file's high-water mark: the last kill-relevant timestamp read.
    pub ts: i64,
    pub n: usize,
}

/// The kill tracker's state. Three buckets keyed zone then name:
/// `kills` holds any mob the roster knows ANYWHERE, `wit` the witnessed ones, `unmatched`
/// names no zone lists (players, wiki gaps). The unplaced zone is "?".
#[derive(Clone, Debug, Default)]
pub struct TrackerState {
    pub kills: HashMap<String, HashMap<String, Tally>>,
    pub wit: HashMap<String, HashMap<String, Tally>>,
    pub unmatched: HashMap<String, HashMap<String, u32>>,
    pub chars: Vec<String>,
    pub files: HashMap<String, FileMark>,
    pub settings: TrackerSettings,
}

pub const UNPLACED: &str = "?";

fn bump(bucket: &mut HashMap<String, HashMap<String, Tally>>, z: &str, n: &str, ts: i64) {
    let zb = bucket.entry(z.to_string()).or_default();
    match zb.get_mut(n) {
        Some(e) => {
            e.c += 1;
            if ts < e.t {
                e.t = ts;
            }
        }
        None => {
            zb.insert(n.to_string(), Tally { c: 1, t: ts });
        }
    }
}

/// Merge parsed kills into the state. `hwm` gates each event on the file's
/// high-water mark (a growing log dropped again must not double count); a live tailer feeds
/// each line exactly once and passes `false`, or a candidate that resolves one flush later than
/// an earlier one sharing its second would be eaten. Returns how many events landed.
pub fn ingest_kills(
    state: &mut TrackerState,
    name_zones: &HashMap<String, BTreeSet<String>>,
    file_name: &str,
    kills: &[KillEvent],
    last_ts: i64,
    lines: usize,
    hwm: bool,
) -> usize {
    let mark = state.files.get(file_name).map(|f| f.ts).unwrap_or(0);
    let mut added = 0;
    for ev in kills {
        if hwm && ev.ts <= mark {
            continue;
        }
        let mut z = ev.zone.as_str();
        /* A kill before any zone line still lands if the mob exists in exactly one zone;
         * otherwise it waits in the unplaced bucket. */
        if z == UNPLACED {
            if let Some(zs) = name_zones.get(&ev.name) {
                if zs.len() == 1 {
                    z = zs.iter().next().expect("len is 1");
                }
            }
        }
        let known = name_zones.contains_key(&ev.name);
        if ev.credit == Credit::Wit {
            if known {
                bump(&mut state.wit, z, &ev.name, ev.ts);
                added += 1;
            }
            /* witnessed unknown-name deaths are mostly other players: dropped */
        } else if known {
            bump(&mut state.kills, z, &ev.name, ev.ts);
            added += 1;
        } else if !ev.name.ends_with(" pet") {
            /* An NPC's pet dies under its owner's name with " pet" on the end, and there is one
             * of those for every caster mob in the game. They are not things a completionist is
             * hunting, and listing them would bury the names that are. This drops only names the
             * roster does NOT know: the handful of pets the roster does list matched `known`
             * above and never reach here. */
            let zb = state.unmatched.entry(z.to_string()).or_default();
            *zb.entry(ev.name.clone()).or_insert(0) += 1;
            added += 1;
        }
    }
    if let Some(ch) = character_of_log(file_name) {
        if !state.chars.iter().any(|c| c == &ch) {
            state.chars.push(ch);
        }
    }
    state.files.insert(
        file_name.to_string(),
        FileMark {
            ts: last_ts,
            n: lines,
        },
    );
    added
}

/// Every killed name across all zones including the unplaced bucket: the cross-zone credit
/// pool for named mobs and generic-everywhere.
pub fn global_killed(s: &TrackerState) -> HashSet<String> {
    let mut g = HashSet::new();
    for z in s.kills.values() {
        g.extend(z.keys().cloned());
    }
    if s.settings.witnessed {
        for z in s.wit.values() {
            g.extend(z.keys().cloned());
        }
    }
    g
}

fn in_zone(bucket: &HashMap<String, HashMap<String, Tally>>, zkey: &str, n: &str) -> bool {
    bucket.get(zkey).is_some_and(|zb| zb.contains_key(n))
}

/// Is this roster row ticked? A kill in a zone credits that zone's row; a
/// NAMED kill credits every zone listing the name (same individual); a generic kill credits
/// other zones only with generic_everywhere; witnessed kills only while the toggle is on.
pub fn credited(s: &TrackerState, glob: &HashSet<String>, zkey: &str, row: &MobRow) -> bool {
    let n = norm_mob(&row.n);
    if in_zone(&s.kills, zkey, &n) {
        return true;
    }
    if s.settings.witnessed && in_zone(&s.wit, zkey, &n) {
        return true;
    }
    (row.named || s.settings.generic_everywhere) && glob.contains(&n)
}

/// Is this zone left out of completion? Per-zone choices beat the city rule.
pub fn zone_ignored(s: &TrackerState, zkey: &str, zone: Option<&ZoneRoster>) -> bool {
    if s.settings.ignored_zones.iter().any(|z| z == zkey) {
        return true;
    }
    if s.settings.unignored_zones.iter().any(|z| z == zkey) {
        return false;
    }
    s.settings.ignore_cities && zone.is_some_and(|z| z.city)
}

/// A number to sort a roster row by, out of a level field that is free text.
///
/// It has to be forgiving because the field genuinely is: kills-data.json's 6,833 rows carry 666
/// distinct level strings. 5,072 are a plain number and 1,438 are a dash range, which is 95% of
/// the rows and the reason the two rules below are the two rules. After that it is a long tail of
/// one-offs: 37 tilde ranges, 64 rows with nothing in the field, 16 reading "??", and the rest
/// prose that no rule should pretend to read ("Blue to 13.", "36 or 50", "You can't consider
/// that."). A range answers with its midpoint because that is the value that sorts it among the
/// plain numbers; anything unreadable answers None and sorts last, rather than defaulting to a
/// number nobody wrote.
pub fn lvl_num(raw: &str) -> Option<f64> {
    static RANGE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d+)\s*-\s*(\d+)").expect("constant"));
    static NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").expect("constant"));
    if raw.is_empty() {
        return None;
    }
    let s = raw.replace('~', "-");
    if let Some(r) = RANGE.captures(&s) {
        let a: f64 = r[1].parse().ok()?;
        let b: f64 = r[2].parse().ok()?;
        return Some((a + b) / 2.0);
    }
    NUM.find(&s).and_then(|m| m.as_str().parse().ok())
}

/// The level used for ordering a row: the precomputed `lv`, else `lvl` read by `lvl_num`.
pub fn row_level(r: &MobRow) -> Option<f64> {
    r.lv.or_else(|| r.lvl.as_deref().and_then(lvl_num))
}

/// The roster order: named before generic, level high to low inside each, unknown level last,
/// then name.
pub fn sort_rows<'a>(rows: &[&'a MobRow]) -> Vec<&'a MobRow> {
    let mut v: Vec<&MobRow> = rows.to_vec();
    v.sort_by(|a, b| {
        if a.named != b.named {
            return if b.named {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Less
            };
        }
        let la = row_level(a);
        let lb = row_level(b);
        match (la, lb) {
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            (Some(_), None) => return std::cmp::Ordering::Less,
            (Some(x), Some(y)) if x != y => {
                return y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal)
            }
            _ => {}
        }
        a.n.to_lowercase().cmp(&b.n.to_lowercase())
    });
    v
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ZoneSum {
    pub done: usize,
    pub total: usize,
    pub ignored: bool,
}

/// Per-zone and overall completion over the roster, settings applied.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub zones: BTreeMap<String, ZoneSum>,
    pub done: usize,
    pub total: usize,
    pub zones_done: usize,
    pub zones_total: usize,
    pub glob: HashSet<String>,
}

pub fn summarize(s: &TrackerState, roster: &Roster) -> Summary {
    let glob = global_killed(s);
    let mut sum = Summary::default();
    for (key, z) in &roster.zones {
        if z.mobs.is_empty() {
            continue;
        }
        let ig = zone_ignored(s, key, Some(z));
        let d = z
            .mobs
            .iter()
            .filter(|row| credited(s, &glob, key, row))
            .count();
        sum.zones.insert(
            key.clone(),
            ZoneSum {
                done: d,
                total: z.mobs.len(),
                ignored: ig,
            },
        );
        if !ig {
            sum.done += d;
            sum.total += z.mobs.len();
            sum.zones_total += 1;
            if d == z.mobs.len() {
                sum.zones_done += 1;
            }
        }
    }
    sum.glob = glob;
    sum
}

/* ============================================================== tail reader == */

/// The tail cap: the newest 40MB of the active log. The kill tracker and the feed are rebuilt
/// from it on every launch, so this is exactly how much history a launch can see, and the screens
/// print it for that reason.
///
/// IT IS A CHOICE, NOT A MEASUREMENT, and it is deliberately five times the browser's.
/// `web/tail.js` caps its bootstrap at 8MB because a page is read on a phone and has to be
/// interactive at once.
/// This binary is a desktop app the reader leaves running beside the game, it does the read on a
/// worker thread with the rail saying WORKING, and the thing it is trying to avoid is a tracker
/// that forgets last night. Nothing here has timed the two against each other; if that timing is
/// ever done, this constant and that sentence are what it should replace.
pub const TAIL_CAP: u64 = 40 * 1024 * 1024;

/// `TAIL_CAP` as the screens print it ("40 MB"), from the constant, so no screen restates the
/// number and drifts from it.
pub fn tail_cap_text() -> String {
    let mb = TAIL_CAP / (1024 * 1024);
    if mb > 0 && TAIL_CAP % (1024 * 1024) == 0 {
        format!("{mb} MB")
    } else {
        format!("{} KB", TAIL_CAP / 1024)
    }
}

/// Bytes `[start, end)` of a file, and only the ones that were really there.
///
/// A short answer is normal rather than exceptional here: the game is appending to this file
/// while it is read, the caller's size came from an earlier `stat`, the file can be rolled over
/// or truncated in between, and a read from a network drive is allowed to return less than asked
/// for no reason at all.
///
/// So the buffer is truncated to what was actually read and never padded back up to the length
/// requested. Padding would be worse than an error in two separate ways, which is why it is
/// called out rather than left to the reader: the zero bytes become NUL characters inside
/// otherwise valid lines, and the caller advances its cursor over bytes it has not seen, so the
/// text that eventually lands there is skipped for good. `web/tail.js` states the same rule for
/// the browser reader, and for the same reason.
pub fn read_range(path: &Path, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)?;
    let want = end.saturating_sub(start) as usize;
    let mut buf = vec![0u8; want];
    f.seek(SeekFrom::Start(start))?;
    let mut n = 0usize;
    while n < want {
        let got = f.read(&mut buf[n..])?;
        if got == 0 {
            break; /* EOF: the file is shorter than the stat promised */
        }
        n += got;
    }
    buf.truncate(n);
    Ok(buf)
}

/// A bootstrap read: the text, and the byte the text runs to, which is where the appended-bytes
/// cursor must start (less than `size` when the file shrank under us).
#[derive(Debug, PartialEq, Eq)]
pub struct TailText {
    pub text: String,
    pub end: u64,
    /// Where the read started; non-zero means the cap cut the file and the first line went.
    pub start: u64,
}

/// At most `TAIL_CAP` bytes from the end of a log, first partial line dropped.
pub fn read_tail(path: &Path, size: u64) -> std::io::Result<TailText> {
    read_tail_capped(path, size, TAIL_CAP)
}

///
/// # THE TEXT ENDS AT ITS LAST NEWLINE, AND `end` WITH IT
///
/// The game writes a line and then its newline, so bytes after the last newline are a line being
/// written, not a line. This used to keep them in `text` and set `end` past them, and the cursor
/// built on `end` (`Tail::at`) then read the REST of that line as a line of its own. One real line
/// became two fragments, neither of them whole: the first with a stamp and half a sentence, the
/// second with no stamp at all. Every fold of the bootstrap text saw the first, the live stream saw
/// the second, and nothing saw the line.
///
/// IT MATTERS MOST FOR THE GROUP, because the group is carried for the whole session. A torn
/// `You have been removed from the group.` read as `...You have been rem` and `oved from the
/// group.` reaches no party at all (`grimoire_parse::group` matches every form whole and skips a
/// line with no stamp), so the live fold goes on naming a group that ended, and the store keeps
/// the first answer it is given. So the torn bytes are left UNREAD: `end` stops at the last
/// newline, and the next `read_appended` from there reads the line whole once its newline lands.
///
/// A READ WITH NO NEWLINE IN IT AT ALL IS THEREFORE EMPTY, and `end` is where it started. That
/// used to be kept whole (a search for a first newline that found none cut nothing), and it was
/// the same fragment in its purest form.
pub fn read_tail_capped(path: &Path, size: u64, cap: u64) -> std::io::Result<TailText> {
    let start = size.saturating_sub(cap);
    let mut bytes = read_range(path, start, size)?;
    let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    bytes.truncate(whole);
    let end = start + bytes.len() as u64;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 {
        /* The cap landed inside the first line, which goes. */
        if let Some(i) = text.find('\n') {
            text.drain(..=i);
        }
    }
    Ok(TailText { text, end, start })
}

/// The appended-bytes cursor for one file. `remainder` holds the trailing partial line as
/// BYTES rather than text, so a chunk boundary inside a multibyte character cannot corrupt it.
#[derive(Debug, Default, Clone)]
pub struct Tail {
    pub offset: u64,
    remainder: Vec<u8>,
}

impl Tail {
    pub fn at(offset: u64) -> Tail {
        Tail {
            offset,
            remainder: Vec::new(),
        }
    }
}

/// Advance the cursor over the bytes appended since `tail.offset` and return the whole lines
/// they complete; the trailing partial line waits in the remainder. The cursor moves by what
/// was READ, not by what the stat said was there. Returns nothing, cursor untouched, when the
/// file cannot be read.
pub fn read_appended(path: &Path, tail: &mut Tail, size: u64) -> Vec<String> {
    let Ok(bytes) = read_range(path, tail.offset, size) else {
        return Vec::new();
    };
    tail.offset += bytes.len() as u64;
    let mut buf = std::mem::take(&mut tail.remainder);
    buf.extend_from_slice(&bytes);
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(i) = buf[from..].iter().position(|b| *b == b'\n') {
        let line = &buf[from..from + i];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if !line.is_empty() {
            out.push(String::from_utf8_lossy(line).into_owned());
        }
        from += i + 1;
    }
    tail.remainder = buf[from..].to_vec();
    out
}

/* =========================================================== inventory dump == */

/// Where a dump row lives. Declared in display order, which the derived `Ord` follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// The slots the dump can report you wearing: the wiki's twenty
/// plus "Any Slot", which no item record names but the dump prints.
pub const WORN_SLOTS: [&str; 21] = [
    "Charm",
    "Ear",
    "Head",
    "Face",
    "Neck",
    "Shoulders",
    "Arms",
    "Back",
    "Wrist",
    "Range",
    "Hands",
    "Primary",
    "Secondary",
    "Fingers",
    "Chest",
    "Legs",
    "Feet",
    "Waist",
    "Ammo",
    "Power Source",
    "Any Slot",
];

struct InvRx {
    worn: Regex,
    tier: Regex,
    exalt: Regex,
    sub: Regex,
    root: Regex,
    bags: Regex,
    storage: Regex,
    exalts: Regex,
    bank: Regex,
    shared: Regex,
    depot: Regex,
    hoard: Regex,
    keyring: Regex,
    file: Regex,
}

static INV: LazyLock<InvRx> = LazyLock::new(|| {
    let r = |s: &str| Regex::new(s).expect("constant");
    InvRx {
        worn: r(
            r"^(Charm|Ear|Head|Face|Neck|Shoulders|Arms|Back|Wrist|Range|Hands|Primary|Secondary|Fingers?|Ring|Chest|Legs|Feet|Waist|Ammo|Power Source|Any Slot)$",
        ),
        tier: r(r"\s\+(\d+)$"),
        exalt: r(r"\s*\(Exaltation\)$"),
        sub: r(r"-Slot\d+$"),
        root: r(r"(-Slot\d+)+$"),
        /* The client writes "General 1" WITH a space and "Bank1" without, so every matcher takes
         * an optional one. */
        bags: r(r"^(General ?\d+|Held)$"),
        storage: r(r"^(Equipment|Activated)$"),
        exalts: r(r"^Augmentation$"),
        bank: r(r"^Bank ?\d+$"),
        shared: r(r"^SharedBank ?\d*$"),
        depot: r(r"^Personal-Depot"),
        hoard: r(r"(?i)^Dragon"),
        keyring: r(r"^KeyRing"),
        file: r(r"(?i)^(?P<ch>[^_]+)_(?P<srv>[^-]+)-Inventory\.txt$"),
    }
});

/// Base name and tier: the dump stars some items ("Backpack*"), and names carry the
/// upgrade tier ("Giant Snake Fang +4", same item id as +0). Tier is capped at 10.
pub fn base_name(name: &str) -> (String, u8) {
    let plain = name.trim_end_matches('*');
    match INV.tier.captures(plain) {
        Some(m) => {
            let tier: u32 = m[1].parse().unwrap_or(0);
            let cut = m.get(0).map(|g| g.start()).unwrap_or(plain.len());
            (plain[..cut].to_string(), tier.min(10) as u8)
        }
        None => (plain.to_string(), 0),
    }
}

/// A location string is a chain: "General 8-Slot6-Slot7" is a thing inside a thing inside bag
/// 8. The root decides which place you walk to.
pub fn root_loc(loc: &str) -> String {
    INV.root.replace(loc, "").into_owned()
}

pub fn loc_section(loc: &str) -> Section {
    let r = root_loc(loc);
    let i = &*INV;
    if i.worn.is_match(&r) {
        Section::Worn
    } else if i.bags.is_match(&r) {
        Section::Bags
    } else if i.storage.is_match(&r) {
        Section::Storage
    } else if i.exalts.is_match(&r) {
        Section::Exalts
    } else if i.bank.is_match(&r) {
        Section::Bank
    } else if i.shared.is_match(&r) {
        Section::Shared
    } else if i.depot.is_match(&r) {
        Section::Depot
    } else if i.hoard.is_match(&r) {
        Section::Hoard
    } else if i.keyring.is_match(&r) {
        Section::KeyRing
    } else {
        Section::Other
    }
}

/// One row of an `/outputfile inventory` dump at full granularity. `sub` is a row nested inside
/// another (a bag's contents, or a stone socketed into a worn item); `exalt` is an exaltation
/// stone, which names itself after the item it was rendered from; `empty` is the dump's own
/// "Empty" placeholder, the only evidence that a socket is open and empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvRow {
    pub loc: String,
    pub root: String,
    pub section: Section,
    pub name: String,
    pub base: String,
    pub tier: u8,
    pub exalt: bool,
    pub sub: bool,
    pub empty: bool,
    pub item_id: u64,
    pub count: u32,
    /// The fifth column, absent on the three-column storage rows.
    pub slots: Option<u32>,
}

/// The rows of an `/outputfile inventory` dump: tab separated, CRLF, with "Empty" placeholder
/// rows for every slot you are not using.
///
/// TWO THINGS ABOUT THE FILE DECIDE THE TWO ODD RULES BELOW. It is not one table but two, and the
/// second (the key ring) repeats the header row, so a header is skipped on its SHAPE, by the
/// literal words `Name` and `ID` standing in columns two and three, rather than by position or
/// by counting one at the top. And the two tables are not the same width: the key ring's rows
/// carry three columns where the inventory's carry five. Rows are therefore accepted from three
/// columns up, with the two missing fields read as absent rather than as zero, because a row the
/// dump bothered to print is a thing you own whatever it declines to say about it.
pub fn parse_rows(text: &str, keep_empty: bool) -> Vec<InvRow> {
    let mut rows = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let f: Vec<&str> = raw.split('\t').collect();
        if f.len() < 3 || (f[1] == "Name" && f[2] == "ID") {
            continue;
        }
        let loc = f[0];
        let name = f[1];
        if name.is_empty() {
            continue;
        }
        let sub = INV.sub.is_match(loc);
        if name == "Empty" {
            if keep_empty {
                rows.push(InvRow {
                    loc: loc.to_string(),
                    root: root_loc(loc),
                    section: loc_section(loc),
                    name: name.to_string(),
                    base: String::new(),
                    tier: 0,
                    exalt: false,
                    sub,
                    empty: true,
                    item_id: 0,
                    count: 0,
                    slots: None,
                });
            }
            continue;
        }
        let exalt = INV.exalt.is_match(name);
        let (base, tier) = base_name(&INV.exalt.replace(name, ""));
        /* A Count that is missing, unreadable or zero is read as one. The dump only prints a row
         * for something you actually hold, so whatever the column says, there is at least one of
         * it; reading a blank as zero would make an item you own disappear from its own list. */
        let count = if f.len() > 3 {
            f[3].trim()
                .parse::<u32>()
                .ok()
                .filter(|c| *c != 0)
                .unwrap_or(1)
        } else {
            1
        };
        rows.push(InvRow {
            loc: loc.to_string(),
            root: root_loc(loc),
            section: loc_section(loc),
            name: name.to_string(),
            base,
            tier,
            exalt,
            sub,
            empty: false,
            item_id: f[2].trim().parse().unwrap_or(0),
            count,
            slots: if f.len() > 4 {
                f[4].trim().parse().ok()
            } else {
                None
            },
        });
    }
    rows
}

/// A worn item: the slot as the dump names it (Finger and Ring folded to Fingers), and the
/// index of ITS row in `InventoryDump::all`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WornEntry {
    pub slot: String,
    pub name: String,
    pub base: String,
    pub tier: u8,
    pub row: usize,
}

/// One parsed `/outputfile inventory` dump.
#[derive(Clone, Debug)]
pub struct InventoryDump {
    pub path: PathBuf,
    /// From the file name `<Char>_<server>-Inventory.txt`. Whose bags these are matters: the
    /// newest dump on disk is not necessarily the character whose log is being tailed.
    pub character: Option<String>,
    pub server: Option<String>,
    pub modified: Option<DateTime<Utc>>,
    pub read_at: DateTime<Utc>,
    /// Every row including "Empty" placeholders, in dump order.
    pub all: Vec<InvRow>,
    /// Worn items: top-level rows whose location is a worn slot.
    pub worn: Vec<WornEntry>,
}

impl InventoryDump {
    pub fn parse(path: &Path, text: &str, modified: Option<DateTime<Utc>>) -> InventoryDump {
        let all = parse_rows(text, true);
        let mut worn = Vec::new();
        for (i, r) in all.iter().enumerate() {
            if r.empty || r.sub || !INV.worn.is_match(&r.loc) {
                continue;
            }
            let slot = if r.loc == "Finger" || r.loc == "Ring" {
                "Fingers"
            } else {
                r.loc.as_str()
            };
            worn.push(WornEntry {
                slot: slot.to_string(),
                name: r.name.clone(),
                base: r.base.clone(),
                tier: r.tier,
                row: i,
            });
        }
        let (character, server) = match path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| INV.file.captures(n))
        {
            Some(m) => (Some(m["ch"].to_string()), Some(m["srv"].to_string())),
            None => (None, None),
        };
        InventoryDump {
            path: path.to_path_buf(),
            character,
            server,
            modified,
            read_at: Utc::now(),
            all,
            worn,
        }
    }

    /// The rows every consumer but the socket reader walks: no placeholders.
    pub fn rows(&self) -> impl Iterator<Item = &InvRow> {
        self.all.iter().filter(|r| !r.empty)
    }

    /// What is worn in one slot, in dump order. The Sources ledger counts the worn set through
    /// this, one call per `WORN_SLOTS` entry.
    pub fn worn_in(&self, slot: &str) -> Vec<&WornEntry> {
        self.worn.iter().filter(|w| w.slot == slot).collect()
    }

    /// Rows per section, in `Section` order, for a ledger line: "Worn 21, Bags 80, Bank 24".
    /// Only sections with a row are listed; a count of zero is not a fact worth a word.
    pub fn section_counts(&self) -> Vec<(Section, usize)> {
        let mut counts: Vec<(Section, usize)> = Vec::new();
        for r in self.rows() {
            let sec = loc_section(&root_loc(&r.loc));
            match counts.iter_mut().find(|(s, _)| *s == sec) {
                Some((_, n)) => *n += 1,
                None => counts.push((sec, 1)),
            }
        }
        /* `Section` is declared in display order and derives Ord on it */
        counts.sort_by_key(|(s, _)| *s);
        counts
    }
}

/// The `/outputfile achievements` dump, found and read the way the inventory dump is: the newest
/// `*-Achievements.txt` in the game folder or the Logs folder. The ingest keeps the TEXT and the
/// line count; the Unlocks screen parses the text with the achievements grammar it carries, so
/// D7's one ingest lists the file, when it was read and how many rows it had, and the grammar
/// lives in one place.
#[derive(Clone, Debug)]
pub struct AchievementsDump {
    pub path: PathBuf,
    pub character: Option<String>,
    pub server: Option<String>,
    pub modified: Option<DateTime<Utc>>,
    pub read_at: DateTime<Utc>,
    pub text: String,
    /// Non-empty lines: the row count the Settings screen's SOURCES ledger prints.
    pub lines: usize,
}

impl AchievementsDump {
    pub fn parse(path: &Path, text: String, modified: Option<DateTime<Utc>>) -> AchievementsDump {
        static NAME: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?i)^(?P<ch>[^_]+)_(?P<srv>[^-]+)-Achievements\.txt$").expect("constant")
        });
        let (character, server) = match path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| NAME.captures(n))
        {
            Some(m) => (Some(m["ch"].to_string()), Some(m["srv"].to_string())),
            None => (None, None),
        };
        let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
        AchievementsDump {
            path: path.to_path_buf(),
            character,
            server,
            modified,
            read_at: Utc::now(),
            text,
            lines,
        }
    }
}

/* ================================================================= sources == */

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Log,
    Inventory,
    Achievements,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Log => "EQ log",
            SourceKind::Inventory => "inventory dump",
            SourceKind::Achievements => "achievements dump",
        }
    }
}

/// One line of the Settings screen's SOURCES ledger.
#[derive(Clone, Debug)]
pub struct Source {
    pub kind: SourceKind,
    pub path: PathBuf,
    pub last_read: Option<DateTime<Utc>>,
    pub records: usize,
    /// Why this source has no records, or what went wrong reading it.
    pub problem: Option<String>,
}

/// One `eqlog_<char>_<server>.txt` in the log folder.
#[derive(Clone, Debug)]
pub struct LogFile {
    pub path: PathBuf,
    pub character: Option<String>,
    pub server: Option<String>,
    pub modified: Option<SystemTime>,
    pub size: u64,
}

impl LogFile {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    }
}

/// The character in `eqlog_<character>_<server>.txt`.
pub fn character_of_log(file_name: &str) -> Option<String> {
    static CH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"eqlog_([^_]+)_").expect("constant"));
    CH.captures(file_name).map(|m| m[1].to_string())
}

fn server_of_log(file_name: &str) -> Option<String> {
    static SV: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^eqlog_[^_]+_(.+)\.txt$").expect("constant"));
    SV.captures(file_name).map(|m| m[1].to_string())
}

/// Where the client writes logs on a default install: `Daybreak Game Company\Installed
/// Games\EverQuest Legends\Logs` under the Public profile on Windows, and the same layout inside
/// the osxEQL wineprefix on a Mac.
/// The public-profile path is read from the environment first so a machine that moved its
/// Public folder is still found; the literal is the documented default.
pub fn usual_log_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(target_os = "windows") {
        if let Some(public) = std::env::var_os("PUBLIC") {
            v.push(
                PathBuf::from(public)
                    .join("Daybreak Game Company")
                    .join("Installed Games")
                    .join("EverQuest Legends")
                    .join("Logs"),
            );
        }
        let literal = PathBuf::from(
            r"C:\Users\Public\Daybreak Game Company\Installed Games\EverQuest Legends\Logs",
        );
        if !v.contains(&literal) {
            v.push(literal);
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = dirs::home_dir() {
            v.push(
                home.join("Library")
                    .join("Application Support")
                    .join("osxEQL")
                    .join("prefix")
                    .join("drive_c")
                    .join("users")
                    .join("Public")
                    .join("Daybreak Game Company")
                    .join("Installed Games")
                    .join("EverQuest Legends")
                    .join("Logs"),
            );
        }
    }
    v
}

/// The log folder in use, and every folder that was tried to find it. A settings value is
/// taken as given even if it does not exist (the screen then says it is unreadable, which is
/// the right complaint for a wrong setting); the usual paths are only used when they exist.
#[derive(Clone, Debug, Default)]
pub struct LogDirReport {
    pub dir: Option<PathBuf>,
    pub tried: Vec<PathBuf>,
    pub problem: Option<String>,
}

fn resolve_log_dir(from_settings: Option<&Path>) -> LogDirReport {
    if let Some(d) = from_settings {
        return LogDirReport {
            dir: Some(d.to_path_buf()),
            tried: vec![d.to_path_buf()],
            problem: None,
        };
    }
    let tried = usual_log_dirs();
    let dir = tried.iter().find(|d| d.is_dir()).cloned();
    let problem = if dir.is_none() {
        Some(if tried.is_empty() {
            String::from("No log folder is set and this platform has no usual place to look. Set one in Settings.")
        } else {
            format!(
                "No log folder is set and none of the usual places exist: {}. Set the Logs folder in Settings.",
                tried.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("; ")
            )
        })
    } else {
        None
    };
    LogDirReport {
        dir,
        tried,
        problem,
    }
}

/// Every `eqlog_*.txt` in the folder, case-insensitive on the name.
fn list_logs(dir: &Path) -> Result<Vec<LogFile>, String> {
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^eqlog_.+\.txt$").expect("constant"));
    let rd = std::fs::read_dir(dir)
        .map_err(|e| format!("Log folder is unreadable: {}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !NAME.is_match(name) {
            continue;
        }
        let Ok(md) = entry.metadata() else { continue };
        out.push(LogFile {
            path: entry.path(),
            character: character_of_log(name),
            server: server_of_log(name),
            modified: md.modified().ok(),
            size: md.len(),
        });
    }
    out.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(out)
}

/// The newest `*-Inventory.txt` across the game folder (the parent of Logs) and the log folder
/// itself. Returns the pick and whether any folder was readable.
fn newest_dump(log_dir: &Path) -> (Option<(PathBuf, SystemTime)>, bool) {
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)-Inventory\.txt$").expect("constant"));
    newest_matching(log_dir, &NAME)
}

/// The newest `*-Achievements.txt`, same two folders.
fn newest_achievements(log_dir: &Path) -> (Option<(PathBuf, SystemTime)>, bool) {
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)-Achievements\.txt$").expect("constant"));
    newest_matching(log_dir, &NAME)
}

fn newest_matching(log_dir: &Path, name_rx: &Regex) -> (Option<(PathBuf, SystemTime)>, bool) {
    let mut best: Option<(PathBuf, SystemTime)> = None;
    let mut readable = false;
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = log_dir.parent() {
        dirs.push(parent.to_path_buf());
    }
    dirs.push(log_dir.to_path_buf());
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        readable = true;
        for entry in rd.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name_rx.is_match(name) {
                continue;
            }
            let Ok(md) = entry.metadata() else { continue };
            let Ok(m) = md.modified() else { continue };
            let newer = match &best {
                Some((_, bm)) => m > *bm,
                None => true,
            };
            if newer {
                best = Some((entry.path(), m));
            }
        }
    }
    (best, readable)
}

fn sys_to_utc(t: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(t)
}

/* ================================================================== session == */

/// The longest silence between two events that still counts as time at the keyboard.
///
/// [`Session::bump_active`] carries the gap distribution this was read off and the argument for
/// putting the cut where it is. It is named rather than written in place so the screens, the
/// tests and the rule itself cannot drift apart.
pub const SESSION_GAP_MAX: i64 = 1800;

/// The session strip's numbers. Counted from LIVE events only: the bootstrap tail is history, and
/// a strip that counted it would report a week of kills as this sitting's. Active time adds up
/// event-to-event gaps under [`SESSION_GAP_MAX`]; a longer silence is time away from the game,
/// not time spent playing it, and does not count.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub kills: u32,
    pub loots: u32,
    pub xp_sum: f64,
    pub active_sec: i64,
    last_ev_ts: i64,
}

impl Session {
    /// Time between two events counts as played time when the gap is under half an hour.
    ///
    /// THE CUT IS TAKEN FROM THE GAP DISTRIBUTION OF A REAL LOG, not from a round number that
    /// sounded right. Over `tests/fixtures/princess-night.txt`, thirty hours of one character's
    /// play, there are 5,883 gaps between consecutive events: 5,787 of them are a minute or less,
    /// 75 fall in one to five minutes, 17 in five to fifteen, and then the distribution empties
    /// out. Exactly one gap lands between fifteen and thirty minutes, and the only three above
    /// thirty minutes run to nine hours, which is somebody sleeping.
    ///
    /// So anywhere in the fifteen-to-thirty-minute range separates the same two populations, and
    /// the cut sits at the far end of that empty stretch: it counts the one ambiguous gap as play
    /// rather than throwing away time that was probably a slow camp, and still refuses the three
    /// that are plainly absences. `session_counts_a_gap_under_the_cut_and_refuses_one_over`
    /// below fails if this value moves in either direction.
    fn bump_active(&mut self, ts: i64) {
        if self.last_ev_ts != 0 && ts > self.last_ev_ts && ts - self.last_ev_ts < SESSION_GAP_MAX {
            self.active_sec += ts - self.last_ev_ts;
        }
        if ts > self.last_ev_ts {
            self.last_ev_ts = ts;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.kills == 0 && self.loots == 0 && self.xp_sum == 0.0
    }
}

/* =================================================================== ingest == */

/* =================================================================== fights == */

/// The quiet window this build cuts fights at, taken from the engine rather than restated.
///
/// A `30` typed here would be a SECOND OPINION about a measured judgement. [`QUIET_SECONDS`] is
/// not a round number somebody liked: the workbench's own help says every value from 29 to 167
/// cuts the reference capture identically, which is the evidence for it, and the day that
/// constant moves this screen has to move with it or the desktop and the CLI will quietly
/// disagree about how many fights a log contains.
///
/// The conversion exists only because the desktop row contract types the window `u32` while the
/// engine, whose spans are `i64`, types it `i64`. See the note in `unproven`: this is the one
/// place in the wiring where the two disagree, and here is where it costs a line instead of a
/// defect. The fallback arm is unreachable while the constant is a small positive number, and it
/// saturates UP on purpose: a too-long window merges two fights into one row that is at least
/// honestly labelled, while a zero window would cut one fight into hundreds of rows for
/// encounters that never happened.
/// # THERE WERE TWO OF THESE AND ONLY ONE OF THEM SHIPPED
///
/// `crate::fights::quiet_window` has the same body, the same doc and the same claim to being
/// the single conversion point, and it had ZERO production callers: every `fold_text` in the
/// app came through this private copy. Nothing was wrong on screen, because the two agreed.
/// That is the hazard rather than the reprieve: retune the public one, or fix a saturation bug
/// in it, and the app goes on cutting fights with this one while
/// `fights::the_desktop_window_is_the_engines_measured_window` stays green, because it asserts
/// against the copy nothing ships.
///
/// SO THIS IS A RE-EXPORT AND NOT A BODY. One conversion, one place, and the guard over there
/// now covers the number this file actually folds with.
use crate::fights::{combat_window, quiet_window};

/// Fold a bootstrap tail into owned fight rows, and say which row the 40MB cap made short.
///
/// THE FOLD IS HERE AND NOT IN THE SCREEN, and that is not a preference. `Entry`, `Fight`,
/// `Participant` and `Fights` all borrow the log text; the text is `TailText::text`, a local of
/// the worker thread; the result rides home over an `mpsc` channel, which wants `Send + 'static`.
/// [`fold_text`] is the one place a borrow becomes an owned row, so this function is the border,
/// and nothing borrowed crosses it.
///
/// THE CUT FLAG IS THE SECOND JOB AND IT IS NOT DECORATION. `read_tail` keeps at most
/// [`TAIL_CAP`] bytes and `tt.start > 0` says it threw the rest away, so the window can open in
/// the MIDDLE of a fight: the aggregator opens a fresh fight on the first line it sees and reports
/// a damage total missing everything before the cut. This is not hypothetical. A 40MB tail of
/// this machine's own eqlog_Reviir_neriak.txt opens on the first stamped line of a greater
/// minotaur fight and reports 36,071 damage over 285s for an encounter whose beginning is not in
/// the file. A number like that reads as a measurement, which is precisely why a row that prints
/// it without saying it is short is worse than no row at all: the reader cannot see it is wrong.
///
/// ONLY THE OLDEST ROW, AND ONLY WHEN THE CAP BIT. Every later fight opened inside the window and
/// is whole. `fold_text` hands rows back in log order (the aggregator opens fights in log order
/// and closes them in the same order; the workbench prints #1 as the oldest, and the test below
/// pins it), so the oldest row is index 0. It is flagged whenever the file was cut even though a
/// quiet stretch at the cut point would have left it whole too: over-warning on one row costs a
/// word, under-warning costs the reader a wrong number they have no way to spot.
/// APPEND, AND ENFORCE THE ONE CEILING NOTHING MAY EXCEED. Returns whether anything was dropped.
///
/// # THIS USED TO TRIM TO [`LIVE_LINES`] AND IT CUT INTO THE FIGHT IT EXISTS TO REPORT
///
/// `Ingest::current_fight` is the last row of a fold over this buffer, so a trim that does not
/// know where the open fight began will happily throw its opening away. Once a fight has produced
/// more than `LIVE_LINES` lines, every poll dropped as many of its oldest lines as it appended new
/// ones and the fold's view of the fight started later and later:
///
///   * THE CLOCK STOPPED, because `secs` is last stamp minus first and both ends were now moving
///     together. The owner's screenshot of a Plane of Hate pull reads `02:40` and would not
///     advance. Measured in `a_fight_longer_than_the_live_window_is_not_reported_as_a_short_one`:
///     a 4,999 second fight reported as 139.
///   * AND THE DAMAGE WAS A FRACTION of what the fight had dealt, with nothing on screen saying
///     so, which is the half a reader cannot catch: a stalled clock is visible and a total that
///     is quietly short is not.
///
/// A RAID ZONE IS WHERE IT BITES, because what fills the window is LINE RATE and not time. Forty
/// people swinging make twenty thousand lines in a couple of minutes, which is why four fights of
/// the reference capture never came near it.
///
/// SO THE LINE COUNT IS NO LONGER A TRIM, IT IS A FLOOR. [`trim_recent`] does the trimming and it
/// only ever drops fights that are CLOSED. What is left here is a hard ceiling, so a single
/// endless fight cannot grow the buffer without bound, and it reports when it fires because a
/// fight whose opening was dropped is a fight whose totals are a floor.
///
/// # A LINE THAT LEAVES THE FRONT GOES INTO `before`, AND THAT IS WHY IT IS AN ARGUMENT
///
/// `before` is the reader's group as of the line in front of `buf[0]` (see
/// [`ActiveLog::before`]). Dropping a line without pushing it there loses whatever it said: a
/// group formed in the dropped lines is `Unknown` to every later fold, and a removal dropped is a
/// group that never ended. Both ways that pop the front, this one and [`trim_recent`], take the
/// party, so a third cannot be written without being handed it.
fn push_recent(
    buf: &mut VecDeque<String>,
    before: &mut Party,
    lines: impl IntoIterator<Item = String>,
) -> bool {
    for l in lines {
        buf.push_back(l);
    }
    let mut dropped = false;
    while buf.len() > LIVE_CAP {
        if let Some(gone) = buf.pop_front() {
            before.push(&gone);
        }
        dropped = true;
    }
    dropped
}

/// DROP THE FIGHTS THAT ARE OVER, AND NOTHING ELSE.
///
/// CALLED AFTER THE FOLD AND NOT BEFORE, because it is the fold that says where the open fight
/// begins. `rows.last()` is the fight [`Ingest::current_fight`] hands to every live surface, and
/// `FightRow::start` is "the stamp of the first combat line, exactly as the log printed it".
///
/// MATCHED ON THE STAMP TEXT AND NOT ON A PARSED TIME, deliberately. The stamp is kept as text
/// precisely because the log carries no zone offset, so there is no ordering to compare against;
/// what there is, is the exact string. The first line in the buffer carrying it is at or BEFORE
/// the fight's first combat line (a stamp covers a whole second, and the log prints up to thirty
/// two lines inside one), so cutting there can only ever keep too much. Keeping too much is a
/// slightly larger fold; dropping too much is the defect this whole function exists to fix.
///
/// AND NEVER BELOW [`LIVE_LINES`], so the effects panel and anything else reading the tail keeps
/// the history it had. Between pulls that leaves the buffer where it has always been.
///
/// EVERY LINE IT DROPS GOES INTO `before`, for the reason [`push_recent`] gives. This is the path
/// that fires on an ordinary evening: a group formed an hour ago has long since slid off the front
/// of the window, and what the fold still knows about it is what this pushed.
fn trim_recent(buf: &mut VecDeque<String>, before: &mut Party, rows: &[FightRow]) {
    let Some(open) = rows.last() else {
        return;
    };
    if rows.len() < 2 || buf.len() <= LIVE_LINES {
        return;
    }
    let stamp = format!("[{}]", open.start);
    let Some(at) = buf.iter().position(|l| l.starts_with(&stamp)) else {
        return;
    };
    /* The floor is measured from the END, so this is how many may go. */
    let spare = buf.len() - LIVE_LINES;
    for _ in 0..at.min(spare) {
        if let Some(gone) = buf.pop_front() {
            before.push(&gone);
        }
    }
}

/// RE-FOLD THE END OF THE LOG AND SAY WHETHER THE LAST FIGHT IS STILL GOING.
///
/// THE OWNER NAME MATTERS HERE FOR THE SAME REASON IT MATTERS IN THE BOOTSTRAP: without it the
/// reader is two rows in his own fight, once as `you` and once under his character name, and the
/// DPS overlay would show him twice with his damage split between them.
///
/// THE OPEN-FIGHT RULE. `fold_text` always closes the final fight with `Ended::EndOfLog`, because
/// the text ran out and it cannot know why. This file has not run out: it is being appended to. So
/// the last fight is treated as STILL GOING when the gap between its last line and the newest line
/// the fold saw is inside the engine's own quiet window. Both stamps come out of the same fold, in
/// the same units, so no wall clock and no timezone enters this.
///
/// # THE GROUP STARTS FROM `before`, NOT FROM NOTHING
///
/// The window is the last twenty thousand lines, and a group is formed once and then says nothing
/// for hours. Folded from an empty party, every live fight after the formation slid off the front
/// would be not known, on every poll, for the rest of the evening, while the bootstrap's list of
/// the very same fights said who was in the group. So the fold starts from a CLONE of the party
/// that has seen every line in front of `buf[0]`. A clone, because the next poll must start from
/// the same point and not from this poll's end: pushing the window twice replays it, and a replay
/// is a clock stepping back.
fn fold_recent(
    buf: &VecDeque<String>,
    before: &Party,
    owner: Option<&str>,
) -> (Vec<FightRow>, bool, i64) {
    if buf.is_empty() {
        return (Vec::new(), false, i64::MAX);
    }
    let text: String = buf.iter().fold(String::new(), |mut s, l| {
        s.push_str(l);
        s.push('\n');
        s
    });
    let (rows, _) = fold_text_after(&text, quiet_window(), owner, before.clone());

    /* THE NEWEST STAMP IN THE WINDOW, WHICH IS NOT THE LAST FIGHT'S LAST STAMP.
     *
     * Chat, casts, loot and auto-attack toggles are graded `Beat::Idle`: they neither extend the
     * open fight nor close it, so they never reach `FightRow::end`. They ARE how the file proves
     * the reader is alive and not swinging, which is the whole question `still_going` asks, so
     * the gap has to be measured against the newest line the fold SAW rather than the newest line
     * it folded. Read backwards off the same buffer, through the same grammar. */
    let newest = buf
        .iter()
        .rev()
        .find_map(|l| grimoire_parse::combat::parse(l).map(|e| e.at.to_owned()));

    /* THE FIRST ROW IS ALWAYS SUSPECT AND IS NOT MARKED `cut` HERE. This window is a slice out of
     * the middle of a file by construction, so its oldest fight is nearly always clipped; saying
     * so on every row would be crying wolf, and nothing reads this list but the overlay, which
     * shows the LAST row. `Ingest::fights` is the list that carries an honest `cut`. */
    let open = rows
        .last()
        .is_some_and(|r| still_going(r, &rows, newest.as_deref()));
    /* AND HOW LONG AGO THE LAST BLOW LANDED, IN THE LOG'S OWN SECONDS, because the mark beside the
     * meter has three things to say and `open` can only carry two of them. `i64::MAX` is "the log
     * cannot place it", which reads as closed rather than as held. */
    let gap = rows
        .last()
        .and_then(|r| {
            Some(
                grimoire_parse::fights::seconds(newest.as_deref()?)?
                    - grimoire_parse::fights::seconds(&r.end)?,
            )
        })
        .unwrap_or(i64::MAX);
    (rows, open, gap)
}

/// Is this the fight the reader is IN, rather than the last one he finished?
///
/// SPLIT OUT SO IT CAN BE DRIVEN WITHOUT A LOG ON DISK. The whole rule is one comparison and it is
/// the difference between an overlay that says "the log stopped" at somebody mid-pull and one that
/// says he is still swinging.
/// # THE GAP WAS DOCUMENTED AND NEVER APPLIED, SO `IN COMBAT` NEVER WENT OUT
///
/// This returned `true` on one test: `last.ended != "the log stopped"`. `fold_recent`'s own doc
/// says the rule is a GAP -- "the last fight is treated as STILL GOING when the gap between its
/// last line and the newest line the fold saw is inside the engine's own quiet window" -- and no
/// line of this function ever looked at a stamp.
///
/// AND THE ONE TEST IT DID MAKE CAN ESSENTIALLY NEVER FAIL. The aggregator emits `Ended::Quiet`
/// only on a fight it is closing to open a NEW one, and `Fights::finish` closes the last fight as
/// `EndOfLog` however many minutes of silence preceded it. So the last row of any `fold_recent` is
/// `the log stopped` unless a zone line cut it.
///
/// WHAT A READER SAW: `IN COMBAT` in live green on the Live page and the DPS overlay, over a
/// finished fight's frozen totals, for as long as he sat medding or reading chat. The one number
/// on that window a person checks before believing any other, stuck on.
///
/// THE GAP IS MEASURED IN THE LOG'S OWN CLOCK and against the engine's own window. Both stamps
/// come out of the same buffer through the same grammar, so no wall clock and no timezone enters
/// this: an app left running overnight on a stale file must not decide a pull is over because
/// real time passed, and one whose reader is genuinely mid-pull must not decide it is over
/// because the machine is slow.
fn still_going(last: &FightRow, all: &[FightRow], newest: Option<&str>) -> bool {
    /* A fight the engine closed for a REASON that is not the end of the text is finished, whatever
     * its stamps say: it went quiet, or the reader zoned, and both are real endings. */
    if last.ended != "the log stopped" {
        return false;
    }
    /* AND THE ENGINE ONLY EVER SAYS THAT ABOUT THE LAST ROW, so a list whose last row says it is
     * the one this is about. Asserted rather than assumed because a fold that started closing
     * earlier rows that way would make every fight look live. */
    debug_assert!(
        all.iter()
            .rev()
            .skip(1)
            .all(|r| r.ended != "the log stopped"),
        "only the newest fight can end because the text ran out"
    );

    /* NO READABLE STAMP AT THE END OF THE WINDOW: the fold cannot place the present, so it keeps
     * the engine's own answer rather than inventing an ending. */
    let (Some(newest), Some(end)) = (
        newest.and_then(grimoire_parse::fights::seconds),
        grimoire_parse::fights::seconds(&last.end),
    ) else {
        return true;
    };
    /* A STAMP BEFORE THE FIGHT'S OWN END is a clock that stepped back, which the engine has a
     * whole `Ended::Backwards` arm for. Not an ending, and not a gap either. */
    if newest < end {
        return true;
    }
    /* THE COMBAT WINDOW AND NOT THE QUIET ONE. `quiet_window` is how long a fight may go silent
     * before the NEXT line counts as a different fight, which is a question about boundaries and
     * is deliberately generous. This is the question "is he swinging now", and answering it with
     * the boundary constant is what kept `IN COMBAT` lit for thirty seconds over a dead mob. */
    newest - end <= i64::from(combat_window())
}

/// EVERY `X begins casting Y.` IN `text`, INTO A CLASS BOOK.
///
/// THROUGH THE GRAMMAR AND NOT A PATTERN WRITTEN HERE. `combat::Event::CastStart` has carried the
/// caster and the spell since the grammar was written and nothing read it; a second reader of the
/// same line shape is a second thing to keep in step with the client.
///
/// THE READER'S OWN CASTS ARE SKIPPED, and that is not an oversight. `Actor::You` is the reader,
/// whose trio is a SETTING (`MY LEGEND / Gear`) and not something this app has to infer; inferring
/// it anyway would put a second answer beside a stated one and they would eventually disagree.
fn read_casts(text: &str, book: &mut crate::class::Book) {
    for line in text.lines() {
        let Some(entry) = grimoire_parse::combat::parse(line) else {
            continue;
        };
        if let grimoire_parse::combat::Reading::Event(grimoire_parse::combat::Event::CastStart {
            caster: grimoire_parse::combat::Actor::Named(who),
            spell,
        }) = entry.reading
        {
            book.saw(who, spell);
        }
    }
}

/// THE LIVE WINDOW AND THE GROUP IN FRONT OF IT, OFF ONE SPLIT OF THE BOOTSTRAP TEXT.
///
/// The window is the last [`LIVE_LINES`] lines, as it always was. Every line before those goes into
/// the party and into nothing else, and every line after goes into the window and into nothing
/// else. That split is the invariant [`ActiveLog::before`] rests on, so it is made once, here, by
/// counting, rather than by two iterators that each decide where the window starts.
///
/// THE BOOTSTRAP'S OWN FOLD SEES THE SAME LINES IN THE SAME ORDER. `fold_tail` pushes the whole of
/// `text` into a fresh party; this pushes the head into one and the live fold pushes the window into
/// a clone of it. So a fight in both lists carries the same group in both, which is what
/// `a_live_fold_after_the_bootstrap_keeps_the_group_the_bootstrap_saw_form` pins.
///
/// WHAT IT COSTS: one more pass of `Party::push` over the head of a tail of at most [`TAIL_CAP`]
/// bytes, on the worker, inside a scan that already reads, regexes and folds the same bytes twice.
fn seed_live(text: &str) -> (VecDeque<String>, Party) {
    let head = text.lines().count().saturating_sub(LIVE_LINES);
    let mut before = Party::new();
    let mut lines = text.lines();
    for l in lines.by_ref().take(head) {
        before.push(l);
    }
    let mut recent = VecDeque::new();
    push_recent(&mut recent, &mut before, lines.map(str::to_owned));
    (recent, before)
}

fn fold_tail(tt: &TailText, owner: Option<&str>) -> (Vec<FightRow>, u32) {
    let (mut rows, unreadable) = fold_text(&tt.text, quiet_window(), owner);
    /* THROUGH `mark_clipped` AND NOT BY HAND, and the hand-rolled version was here.
     *
     * `FightRow::cut`'s own doc says the flag "is left false here and stamped by
     * `mark_clipped`", which was false about the shipping build: `mark_clipped` had no caller
     * outside its own test, so `only_the_oldest_row_carries_the_tail_cap_warning` certified a
     * function the app never ran while the warning a reader actually sees came from these four
     * lines, which nothing covered.
     *
     * AND IT TAKES THE BOOL RATHER THAN GUARDING ON IT, which is the half the hand-rolled copy
     * could not do: passing `false` CLEARS the flag, so a re-fold of a log since read whole
     * cannot leave a stale floor warning on it. */
    crate::fights::mark_clipped(&mut rows, tt.start > 0);
    (rows, unreadable)
}

const TAIL_POLL: Duration = Duration::from_millis(1000);

/// HOW MUCH OF THE END OF THE LOG THE LIVE FOLD RE-READS, in lines.
///
/// THE FIGHTS LIST FROM A BOOTSTRAP IS A PHOTOGRAPH AND THIS IS THE MOVING PICTURE. `scan` folds
/// the whole 40MB tail once, on the worker, and `tail` has never touched `fights` since: it reads
/// the appended bytes and feeds only the kill and loot stream. So a fights list, and every screen
/// built on it, was frozen at whatever the app saw when it started. Measured 2026-09-05, and it is
/// why the DPS overlay never changed fight.
///
/// RE-FOLDING THE WHOLE 40MB EVERY SECOND IS NOT THE FIX. That fold is 0.128s and a 40MB read
/// beside it, once a second, for ever. This window is the last few thousand lines, which is bounded
/// work no matter how long the session runs.
///
/// WHY THIS MANY. A fight closes after [`quiet_window`] seconds of nothing, so the live fight plus
/// the one before it is all this has to contain, and the reference capture's busiest fight is 1,626
/// combat lines over four and a half minutes. 20,000 lines is an order of magnitude of headroom
/// over that, and a fold of it costs roughly a hundredth of what the bootstrap costs.
/// THE FLOOR: the buffer never trims below this, so the effects panel keeps its history.
const LIVE_LINES: usize = 20_000;

/// THE CEILING: one endless fight may not grow the buffer past this.
///
/// TWENTY TIMES THE FLOOR, which on the owner's own logs is between one and two hours of a raid
/// zone at full tilt, and about forty megabytes of `String`. A fight that reaches it has its
/// opening dropped and is marked `cut`, so the page says its totals are a floor rather than
/// printing a fraction as a measurement.
const LIVE_CAP: usize = 400_000;
const DUMP_POLL: Duration = Duration::from_millis(3000);
/// How many loot events are kept for the feed: enough that a filter over it still has history to
/// work on after a long session.
const LOOT_CAP: usize = 500;

/// The file being tailed, with its stream and cursor.
struct ActiveLog {
    file: LogFile,
    stream: KillReader,
    tail: Tail,
    read_at: DateTime<Utc>,
    /// How much of the file the bootstrap covered: the byte it started at.
    tail_start: u64,
    /// Kill events attributed to this file, bootstrap plus live.
    kills: usize,
    /// THE END OF THE LOG, KEPT SO IT CAN BE FOLDED AGAIN AS IT GROWS. Newest last, capped at
    /// [`LIVE_LINES`].
    ///
    /// LINES AND NOT AN AGGREGATOR, AND THAT IS FORCED. `grimoire_parse::fights::Fights::finish`
    /// takes the aggregator BY VALUE and every type inside it borrows the log text, so there is no
    /// way to hold one open across polls and ask it what it has so far. Keeping the TEXT and
    /// re-folding it is the only shape that works without changing the engine.
    recent: VecDeque<String>,
    /// THE READER'S GROUP AS OF THE LINE IN FRONT OF `recent[0]`, from bytes this app actually read.
    ///
    /// # THE ONE PIECE OF FOLD STATE THAT CAN BE HELD OPEN, AND WHY IT HAS TO BE
    ///
    /// `recent` says the aggregator cannot be kept across polls, and that is true of `Fights`. It
    /// is not true of `grimoire_parse::group::Party`, which owns its strings and is `Clone`, and it
    /// must not be re-derived from `recent`: the lines that formed a group left the window long ago
    /// on any real evening, so a party folded from the window alone is `Unknown` for every fight
    /// after them. [`fold_recent`] clones this and pushes the window into the clone.
    ///
    /// # THE INVARIANT, AND THE THREE PLACES THAT KEEP IT
    ///
    /// This party has seen every line before `recent[0]`, and no line of `recent`.
    ///   * [`seed_live`] builds both at bootstrap from ONE split of the tail text.
    ///   * [`push_recent`] and [`trim_recent`], the only two ways a line leaves the front, push it
    ///     here as it goes.
    ///
    /// A LINE PUSHED TWICE BREAKS IT AS SURELY AS A LINE MISSED. A member leaving twice looks like a
    /// stranger leaving and revokes the group, and a window whose stamps are older than this
    /// party's newest reads as the clock stepping back.
    ///
    /// A CLIPPED TAIL STARTS AT `Unknown` AND THAT IS RIGHT. When `tail_start > 0` the bytes before
    /// the cut were never read, so nothing is carried from them; the fights they would have settled
    /// are `None`, which is what this app knows about them.
    before: Party,
    /// Whether [`LIVE_CAP`] has ever thrown away a line of this file.
    ///
    /// STICKY, BECAUSE THE DAMAGE IS. Once a fight's opening has gone it does not come back, and
    /// the fight it belonged to keeps being reported for as long as it runs.
    recent_cut: bool,
}

/// What the worker thread hands back: everything a bootstrap decides.
/// What the worker thread hands back: everything a bootstrap decides.
struct Scan {
    dir: LogDirReport,
    logs: Vec<LogFile>,
    active: Option<ActiveLog>,
    /// Events from the bootstrap tail, kills flushed, in log order.
    events: Vec<Event>,
    /// The bootstrap tail cut into fights, OWNED. Every engine type borrows the log text and the
    /// text is a worker-thread local, so a borrowed row could not cross this channel at all; see
    /// `fold_tail`. Oldest first, and the oldest carries `cut` when the 40MB cap bit.
    fights: Vec<FightRow>,
    /// Lines the fold found carrying a stamp it could not read. Carried beside the rows and never
    /// derived from them: a reader has to be able to see how much of the file the fight numbers
    /// do NOT rest on, and a count that lives somewhere else drifts from the rows it qualifies.
    fights_unreadable: u32,
    active_problem: Option<String>,
    roster: Option<Result<Arc<Roster>, String>>,
    inventory: Option<Result<InventoryDump, String>>,
    inv_sig: Option<(PathBuf, SystemTime)>,
    inv_readable: bool,
    achievements: Option<Result<AchievementsDump, String>>,
    ach_sig: Option<(PathBuf, SystemTime)>,
}

struct ScanConfig {
    log_dir: Option<PathBuf>,
    data_root: Option<PathBuf>,
    roster: Option<Arc<Roster>>,
}

/// The whole bootstrap, on the worker. Nothing in here panics on a bad file: every failure is a
/// `problem` string that names the path.
/// The whole bootstrap, on the worker. Nothing in here panics on a bad file: every failure is a
/// `problem` string that names the path.
fn scan(cfg: ScanConfig) -> Scan {
    let roster = match cfg.roster {
        Some(r) => Some(Ok(r)),
        None => cfg
            .data_root
            .as_deref()
            .map(|root| Roster::load(root).map(Arc::new)),
    };
    let name2key: Arc<HashMap<String, String>> = match &roster {
        Some(Ok(r)) => r.name2key(),
        _ => Arc::new(HashMap::new()),
    };

    let mut dir = resolve_log_dir(cfg.log_dir.as_deref());
    let mut logs = Vec::new();
    let mut active = None;
    let mut events = Vec::new();
    let mut fights = Vec::new();
    let mut fights_unreadable = 0;
    let mut active_problem = None;
    let mut inventory = None;
    let mut inv_sig = None;
    let mut inv_readable = false;
    let mut achievements = None;
    let mut ach_sig = None;

    if let Some(d) = dir.dir.clone() {
        match list_logs(&d) {
            Err(e) => dir.problem = Some(e),
            Ok(found) => {
                logs = found;
                if logs.is_empty() {
                    active_problem = Some(format!(
                        "No eqlog_*.txt files in {}. Is logging on? (/log)",
                        d.display()
                    ));
                } else {
                    let file = logs[0].clone();
                    match read_tail(&file.path, file.size) {
                        Err(e) => active_problem = Some(format!("{}: {e}", file.path.display())),
                        Ok(tt) => {
                            /* One pass over the tail (`parse_log`): kills for the tracker, loot
                             * for the feed, and the final zone so the live stream starts where
                             * the player is. The live stream is a NEW one seeded with that zone,
                             * because the bootstrap stream's candidates were all flushed. */
                            let boot = parse_log(&tt.text, Arc::clone(&name2key));
                            events.extend(boot.events);
                            /* The SECOND pass over the same text, on this same worker thread, for
                             * the same reason the first one is here: `tt.text` is a local of this
                             * function and the fold's output has to outlive it. Measured at
                             * 0.128s for a full 40MB tail against 0.129s for the whole of
                             * `grimoire fights` on the same bytes, so this is the cheaper half of
                             * a scan that already reads and regexes 40MB, and it lands inside a
                             * wait the reader is already in (`scanning()` is true throughout).
                             *
                             * THE OWNER NAME COMES FROM THE FILE NAME AND NOWHERE ELSE. It is
                             * never in a line, so the aggregator cannot know it; without it the
                             * player appears twice in every participant table, once as `you` and
                             * once under their own name. `character_of_log` already read it off
                             * `eqlog_<char>_<server>.txt` when this file was listed. */
                            let (rows, unreadable) = fold_tail(&tt, file.character.as_deref());
                            fights = rows;
                            fights_unreadable = unreadable;
                            /* SEED THE LIVE WINDOW OFF THE SAME BYTES, so the overlay has the
                             * fight the reader just finished the moment the app opens rather
                             * than being blank until he pulls something. */
                            /* THE SEED IS A TAIL AND SO ITS OLDEST FIGHT IS CLIPPED BY
                             * construction, but the ceiling did not eat it:  is about
                             * this buffer throwing lines away, and this is the first write to it.
                             *
                             * AND THE GROUP IS CARRIED IN WITH IT, off the same split of the same
                             * bytes. See `seed_live`. */
                            let (recent, before) = seed_live(&tt.text);
                            let mut stream = KillReader::new(Arc::clone(&name2key));
                            stream.zone = boot.stream.zone.clone();
                            stream.last_ts = boot.stream.last_ts;
                            stream.lines = boot.stream.lines;
                            let kills = events
                                .iter()
                                .filter(|e| matches!(e, Event::Kill(_)))
                                .count();
                            active = Some(ActiveLog {
                                file,
                                stream,
                                tail: Tail::at(tt.end),
                                read_at: Utc::now(),
                                tail_start: tt.start,
                                kills,
                                recent,
                                before,
                                recent_cut: false,
                            });
                        }
                    }
                }
            }
        }
        let (pick, readable) = newest_dump(&d);
        inv_readable = readable;
        if let Some((p, m)) = pick {
            inventory = Some(
                std::fs::read_to_string(&p)
                    .map(|text| InventoryDump::parse(&p, &text, Some(sys_to_utc(m))))
                    .map_err(|e| format!("{}: {e}", p.display())),
            );
            inv_sig = Some((p, m));
        }
        let (pick, _) = newest_achievements(&d);
        if let Some((p, m)) = pick {
            achievements = Some(read_achievements(&p, m));
            ach_sig = Some((p, m));
        }
    }

    Scan {
        dir,
        logs,
        active,
        events,
        fights,
        fights_unreadable,
        active_problem,
        roster,
        inventory,
        inv_sig,
        inv_readable,
        achievements,
        ach_sig,
    }
}

/// The achievements dump is written by the client in the console's code page; read lossily so a
/// stray byte in a task name never hides the whole file.
fn read_achievements(p: &Path, m: SystemTime) -> Result<AchievementsDump, String> {
    std::fs::read(p)
        .map(|b| {
            AchievementsDump::parse(
                p,
                String::from_utf8_lossy(&b).into_owned(),
                Some(sys_to_utc(m)),
            )
        })
        .map_err(|e| format!("{}: {e}", p.display()))
}

/// Why no dump was found, in words that name the fix. With no Logs folder resolved the fix is
/// Settings, not a slash command: the folder is where the dump is looked for, and a person told
/// to type `/outputfile inventory` with nowhere for the app to look would type it for nothing.
fn no_dump_words(dir: &LogDirReport, readable: bool, file: &str, command: &str) -> String {
    match &dir.dir {
        None => dir.problem.clone().unwrap_or_else(|| "No log folder is set, so there is nowhere to look for a dump. Set the Logs folder in Settings.".to_owned()),
        Some(_) if !readable => "Game folder is unreadable.".to_owned(),
        Some(_) => format!("No {file} found beside the Logs folder. In game: {command}"),
    }
}

/// The one ingest. Construct from settings, call `tail()` once per frame (it is rate limited
/// and cheap), read `kills()`, `loot()`, `inventory()`, `sources()`.
/// The one ingest. Construct from settings, call `tail()` once per frame (it is rate limited
/// and cheap), read `kills()`, `loot()`, `fights()`, `inventory()`, `sources()`.
/// ONE LOG'S WHOLE-FILE FOLD, on its way back from the refill thread. See [`Ingest::start_refill`].
struct RefillBatch {
    who: crate::store::Owner,
    log: String,
    rows: Result<Vec<FightRow>, String>,
}

pub struct Ingest {
    /// WHAT EACH NAMED CASTER HAS BEEN PROVED TO BE. See [`crate::class`].
    ///
    /// ACCUMULATED AND NEVER REBUILT, which is the point of keeping it here rather than folding it
    /// with the fights. A class proved by one spell an hour ago stays proved: the evidence is a
    /// line that has already been read, and the live window it was read in has long since slid
    /// past it. Rebuilding from the window each poll would make a Wizard stop being a Wizard the
    /// moment he went quiet.
    classes: crate::class::Book,
    /// FIGHTS KEPT BETWEEN RUNS, or `None`, which is the DEFAULT. See [`Ingest::use_store`].
    store: Option<crate::store::Store>,
    /// How many fights this process has written, and how many it found already there.
    stored: crate::store::Wrote,
    /// EVERY FINISHED FIGHT THIS CHARACTER HAS ON DISK, oldest first.
    ///
    /// # THE DASHBOARD'S SOURCE, AND IT COSTS NOTHING EXTRA
    ///
    /// `keep_fights` already reads `Store::all` on every adopt to rebuild the hit point book,
    /// and then dropped the rows. They are kept here instead, so the Dashboards page can look
    /// back over a night or a week without touching the disk on a frame. `keep_live_fights`
    /// appends the fights it writes, so this stays chronological without a second read.
    ///
    /// NOT `fights`. That list is the bootstrap's fold of the log's TAIL and is never rewritten;
    /// this is the store, which survives the tail moving on and survives a relaunch. The two
    /// answer different questions and the pages that read them say which.
    history: Vec<FightRow>,
    /// BUMPED EVERY TIME `history` CHANGES, so a screen that caches a reading of it can tell.
    ///
    /// ITS LENGTH AND ITS NEWEST STAMP ARE NOT ENOUGH ANY MORE. A refill rewrites rows that are
    /// already there and moves neither, and a cache keyed on them keeps showing the rows as they
    /// were before it. See [`Ingest::start_refill`].
    history_gen: u64,
    /// WHAT EVERY MOB IN THE STORE HAS BEEN SEEN TO ABSORB. See [`crate::hp`].
    ///
    /// BUILT WHEN THE STORE IS WRITTEN AND NOT PER FRAME. It walks every fight this character has
    /// ever had, which is the point of it and far too much to do sixty times a second.
    ///
    /// "A BOOTSTRAP IS THE ONLY MOMENT THE STORE CHANGES" IS WHAT THIS SAID, AND IT WAS THE BUG.
    /// It was true of the code and it was never true of the app: a bootstrap is the only moment
    /// this list changes, so a reader who killed the same named mob ten times in an evening got the
    /// denominator and the kill count he had at LAUNCH, for the whole session, on a book that was
    /// learning nothing. Fights finished while the app runs are now written to the store and folded
    /// in here as they finish: see [`Ingest::keep_live_fights`], which is the ONLY way out
    /// of this buffer. There was a second, `live_fights`, and it is gone: it had no caller, and
    /// the page that would have wanted one proved it must not have it. The oldest row here opens
    /// wherever the ring buffer starts, so its `start` is the stamp of the first line the fold
    /// could see rather than the fight's own, and drawing that in a table of fights is an
    /// invented number with the label filed off. `keep_live_fights` refuses that same row, which
    /// is why the store is a safe door and a raw slice was not.
    hp: std::collections::BTreeMap<String, crate::hp::Reading>,
    /// HOW MANY READINGS IN `hp` CAN ACTUALLY ANSWER, kept beside the book rather than counted per
    /// frame. See [`Ingest::hp_known`], which is where the number is explained and where it was
    /// wrong. Every write to `hp` must be followed by `hp_recount`.
    hp_measured: usize,
    /// THE START STAMPS OF LIVE FIGHTS THIS APP WATCHED OPEN AND HAS NOT STORED YET.
    ///
    /// # A FIGHT MAY ONLY BE STORED IF ITS FIRST LINE WAS INSIDE THE WINDOW
    ///
    /// `fold_recent` says it plainly in its own doc: the live window is a slice out of the middle
    /// of a file, so its OLDEST fight is nearly always clipped, and it does not mark that row `cut`
    /// because on that path the warning would be on every row. A clipped row's `start` is not the
    /// fight's start and its damage is a floor. Storing one is worse than dropping it twice over:
    /// the start stamp is the store's dedupe key, so the floor would sit in the store forever and
    /// REJECT the whole fight when the next bootstrap offers it, and `hp` would take the floor for
    /// a mob's health.
    ///
    /// SO WHOLENESS IS PROVED BY WATCHING, NOT BY GUESSING AT THE BUFFER. A fight seen as the OPEN
    /// row of a fold that had a row before it is a fight whose first combat line the fold saw, with
    /// another fight's lines in front of it: its start is the real one. That is the only condition
    /// under which a stamp is put in here, and a stamp is taken out when the fight is stored.
    ///
    /// CLEARED ON EVERY BOOTSTRAP, because it is about one file's buffer and a bootstrap may have
    /// moved to another character's log entirely.
    live_whole: BTreeSet<String>,
    /// EVERY ONE-WORD NAME ANY FIGHT READ BY THIS INGEST HAS PROVEN IS A MOB, folded to lower case.
    ///
    /// ONE BOOK FOR EVERY CHARACTER AND NEVER CLEARED, because it is a fact about the world and not
    /// about the reader: the game will not name a player after an NPC, so `Xicotl` proven a mob on
    /// one night is a mob on every night. See [`Ingest::refresh_foes`].
    foe_book: BTreeSet<String>,
    /// The resolved tag per caster, and how many casters were in the book when it was resolved.
    ///
    /// CACHED BECAUSE THE LOOKUP IS A SCAN. `class::read` walks two thousand records per caster;
    /// doing that for every fighter on every frame would be the most expensive thing the app does,
    /// and the answer only changes when somebody casts something new. The count is the key: the
    /// book only ever grows, so a length that has not moved is a book that has not moved.
    tags: std::collections::BTreeMap<String, String>,
    tagged_at: usize,
    log_dir_setting: Option<PathBuf>,
    data_root: Option<PathBuf>,
    dir: LogDirReport,
    roster: Option<Arc<Roster>>,
    roster_problem: Option<String>,
    logs: Vec<LogFile>,
    active: Option<ActiveLog>,
    active_problem: Option<String>,
    kills: Vec<KillEvent>,
    loot: Vec<LootEvent>,
    /// The last bootstrap's fights, oldest first. As of `scanned_at()` and not a line later: the
    /// live path deliberately does not touch this (see the module note).
    fights: Vec<FightRow>,
    fights_unreadable: u32,
    /// THE END OF THE LOG, RE-FOLDED ON EVERY POLL THAT BROUGHT NEW LINES. Oldest first, like
    /// `fights`, and covering only the last [`LIVE_LINES`] lines rather than the whole tail.
    ///
    /// A SECOND LIST AND NOT A REWRITE OF THE FIRST, deliberately. `fights` is the HISTORY the
    /// Fights table pages through and it reaches back 40MB; this is the LIVE end and it reaches
    /// back a few thousand lines. Splicing one into the other would need a rule for the seam, and
    /// the seam is the one place the two folds can disagree; two lists with two owners cannot.
    live: Vec<FightRow>,
    /// Whether [`Ingest::current_fight`] is still going. See `fight_is_live`.
    live_open: bool,
    /// The character name the live fold folds `you` against, mirrored off the active log so the
    /// fold does not have to reach into `active` while `active` is borrowed mutably.
    live_owner: Option<String>,
    session: Session,
    tracker: TrackerState,
    inventory: Option<InventoryDump>,
    inv_problem: Option<String>,
    inv_sig: Option<(PathBuf, SystemTime)>,
    achievements: Option<AchievementsDump>,
    ach_problem: Option<String>,
    ach_sig: Option<(PathBuf, SystemTime)>,
    rx: Option<mpsc::Receiver<Scan>>,
    /// THE REFILL THREAD'S RESULTS, while one is running. See [`Ingest::start_refill`].
    refill_rx: Option<mpsc::Receiver<RefillBatch>>,
    /// Whose logs the running refill is reading, so a character change can start the new one.
    refill_for: Option<crate::store::Owner>,
    scan_started: Option<Instant>,
    rescan_again: bool,
    /// WHEN THE APP LAST SAW A NEW LOG LINE, ON THE MACHINE.S OWN CLOCK.
    ///
    /// THE ONE THING THE LOG CANNOT SAY. `still_going` compares two stamps out of the file, which
    /// is right for everything the file describes and useless the moment the file stops growing:
    /// kill a mob and stand still, and the newest stamp never moves, so the gap never widens and
    /// `IN COMBAT` stays lit until something, anything, gets written. A log that stopped being
    /// written is a reader who is not fighting, and only a real clock can notice.
    ///
    /// NOT A SUBSTITUTE FOR THE LOG CLOCK, A SECOND GUARD BESIDE IT. While lines keep arriving the
    /// stamps answer it; this answers it when they stop.
    last_line_at: Option<Instant>,

    /// HOW LONG AGO THE LAST BLOW LANDED, IN THE LOG'S OWN SECONDS, as of the last fold.
    ///
    /// Paired with `last_line_at`, which says how long ago that fold saw its newest byte. The sum
    /// of the two is how long it has really been since the reader swung, which is the only figure
    /// that can tell a held encounter from a closed one while the file sits still.
    live_gap: i64,
    last_poll: Option<Instant>,
    last_dump_poll: Option<Instant>,
    /// The last bootstrap's wall time, for "read 12s ago".
    scanned_at: Option<DateTime<Utc>>,
    /// WHEN THE LOG FOLDER WAS LAST LISTED, which is not [`Ingest::scanned_at`].
    ///
    /// TWO CLOCKS BECAUSE THERE ARE TWO READS, AND THE LOGS PAGE HAD ONLY ONE OF THEM. A bootstrap
    /// lists the folder AND folds up to forty megabytes of the newest file; a poll re-lists the
    /// folder every second and folds nothing. So the candidates table is seconds old on a page
    /// whose only stamp described a fold that may have run at launch, and a reader looking at a
    /// file that appeared two minutes ago could not tell whether the app had noticed it.
    ///
    /// SET WHERE THE LISTING IS ASSIGNED AND NOWHERE ELSE, which is `adopt` (the bootstrap's, off
    /// the worker) and `tail` (the poll's). Stamped at the moment the list REACHES this struct
    /// rather than the moment the directory was walked, exactly as `scanned_at` is: the worker's
    /// own walk happened a few milliseconds earlier and inventing that instant here would be a
    /// second number nothing measured.
    listed_at: Option<DateTime<Utc>>,
}

impl Ingest {
    /// Build from settings and start the first scan on a worker thread. Returns at once.
    pub fn new(settings: &crate::settings::Settings) -> Ingest {
        /* D6: the snapshot root is a setting first, then wherever the data module finds it. */
        let data_root = settings
            .data_root
            .clone()
            .or_else(crate::data::Snapshot::locate);
        let mut me = Ingest {
            classes: crate::class::Book::default(),
            /* NO STORE UNLESS SOMEBODY HANDS ONE OVER. See `use_store`. */
            store: None,
            stored: crate::store::Wrote::default(),
            history: Vec::new(),
            history_gen: 0,
            hp: Default::default(),
            hp_measured: 0,
            live_whole: BTreeSet::new(),
            foe_book: BTreeSet::new(),
            tags: std::collections::BTreeMap::new(),
            tagged_at: 0,
            live: Vec::new(),
            live_open: false,
            last_line_at: None,
            live_gap: i64::MAX,
            live_owner: None,
            log_dir_setting: settings.log_dir.clone(),
            data_root,
            dir: LogDirReport::default(),
            roster: None,
            roster_problem: None,
            logs: Vec::new(),
            active: None,
            active_problem: None,
            kills: Vec::new(),
            loot: Vec::new(),
            fights: Vec::new(),
            fights_unreadable: 0,
            session: Session::default(),
            /* THE COUNTING FILTERS COME FROM THE SAVED SETTINGS AND NOT FROM `Default`, and they
             * are seeded HERE rather than on the first frame the Kills view happens to be drawn.
             * Every window builds its own `Ingest` (`main::App::new`, `windows::ChildCx::new`),
             * and a filter applied to one of them and not the other prints two completion
             * percentages for one roster. Seeding at construction is what makes both agree from
             * the first frame. See `TrackerSettings`. */
            tracker: TrackerState {
                settings: settings.tracker.clone(),
                ..TrackerState::default()
            },
            inventory: None,
            inv_problem: None,
            inv_sig: None,
            achievements: None,
            ach_problem: None,
            ach_sig: None,
            rx: None,
            refill_rx: None,
            refill_for: None,
            scan_started: None,
            rescan_again: false,
            last_poll: None,
            last_dump_poll: None,
            scanned_at: None,
            listed_at: None,
        };
        me.rescan();
        me
    }

    /// Re-read everything: the log folder, the newest log's tail, the newest dump. Runs on a
    /// worker; the result lands on the next `tail()`. A rescan asked for while one is running
    /// queues exactly one more.
    pub fn rescan(&mut self) {
        if self.rx.is_some() {
            self.rescan_again = true;
            return;
        }
        let cfg = ScanConfig {
            log_dir: self.log_dir_setting.clone(),
            data_root: self.data_root.clone(),
            roster: self.roster.clone(),
        };
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("grimoire-ingest-scan".into())
            .spawn(move || {
                let _ = tx.send(scan(cfg));
            })
            .map(|_| {
                self.rx = Some(rx);
                self.scan_started = Some(Instant::now());
            })
            .unwrap_or_else(|e| {
                self.dir.problem = Some(format!("could not start the log reader thread: {e}"));
            });
    }

    /// Point the ingest at a different log folder or data root (Settings changed) and rescan.
    ///
    /// # AND IT RETURNS WITHOUT A RESCAN WHEN NEITHER MOVED, WHICH IS NEW
    ///
    /// `windows.rs` carried a comment asserting this function "compares the resolved folder and
    /// returns without a rescan when it is the same", and it did no such thing: it copied both
    /// paths, dropped the roster and called `rescan` unconditionally. That matters because of who
    /// calls it. `Windows::sync` answers ANY changed settings JSON this way, so every edit made in
    /// the main window while a pop-out was open (a fight note, an overlay width, an LFG post) paid
    /// for a fold of the last forty megabytes of the log on a worker, in the pop-out, while the
    /// owner was raiding. The comment was a description of the intended behaviour written as if it
    /// had happened, which is how a guard that does not exist survives a reading; the guard is here
    /// now and the doc it was relied on for is this one.
    ///
    /// THE COMPARISON IS AGAINST THE RESOLVED VALUES AND NOT THE SETTINGS FIELDS. `data_root` falls
    /// back to `data::Snapshot::locate()` when the setting is empty, so clearing a setting that
    /// pointed at the folder `locate` finds anyway moves nothing on disk and must not cost a
    /// bootstrap. `log_dir` is stored as given (`resolve_log_dir` is what searches, and it runs on
    /// the worker), so its comparison is the setting's own value.
    ///
    /// THE FILTERS ARE COPIED BEFORE THE GUARD AND NOT AFTER IT. They are the one thing here that
    /// is NOT a path, and they are the common case: a Kills checkbox saves settings and changes no
    /// folder, so seeding them after an early return would mean the checkbox reached the file, came
    /// back down through `sync`, and stopped one field short of the ingest that counts the kills.
    pub fn reconfigure(&mut self, settings: &crate::settings::Settings) {
        self.tracker.settings = settings.tracker.clone();
        let log_dir = settings.log_dir.clone();
        let data_root = settings
            .data_root
            .clone()
            .or_else(crate::data::Snapshot::locate);
        if self.log_dir_setting == log_dir && self.data_root == data_root {
            return;
        }
        self.log_dir_setting = log_dir;
        self.data_root = data_root;
        self.roster = None;
        self.rescan();
    }

    /// True while a bootstrap is in flight on the worker.
    pub fn scanning(&self) -> bool {
        self.rx.is_some()
    }

    fn adopt(&mut self, s: Scan) {
        self.dir = s.dir;
        self.logs = s.logs;
        /* AND THE LISTING IS STAMPED HERE TOO, NOT ONLY IN `tail`. A bootstrap lists the folder on
         * the worker and this is where that listing arrives, so a page reading `listed_at` before
         * the first poll would otherwise be told the candidates table had never been read while
         * looking straight at it. See the field. */
        self.listed_at = Some(Utc::now());
        /* WHOLESALE, LIKE THE EVENT LISTS ABOVE AND FOR A SHARPER REASON. The kills and loot
         * lists describe the tailed file and that file may have changed; a fight list is worse
         * than that if merged, because fights carry their own clock. Rows from two files
         * interleave by nothing at all and read as one continuous history that never happened.
         * There is no high-water mark to gate them with either: a fight has no identity, only a
         * start stamp, and the same camp fought twice at the same minute on two characters would
         * dedup into one. So the new fold is the whole truth and the old one is gone, including
         * when the new one is EMPTY: a scan that found no log must leave no fights, or the
         * screen keeps showing the last folder's numbers under the new folder's name. */
        self.fights = s.fights;
        /* AND THE LIVE END, FOLDED FROM THE SAME BOOTSTRAP BYTES. Without this the overlay is
         * blank from launch until the reader's next pull, which on a quiet evening is a window
         * that looks broken. */
        self.live_owner = s.active.as_ref().and_then(|a| a.file.character.clone());
        /* THE BOOTSTRAP'S OWN BYTES ARE THE BIGGEST SINGLE SOURCE OF CAST LINES, and reading them
         * here is what puts classes on the table from the first frame rather than from the
         * reader's next pull. */
        if let Some(a) = s.active.as_ref() {
            for line in &a.recent {
                read_casts(line, &mut self.classes);
            }
        }
        let (rows, open, gap) = s
            .active
            .as_ref()
            /* `a.before` AND `a.recent` OFF THE SAME `a`, which is the scan being adopted. Not
             * `self.active`, which is still the file being replaced here. */
            .map(|a| fold_recent(&a.recent, &a.before, self.live_owner.as_deref()))
            .unwrap_or_default();
        self.live_gap = gap;
        self.live = rows;
        self.refresh_foes();
        let mut rows = std::mem::take(&mut self.live);
        self.stamp_classes(&mut rows);
        self.live = rows;

        self.live_open = open;
        /* THE WATCH LIST IS ABOUT ONE FILE'S BUFFER AND THIS IS A NEW ONE. Cleared BEFORE the watch
         * below and not after it, which is not a style choice: reversed, this would throw away the
         * stamp it had just recorded and the fight in progress at launch would never be stored. */
        self.live_whole.clear();
        /* AND THE FIGHT IN PROGRESS AT LAUNCH IS WATCHED FROM HERE. `keep_fights` holds the last
         * row of the bootstrap back because it cannot tell an open fight from a finished one; this
         * path CAN (`live_open` is that answer), so the pull the reader is in when he opens the app
         * is stored when it ends rather than waiting for the next bootstrap.
         *
         * AFTER `self.live` AND `self.live_open`, BOTH OF WHICH IT READS. This tree's classic
         * defect is a step placed before the thing it depends on is assigned, and `keep_fights` two
         * screens down carries the scar of exactly that.
         *
         * AND THE FRESH-LOG ANSWER COMES OFF `s` AND NOT OFF `self`, for that same reason and it
         * is not hypothetical: `self.active` is assigned further down this function, so reading it
         * here would ask about the file being REPLACED. At launch that is `None` and the answer
         * would be a quiet `false` on the one pass where the first fight of a fresh log can be
         * watched at all. `s` is the scan being adopted, which is the file this fold came out
         * of. */
        self.watch_open_fight(s.active.as_ref().is_some_and(|a| a.tail_start == 0));
        self.fights_unreadable = s.fights_unreadable;
        self.active_problem = s.active_problem;
        match s.roster {
            Some(Ok(r)) => {
                self.roster = Some(r);
                self.roster_problem = None;
            }
            Some(Err(e)) => {
                self.roster = None;
                self.roster_problem = Some(e);
            }
            None => {
                self.roster = None;
                self.roster_problem = Some(match &self.data_root {
                    Some(root) => format!("no data root at {}", root.display()),
                    None => format!(
                        "no snapshot root: put the snapshot (with kills-data.json) at one of {}, or name its path on Settings",
                        crate::data::candidates().iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("; ")
                    ),
                });
            }
        }
        /* A new bootstrap replaces the event lists: they describe the tailed file, and that file
         * may have changed. The tracker buckets are merged, gated by the high-water mark, so a
         * re-read of the same file adds nothing twice. */
        self.kills.clear();
        self.loot.clear();
        let mut kills = Vec::new();
        for ev in s.events {
            match ev {
                Event::Kill(k) => kills.push(k),
                Event::Loot(l) => self.loot.push(l),
                Event::Xp(_) => {}
            }
        }
        if let Some(a) = &s.active {
            let name = a.file.name();
            let name_zones = self
                .roster
                .as_ref()
                .map(|r| r.name_zones().clone())
                .unwrap_or_default();
            ingest_kills(
                &mut self.tracker,
                &name_zones,
                &name,
                &kills,
                a.stream.last_ts,
                a.stream.lines,
                true,
            );
        }
        self.kills = kills;
        if self.loot.len() > LOOT_CAP {
            let cut = self.loot.len() - LOOT_CAP;
            self.loot.drain(..cut);
        }
        self.active = s.active;

        /* AND THE BOOTSTRAP'S FIGHTS ARE KEPT.
         *
         * AFTER `self.active`, AND IT WAS BEFORE. `keep_fights` reads the character and server
         * off the active log to know whose fights these are, and on the first bootstrap the old
         * `self.active` is None, so it returned early and stored nothing at all. The test caught it
         * at zero against three. Order of operations, which is the first thing to suspect when a
         * thing that should have run did not.
         *
         * SEE `keep_fights` for why the last row is held back.
         */
        self.keep_fights();
        /* AND STORED NIGHTS OLDER THAN THIS TAIL GET THE GROUP THE WHOLE LOG PROVES, once per log.
         * AFTER `keep_fights`, which is what put this character's history in place. See
         * `start_refill`. */
        self.start_refill();
        match s.inventory {
            Some(Ok(d)) => {
                self.inventory = Some(d);
                self.inv_problem = None;
            }
            Some(Err(e)) => {
                self.inv_problem = Some(e);
            }
            None => {
                self.inv_problem = Some(no_dump_words(
                    &self.dir,
                    s.inv_readable,
                    "*-Inventory.txt",
                    "/outputfile inventory",
                ));
            }
        }
        self.inv_sig = s.inv_sig;
        match s.achievements {
            Some(Ok(d)) => {
                self.achievements = Some(d);
                self.ach_problem = None;
            }
            Some(Err(e)) => {
                self.ach_problem = Some(e);
            }
            None => {
                self.ach_problem = Some(no_dump_words(
                    &self.dir,
                    s.inv_readable,
                    "*-Achievements.txt",
                    "/outputfile achievements",
                ));
            }
        }
        self.ach_sig = s.ach_sig;
        self.scanned_at = Some(Utc::now());
        self.last_poll = Some(Instant::now());
        self.last_dump_poll = Some(Instant::now());
    }

    fn drain_scan(&mut self) {
        let Some(rx) = &self.rx else { return };
        match rx.try_recv() {
            Ok(s) => {
                self.rx = None;
                self.scan_started = None;
                self.adopt(s);
                if self.rescan_again {
                    self.rescan_again = false;
                    self.rescan();
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
                self.scan_started = None;
                self.dir.problem =
                    Some(String::from("the log reader thread ended without a result"));
            }
        }
    }

    /// READ EVERY LOG THIS CHARACTER HAS, WHOLE, ONCE, AND FILL IN WHAT THE STORE COULD NOT KNOW.
    ///
    /// # WHY A SECOND READ, WHEN THE BOOTSTRAP ALREADY READ THE LOG
    ///
    /// The bootstrap reads the last [`TAIL_CAP`] bytes, which is what keeps a launch fast, and the
    /// store holds nights far older than that. On the owner's machine the first stored fight is
    /// 51 MB into a 165 MB log and the tail starts at 123 MB, so no launch folds those fights
    /// again, and [`crate::store::Store::append`] never rewrites a fight it holds. Every fight
    /// stored before the group reader existed would say `group not known` for good.
    ///
    /// # WHY ONCE
    ///
    /// A whole log is hundreds of megabytes. [`crate::store::GROUP_REFILL`] marks each file after
    /// its refill is written, and a marked file is not read whole again. A file that could not be
    /// read, or a month that moved under the rewrite, is not marked, so the next launch retries.
    ///
    /// # WHAT IT CANNOT FILL, SAID OUT LOUD
    ///
    /// A fight whose log file is gone, and a fight an older build folded under a different quiet
    /// window (same start, different span). Both are left exactly as they were: see
    /// [`crate::store::Store::refill`].
    ///
    /// THE READ AND THE FOLD ARE ON THEIR OWN THREAD; THE STORE IS WRITTEN ON THIS ONE, in
    /// [`Ingest::drain_refill`], which is the thread that appends. Only the pop-out's own ingest
    /// can race it, and the store refuses that rewrite rather than lose the append.
    fn start_refill(&mut self) {
        if self.refill_rx.is_some() {
            return;
        }
        let (Some(store), Some(who)) = (self.store.as_ref(), self.owner()) else {
            return;
        };
        let todo: Vec<LogFile> = self
            .logs
            .iter()
            .filter(|f| {
                f.character.as_deref() == Some(who.character.as_str())
                    && f.server.as_deref() == Some(who.server.as_str())
            })
            .filter(|f| !store.refilled(&who, crate::store::GROUP_REFILL, &f.name()))
            .cloned()
            .collect();
        if todo.is_empty() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let for_who = who.clone();
        let spawned = std::thread::Builder::new()
            .name("grimoire-refill".into())
            .spawn(move || {
                for f in todo {
                    let rows = read_tail_capped(&f.path, f.size, u64::MAX)
                        .map(|tt| fold_text(&tt.text, quiet_window(), f.character.as_deref()).0)
                        .map_err(|e| e.to_string());
                    let batch = RefillBatch {
                        who: for_who.clone(),
                        log: f.name(),
                        rows,
                    };
                    if tx.send(batch).is_err() {
                        return;
                    }
                }
            });
        match spawned {
            Ok(_) => {
                self.refill_rx = Some(rx);
                self.refill_for = Some(who);
            }
            Err(e) => log::warn!("stored fights were not refilled: no thread: {e}"),
        }
    }

    /// TAKE WHAT THE REFILL THREAD HAS FINISHED. See [`Ingest::start_refill`].
    fn drain_refill(&mut self) {
        loop {
            let Some(rx) = &self.refill_rx else { return };
            match rx.try_recv() {
                Ok(batch) => self.apply_refill(batch),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.refill_rx = None;
                    /* A CHARACTER CHANGE WHILE IT RAN is the one reason to go again at once: the
                     * new character's logs were turned away by the guard at the top of
                     * `start_refill`. Anything that failed goes again next launch, never once a
                     * poll, because a whole log read every second is not a retry. */
                    if self.refill_for.take() != self.owner() {
                        self.start_refill();
                    }
                    return;
                }
            }
        }
    }

    /// WRITE ONE LOG'S REFILL, and reread the history when it changed this character's fights.
    fn apply_refill(&mut self, batch: RefillBatch) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let rows = match batch.rows {
            Ok(r) => r,
            Err(e) => {
                log::warn!(
                    "{}: not refilled, the log could not be read: {e}",
                    batch.log
                );
                return;
            }
        };
        let did = match store.refill(&batch.who, &rows) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("{}: not refilled: {e}", batch.log);
                return;
            }
        };
        log::info!(
            "{}: {} stored fights given their group, {} given pets, {} left as they were",
            batch.log,
            did.grouped,
            did.petted,
            did.differs
        );
        if let Err(e) = store.mark_refilled(&batch.who, crate::store::GROUP_REFILL, &batch.log) {
            log::warn!(
                "{}: refilled but not marked, so it is read again: {e}",
                batch.log
            );
        }
        /* ANOTHER CHARACTER'S REFILL IS WRITTEN AND NOT SHOWN: the history on screen is this
         * character's, and a refill that finished after a switch belongs to the one before. */
        if did.grouped + did.petted > 0 && self.owner().as_ref() == Some(&batch.who) {
            let (all, _torn) = store.all(&batch.who);
            self.set_history(all);
        }
    }

    fn handle_live(&mut self, ev: Event) {
        match ev {
            Event::Xp(x) => {
                self.session.bump_active(x.ts);
                self.session.xp_sum += x.pct;
            }
            Event::Loot(l) => {
                self.session.bump_active(l.ts);
                self.session.loots += 1;
                self.loot.push(l);
                if self.loot.len() > LOOT_CAP {
                    self.loot.remove(0);
                }
            }
            Event::Kill(k) => {
                self.session.bump_active(k.ts);
                self.session.kills += 1;
                if let Some(a) = &mut self.active {
                    a.kills += 1;
                    let name = a.file.name();
                    let last_ts = a.stream.last_ts;
                    let name_zones = self
                        .roster
                        .as_ref()
                        .map(|r| r.name_zones().clone())
                        .unwrap_or_default();
                    /* hwm false: within a run every line is fed exactly once (the byte cursor
                     * dedups), and the gate would eat same-second stragglers. */
                    ingest_kills(
                        &mut self.tracker,
                        &name_zones,
                        &name,
                        std::slice::from_ref(&k),
                        last_ts,
                        0,
                        false,
                    );
                }
                self.kills.push(k);
            }
        }
    }

    /// THE LAST `n` LINES OF THE LOG, oldest first.
    ///
    /// BOUNDED BY THE CALLER, because the live window holds twenty thousand lines and a screen
    /// that walked all of them once a frame would cost more than the fold that produced them. A
    /// panel about what is currently on somebody needs the last few hundred at most: an effect
    /// announced further back than that has either worn off or is not news.
    ///
    /// BORROWED, NEVER COPIED. These are the strings the live fold already owns.
    pub fn recent_tail(&self, n: usize) -> impl Iterator<Item = &str> {
        let recent = self.active.as_ref().map(|a| &a.recent);
        recent
            .into_iter()
            .flat_map(move |r| r.iter().skip(r.len().saturating_sub(n)))
            .map(String::as_str)
    }

    /// THE FIGHT HAPPENING NOW, RE-FOLDED FROM THE END OF THE LOG, or the last one to finish.
    ///
    /// NOT `fights().last()`, WHICH IS WHERE THE DPS OVERLAY USED TO READ AND WAS THE BUG. That
    /// list comes from the bootstrap `scan` and is never rewritten while the app runs, so an
    /// overlay reading it showed whichever fight happened to be last when the app started and never
    /// changed again, for the whole session.
    ///
    /// THE LIVE FIGHT IS IN HERE AND IT IS OPEN. `fold_text` closes the final fight with
    /// `Ended::EndOfLog` because the text stops, which is exactly what an in-progress fight looks
    /// like from the end of a log that is still being written. [`Ingest::fight_is_live`] is what
    /// tells the two apart, and the overlay must ask rather than print "the log stopped" at
    /// somebody mid-pull.
    ///
    /// `None` UNTIL THERE IS SOMETHING TO SAY, which is a real state on a fresh log and on a
    /// character who has not hit anything yet.
    pub fn current_fight(&self) -> Option<&FightRow> {
        self.live.last()
    }

    /// Is [`Ingest::current_fight`] still going, or is it over?
    ///
    /// THE ENGINE'S OWN RULE, ASKED THE ONLY WAY IT CAN BE FROM OUT HERE. A fight ends after
    /// `quiet_window` seconds of nothing, and the fold cannot know whether the silence after the
    /// last line is a lull or the end of the file, so it always calls the final fight
    /// `Ended::EndOfLog`. What this app knows and the fold does not is that the file is STILL BEING
    /// WRITTEN, so a final fight whose last line is recent is a fight in progress.
    ///
    /// MEASURED AGAINST THE LOG'S OWN CLOCK AND NEVER THE WALL CLOCK. The stamps carry no zone
    /// offset (see `FightRow::end`), so comparing one to `Utc::now()` would be off by the reader's
    /// offset from UTC and would call every fight live, or none, depending which side of the world
    /// he is on. `live_newest` is the newest stamp the fold itself read, in the same units.
    pub fn fight_is_live(&self) -> bool {
        self.pulse().fighting()
    }

    /// WHAT THE MARK BESIDE THE METER IS ENTITLED TO SAY: swinging, held, or closed.
    ///
    /// # TWO CLOCKS, AND BOTH ARE NEEDED
    ///
    /// `live_gap` is how long the LOG says it has been since the last blow, measured from the
    /// newest line the fold read. That is the whole answer while the file is growing and no answer
    /// at all once it stops: stand over a corpse and both stamps sit still, so the gap never
    /// widens. `last_line_at` is when those bytes actually arrived on this machine, so its elapsed
    /// time is the rest of the gap. Added, they are how long it has really been.
    ///
    /// # WHY HELD IS NOT JUST A SLOWER GREEN
    ///
    /// A pull that ends is not always over. The owner asked for the encounter to stay up for
    /// [`crate::fights::HOLD_SECONDS`] in case the next mob is already on its way, with the mark
    /// saying which of the two it is rather than the figures disappearing. Nothing about what is
    /// DRAWN changes across these three: the totals stay exactly where they are, and only the
    /// claim beside them moves.
    pub fn pulse(&self) -> crate::fights::Pulse {
        use crate::fights::{Pulse, COMBAT_SECONDS, HOLD_SECONDS};
        if self.live.is_empty() {
            return Pulse::Closed;
        }
        let since = self.seconds_since_the_last_blow();
        if self.live_open && since <= COMBAT_SECONDS {
            return Pulse::Fighting;
        }
        /* THE HOLD RUNS FROM THE END OF THE FIGHT, NOT FROM THE END OF THE WINDOW. A pull closed by
         * its kill ends on the death line, so the hold starts there and a reader gets the six
         * seconds he asked for rather than six seconds after a window that never applied. */
        if since <= COMBAT_SECONDS.max(0) + HOLD_SECONDS {
            return Pulse::Holding;
        }
        Pulse::Closed
    }

    /// The two clocks added: what the log says, plus what has happened since it last said anything.
    fn seconds_since_the_last_blow(&self) -> i64 {
        if self.live_gap == i64::MAX {
            return i64::MAX;
        }
        let waited = self
            .last_line_at
            .map(|t| i64::try_from(t.elapsed().as_secs()).unwrap_or(i64::MAX))
            .unwrap_or(0);
        self.live_gap.saturating_add(waited)
    }

    /// The pump. Adopts a finished scan, then once a second: notices a newer log taking over,
    /// reads the bytes appended to the active one, feeds them to the stream, and every three
    /// seconds re-reads the dump if it changed. Returns HOW MANY lines were fed by this call, so
    /// a caller can repaint at once when the log moved instead of waiting for its clock; the
    /// lines themselves go to the stream and the screens read the events, not the text. (The
    /// first cut returned the `Vec<String>` and both callers dropped it.) Cheap when nothing is
    /// due: zero.
    pub fn tail(&mut self) -> usize {
        self.drain_scan();
        /* BEFORE THE POLL CLOCK, so a finished refill lands on the next frame and not a second
         * later. It is a store write and a reread, not a read of the log. */
        self.drain_refill();
        let now = Instant::now();
        if self
            .last_poll
            .is_some_and(|t| now.duration_since(t) < TAIL_POLL)
        {
            return 0;
        }
        self.last_poll = Some(now);
        let mut lines = Vec::new();
        if self.rx.is_some() {
            return 0;
        }
        let Some(dir) = self.dir.dir.clone() else {
            return 0;
        };

        /* The ACTIVE file is the most recently modified eqlog_*.txt: the client writes one file
         * per character and server, so the busy one is the character being played. */
        if let Ok(found) = list_logs(&dir) {
            let newest = found.first().map(|f| f.path.clone());
            self.logs = found;
            /* THE LISTING IS RE-READ EVERY POLL AND NOTHING SAID WHEN. See `Ingest::listed_at`:
             * the Logs page could date the last full FOLD and could not date the table of
             * candidates sitting above it, which is the newer of the two by up to a whole
             * session. */
            self.listed_at = Some(Utc::now());
            let current = self.active.as_ref().map(|a| a.file.path.clone());
            if newest.is_some() && newest != current {
                self.rescan();
                return 0;
            }
        }

        /* WHETHER THE CLIENT ROLLED THE FILE UNDER US, answered inside the borrow below and acted
         * on after it: `rescan` takes `&mut self` and `self.active` is borrowed for the whole of
         * that block. */
        let mut rolled = false;
        if let Some(a) = &mut self.active {
            match std::fs::metadata(&a.file.path) {
                Err(e) => {
                    a.file.size = 0;
                    self.active_problem = Some(format!("{}: {e}", a.file.path.display()));
                }
                Ok(md) => {
                    /* AND THE FAILURE IS OVER, SO THE PAGE STOPS SAYING IT IS HAPPENING.
                     *
                     * THE DEFECT: the arm above set `active_problem` and NOTHING ever cleared it.
                     * One stat that lost a race with an antivirus scan or the client's own file
                     * handle painted the Logs page and the pop-out Parser red for the life of the
                     * process, while the lines four lines below went on being read and every number
                     * on every other screen went on moving. A warning that cannot go out is a
                     * warning a reader learns to ignore, which costs him the one that matters.
                     *
                     * THIS STAT SUCCEEDING IS THE WHOLE OF THE EVIDENCE, and it is enough because
                     * of who writes this field. `adopt` assigns it wholesale off the bootstrap, and
                     * every branch of `scan` that sets it leaves `active` as `None` (no
                     * `eqlog_*.txt` at all, or a tail that would not read), so inside this block
                     * `active` is `Some` and the only message that can be standing here is the one
                     * the arm above wrote about this very file. Clearing it cannot swallow the
                     * folder-level problems, which live in `dir.problem` and are printed beside it.
                     *
                     * FIRST IN THE ARM, BEFORE THE SIZE AND THE READ. `size` is reset to zero by
                     * the failure arm and `logs.rs` reads the pair together ("size zero AND a
                     * problem" is how it says a file went away), so the two facts have to move in
                     * the same direction on the same poll. */
                    self.active_problem = None;
                    let size = md.len();
                    a.file.size = size;
                    a.file.modified = md.modified().ok();
                    if size < a.tail.offset {
                        /* File shrank: the client rotated or reset it. Start over from byte 0;
                         * the high-water mark keeps already-counted kills from recounting. */
                        a.tail = Tail::at(0);
                        /* AND THE SKIPPED-BYTES CLAIM GOES WITH IT.
                         *
                         * `tail_start` was written once at bootstrap and never again, so after a
                         * rotation the Logs page went on reporting "The read started at byte
                         * 41,943,040: everything before that byte was not read" over a two
                         * kilobyte file the app had just re-read WHOLE from byte 0. A start byte
                         * twenty thousand times the size of the file it claims to be inside.
                         *
                         * ZERO IS NOT A GUESS HERE: the cursor above it is being set to zero on
                         * the same line, so the next read genuinely does take the file from its
                         * first byte. The two are one fact and they now move together. */
                        a.tail_start = 0;
                        /* AND THE WHOLE BOOTSTRAP IS REDONE, WHICH THE CURSOR RESET ALONE NEVER
                         * DID.
                         *
                         * THE DEFECT: everything a bootstrap owns went on describing the file that
                         * is gone. `fights` is the fold of the OLD bytes, `fights_unreadable` is
                         * that fold's denominator and `scanned_at` dates it, and none of the three
                         * is touched here; only the cursor moved. So after the client rolled the
                         * log the Fights table paged through a history of a file that no longer
                         * exists, under a stamp saying when the file that no longer exists was
                         * read, and the only thing on screen that had noticed was the byte count.
                         *
                         * A RESCAN AND NOT A STAMP, and the arm forty lines above this one is the
                         * argument. A DIFFERENT log taking over is answered there with exactly
                         * `self.rescan(); return 0;`, because the history in hand is about another
                         * file. A rotation is that same event: same path, different file. Stamping
                         * when it happened would let one page SAY its numbers predate the roll and
                         * would leave every other page printing them; re-folding makes them true
                         * for all of them. It is not the expensive read it looks like either, since
                         * a rolled log is a few kilobytes.
                         *
                         * THE READ BELOW IS SKIPPED FOR THIS POLL. Feeding the new file's opening
                         * lines to a stream and a live window that describe the old one would put
                         * two files' lines in one fold, which is the merge `adopt` refuses by name
                         * ("rows from two files interleave by nothing at all"). The bootstrap that
                         * is about to land reads those same bytes from byte zero. */
                        rolled = true;
                    }
                    if size > a.tail.offset && !rolled {
                        let path = a.file.path.clone();
                        lines = read_appended(&path, &mut a.tail, size);
                        if !lines.is_empty() {
                            a.read_at = Utc::now();
                        }
                    }
                }
            }
        }
        if rolled {
            self.rescan();
            return 0;
        }
        if !lines.is_empty() {
            /* THE MACHINE'S CLOCK, STAMPED WHERE THE BYTES ARRIVED. Not `a.read_at`, which is a
             * `Utc::now()` for the Logs page to print: this is compared against a later
             * `Instant::now()` and so must come from the monotonic clock, which a clock change or
             * a daylight-saving step cannot move. */
            self.last_line_at = Some(Instant::now());
            let mut events = Vec::new();
            if let Some(a) = &mut self.active {
                for l in &lines {
                    events.extend(a.stream.feed(l));
                }

                /* THE LIVE FOLD, AND THIS IS THE LINE THE OVERLAY LIVES OR DIES BY. Everything
                 * above this used to be the whole of what a poll did: feed the kill and loot
                 * stream and nothing else. The fights list came from the bootstrap and was never
                 * written again, so every screen reading it was frozen at launch.
                 *
                 * BOUNDED WORK. `push_recent` caps the buffer, so this fold costs the same on
                 * hour six as it does on minute one, whatever the log has grown to. */
                a.recent_cut |= push_recent(&mut a.recent, &mut a.before, lines.iter().cloned());
            }
            /* THE NEW LINES ONLY, because the book accumulates. Re-reading the whole window every
             * poll would cost the same as the fold beside it and prove nothing it does not
             * already hold. */
            for l in &lines {
                read_casts(l, &mut self.classes);
            }
            if let Some(a) = &mut self.active {
                /* AFTER `push_recent`, whose ceiling may have moved lines into `a.before`, and
                 * BEFORE `trim_recent`, which moves more: the party read here is the one in front
                 * of exactly the buffer folded here. */
                let (mut rows, open, gap) =
                    fold_recent(&a.recent, &a.before, self.live_owner.as_deref());
                /* THE FIGHT WHOSE OPENING THE CEILING ATE IS MARKED, and this is the first row
                 * on the live path that is ever marked at all: `fold_recent` says in as many
                 * words that it does not mark its oldest fight, because a slice out of the
                 * middle of a file is nearly always clipped and a warning on every fight is a
                 * warning on none. The ceiling is different: it fires only when this buffer
                 * actually threw away lines of a fight it is still reporting. */
                if a.recent_cut {
                    if let Some(r) = rows.first_mut() {
                        r.cut = true;
                    }
                }
                /* TRIM AFTER THE FOLD. It is the fold that knows where the open fight began. */
                trim_recent(&mut a.recent, &mut a.before, &rows);
                self.live_open = open;
                self.live_gap = gap;
                self.live = rows;
            }
            /* AND THE READING GOES ONTO THE ROWS. Split out of the block above because the
             * borrow of `self.active` is still live inside it. */
            /* FOES BEFORE CLASSES, so a mob this fold proved never gets a class chip. */
            self.refresh_foes();
            let mut rows = std::mem::take(&mut self.live);
            self.stamp_classes(&mut rows);
            self.live = rows;
            /* THE FIGHT THE READER IS IN IS NOTED, AND THE ONES THAT HAVE ENDED ARE KEPT.
             *
             * ORDER, AND IT IS LOAD BEARING. Both read `self.live` and `self.live_open`, so both
             * come AFTER the fold that assigns them and after the class stamping, and what reaches
             * the store is then the row a screen would show. Between themselves they do not race:
             * the watch takes the row that is still open and the keep takes the rows that are not,
             * so no poll can hand one row to both.
             *
             * `tail_start` IS THIS FILE'S, and here `self.active` IS the file being tailed: this
             * is the call site where reading it is right. See `watch_open_fight` for the one
             * where it was not. */
            let from_the_top = self.active.as_ref().is_some_and(|a| a.tail_start == 0);
            self.watch_open_fight(from_the_top);
            self.keep_live_fights();
            for ev in events {
                self.handle_live(ev);
            }
        }

        let dump_due = match self.last_dump_poll {
            Some(t) => now.duration_since(t) >= DUMP_POLL,
            None => true,
        };
        if dump_due {
            self.last_dump_poll = Some(now);
            let (pick, readable) = newest_dump(&dir);
            match pick {
                None => {
                    if self.inventory.is_none() {
                        self.inv_problem = Some(no_dump_words(
                            &self.dir,
                            readable,
                            "*-Inventory.txt",
                            "/outputfile inventory",
                        ));
                    }
                }
                Some(sig) => {
                    if self.inv_sig.as_ref() != Some(&sig) {
                        match std::fs::read_to_string(&sig.0) {
                            Ok(text) => {
                                self.inventory = Some(InventoryDump::parse(
                                    &sig.0,
                                    &text,
                                    Some(sys_to_utc(sig.1)),
                                ));
                                self.inv_problem = None;
                                self.inv_sig = Some(sig);
                            }
                            Err(e) => {
                                /* keep the last good dump, but say the new one did not read */
                                self.inv_problem = Some(format!("{}: {e}", sig.0.display()));
                            }
                        }
                    }
                }
            }
            /* The achievements dump, on the same cadence: it changes only when the player types
             * the command, and an mtime check every three seconds costs nothing. */
            let (pick, readable) = newest_achievements(&dir);
            match pick {
                None => {
                    if self.achievements.is_none() {
                        self.ach_problem = Some(no_dump_words(
                            &self.dir,
                            readable,
                            "*-Achievements.txt",
                            "/outputfile achievements",
                        ));
                    }
                }
                Some(sig) => {
                    if self.ach_sig.as_ref() != Some(&sig) {
                        match read_achievements(&sig.0, sig.1) {
                            Ok(d) => {
                                self.achievements = Some(d);
                                self.ach_problem = None;
                                self.ach_sig = Some(sig);
                            }
                            Err(e) => {
                                self.ach_problem = Some(e);
                            }
                        }
                    }
                }
            }
        }
        lines.len()
    }

    /// Every kill the stream has resolved, bootstrap tail then live, in log order.
    pub fn kills(&self) -> &[KillEvent] {
        &self.kills
    }

    /// The last `LOOT_CAP` loot events, oldest first.
    pub fn loot(&self) -> &[LootEvent] {
        &self.loot
    }

    /// The fights the last bootstrap folded out of the tail, oldest first.
    ///
    /// AS OF `scanned_at()`, NOT AS OF NOW, and a screen that draws this owes the reader that
    /// word. `tail()` feeds appended lines to the kill stream and does not re-fold, so a fight
    /// that started after the last scan is not here and a fight that is still going shows its
    /// state at scan time. Kills and loot on the same screen ARE live; the difference is real and
    /// invisible unless said.
    ///
    /// The oldest row carries `cut` when the 40MB cap clipped the file: its damage total is
    /// missing whatever happened before the window opened.
    /// PUT EACH FIGHTER'S CLASS READING ON THE ROW.
    ///
    /// AFTER THE FOLD AND NOT INSIDE IT. `grimoire_parse` has no corpus and no cast book; the fold
    /// answers what happened and this answers who did it. Keeping them apart is what lets the
    /// engine be tested against the log alone.
    ///
    /// ONLY PLAYERS. `Who::player` is the same filter every table in this app ranks by, and a mob
    /// with a class beside it would be this page claiming the pull has a trio.
    fn stamp_classes(&self, rows: &mut [FightRow]) {
        if self.tags.is_empty() {
            return;
        }
        for r in rows.iter_mut() {
            let FightRow { fighters, foes, .. } = r;
            for f in fighters.iter_mut() {
                if !crate::fights::player_in(foes, &f.who) {
                    continue;
                }
                f.class = self.tags.get(f.who.text()).cloned();
            }
        }
    }

    /// RESOLVE THE CAST BOOK AGAINST A CORPUS, and stamp what is already folded.
    ///
    /// THE CORPUS COMES FROM THE CALLER because `Ingest` does not own one: the snapshot is the
    /// App's and reaches screens through `Cx::data`. Passing it in beats a second copy of two
    /// thousand records living here.
    ///
    /// CHEAP TO CALL EVERY FRAME, WHICH IS THE POINT. It returns at once unless the book has grown
    /// since the last resolve, so the scan happens on the frames somebody casts something new and
    /// on no others.
    pub fn resolve_classes(&mut self, spells: &[crate::data::Spell]) {
        if spells.is_empty() || self.classes.len() == self.tagged_at {
            return;
        }
        self.tagged_at = self.classes.len();
        self.tags = self
            .classes
            .casters()
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|who| {
                let tag = self.classes.of(&who, spells).and_then(|s| s.tag())?;
                Some((who, tag))
            })
            .collect();
        /* AND WHAT IS ALREADY FOLDED GETS THE ANSWER, so a class proved on this frame appears on
         * this frame rather than after the reader's next swing. */
        let mut live = std::mem::take(&mut self.live);
        self.stamp_classes(&mut live);
        self.live = live;
        let mut fights = std::mem::take(&mut self.fights);
        self.stamp_classes(&mut fights);
        self.fights = fights;
    }

    /// GIVE THIS INGEST SOMEWHERE TO KEEP FIGHTS. Production calls this once, with
    /// `Store::app_data`; nothing else does.
    ///
    /// # OPT IN, AND THAT IS A SAFETY RULE RATHER THAN A STYLE
    ///
    /// The first cut of this opened `Store::app_data()` inside `Ingest::new`, so every construction
    /// of an `Ingest` anywhere pointed at the OWNER'S OWN `%APPDATA%\eql-grimoire\fights` and
    /// `keep_fights` wrote his real history from a fixture.
    ///
    /// `cfg(test)` WOULD NOT HAVE SAVED IT. Integration tests compile this crate WITHOUT that flag,
    /// and `tests/live_twitch.rs` builds a real `Ingest`. A guard that only covers unit tests is a
    /// guard that fails in exactly the file nobody thinks about.
    ///
    /// THIS TREE HAS DONE THIS BEFORE. `settings`'s own module note records a valet test writing
    /// its fixture over the operator's saved folders, which is why `Settings::save` refuses under
    /// test and `save_to` exists. Opting in is the stronger version of the same lesson: a test
    /// cannot forget to avoid a thing it never asked for.
    pub fn use_store(&mut self, store: Option<crate::store::Store>) {
        self.store = store;
    }

    /// KEEP EVERY CLOSED FIGHT THE BOOTSTRAP FOUND.
    ///
    /// # WHY THE BOOTSTRAP LIST AND NOT THE LIVE ONE
    ///
    /// `Ingest::fights` is the bootstrap's read of the tail: every fight in it is finished, because
    /// the scan ran over bytes that were already on disk. `Ingest::live` is the opposite -- its last
    /// row is the fight in progress, whose totals are still growing -- and storing that would write
    /// a half fight whose start stamp then BLOCKS the whole one, because the stamp is the dedupe
    /// key and the half got there first.
    ///
    /// THE OPEN FIGHT IS DROPPED BY NAME rather than by trusting the list: `Ended::EndOfLog` means
    /// the text ran out, and from the end of a file still being appended to that is exactly what an
    /// open fight looks like. `still_going` is the rule that tells those apart and it needs the
    /// newest stamp, which this does not have, so the conservative reading is used here: the LAST
    /// row of a tail read is never stored, and it is stored on the next bootstrap when a later
    /// fight has closed it.
    ///
    /// CALLED ON ADOPT AND NOWHERE ELSE. A bootstrap is the only moment this list changes. What
    /// happens to the fights fought WHILE the app runs is [`Ingest::keep_live_fights`].
    fn keep_fights(&mut self) {
        let (Some(store), Some(who)) = (self.store.as_ref(), self.owner()) else {
            /* DEFECT: THE PREVIOUS CHARACTER'S NIGHTS STAYED ON THE DASHBOARD.
             *
             * With no active log, or a log whose file name carries no character and server,
             * this returned and left `history` and the hit point book as they were: another
             * person's, under the new folder. `adopt` refuses exactly that for `self.fights` by
             * name, and this is the same rule for the store's two readings. Safe on the
             * store-is-None branch too: nothing but this function and `keep_live_fights` ever
             * writes either, and both need a store. */
            self.set_history(Vec::new());
            self.hp = Default::default();
            self.hp_recount();
            return;
        };
        let closed = match self.fights.len() {
            0 => &[][..],
            n => &self.fights[..n - 1],
        };
        match store.append(&who, closed) {
            Ok(w) => {
                self.stored.added += w.added;
                self.stored.already += w.already;
            }
            Err(e) => log::warn!("fights were not kept: {e}"),
        }

        /* AND THE HIT POINT BOOK IS REBUILT FROM EVERYTHING THIS CHARACTER HAS EVER DONE.
         *
         * HERE BECAUSE THIS IS WHEN THE STORE CHANGES. `hp::read` walks every stored fight, so
         * it belongs on the one event that adds to them rather than on a frame. The whole
         * history is the right scope: twelve princess kills that agree came from a night, and
         * a book rebuilt per session would start empty every launch.
         *
         * A TORN FILE COSTS ITS OWN FIGHTS AND NOTHING MORE. `Store::all` skips lines it cannot
         * parse and the count is on the Logs page; the book is simply built from the rest. */
        let (all, _torn) = store.all(&who);
        self.hp = crate::hp::read(&all);
        self.hp_recount();
        self.set_history(all);
    }

    /// WHOSE FIGHTS THESE ARE, off the active log's file name.
    ///
    /// `None` WHEN THERE IS NOBODY TO FILE UNDER, which is a log this build's name split could not
    /// read and a session with no active log at all. Filing either under a guess would mix two
    /// characters' nights into one history, and the hit point book is keyed on that history.
    fn owner(&self) -> Option<crate::store::Owner> {
        let f = self.active.as_ref().map(|a| &a.file)?;
        Some(crate::store::Owner {
            character: f.character.clone()?,
            server: f.server.clone()?,
        })
    }

    /// NOTE THE FIGHT THAT IS STILL GOING, so it may be stored once it is not.
    ///
    /// See [`Ingest::live_whole`] for the rule and why it is the only honest one available here.
    /// The row has to be OPEN (a fight the fold has not finished cannot have been clipped at its
    /// far end by anything but the ceiling below), and its first combat line has to have been
    /// INSIDE the window rather than off the front of it.
    ///
    /// # THE SECOND TEST USED TO BE `self.live.len() >= 2` AND THAT MISSED THE FIRST FIGHT OF THE
    /// # NIGHT, EVERY NIGHT
    ///
    /// A row before this one in the same fold proves the window opened before this fight did. It
    /// is a sound proof and it is not the only one, and taking it as the only one had a cost that
    /// shows up in exactly the case a reader notices: a fresh log (the reader typed `/log on`, or
    /// the client rolled the file) has ONE fight in its first fold, so the first pull of the
    /// evening was never watched, never whole, and never stored until the next bootstrap read it
    /// back off the disk. The hit point book behind the Live header's `of ~N` therefore learned
    /// nothing from the fight the reader was most likely to be looking at.
    ///
    /// # THE OTHER PROOF IS THAT NOTHING WAS SKIPPED
    ///
    /// `fold_recent` clips because the live window is a slice out of the MIDDLE of a file. When
    /// `tail_start` is zero the window is not a slice out of the middle: the fold began at byte 0,
    /// so there is nothing in front of the first row to have been cut off, and its start stamp is
    /// the fight's own. That is a fact about the FILE and not a guess about the fold.
    ///
    /// BOTH TESTS ARE KEPT AND EITHER WILL DO, because they cover different sessions: `tail_start`
    /// is zero for a fresh or rolled log and non-zero for the 40MB tail of a long one, where the
    /// preceding row is the only proof available.
    /// `from_the_top` IS AN ARGUMENT AND NOT A READ OF `self.active`, AND THAT IS THE WHOLE
    /// REASON THIS SIGNATURE IS NOT `(&mut self)`.
    ///
    /// The first draft read `self.active.tail_start` in here. It was correct at the `tail` call
    /// site and silently WRONG at the `adopt` one: `adopt` calls this fifty eight lines before it
    /// assigns `self.active`, so the field still held the PREVIOUS file, which at launch is
    /// `None`. The fresh-log case was therefore never true on the one pass that could have used
    /// it, and the fix looked applied while doing nothing at all.
    ///
    /// THAT IS THIS TREE'S CLASSIC DEFECT, and the comment at the `adopt` call site names it six
    /// lines above the call, about a different field. Reading state a caller has not finished
    /// assembling is a mistake positional ordering cannot prevent and an argument cannot make:
    /// each caller now has to say which file it means, and the one adopting a new file answers
    /// from the scan it is adopting.
    fn watch_open_fight(&mut self, from_the_top: bool) {
        if !self.live_open || self.live.is_empty() {
            return;
        }
        if self.live.len() < 2 && !from_the_top {
            return;
        }
        let Some(r) = self.live.last() else { return };
        /* THE CEILING ATE THE START OF THIS ONE. `push_recent` throws lines away when a single
         * fight outgrows `LIVE_CAP` and that row is marked; its damage is a floor from then on. */
        if r.cut {
            return;
        }
        self.live_whole.insert(r.start.clone());
    }

    /// KEEP THE FIGHTS THAT FINISHED WHILE THE APP WAS RUNNING, AND LET THE BOOK LEARN FROM THEM.
    ///
    /// # THE DEFECT THIS EXISTS FOR
    ///
    /// `keep_fights` runs on a bootstrap, and a bootstrap happens at launch and when a DIFFERENT
    /// log file takes over. A reader playing one character all evening therefore had exactly one:
    /// every fight he fought after launch reached the screens (the live fold is re-run every poll)
    /// and reached NOTHING that lasts. The hit point book behind the Live header's `of ~N` was
    /// built once out of the store and never grew, so ten kills of the same named mob in one
    /// evening improved neither the figure nor the "of 11 kills" count printed beside it, and the
    /// store itself did not get those fights until the next launch read them back off the log.
    ///
    /// # WHAT MAY BE STORED, AND WHY THE TEST IS NOT "IS IT CLOSED"
    ///
    /// Closed is necessary and it is not sufficient. The live window is a slice out of the middle
    /// of a file and its oldest fight is nearly always clipped ([`fold_recent`] says so in its own
    /// doc and does not mark the row), so "every row but the open one" would write a clipped fight
    /// under a start stamp that is not the fight's start. The store's dedupe key IS that stamp, so
    /// that row would then sit there forever and reject the whole fight when a later bootstrap
    /// offered it, and `hp` would read the floor as a mob's health, which is this app inventing a
    /// number. [`Ingest::live_whole`] holds the stamps that were proved whole by being watched.
    ///
    /// # ONE ROW PER CALL TO THE STORE, ON PURPOSE
    ///
    /// The book may only be told about a fight the store had NOT already got, or a kill the
    /// bootstrap already folded is counted twice and the count beside the figure lies. `append`
    /// answers that per call, so each row goes in its own call and only a row that came back
    /// `added` is folded. It is one file read per fight kept, a few dozen times an evening.
    ///
    /// WHAT THAT COSTS, SAID OUT LOUD: two ingests reading one log (the pop-out builds its own and
    /// points it at the same store) race for the write, and the one that loses sees `already` and
    /// does NOT fold that kill into its own book until its next bootstrap. That is the conservative
    /// half of the trade and it is the right half: a book one kill behind is honest, and a book that
    /// folded whatever it stored would count the bootstrap's kills twice and print a denominator no
    /// night ever produced.
    fn keep_live_fights(&mut self) {
        if self.live_whole.is_empty() {
            return;
        }
        /* NOWHERE TO PUT THEM IS THE FIRST QUESTION AND NOT THE LAST, because the answer is no on
         * every poll of an ingest nobody handed a store to (which is the DEFAULT: see `use_store`)
         * and the rows are cloned below. Asked after the clone, this would copy every finished
         * fight in the window, once a second, to throw them all away. */
        let (Some(store), Some(who)) = (self.store.as_ref(), self.owner()) else {
            return;
        };
        /* THE OPEN ROW IS NOT A CANDIDATE. Its totals are still growing and `still_going` is what
         * says so; everything before it in the fold is finished. */
        let end = self.live.len().saturating_sub(usize::from(self.live_open));
        let take: Vec<FightRow> = self.live[..end]
            .iter()
            .filter(|r| !r.cut && self.live_whole.contains(&r.start))
            .cloned()
            .collect();
        let mut learned = false;
        for r in &take {
            match store.append(&who, std::slice::from_ref(r)) {
                Ok(w) => {
                    self.stored.added += w.added;
                    self.stored.already += w.already;
                    /* THE HISTORY IS THE RECORD OF WHAT THIS INGEST HAS FOLDED, and both the
                     * book and the list take a fight on PRESENCE, guarded by the start stamp.
                     *
                     * DEFECT: both were under `if w.added > 0`. With the main window and the
                     * pop-out on one store, whichever ingest lost the race got `already` back
                     * and dropped the fight from its history AND its hit point book for good,
                     * so the two dashboards printed different fight counts for one night and
                     * the loser's `of ~N` never learned the kill. `already` is positive proof
                     * the store holds the row; the loser has every right to it. The guard is
                     * what stops a row being folded twice, which is the only thing `added`
                     * was protecting. */
                    if !self.history.iter().any(|h| h.start == r.start) {
                        crate::hp::fold_into(&mut self.hp, std::slice::from_ref(r));
                        self.history.push(r.clone());
                        self.history_gen += 1;
                        learned = true;
                    }
                }
                /* A DISK THAT REFUSED THE WRITE IS NOT RETRIED HERE and the stamp is dropped
                 * below with the rest: a retry every second would be a warning per second in the
                 * log for a condition the reader cannot fix mid-pull, and the next bootstrap
                 * offers the same fight again off the file it is still sitting in. */
                Err(e) => log::warn!("a finished fight was not kept: {e}"),
            }
        }
        for r in &take {
            self.live_whole.remove(&r.start);
        }
        if learned {
            self.hp_recount();
        }
    }

    /// COUNT THE READINGS THAT CAN ANSWER, once, where the book changes.
    ///
    /// NOT PER FRAME, and that is the only reason this is a stored number rather than a fold over
    /// `hp` inside [`Ingest::hp_known`]. `Reading::expect` runs `modal`, whose own doc calls itself
    /// O(n^2) "on a list that is a few dozen long at most" -- true per mob per night, and the book
    /// spans a character's whole history across every mob in it. A dashboard tile asking that
    /// question sixty times a second would be the most expensive thing on the frame.
    ///
    /// EVERY WRITE TO `self.hp` MUST BE FOLLOWED BY THIS. There are two of them, `keep_fights` and
    /// `keep_live_fights`, and a third that forgets would leave the tile printing a count of a book
    /// that has moved on.
    fn hp_recount(&mut self) {
        self.hp_measured = self.hp.values().filter(|r| r.expect().is_some()).count();
    }

    /// WHAT A NAMED MOB HAS BEEN SEEN TO ABSORB, over every fight this character has stored.
    ///
    /// FOLDED CASE, as `hp::read` keys it and as the engine's own `same` compares names: the
    /// game capitalises inconsistently and `A thunder spirit princess` is the same mob.
    pub fn hp_of(&self, mob: &str) -> Option<&crate::hp::Reading> {
        self.hp.get(&mob.to_ascii_lowercase())
    }

    /// HOW MANY MOBS THIS APP CAN ACTUALLY ANSWER FOR.
    ///
    /// # THIS WAS THE BOOK'S LENGTH AND THAT WAS A NUMBER NOTHING HAD MEASURED
    ///
    /// It returned `self.hp.len()`, and the Dashboards tile prints it big with the words "mobs
    /// measured" under a tooltip that promises "kinds of mob this character has killed often enough
    /// for their kills to agree on what they absorb". `hp::read` puts a row in that map for every
    /// name that ever died, INCLUDING names whose every kill it threw away: a fight where two mobs
    /// of one name died shares one damage row between them and is dropped, a kill the grammar could
    /// not attribute comes back with zero taken and is dropped, and a clipped fight is a floor and
    /// is dropped. Each of those still leaves a row. So the tile was reporting mobs SEEN, in the
    /// words for mobs measured, on a stream, and the gap grows with the size of the store.
    ///
    /// WHAT IT MEANS NOW IS WHAT THE TOOLTIP ALREADY CLAIMED: the readings that can stand a figure
    /// up, which is `Reading::expect` and nothing looser. A mob killed once is real and is not in
    /// this count, because one kill cannot show a spread and this app does not print a measurement
    /// it cannot defend.
    pub fn hp_known(&self) -> usize {
        self.hp_measured
    }

    /// How many fights this process wrote to the store, and how many were already there.
    pub fn stored(&self) -> crate::store::Wrote {
        self.stored
    }

    /// The fight store, when there is one.
    pub fn store(&self) -> Option<&crate::store::Store> {
        self.store.as_ref()
    }

    /// What every named caster has been proved to be. See [`crate::class`].
    pub fn classes(&self) -> &crate::class::Book {
        &self.classes
    }

    pub fn fights(&self) -> &[FightRow] {
        &self.fights
    }

    /// Every finished fight on disk for this character, oldest first. See the field.
    pub fn history(&self) -> &[FightRow] {
        &self.history
    }

    /// See the field. A screen that caches a reading of [`Ingest::history`] keys on this.
    pub fn history_gen(&self) -> u64 {
        self.history_gen
    }

    /// IS A REFILL STILL READING LOGS? See [`Ingest::start_refill`].
    pub fn refilling(&self) -> bool {
        self.refill_rx.is_some()
    }

    /// REPLACE THE HISTORY, and say so to anybody caching a reading of it.
    fn set_history(&mut self, rows: Vec<FightRow>) {
        self.history = rows;
        self.history_gen += 1;
        /* STORED ROWS PREDATE `FightRow::foes`, so they are proven again from the damage they
         * recorded, and every list learns what they prove. */
        self.refresh_foes();
    }

    /// LEARN WHICH ONE-WORD NAMES ARE MOBS FROM EVERY FIGHT THIS INGEST HOLDS, AND STAMP THEM ALL.
    ///
    /// # WHY ONE BOOK AND NOT EACH FIGHT'S OWN EVIDENCE
    ///
    /// A fight proves a mob only when the mob traded damage with the reader's side in it. The owner's
    /// store has `Xicotl` punching him on Sep 7 at 16:11 and, half an hour later, hitting a player
    /// called `Xeoning` with the reader nowhere near it. That second fight proves nothing on its own
    /// and `Xicotl` went back on the roster as a player. The game does not let a player take an
    /// NPC's name, so what one fight proved is true of all of them.
    ///
    /// # WHEN
    ///
    /// Whenever a list changes: [`Ingest::set_history`], and after each live fold and each adopt,
    /// before classes are stamped. It is every stored fight's fighters walked once, a few hundred
    /// fights of twenty names, and the history's generation moves only when a row's foes did.
    fn refresh_foes(&mut self) {
        for r in self
            .history
            .iter()
            .chain(self.fights.iter())
            .chain(self.live.iter())
        {
            for n in crate::fights::proven_foes(&r.fighters, r.group.as_deref(), &r.pets) {
                self.foe_book.insert(n.to_ascii_lowercase());
            }
        }
        if Self::apply_foes(&self.foe_book, &mut self.history) {
            self.history_gen += 1;
        }
        Self::apply_foes(&self.foe_book, &mut self.fights);
        Self::apply_foes(&self.foe_book, &mut self.live);
    }

    /// EVERY ROW'S `foes` SET TO THE ONE-WORD FIGHTERS THE BOOK NAMES. True when any row changed.
    fn apply_foes(book: &BTreeSet<String>, rows: &mut [FightRow]) -> bool {
        let mut changed = false;
        for r in rows.iter_mut() {
            let mut foes: Vec<String> = Vec::new();
            for x in &r.fighters {
                if let crate::fights::Who::Named(n) = &x.who {
                    if x.who.player()
                        && book.contains(&n.to_ascii_lowercase())
                        && !foes.iter().any(|f| f.eq_ignore_ascii_case(n))
                    {
                        foes.push(n.clone());
                    }
                }
            }
            if r.foes != foes {
                r.foes = foes;
                changed = true;
            }
        }
        changed
    }

    /// [`Ingest::set_history`] for a screen's test, which has no store rewrite to drive it.
    #[cfg(test)]
    pub(crate) fn replace_history(&mut self, rows: Vec<FightRow>) {
        self.set_history(rows);
    }

    /// TAKE ANOTHER `Ingest`'S BOOTSTRAP HISTORY AS THIS ONE'S OWN, so two windows of one
    /// application cannot print two totals for one log.
    ///
    /// # WHY TWO WINDOWS DISAGREE AT ALL
    ///
    /// A pop-out builds its own `Ingest` (`windows::ChildCx::new`) and points it at the same
    /// folder, so `fights` on each side is a fold of a GROWING file taken at a different moment.
    /// Neither is wrong about the bytes it read and they are not the same bytes, so the Fights
    /// count in the main window and the Fights count six inches to its right can differ all
    /// evening with nothing on either screen saying why.
    ///
    /// # THE THREE VALUES A BOOTSTRAP OWNS, AND NOTHING THE LIVE FOLD OWNS
    ///
    /// `adopt` writes exactly these three off a scan of its own, and this writes the same three off
    /// somebody else's. `live`, `live_open`, `live_whole`, the kill stream and the cursor are all
    /// about the file THIS ingest is tailing, and copying them would mean two ingests sharing one
    /// byte cursor over two `File` handles. They are deliberately untouched.
    ///
    /// `scanned_at` IS COPIED WITH THE ROWS AND THAT IS WHAT MAKES THE ADOPTION SELF-HEALING. The
    /// caller compares the two stamps to decide whether to adopt; copying the source's stamp is
    /// what makes the next comparison say "already in step". Leaving this ingest's own stamp
    /// behind would re-adopt the same rows on every frame, and whichever side rescans next has a
    /// stamp the other does not, so the newer history wins from either direction.
    pub fn adopt_history(
        &mut self,
        fights: Vec<crate::fights::FightRow>,
        unreadable: u32,
        scanned_at: Option<DateTime<Utc>>,
    ) {
        self.fights = fights;
        self.fights_unreadable = unreadable;
        self.scanned_at = scanned_at;
    }

    /// How many lines the fold found carrying a stamp it could not read, in the same tail these
    /// fights came from. Not a curiosity: it is the denominator. Fight totals rest on the lines
    /// the parser placed in time, and a list that cannot say how many it could not place is
    /// reporting rather than measuring.
    pub fn fights_unreadable(&self) -> u32 {
        self.fights_unreadable
    }

    /// The newest dump on disk, parsed, if one read.
    pub fn inventory(&self) -> Option<&InventoryDump> {
        self.inventory.as_ref()
    }

    pub fn inventory_problem(&self) -> Option<&str> {
        self.inv_problem.as_deref()
    }

    /// The newest achievements dump on disk, read, if one read. The Unlocks screen parses its
    /// text; the SOURCES ledger lists it.
    pub fn achievements(&self) -> Option<&AchievementsDump> {
        self.achievements.as_ref()
    }

    pub fn achievements_problem(&self) -> Option<&str> {
        self.ach_problem.as_deref()
    }

    /// Why there is no Logs folder to read, when there is none: the settings value is unset and
    /// none of the usual places exist. The words name the fix (Settings), and every screen whose
    /// empty state would otherwise say "/log" reads this first, because logging on in a folder
    /// the app is not looking at helps nobody.
    pub fn log_dir_problem(&self) -> Option<&str> {
        self.dir.problem.as_deref()
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn tracker(&self) -> &TrackerState {
        &self.tracker
    }

    pub fn tracker_mut(&mut self) -> &mut TrackerState {
        &mut self.tracker
    }

    pub fn roster(&self) -> Option<&Roster> {
        self.roster.as_deref()
    }

    /// Why there is no roster, when there is none.
    pub fn roster_problem(&self) -> Option<&str> {
        self.roster_problem.as_deref()
    }

    pub fn log_dir(&self) -> &LogDirReport {
        &self.dir
    }

    /// The file being tailed.
    pub fn active_log(&self) -> Option<&LogFile> {
        self.active.as_ref().map(|a| &a.file)
    }

    /// WHEN THIS CANDIDATE LOG WAS LAST WRITTEN, off the ingest's OWN listing.
    ///
    /// # THE TABLE IS SORTED ON A NUMBER IT COULD NOT PRINT
    ///
    /// `list_logs` orders the candidates newest first and that order is the whole reason one of
    /// them is tailed and the others are not, so "why is it reading that one" is answered by a
    /// column the page had no way to draw: [`Source`] does not carry a modified time and
    /// `Ingest::logs` is private.
    ///
    /// OFF THE LISTING AND NOT OFF THE DISK, which is the point of it being here rather than a
    /// `metadata` call on the page. The sort ran on THESE values; a page that stat'd each file
    /// again would print times measured at a different moment from the ones the order rests on, and
    /// the two would disagree exactly when a file is being written, which is always.
    ///
    /// AN ACCESSOR AND NOT A `modified` FIELD ON [`Source`], deliberately. `Source` is built by
    /// literal in this file, in `nav.rs`, in `settings.rs` and in several screens, four of them in
    /// this module's own tests; a new field breaks every one of them at once across lanes that do
    /// not share a file.
    ///
    /// `None` for a path this ingest has not listed, and for a file whose `modified` the platform
    /// would not give.
    pub fn log_modified(&self, path: &Path) -> Option<SystemTime> {
        self.logs
            .iter()
            .find(|f| f.path == path)
            .and_then(|f| f.modified)
    }

    /// Why nothing is being tailed, when nothing is.
    pub fn active_problem(&self) -> Option<&str> {
        self.active_problem.as_deref()
    }

    /// The character the tailed log belongs to, from its file name.
    pub fn active_character(&self) -> Option<&str> {
        self.active
            .as_ref()
            .and_then(|a| a.file.character.as_deref())
    }

    /// The zone the stream currently believes the player is in (an atlas key), or "?".
    pub fn current_zone(&self) -> Option<&str> {
        self.active.as_ref().map(|a| a.stream.zone.as_str())
    }

    /// The byte the bootstrap started at: non-zero means only the last 40MB were read.
    pub fn tail_start(&self) -> Option<u64> {
        self.active.as_ref().map(|a| a.tail_start)
    }

    pub fn scanned_at(&self) -> Option<DateTime<Utc>> {
        self.scanned_at
    }

    /// When the log FOLDER was last listed, which is not [`Ingest::scanned_at`]. See the field for
    /// why a page needs both stamps and could only print one of them.
    pub fn listed_at(&self) -> Option<DateTime<Utc>> {
        self.listed_at
    }

    /// When the active log last yielded lines, or when the bootstrap ran.
    pub fn last_read(&self) -> Option<DateTime<Utc>> {
        self.active.as_ref().map(|a| a.read_at)
    }

    /// The rows of the Settings screen's SOURCES ledger.
    pub fn sources(&self) -> Vec<Source> {
        let mut out = Vec::new();
        let active_path = self.active.as_ref().map(|a| a.file.path.clone());
        for f in &self.logs {
            if Some(&f.path) == active_path.as_ref() {
                let a = self.active.as_ref().expect("active path came from active");
                out.push(Source {
                    kind: SourceKind::Log,
                    path: f.path.clone(),
                    last_read: Some(a.read_at),
                    records: a.kills,
                    problem: self.active_problem.clone(),
                });
            } else {
                let newest = self.logs.first().map(|n| n.name()).unwrap_or_default();
                out.push(Source {
                    kind: SourceKind::Log,
                    path: f.path.clone(),
                    last_read: None,
                    records: 0,
                    problem: Some(format!("not tailed: only the most recently written log is read, and that is {newest}")),
                });
            }
        }
        if self.logs.is_empty() {
            if let Some(d) = &self.dir.dir {
                out.push(Source {
                    kind: SourceKind::Log,
                    path: d.clone(),
                    last_read: None,
                    records: 0,
                    problem: self
                        .active_problem
                        .clone()
                        .or_else(|| self.dir.problem.clone()),
                });
            }
        }
        let game = self.dir.dir.as_ref().map(|d| {
            d.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| d.clone())
        });
        match &self.inventory {
            Some(d) => out.push(Source {
                kind: SourceKind::Inventory,
                path: d.path.clone(),
                last_read: Some(d.read_at),
                records: d.rows().count(),
                problem: self.inv_problem.clone(),
            }),
            None => {
                if let Some(game) = &game {
                    out.push(Source {
                        kind: SourceKind::Inventory,
                        path: game.clone(),
                        last_read: None,
                        records: 0,
                        problem: self.inv_problem.clone(),
                    });
                }
            }
        }
        match &self.achievements {
            Some(d) => out.push(Source {
                kind: SourceKind::Achievements,
                path: d.path.clone(),
                last_read: Some(d.read_at),
                records: d.lines,
                problem: self.ach_problem.clone(),
            }),
            None => {
                if let Some(game) = &game {
                    out.push(Source {
                        kind: SourceKind::Achievements,
                        path: game.clone(),
                        last_read: None,
                        records: 0,
                        problem: self.ach_problem.clone(),
                    });
                }
            }
        }
        out
    }
}

/* ==================================================================== tests == */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fights::probe;

    /// THE SESSION GAP CUT, PINNED FROM BOTH SIDES OF IT.
    ///
    /// Active time is the sum of the gaps shorter than [`SESSION_GAP_MAX`], and both directions
    /// of drift do damage: a cut that creeps up starts reporting a night away from the keyboard
    /// as time played, and one that creeps down throws away real time spent at a slow camp. The
    /// two cases below sit one second apart on either side of the boundary, so this fails if the
    /// constant moves either way rather than only if it is removed.
    #[test]
    fn session_counts_a_gap_under_the_cut_and_refuses_one_over() {
        let base = 1_700_000_000;

        /* THE SECONDS BELOW ARE LITERAL ON PURPOSE. Written as SESSION_GAP_MAX - 1 and
         * SESSION_GAP_MAX they would move with the constant, so they would hold for whatever
         * value it happened to take and this test could never fail. Spelled out, they pin it. */
        assert_eq!(
            SESSION_GAP_MAX, 1_800,
            "the seconds below are this constant, written out so they are able to disagree with it"
        );

        let mut under = Session::default();
        under.bump_active(base);
        under.bump_active(base + 1_799);
        assert_eq!(
            under.active_sec, 1_799,
            "a gap one second under the cut is time at the keyboard"
        );

        let mut over = Session::default();
        over.bump_active(base);
        over.bump_active(base + 1_800);
        assert_eq!(
            over.active_sec, 0,
            "a gap that reaches the cut is an absence and adds nothing"
        );
    }

    /// The real snapshot beside this crate. Tests that need it FAIL when it is absent, unless
    /// GRIMOIRE_NO_DATA=1 says the skip is deliberate; a silent pass on nothing proves nothing.
    fn data_root() -> Option<PathBuf> {
        if std::env::var("GRIMOIRE_NO_DATA").as_deref() == Ok("1") {
            crate::data::testdata::say_skipped("GRIMOIRE_NO_DATA=1, real data files not read");
            return None;
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
        let kd = root.join("kills-data.json");
        assert!(
            kd.is_file(),
            "the real data file {} is missing. Copy the snapshot's kills-data.json there, or set GRIMOIRE_NO_DATA=1 to skip on purpose.",
            kd.display()
        );
        Some(root)
    }

    fn map(pairs: &[(&str, &str)]) -> Arc<HashMap<String, String>> {
        Arc::new(
            pairs
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        )
    }

    fn events(text: &str, name2key: Arc<HashMap<String, String>>) -> Vec<Event> {
        let mut ks = KillReader::new(name2key);
        let mut out = Vec::new();
        for l in text.lines() {
            out.extend(ks.feed(l));
        }
        out.extend(ks.flush());
        out
    }

    fn kills_of(evs: &[Event]) -> Vec<&KillEvent> {
        evs.iter()
            .filter_map(|e| {
                if let Event::Kill(k) = e {
                    Some(k)
                } else {
                    None
                }
            })
            .collect()
    }

    /* ---- the grammar, one real line per pattern (this machine's log, 2026-07 to 2026-09) ---- */

    #[test]
    fn ts_reads_the_wall_clock_as_utc() {
        let m = RX
            .xp
            .captures("[Mon Aug 31 14:50:58 2026] You gain experience! (4.078%)")
            .unwrap();
        let ts = to_sec(&m).unwrap();
        assert_eq!(hhmmss(ts), "14:50:58");
        assert_eq!(
            DateTime::<Utc>::from_timestamp(ts, 0)
                .unwrap()
                .format("%Y-%m-%d")
                .to_string(),
            "2026-08-31"
        );
        /* single-digit day, space padded */
        let m = RX
            .died
            .captures("[Tue Sep  1 20:04:54 2026] A zol ghoul knight died.")
            .unwrap();
        assert_eq!(
            DateTime::<Utc>::from_timestamp(to_sec(&m).unwrap(), 0)
                .unwrap()
                .format("%Y-%m-%d")
                .to_string(),
            "2026-09-01"
        );
    }

    #[test]
    fn rx_slain_you() {
        let m = RX
            .slain_you
            .captures("[Mon Aug 31 14:50:58 2026] You have slain a skeletal monk!")
            .unwrap();
        assert_eq!(&m["n"], "a skeletal monk");
        assert!(
            RX.slain_you
                .captures("[Mon Aug 31 14:50:58 2026] You have slain a skeletal monk")
                .is_none(),
            "needs the bang"
        );
    }

    #[test]
    fn rx_slain_by() {
        let m = RX
            .slain_by
            .captures("[Mon Aug 31 14:54:04 2026] A dusty werebat has been slain by Chaotiq!")
            .unwrap();
        assert_eq!(&m["n"], "A dusty werebat");
        assert_eq!(&m["by"], "Chaotiq");
    }

    #[test]
    fn rx_died() {
        let m = RX
            .died
            .captures("[Tue Sep 01 20:04:54 2026] A zol ghoul knight died.")
            .unwrap();
        assert_eq!(&m["n"], "A zol ghoul knight");
        let m = RX
            .died
            .captures("[Sun Jul 12 04:22:56 2026] You died.")
            .unwrap();
        assert_eq!(&m["n"], "You");
    }

    #[test]
    fn rx_xp_plain_party_group() {
        let m = RX
            .xp
            .captures("[Mon Aug 31 14:50:58 2026] You gain experience! (4.078%)")
            .unwrap();
        assert_eq!(&m["pct"], "4.078");
        let m = RX
            .xp
            .captures("[Sat Jul 11 22:22:28 2026] You gain party experience! (1.012%)")
            .unwrap();
        assert_eq!(&m["pct"], "1.012");
        assert!(RX
            .xp
            .is_match("[Sat Jul 11 22:22:28 2026] You gain group experience! (12%)"));
    }

    #[test]
    fn rx_zone_and_filters() {
        let m = RX
            .zone
            .captures("[Mon Aug 31 17:34:53 2026] You have entered Oggok.")
            .unwrap();
        assert_eq!(&m["z"], "Oggok");
        let m = RX
            .zone
            .captures("[Mon Jul 13 11:54:54 2026] You have entered The City of Guk 3 (Fused).")
            .unwrap();
        assert_eq!(&m["z"], "The City of Guk 3 (Fused)");
        let t = RX.zone_tier.captures("The City of Guk 3 (Fused)").unwrap();
        assert_eq!(&t[1], "The City of Guk");
        assert!(RX.zone_tier.captures("The City of Guk").is_none());
        assert!(
            RX.zone_tier.captures("Somewhere 5 (Fused)").is_none(),
            "tiers run 1 to 4"
        );
        assert!(RX
            .zone_skip
            .is_match("an area where levitation effects do not function"));
        assert!(RX.zone_skip.is_match("an Arena"));
        assert!(RX.zone_skip.is_match("the Drunken Monkey"));
        assert!(!RX.zone_skip.is_match("Oggok"));
    }

    #[test]
    fn rx_loot_every_tail() {
        let m = RX.loot.captures("[Mon Jul 13 02:41:34 2026] You looted a Froglok Meat from a froglok ton knight's corpse and sold it for 7 copper.").unwrap();
        assert_eq!(&m["item"], "Froglok Meat");
        assert_eq!(&m["mob"], "a froglok ton knight");
        assert_eq!(
            loot_disp(&m),
            (Disposition::Sold, Some("7 copper".to_string()))
        );

        let m = RX.loot.captures("[Mon Jul 13 02:41:42 2026] You looted a Fine Steel Spear +3 from a froglok ton knight's corpse and sold it for 4 platinum, 5 gold, 7 silver and 1 copper.").unwrap();
        assert_eq!(&m["item"], "Fine Steel Spear +3");
        assert_eq!(
            loot_disp(&m).1.as_deref(),
            Some("4 platinum, 5 gold, 7 silver and 1 copper")
        );

        let m = RX.loot.captures("[Tue Jul 14 00:30:18 2026] You looted an Eye of Urd +1 from an urd ghoul wizard's corpse and sold it for free.").unwrap();
        assert_eq!(&m["item"], "Eye of Urd +1");
        assert_eq!(loot_disp(&m), (Disposition::SoldFree, None));

        let m = RX.loot.captures("[Tue Sep 01 17:49:37 2026] You looted 2 Phosphorous Powder from a zol ghoul knight's corpse and stored it in your tradeskill depot").unwrap();
        assert_eq!(&m["qty"], "2");
        assert_eq!(&m["item"], "Phosphorous Powder");
        assert_eq!(loot_disp(&m), (Disposition::Depot, None));

        let m = RX.loot.captures("[Mon Jul 13 12:33:10 2026] You looted a Silver-Plated Bracer +3 from the froglok shin lord's corpse to create a Silver-Plated Bracer +4").unwrap();
        assert_eq!(&m["mob"], "the froglok shin lord");
        assert_eq!(loot_disp(&m), (Disposition::Created, None));

        /* bare period: kept. */
        let m = RX.loot.captures("[Mon Jul 13 12:33:10 2026] You looted a Mote of Potential from a froglok shin lord's corpse.").unwrap();
        assert_eq!(loot_disp(&m), (Disposition::Kept, None));
        assert!(m.name("qty").is_none());
    }

    #[test]
    fn rx_loot_manual() {
        let m = RX.loot_manual.captures("[Mon Jul 13 02:49:18 2026] --You have looted a Mote of Major Potential from a froglok shin warrior's corpse.--").unwrap();
        assert_eq!(&m["item"], "Mote of Major Potential");
        assert_eq!(&m["mob"], "a froglok shin warrior");
        /* the lazy item group stops at the first " from ", so an apostrophe in the item is fine */
        let m = RX.loot_manual.captures("[Thu Jul 31 10:00:00 2026] --You have looted a Rambunctious Pet's Skull from a rambunctious pet's corpse.--").unwrap();
        assert_eq!(&m["item"], "Rambunctious Pet's Skull");
        assert_eq!(&m["mob"], "a rambunctious pet");
        let m = RX.loot_manual.captures("[Thu Jul 31 10:00:00 2026] --You have looted 2 Bone Chips from a skeleton's corpse.--").unwrap();
        assert_eq!(&m["qty"], "2");
        assert!(RX.loot.captures("[Thu Jul 31 10:00:00 2026] --You have looted 2 Bone Chips from a skeleton's corpse.--").is_none(), "manual loot must not match the auto pattern");
    }

    /* ---- names ---- */

    #[test]
    fn norm_name_and_norm_mob() {
        assert_eq!(norm_name("an orc legionnaire"), "orc legionnaire");
        assert_eq!(norm_name("orc legionnaire"), "orc legionnaire");
        assert_eq!(norm_name("Ambassador D`Vinn"), "ambassador dvinn");
        assert_eq!(norm_name("The  Fabled   Thing"), "fabled thing");
        assert_eq!(
            norm_name("a an thing"),
            "an thing",
            "exactly one article comes off"
        );
        assert_eq!(norm_mob("a shadowknight (Ogre)"), "shadowknight");
        assert_eq!(norm_mob("Guard Mystan (Northern Felwithe)"), "guard mystan");
        assert_eq!(
            norm_name("Coin of the Tash (Azia)"),
            "coin of the tash (azia)",
            "items keep their parenthetical"
        );
    }

    /* ---- the stream ---- */

    #[test]
    fn blow_credit_is_immediate_but_waits_its_turn() {
        let n2k = map(&[]);
        let text = "[Mon Aug 31 14:50:58 2026] You have slain a skeletal monk!\n";
        let evs = events(text, n2k.clone());
        let ks = kills_of(&evs);
        assert_eq!(ks.len(), 1);
        assert_eq!(ks[0].credit, Credit::Blow);
        assert_eq!(ks[0].name, "skeletal monk");
        assert_eq!(ks[0].zone, "?");

        /* An undecided older candidate holds a newer blow back until it is decidable. */
        let mut ks = KillReader::new(n2k);
        assert!(ks
            .feed("[Mon Aug 31 14:54:04 2026] A dusty werebat has been slain by Chaotiq!")
            .is_empty());
        let out = ks.feed("[Mon Aug 31 14:54:05 2026] You have slain a skeletal monk!");
        assert!(
            out.is_empty(),
            "the blow waits behind the undecided slain_by"
        );
        let out = ks.feed("[Mon Aug 31 14:54:07 2026] You gain experience! (1.000%)");
        let got = kills_of(&out);
        assert_eq!(
            got.len(),
            2,
            "both emit once a later timestamp makes the first decidable"
        );
        assert_eq!(got[0].name, "dusty werebat");
        assert_eq!(
            got[0].credit,
            Credit::Wit,
            "xp at +3s is outside the window"
        );
        assert_eq!(got[1].credit, Credit::Blow);
    }

    #[test]
    fn xp_within_two_seconds_after_is_group_credit() {
        let text = "[Mon Aug 31 14:54:04 2026] A dusty werebat has been slain by Chaotiq!\n\
                    [Mon Aug 31 14:54:06 2026] You gain experience! (1.000%)\n\
                    [Mon Aug 31 14:54:20 2026] You have entered Oggok.\n";
        let evs = events(text, map(&[]));
        let ks = kills_of(&evs);
        assert_eq!(ks.len(), 1);
        assert_eq!(ks[0].credit, Credit::Xp);
        let text = "[Mon Aug 31 14:54:04 2026] A dusty werebat has been slain by Chaotiq!\n\
                    [Mon Aug 31 14:54:07 2026] You gain experience! (1.000%)\n";
        let evs = events(text, map(&[]));
        assert_eq!(
            kills_of(&evs)[0].credit,
            Credit::Wit,
            "three seconds after is witnessed"
        );
    }

    #[test]
    fn xp_within_two_seconds_before_is_group_credit() {
        let text = "[Mon Aug 31 14:54:02 2026] You gain experience! (1.000%)\n\
                    [Mon Aug 31 14:54:04 2026] A dusty werebat has been slain by Chaotiq!\n";
        let evs = events(text, map(&[]));
        assert_eq!(kills_of(&evs)[0].credit, Credit::Xp);
        let text = "[Mon Aug 31 14:54:01 2026] You gain experience! (1.000%)\n\
                    [Mon Aug 31 14:54:04 2026] A dusty werebat has been slain by Chaotiq!\n";
        let evs = events(text, map(&[]));
        assert_eq!(
            kills_of(&evs)[0].credit,
            Credit::Wit,
            "three seconds before is witnessed"
        );
    }

    #[test]
    fn died_lines_are_candidates_except_you() {
        let text = "[Tue Sep 01 20:04:54 2026] A zol ghoul knight died.\n\
                    [Sun Jul 12 04:22:56 2026] You died.\n";
        let evs = events(text, map(&[]));
        let ks = kills_of(&evs);
        assert_eq!(ks.len(), 1);
        assert_eq!(ks[0].name, "zol ghoul knight");
    }

    #[test]
    fn undecided_candidate_is_not_emitted_until_a_later_timestamp_or_flush() {
        let mut ks = KillReader::new(map(&[]));
        assert!(ks
            .feed("[Mon Aug 31 14:54:04 2026] A dusty werebat has been slain by Chaotiq!")
            .is_empty());
        assert!(
            ks.feed("[Mon Aug 31 14:54:06 2026] A werebat has been slain by Chaotiq!")
                .is_empty(),
            "+2s is not > +2s"
        );
        let out = ks.flush();
        assert_eq!(kills_of(&out).len(), 2);
    }

    #[test]
    fn zone_tracking_folds_tiers_and_skips_non_zones() {
        let n2k = map(&[("the city of guk", "guktop"), ("oggok", "oggok")]);
        let mut ks = KillReader::new(n2k);
        ks.feed("[Mon Jul 13 11:54:54 2026] You have entered The City of Guk 3 (Fused).");
        assert_eq!(ks.zone, "guktop", "the tier suffix folds to the base zone");
        ks.feed("[Fri Aug 14 00:36:27 2026] You have entered an area where levitation effects do not function.");
        assert_eq!(ks.zone, "guktop", "ZONE_SKIP lines do not move the zone");
        ks.feed("[Mon Aug 31 17:34:53 2026] You have entered Oggok.");
        assert_eq!(ks.zone, "oggok");
        ks.feed("[Mon Aug 31 17:46:11 2026] You have entered Nowhere Known.");
        assert_eq!(
            ks.zone, "?",
            "an unknown zone name is unplaced, not the last zone"
        );
        let out = ks.feed("[Mon Aug 31 17:46:12 2026] You have slain a thing!");
        assert_eq!(kills_of(&out)[0].zone, "?");
    }

    #[test]
    fn loot_lines_do_not_move_the_high_water_mark() {
        let mut ks = KillReader::new(map(&[]));
        ks.feed("[Mon Aug 31 14:50:58 2026] You have slain a skeletal monk!");
        let hwm = ks.last_ts;
        ks.feed("[Mon Aug 31 14:51:30 2026] You looted a Froglok Meat from a froglok ton knight's corpse and sold it for 7 copper.");
        assert_eq!(ks.last_ts, hwm);
        assert!(
            ks.seen_ts > hwm,
            "but it does make earlier candidates decidable"
        );
    }

    #[test]
    fn loot_events_carry_qty_disp_and_coin() {
        let text = "[Tue Jul 14 00:31:20 2026] You looted 2 Phosphorous Powder from a yun ghoul wizard's corpse and sold it for 2 platinum, 5 gold, 7 silver and 2 copper.\n\
                    [Mon Jul 13 02:49:18 2026] --You have looted a Mote of Major Potential from a froglok shin warrior's corpse.--\n";
        let evs = events(text, map(&[]));
        let loots: Vec<&LootEvent> = evs
            .iter()
            .filter_map(|e| {
                if let Event::Loot(l) = e {
                    Some(l)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(loots.len(), 2);
        assert_eq!(loots[0].qty, 2);
        assert_eq!(loots[0].disp, Disposition::Sold);
        assert_eq!(
            loots[0].sold_for.as_deref(),
            Some("2 platinum, 5 gold, 7 silver and 2 copper")
        );
        assert_eq!(loots[1].qty, 1);
        assert_eq!(loots[1].disp, Disposition::Kept);
        assert_eq!(loots[1].item, "Mote of Major Potential");
    }

    #[test]
    fn short_lines_are_ignored() {
        let mut ks = KillReader::new(map(&[]));
        assert!(ks.feed("").is_empty());
        assert!(ks.feed("You have slain a thing!").is_empty());
        assert_eq!(ks.lines, 0);
    }

    #[test]
    fn parse_log_matches_stream_order_and_hwm() {
        let text = "[Mon Aug 31 14:50:58 2026] You have slain a skeletal monk!\r\n\
                    [Mon Aug 31 14:54:04 2026] A dusty werebat has been slain by Chaotiq!\r\n\
                    [Mon Aug 31 14:54:05 2026] You gain experience! (1.000%)\r\n";
        let p = parse_log(text, map(&[]));
        let kills: Vec<&KillEvent> = p
            .events
            .iter()
            .filter_map(|e| {
                if let Event::Kill(k) = e {
                    Some(k)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(kills.len(), 2);
        assert_eq!(kills[1].credit, Credit::Xp);
        assert_eq!(hhmmss(p.stream.last_ts), "14:54:05");
        assert_eq!(p.stream.lines, 3);
        assert_eq!(
            p.events.len(),
            3,
            "the xp line is an event too; the bootstrap feeds every one"
        );
    }

    #[test]
    fn achievements_dump_is_named_counted_and_listed() {
        let text = "General: Keys\r\nY\tKey to the Sky\t\r\n\t\tObtain the key\r\n\r\n";
        let d = AchievementsDump::parse(
            Path::new("C:/eq/Reviir_neriak-Achievements.txt"),
            text.to_owned(),
            None,
        );
        assert_eq!(d.character.as_deref(), Some("Reviir"));
        assert_eq!(d.server.as_deref(), Some("neriak"));
        assert_eq!(d.lines, 3, "blank lines are not rows");
        assert!(
            d.text.contains("Key to the Sky"),
            "the text is kept for the Unlocks grammar"
        );
        let odd = AchievementsDump::parse(Path::new("C:/eq/whatever.txt"), String::new(), None);
        assert_eq!(odd.character, None);
        assert_eq!(odd.lines, 0);
    }

    #[test]
    fn no_dump_words_send_a_person_to_settings_when_no_folder_is_set() {
        let unset = LogDirReport { dir: None, tried: vec![], problem: Some("No log folder is set and none of the usual places exist: C:/x. Set the Logs folder in Settings.".into()) };
        let w = no_dump_words(&unset, false, "*-Inventory.txt", "/outputfile inventory");
        assert!(w.contains("Settings"), "{w}");
        assert!(
            !w.contains("/outputfile"),
            "a slash command with nowhere to look is the wrong advice: {w}"
        );
        let set = LogDirReport {
            dir: Some(PathBuf::from("C:/eq/Logs")),
            tried: vec![],
            problem: None,
        };
        let w = no_dump_words(&set, true, "*-Achievements.txt", "/outputfile achievements");
        assert!(w.contains("/outputfile achievements"), "{w}");
        assert_eq!(
            no_dump_words(&set, false, "*-Inventory.txt", "/outputfile inventory"),
            "Game folder is unreadable."
        );
    }

    /* ---- the merge and the credit rules ---- */

    fn name_zones(pairs: &[(&str, &[&str])]) -> HashMap<String, BTreeSet<String>> {
        pairs
            .iter()
            .map(|(n, zs)| (n.to_string(), zs.iter().map(|z| z.to_string()).collect()))
            .collect()
    }

    fn kill(ts: i64, zone: &str, name: &str, credit: Credit) -> KillEvent {
        KillEvent {
            ts,
            zone: zone.to_string(),
            name: name.to_string(),
            credit,
        }
    }

    #[test]
    fn ingest_splits_known_witnessed_pets_and_unmatched() {
        let nz = name_zones(&[
            ("skeletal monk", &["unrest"]),
            ("ghoul", &["unrest", "befallen"]),
        ]);
        let mut st = TrackerState::default();
        let kills = [
            kill(10, "unrest", "skeletal monk", Credit::Blow),
            kill(11, "unrest", "skeletal monk", Credit::Xp),
            kill(12, "unrest", "ghoul", Credit::Wit),
            kill(13, "unrest", "Chaotiq", Credit::Wit),
            kill(14, "unrest", "reclusive ghoul magus pet", Credit::Blow),
            kill(15, "unrest", "some player", Credit::Blow),
        ];
        let added = ingest_kills(&mut st, &nz, "eqlog_Reviir_neriak.txt", &kills, 15, 6, true);
        assert_eq!(added, 4);
        assert_eq!(st.kills["unrest"]["skeletal monk"], Tally { c: 2, t: 10 });
        assert_eq!(st.wit["unrest"]["ghoul"].c, 1);
        assert!(
            !st.wit["unrest"].contains_key("Chaotiq"),
            "witnessed unknown names are dropped"
        );
        assert!(
            !st.unmatched
                .get("unrest")
                .is_some_and(|z| z.contains_key("reclusive ghoul magus pet")),
            "unknown pets are dropped"
        );
        assert_eq!(st.unmatched["unrest"]["some player"], 1);
        assert_eq!(st.chars, vec!["Reviir".to_string()]);
        assert_eq!(
            st.files["eqlog_Reviir_neriak.txt"],
            FileMark { ts: 15, n: 6 }
        );
    }

    #[test]
    fn ingest_places_unplaced_kills_when_the_name_is_in_exactly_one_zone() {
        let nz = name_zones(&[
            ("skeletal monk", &["unrest"]),
            ("ghoul", &["unrest", "befallen"]),
        ]);
        let mut st = TrackerState::default();
        let kills = [
            kill(10, "?", "skeletal monk", Credit::Blow),
            kill(11, "?", "ghoul", Credit::Blow),
        ];
        ingest_kills(&mut st, &nz, "eqlog_X_y.txt", &kills, 11, 2, true);
        assert_eq!(st.kills["unrest"]["skeletal monk"].c, 1);
        assert_eq!(
            st.kills["?"]["ghoul"].c, 1,
            "two candidate zones: waits in the unplaced bucket"
        );
    }

    #[test]
    fn ingest_high_water_mark_gates_a_re_drop_but_not_a_live_feed() {
        let nz = name_zones(&[("skeletal monk", &["unrest"])]);
        let mut st = TrackerState::default();
        let kills = [kill(10, "unrest", "skeletal monk", Credit::Blow)];
        assert_eq!(ingest_kills(&mut st, &nz, "f.txt", &kills, 10, 1, true), 1);
        assert_eq!(
            ingest_kills(&mut st, &nz, "f.txt", &kills, 10, 1, true),
            0,
            "same second again: gated"
        );
        assert_eq!(
            ingest_kills(&mut st, &nz, "f.txt", &kills, 10, 1, false),
            1,
            "live feed: not gated"
        );
        assert_eq!(st.kills["unrest"]["skeletal monk"].c, 2);
    }

    fn row(n: &str, named: bool, lvl: Option<&str>) -> MobRow {
        MobRow {
            n: n.to_string(),
            named,
            lv: None,
            lvl: lvl.map(|s| s.to_string()),
            t: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn credited_rules() {
        let nz = name_zones(&[
            ("skeletal monk", &["unrest", "befallen"]),
            ("baxok curhunter", &["akanon", "steamfont"]),
        ]);
        let mut st = TrackerState::default();
        let kills = [
            kill(10, "unrest", "skeletal monk", Credit::Blow),
            kill(11, "akanon", "baxok curhunter", Credit::Blow),
            kill(12, "unrest", "skeletal monk", Credit::Wit),
        ];
        ingest_kills(&mut st, &nz, "f.txt", &kills, 12, 3, false);
        let glob = global_killed(&st);
        let generic = row("a skeletal monk", false, None);
        let named = row("Baxok Curhunter", true, None);
        assert!(
            credited(&st, &glob, "unrest", &generic),
            "a kill in a zone credits that zone's row"
        );
        assert!(
            !credited(&st, &glob, "befallen", &generic),
            "a generic kill does not cross zones"
        );
        assert!(
            credited(&st, &glob, "steamfont", &named),
            "a NAMED kill credits every zone listing the name"
        );
        st.settings.generic_everywhere = true;
        assert!(
            credited(&st, &glob, "befallen", &generic),
            "unless generic_everywhere"
        );

        /* witnessed only counts while the toggle is on, and the toggle is retroactive */
        let mut st = TrackerState::default();
        ingest_kills(
            &mut st,
            &nz,
            "f.txt",
            &[kill(12, "unrest", "skeletal monk", Credit::Wit)],
            12,
            1,
            false,
        );
        let glob = global_killed(&st);
        assert!(!credited(&st, &glob, "unrest", &generic));
        st.settings.witnessed = true;
        let glob = global_killed(&st);
        assert!(credited(&st, &glob, "unrest", &generic));
    }

    #[test]
    fn zone_ignored_precedence() {
        let city = ZoneRoster {
            name: "Oggok".into(),
            city: true,
            mobs: vec![],
            extra: Default::default(),
        };
        let mut st = TrackerState::default();
        assert!(
            zone_ignored(&st, "oggok", Some(&city)),
            "cities are ignored by default"
        );
        st.settings.unignored_zones.push("oggok".into());
        assert!(
            !zone_ignored(&st, "oggok", Some(&city)),
            "an unignore whitelists a city back in"
        );
        st.settings.ignored_zones.push("oggok".into());
        assert!(
            zone_ignored(&st, "oggok", Some(&city)),
            "an explicit ignore beats everything"
        );
        st.settings.ignore_cities = false;
        st.settings.ignored_zones.clear();
        st.settings.unignored_zones.clear();
        assert!(!zone_ignored(&st, "oggok", Some(&city)));
    }

    #[test]
    fn lvl_num_reads_ranges_tildes_and_prose() {
        assert_eq!(lvl_num("59-61"), Some(60.0));
        assert_eq!(lvl_num("24 - 26"), Some(25.0));
        assert_eq!(lvl_num("34 ~ 38"), Some(36.0));
        assert_eq!(lvl_num("~53"), Some(53.0));
        assert_eq!(lvl_num("58"), Some(58.0));
        assert_eq!(lvl_num("?"), None);
        assert_eq!(lvl_num(""), None);
        assert_eq!(lvl_num("varies"), None);
    }

    #[test]
    fn sort_rows_named_first_then_level_desc_unknown_last_then_name() {
        let rows = [
            row("a gorgalask", false, Some("56")),
            row("A blade storm", true, Some("59-61")),
            row("a presence", false, Some("?")),
            row("a fatestealer drake", false, Some("58")),
            row("A crystalline cloud", true, Some("~53")),
            row("a bat", false, Some("58")),
        ];
        let refs: Vec<&MobRow> = rows.iter().collect();
        let sorted: Vec<&str> = sort_rows(&refs).iter().map(|r| r.n.as_str()).collect();
        assert_eq!(
            sorted,
            vec![
                "A blade storm",
                "A crystalline cloud",
                "a bat",
                "a fatestealer drake",
                "a gorgalask",
                "a presence"
            ]
        );
    }

    /* ---- the tail reader ---- */

    fn tmp(name: &str, bytes: &[u8]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grimoire-ingest-{}-{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("eqlog_Test_server.txt");
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn read_range_never_pads_a_short_read() {
        let p = tmp("short", b"abc\ndef\n");
        /* ask for more than is there, as a stale stat would */
        let got = read_range(&p, 0, 8 + 100).unwrap();
        assert_eq!(
            got, b"abc\ndef\n",
            "exactly the bytes present, no zero fill"
        );
        assert!(!got.contains(&0u8));
        let got = read_range(&p, 4, 8 + 100).unwrap();
        assert_eq!(got, b"def\n");
        assert!(
            read_range(&p, 100, 200).unwrap().is_empty(),
            "past EOF reads nothing"
        );
    }
    /// DEFECT: THE FIGHTS NEVER CHANGE WHILE THE APP RUNS.
    ///
    /// Reported by the owner in one sentence: `Ctrl+Alt+D` should be showing each fight and moving
    /// to the new one when a new one starts. It did not, and it could not: `Ingest::fights` is
    /// written once, by the bootstrap `scan`, and `tail` fed only the kill and loot stream. Every
    /// screen built on that list, the DPS overlay included, showed whatever the log said at launch
    /// for the whole session.
    ///
    /// THIS DRIVES THE REAL PATH AND NOT `fold_recent` ON ITS OWN. It plants a log, boots an
    /// `Ingest` against the folder exactly as `App::new` does, APPENDS to the file the way the game
    /// does, and pumps `tail()`. A `fold_recent` unit test would pass just as well with nothing in
    /// the app calling it, which is this tree's signature defect.
    ///
    /// WHAT MUTATION MAKES THIS RED: removing the `fold_recent` call from `Ingest::tail`.
    #[test]
    fn the_current_fight_follows_the_log_as_the_game_writes_it() {
        use std::io::Write as _;

        let dir = probe::planted("live-fold", FIGHT_LOG);
        let mut ing = probe::booted(&dir);

        let first = ing
            .current_fight()
            .expect("the bootstrap seeds the live window off the same bytes it folds")
            .clone();
        assert_eq!(
            first.headline.as_deref(),
            Some("a barbed bone skeleton"),
            "the newest fight in the planted log is the one the overlay opens on"
        );

        /* THE GAME WRITES A NEW PULL. A fresh mob, far enough after the last line that the engine's
         * quiet window has closed the previous fight, so this is unambiguously a NEW fight and not
         * more of the old one. */
        let path = dir.join(format!("eqlog_{}_freeport.txt", probe::OWNER));
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("the planted log opens for append");
            writeln!(
                f,
                "[Wed Jul 15 23:40:00 2026] You slash a thunder spirit princess for 500 points of damage."
            )
            .expect("append");
        }

        /* `tail` rate limits itself to one read a second, so the poll clock has to move. */
        std::thread::sleep(TAIL_POLL + Duration::from_millis(50));
        ing.tail();

        let now = ing.current_fight().expect("there is still a fight");
        assert_eq!(
            now.headline.as_deref(),
            Some("a thunder spirit princess"),
            "the overlay is still showing {:?}, so nothing re-folds the end of the log and the \
             fights list is frozen at whatever the bootstrap saw",
            now.headline
        );
        assert_eq!(now.damage, 500);
        assert!(
            ing.fight_is_live(),
            "a fight whose last line is the last line of a file that is still being written is a \
             fight in progress, not one that ended because the log stopped"
        );

        /* AND IT GROWS WITH THE PULL rather than only switching when a new one starts. */
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("opens");
            writeln!(
                f,
                "[Wed Jul 15 23:40:04 2026] You slash a thunder spirit princess for 250 points of damage."
            )
            .expect("append");
        }
        std::thread::sleep(TAIL_POLL + Duration::from_millis(50));
        ing.tail();
        let grown = ing.current_fight().expect("still a fight");
        assert_eq!(grown.damage, 750, "the fight in progress must accumulate");
        assert_eq!(grown.secs, 4);

        /* THE BOOTSTRAP LIST IS UNTOUCHED BY ANY OF THIS, which is the whole reason there are two.
         * The Fights table is a history page and must not start rewriting itself under the
         * reader; the overlay is the live one. */
        assert_eq!(
            ing.fights().last().map(|r| r.headline.clone()),
            Some(first.headline.clone()),
            "the history list is the bootstrap's and is not the live fold"
        );
    }

    /// Two fights, the second cleanly separated from the first by more than the quiet window.
    const FIGHT_LOG: &str = "\
[Wed Jul 15 23:16:50 2026] You slash a dry bone skeleton for 20 points of damage.
[Wed Jul 15 23:16:53 2026] You have slain a dry bone skeleton!
[Wed Jul 15 23:20:00 2026] You slash a barbed bone skeleton for 6 points of damage.
[Wed Jul 15 23:20:02 2026] You have slain a barbed bone skeleton!
";

    #[test]
    fn read_tail_drops_the_first_partial_line_only_when_cut() {
        let p = tmp("tail", b"aaa\nbbb\nccc\n");
        let t = read_tail_capped(&p, 12, 6).unwrap();
        assert_eq!(
            t.text, "ccc\n",
            "the cap landed inside bbb, which is dropped"
        );
        assert_eq!(t.start, 6);
        assert_eq!(t.end, 12, "the cursor starts where the text ends");
        let t = read_tail_capped(&p, 12, 100).unwrap();
        assert_eq!(
            t.text, "aaa\nbbb\nccc\n",
            "an uncut file keeps its first line"
        );
        assert_eq!(t.start, 0);
        /* the cap lands inside the LAST line: "c\n" is a partial line, so it is dropped and nothing
         * is left */
        let t = read_tail_capped(&p, 12, 2).unwrap();
        assert_eq!(t.text, "");
        assert_eq!(t.end, 12);
        /* NO NEWLINE AT ALL INSIDE THE CAP: nothing in it is a whole line, so nothing is read and
         * the cursor stays where the read began, to take the line whole once it is. This was once
         * kept whole; see `read_tail_capped` for the fragments that made. */
        let p = tmp("tail2", b"abcdefgh");
        let t = read_tail_capped(&p, 8, 4).unwrap();
        assert_eq!(t.text, "", "a line with no newline yet was read as a line");
        assert_eq!(
            t.end, 4,
            "the cursor went past bytes that are not a whole line"
        );
        /* the file shrank between the stat and the read: end reports the truth */
        let p = tmp("tail3", b"abc\nde");
        let t = read_tail_capped(&p, 6 + 50, 100).unwrap();
        assert_eq!(
            t.end, 4,
            "the end is what was read, up to its last whole line"
        );
        assert_eq!(t.text, "abc\n");
    }

    /// DEFECT: A LINE CUT IN HALF AT THE END OF THE BOOTSTRAP READ, READ AS TWO FRAGMENTS.
    ///
    /// The game writes `[stamp] You have been removed from the group.` and its newline, and the
    /// bootstrap can land between the two halves. `read_tail_capped` kept the first half in the text
    /// and started the cursor after it, so the next poll read `oved from the group.` as a line with
    /// no stamp. Neither fragment is a removal, no party ever saw one, and the live fold went on
    /// naming Zarmin in a group that had ended, for the rest of the session.
    ///
    /// DRIVEN THROUGH `Ingest::tail`, because the defect is where the bootstrap's bytes meet the
    /// live stream's and neither side alone shows it.
    ///
    /// WHAT MUTATION MAKES THIS RED: `read_tail_capped` keeping the bytes after the last newline
    /// (the old `let end = start + bytes.len()` over the untruncated read).
    #[test]
    fn a_group_line_torn_at_the_end_of_the_bootstrap_is_read_whole_when_it_lands() {
        let mut lines = vec![
            "[Wed Jul 15 22:00:00 2026] You invite Zarmin to join your group.".to_owned(),
            "[Wed Jul 15 22:00:01 2026] You have joined the group.".to_owned(),
            "[Wed Jul 15 22:00:01 2026] You are now the leader of your group.".to_owned(),
            "[Wed Jul 15 22:00:01 2026] Zarmin has joined the group.".to_owned(),
            "[Wed Jul 15 22:00:10 2026] You slash a dry bone skeleton for 20 points of damage."
                .to_owned(),
        ];
        lines.push("[Wed Jul 15 22:05:00 2026] You have been rem".to_owned());
        let dir = probe::planted("torn-group-line", &lines.join("\n"));
        let mut ing = probe::booted(&dir);
        let path = dir.join(format!("eqlog_{}_freeport.txt", probe::OWNER));
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open the planted log to append");
        std::io::Write::write_all(
            &mut f,
            b"oved from the group.\n[Wed Jul 15 22:10:00 2026] You slash a fire beetle for 4 points of damage.\n",
        )
        .expect("append the rest of the line and a pull");
        drop(f);
        ing.last_poll = None;
        assert!(ing.tail() > 0, "the appended bytes were not read");
        let now = ing.current_fight().expect("a fight after the removal");
        assert_eq!(now.headline.as_deref(), Some("a fire beetle"));
        assert_eq!(
            now.group,
            Some(Vec::new()),
            "the removal was torn between the bootstrap read and the first poll, no party saw it, \
             and a fight after it still names the group that ended"
        );
    }

    #[test]
    fn read_appended_moves_the_cursor_by_bytes_read_and_holds_the_partial_line() {
        let p = tmp("append", b"one\r\ntwo\r\nthr");
        let mut t = Tail::at(0);
        let lines = read_appended(&p, &mut t, 13);
        assert_eq!(lines, vec!["one", "two"]);
        assert_eq!(t.offset, 13);
        assert_eq!(t.remainder, b"thr");
        std::fs::write(&p, b"one\r\ntwo\r\nthree\r\n").unwrap();
        let lines = read_appended(&p, &mut t, 17);
        assert_eq!(lines, vec!["three"], "the held partial line completes");
        assert_eq!(t.offset, 17);
        assert!(t.remainder.is_empty());
        /* a stat that over-promises: the cursor moves by what was READ, not by the stat */
        let lines = read_appended(&p, &mut t, 17 + 40);
        assert!(lines.is_empty());
        assert_eq!(t.offset, 17);
        /* an unreadable file leaves the cursor untouched */
        let mut t2 = Tail::at(5);
        assert!(read_appended(Path::new("Z:/no/such/eqlog.txt"), &mut t2, 99).is_empty());
        assert_eq!(t2.offset, 5);
    }

    #[test]
    fn read_appended_does_not_corrupt_a_multibyte_char_split_across_reads() {
        let text = "caf\u{e9}\r\n".as_bytes();
        let p = tmp("utf8", text);
        let mut t = Tail::at(0);
        /* the first read stops between the two bytes of the e-acute */
        let lines = read_appended(&p, &mut t, 4);
        assert!(lines.is_empty());
        let lines = read_appended(&p, &mut t, text.len() as u64);
        assert_eq!(lines, vec!["caf\u{e9}"]);
    }

    /* ---- the dump ---- */

    /* Rows copied from a real dump on this machine (Reviir_neriak-Inventory.txt, 2026-09-01):
     * the header, worn rows with sockets, a bag with contents, a three-column storage row, the
     * KeyRing table's repeated header. */
    const DUMP: &str = "Location\tName\tID\tCount\tSlots\r\n\
        Any Slot\tSpiky Splintmail +5\t177858\t1\t10\r\n\
        Any Slot-Slot1\tEmpty\t0\t0\t0\r\n\
        Any Slot-Slot8\tSpiky Splintmail (Exaltation)\t177858\t1\t10\r\n\
        Ear\tEmpty\t0\t0\t0\r\n\
        Head\tBlackened Alloy Coif +5\t3600\t1\t10\r\n\
        Fingers\tClawed Knuckle-Ring +4\t10403\t1\t10\r\n\
        Ring\tPlain Band\t77\t1\t10\r\n\
        Primary\tVerishe Mal Greataxe +5\t177918\t1\t10\r\n\
        General 1\tBackpack*\t32601\t1\t8\r\n\
        General 1-Slot1\tIron Ration\t13005\t45\t10\r\n\
        General 1-Slot2\tGiant Snake Fang +12\t9\t0\t10\r\n\
        Held\tEmpty\t0\t0\t0\r\n\
        Bank1\tEmpty\t0\t0\t0\r\n\
        SharedBank1\tEmpty\t0\t0\t0\r\n\
        Augmentation\tSelo`s Drums of the March (Exaltation)\t11626\r\n\
        Equipment\tRefugee Shroud +7\t177823\r\n\
        KeyRing\tName\tID\t\r\n\
        KeyRing\tKey to the Lower Guk Sewers\t5001\t1\t0\r\n\
        Personal-Depot1\tPristine Spider Silk\t900\t3\t0\r\n\
        Dragon's Hoard 1\tGolden Idol\t901\t1\t0\r\n";

    #[test]
    fn parse_rows_skips_both_headers_and_empties_unless_asked() {
        let rows = parse_rows(DUMP, false);
        assert!(rows.iter().all(|r| !r.empty && r.name != "Name"));
        assert_eq!(rows.len(), 14);
        let all = parse_rows(DUMP, true);
        assert_eq!(all.iter().filter(|r| r.empty).count(), 5);
        assert!(
            all.iter().all(|r| r.name != "Name"),
            "the KeyRing table's repeated header is skipped by shape"
        );
    }

    #[test]
    fn parse_rows_reads_tier_star_exalt_sub_count_and_storage_columns() {
        let rows = parse_rows(DUMP, false);
        let by = |n: &str| {
            rows.iter()
                .find(|r| r.name == n)
                .unwrap_or_else(|| panic!("{n}"))
        };
        let r = by("Spiky Splintmail +5");
        assert_eq!(
            (
                r.base.as_str(),
                r.tier,
                r.exalt,
                r.sub,
                r.item_id,
                r.count,
                r.slots
            ),
            ("Spiky Splintmail", 5, false, false, 177858, 1, Some(10))
        );
        let r = by("Spiky Splintmail (Exaltation)");
        assert_eq!(
            (r.base.as_str(), r.exalt, r.sub, r.root.as_str()),
            ("Spiky Splintmail", true, true, "Any Slot")
        );
        let r = by("Backpack*");
        assert_eq!(r.base, "Backpack", "the dump stars some items");
        let r = by("Iron Ration");
        assert_eq!(
            (r.count, r.sub, r.root.as_str(), r.section),
            (45, true, "General 1", Section::Bags)
        );
        let r = by("Giant Snake Fang +12");
        assert_eq!(r.tier, 10, "tier caps at 10");
        assert_eq!(
            r.count, 1,
            "a zero count reads as one: a printed row is at least one item"
        );
        let r = by("Refugee Shroud +7");
        assert_eq!((r.section, r.slots, r.count), (Section::Storage, None, 1));
        let r = by("Selo`s Drums of the March (Exaltation)");
        assert_eq!((r.section, r.exalt), (Section::Exalts, true));
        assert_eq!(by("Key to the Lower Guk Sewers").section, Section::KeyRing);
        assert_eq!(by("Pristine Spider Silk").section, Section::Depot);
        assert_eq!(by("Golden Idol").section, Section::Hoard);
    }

    #[test]
    fn inventory_dump_worn_slots_fold_finger_and_ring_and_skip_sockets() {
        let d = InventoryDump::parse(Path::new("Reviir_neriak-Inventory.txt"), DUMP, None);
        assert_eq!(d.character.as_deref(), Some("Reviir"));
        assert_eq!(d.server.as_deref(), Some("neriak"));
        let fingers = d.worn_in("Fingers");
        assert_eq!(fingers.len(), 2);
        assert_eq!(fingers[1].name, "Plain Band", "Ring folds to Fingers");
        assert_eq!(
            d.worn_in("Any Slot").len(),
            1,
            "the socketed stone is not the worn item"
        );
        assert_eq!(d.worn_in("Ear").len(), 0);
        let head = d.worn_in("Head")[0];
        assert_eq!(
            d.all[head.row].name, "Blackened Alloy Coif +5",
            "each worn entry points at ITS row"
        );
        let counts = d.section_counts();
        assert!(
            counts.iter().any(|(s, n)| *s == Section::Worn && *n > 0),
            "{counts:?}"
        );
        assert!(
            counts.windows(2).all(|w| w[0].0 < w[1].0),
            "sections come in LOC_SECTIONS order: {counts:?}"
        );
        assert!(counts.iter().all(|(_, n)| *n > 0), "a zero is not listed");
        assert_eq!(
            counts.iter().map(|(_, n)| n).sum::<usize>(),
            d.rows().count()
        );
    }

    #[test]
    fn character_and_server_from_log_names() {
        assert_eq!(
            character_of_log("eqlog_Reviir_neriak.txt").as_deref(),
            Some("Reviir")
        );
        assert_eq!(
            server_of_log("eqlog_Reviir_qeynos 1.txt").as_deref(),
            Some("qeynos 1")
        );
        assert_eq!(character_of_log("dbg.txt"), None);
    }

    /* ---- against the real snapshot ---- */

    #[test]
    fn real_roster_loads_and_resolves_the_fixture_zones() {
        let Some(root) = data_root() else { return };
        let r = Roster::load(&root).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            r.zones.len() >= 70,
            "kills-data.json carried 76 zones when measured; got {}",
            r.zones.len()
        );
        let n2k = r.name2key();
        assert_eq!(
            n2k.get("the city of guk").map(String::as_str),
            Some("guktop")
        );
        assert_eq!(
            n2k.get("the plane of sky").map(String::as_str),
            Some("airplane")
        );
        /* the tiered zone line from this machine's log folds onto the real roster */
        let mut ks = KillReader::new(n2k);
        ks.feed("[Mon Jul 13 11:54:54 2026] You have entered The City of Guk 3 (Fused).");
        assert_eq!(ks.zone, "guktop");
        ks.feed("[Mon Aug 31 17:46:11 2026] You have entered The Feerrott.");
        assert_eq!(ks.zone, "feerrott");
        /* a named row sorts first in its zone, and the roster's own fields came through */
        let sky = &r.zones["airplane"];
        let refs: Vec<&MobRow> = sky.mobs.iter().collect();
        let sorted = sort_rows(&refs);
        assert!(sorted[0].named);
        assert!(r.name_zones().contains_key("blade storm"));
        assert!(sky.mobs.iter().any(|m| m.lv.is_some() && m.t.is_some()));
    }

    #[test]
    fn real_roster_summary_starts_at_zero_and_ignores_cities() {
        let Some(root) = data_root() else { return };
        let r = Roster::load(&root).unwrap_or_else(|e| panic!("{e}"));
        let st = TrackerState::default();
        let sum = summarize(&st, &r);
        assert_eq!(sum.done, 0);
        assert!(
            sum.total > 1000,
            "a roster this size has more than a thousand mobs; got {}",
            sum.total
        );
        let cities = r
            .zones
            .values()
            .filter(|z| z.city && !z.mobs.is_empty())
            .count();
        assert!(cities > 0);
        assert_eq!(
            sum.zones.values().filter(|z| z.ignored).count(),
            cities,
            "every city and only cities are ignored by default"
        );
        assert_eq!(
            sum.zones_total + cities,
            r.zones.values().filter(|z| !z.mobs.is_empty()).count()
        );
    }
    /* ---- the fights fold ---- */

    /// Two fights three minutes apart, in the exact line shapes the reference capture prints
    /// (`web/fixtures/eqlog-tail-200k.txt` lines 30, 32, 36, 47, 194). The numbers asserted
    /// against it below are what `grimoire fights` prints for this exact text, which is the
    /// point of asserting them: they are the ENGINE's answer, and a desktop that quietly
    /// computes its own would disagree here first.
    const TWO_FIGHTS: &str = "\
[Wed Jul 15 23:16:50 2026] You cleave a dry bone skeleton for 54 points of damage.
[Wed Jul 15 23:16:52 2026] You cleave a dry bone skeleton for 46 points of damage.
[Wed Jul 15 23:16:53 2026] You have slain a dry bone skeleton!
[Wed Jul 15 23:20:00 2026] You slash a barbed bone skeleton for 6 points of damage.
[Wed Jul 15 23:20:02 2026] You have slain a barbed bone skeleton!
";

    fn fake_row(headline: &str) -> FightRow {
        FightRow {
            start: String::from("Wed Jul 15 23:16:50 2026"),
            end: String::from("Wed Jul 15 23:16:53 2026"),
            secs: 3,
            damage: 100,
            deaths: 1,
            lines: 3,
            ended: String::from("quiet"),
            headline: Some(headline.to_string()),
            fighters: vec![
                crate::fights::Fighter {
                    who: crate::fights::Who::You,
                    dealt: 100,
                    taken: 0,
                    healed: 0,
                    received: 0,
                    swings: 3,
                    landed: 3,
                    avoided: 0,
                    kills: 1,
                    deaths: 0,
                    ..Default::default()
                },
                crate::fights::Fighter {
                    who: crate::fights::Who::Named(headline.to_string()),
                    dealt: 0,
                    taken: 100,
                    healed: 0,
                    received: 0,
                    swings: 0,
                    landed: 0,
                    avoided: 0,
                    kills: 0,
                    deaths: 1,
                    ..Default::default()
                },
            ],
            cut: false,
            ..Default::default()
        }
    }

    fn fake_scan(fights: Vec<FightRow>, fights_unreadable: u32) -> Scan {
        Scan {
            dir: LogDirReport::default(),
            logs: Vec::new(),
            active: None,
            events: Vec::new(),
            fights,
            fights_unreadable,
            active_problem: None,
            roster: None,
            inventory: None,
            inv_sig: None,
            inv_readable: false,
            achievements: None,
            ach_sig: None,
        }
    }

    /// THE DEFECT: the fold never runs on the worker, or it runs and its rows are dropped
    /// between the worker and the channel, and the Fights section shows an empty list for a log
    /// full of fights. That failure is silent by construction, because an empty fight list looks
    /// exactly like a quiet night, and it is the state this whole wiring exists to leave: the
    /// section shipped a banner saying the engine was "not built in this release" about a crate
    /// in the same workspace.
    ///
    /// It also catches the opposite lie. This file is five lines, so `read_tail` cut nothing and
    /// NO row may claim it was clipped. A `cut` flag wired to anything other than the cap would
    /// brand every small log with a warning the reader cannot act on, and a warning that is
    /// always on is a warning nobody reads when it matters.
    #[test]
    fn the_bootstrap_folds_fights_and_carries_them_home() {
        let p = tmp("fights-scan", TWO_FIGHTS.as_bytes());
        let dir = p.parent().unwrap().to_path_buf();
        let s = scan(ScanConfig {
            log_dir: Some(dir),
            data_root: None,
            roster: None,
        });
        assert!(
            s.active.is_some(),
            "the temp eqlog should be the active file"
        );
        assert_eq!(s.fights.len(), 2, "two fights, cut at the three minute gap");
        /* OLDEST FIRST, pinned here because the cut flag brands index 0: an aggregator that ever
         * handed rows back newest-first would put the "this row is short" warning on the one
         * fight in the list that is certainly whole. */
        assert!(
            s.fights[0].start.contains("23:16:50"),
            "the oldest fight comes home first, got {:?}",
            s.fights[0].start
        );
        assert!(s.fights[1].start.contains("23:20:00"));
        assert_eq!(s.fights[0].headline.as_deref(), Some("a dry bone skeleton"));
        assert_eq!(
            s.fights[1].headline.as_deref(),
            Some("a barbed bone skeleton")
        );
        /* the engine's own numbers for this text, read off `grimoire fights` */
        assert_eq!(s.fights[0].secs, 3, "copied from Fight::seconds()");
        assert_eq!(s.fights[0].damage, 100);
        assert_eq!(s.fights[0].deaths, 1);
        assert_eq!(s.fights[0].lines, 3);
        assert_eq!(s.fights[0].participants(), 2, "you and the skeleton");
        /* EVERY ROW SAYS WHY IT ENDED, AND SAYS THE RIGHT THING. This used to prove the point by
         * asserting two rows DIFFERED, which worked while one pull went quiet and the other ran to
         * the end of the file. Both of this log's pulls now end on a kill, so difference proves
         * nothing and the reason itself is asserted instead. */
        assert!(
            s.fights.iter().all(|f| !f.ended.is_empty()),
            "a row with no reason"
        );
        assert_eq!(
            s.fights[0].ended, "everything died",
            "the first pull ends when the skeleton dies, which is what the log says happened"
        );
        assert_eq!(
            s.fights_unreadable, 0,
            "every stamp in this text is readable"
        );
        assert!(
            !s.fights.iter().any(|f| f.cut),
            "nothing was cut: this file is five lines, far under the {} cap",
            tail_cap_text()
        );
    }

    /// THE DEFECT: the 40MB cap opens the window in the MIDDLE of a fight, and that fight's row
    /// prints a damage total missing everything before the cut with nothing on screen saying so.
    /// This is measured, not imagined. A 40MB tail of this machine's own eqlog_Reviir_neriak.txt
    /// opens on the first stamped line of a greater minotaur fight and reports 36,071 damage over
    /// 285s for an encounter whose beginning is not in the file. A number like that reads as a
    /// measurement, which is exactly what makes an unflagged row worse than a missing one: the
    /// reader has no way to see it is wrong.
    ///
    /// And only the oldest row. Every later fight opened inside the window and is whole; flagging
    /// them too would spend the warning on rows that do not need it.
    #[test]
    fn the_cap_flags_the_oldest_row_and_only_that_one() {
        let end = TWO_FIGHTS.len() as u64;
        let whole = TailText {
            text: TWO_FIGHTS.to_string(),
            end,
            start: 0,
        };
        let (rows, _) = fold_tail(&whole, Some("Reviir"));
        assert_eq!(rows.len(), 2);
        assert!(
            !rows[0].cut && !rows[1].cut,
            "start 0 means the whole file was read: nothing to warn about"
        );
        /* `read_tail_drops_the_first_partial_line_only_when_cut` above already proves a non-zero
         * `start` is precisely what a capped read reports, so the flag is tested against that
         * value rather than against a second 40MB file nobody wants in a unit test. */
        let clipped = TailText {
            text: TWO_FIGHTS.to_string(),
            end,
            start: 1,
        };
        let (rows, _) = fold_tail(&clipped, Some("Reviir"));
        assert_eq!(rows.len(), 2);
        assert!(
            rows[0].cut,
            "the window opened inside this fight, so its total is short and must say so"
        );
        assert!(
            !rows[1].cut,
            "a fight that opened inside the window is whole"
        );
    }

    /// THE DEFECT: change the Logs folder in Settings, or let the client roll over to a new log,
    /// and the previous file's fights stay on the screen under the new file's name. The event
    /// lists are already replaced wholesale for this reason and the fold has to be too, with one
    /// extra edge the event lists do not have: an EMPTY new scan must still clear. A folder with
    /// no eqlog in it produces no fights, and a list that keeps its old rows in that case is
    /// showing another character's night as this one's.
    ///
    /// The unreadable count rides the same replacement. If it lagged a scan behind, a log the
    /// parser half understood would report itself fully understood, which is the one number on
    /// that screen whose whole job is to admit doubt.
    #[test]
    fn a_new_scan_replaces_the_fight_list_rather_than_adding_to_it() {
        let dir = std::env::temp_dir().join(format!(
            "grimoire-ingest-{}-fights-adopt",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = crate::settings::Settings {
            log_dir: Some(dir),
            ..crate::settings::Settings::default()
        };
        let mut ig = Ingest::new(&settings);

        ig.adopt(fake_scan(vec![fake_row("a dry bone skeleton")], 7));
        assert_eq!(ig.fights().len(), 1);
        assert_eq!(
            ig.fights()[0].headline.as_deref(),
            Some("a dry bone skeleton")
        );
        assert_eq!(ig.fights_unreadable(), 7);

        ig.adopt(fake_scan(vec![fake_row("a barbed bone skeleton")], 0));
        assert_eq!(
            ig.fights().len(),
            1,
            "the second scan replaced the list; two rows would mean it appended"
        );
        assert_eq!(
            ig.fights()[0].headline.as_deref(),
            Some("a barbed bone skeleton")
        );
        assert_eq!(ig.fights_unreadable(), 0, "the count came with the rows");

        ig.adopt(fake_scan(Vec::new(), 0));
        assert!(
            ig.fights().is_empty(),
            "a scan that found no log must leave no fights"
        );
    }

    /// DEFECT: A FIGHT LONGER THAN THE LIVE WINDOW IS REPORTED AS A SHORTER FIGHT.
    ///
    /// # WHAT THE OWNER SAW
    ///
    /// `IN COMBAT  A HAUNTED CHEST  02:40  2 players named  The Plane of Hate 4 (Refined)`, and
    /// the clock not moving. He is in a raid zone on a forty megabyte log.
    ///
    /// # THE MECHANISM, WHICH IS A SLIDING WINDOW WITH NO FLOOR UNDER THE OPEN FIGHT
    ///
    /// `Ingest::current_fight` is the last row of `fold_recent`, and `fold_recent` folds
    /// `ActiveLog::recent`, which `push_recent` trims to the newest [`LIVE_LINES`] lines. Nothing
    /// in that trim knows where the open fight began. Once a fight has produced more than
    /// `LIVE_LINES` lines, every poll drops as many of its OLDEST lines as it appends new ones, so
    /// the fold's view of the fight starts later and later:
    ///
    ///   * THE CLOCK STOPS. `secs` is last stamp minus first stamp, and both ends now advance
    ///     together, so the duration settles at whatever span the window covers and stays there.
    ///     That is the `02:40` that would not move.
    ///   * AND THE DAMAGE IS A FRACTION. Every total on the meter covers only the window, so a
    ///     twenty minute fight is drawn with the last few minutes' damage under a heading naming
    ///     the fight. Nothing on screen says the figure is short.
    ///
    /// A RAID ZONE IS WHERE IT BITES because line rate is what fills the window, not time: forty
    /// people swinging produce `LIVE_LINES` in a couple of minutes, which is why this never showed
    /// on the reference capture (four fights, none close to the window).
    ///
    /// WHAT MUTATION MAKES THIS RED: reverting the trim in `push_recent` to a bare line count.
    #[test]
    fn a_fight_longer_than_the_live_window_is_not_reported_as_a_short_one() {
        /* A SINGLE FIGHT, LONGER THAN THE WINDOW. One second per line would make the log's own
         * clock the thing under test, so this puts many lines on each second the way a raid
         * does: the log stamps to the second and a busy pull shares one. */
        /* TWENTY A SECOND, AND THE RUN STAYS INSIDE ONE HOUR OF THE LOG CLOCK. The first draft
         * used eight a second, which needed 5,000 seconds and wrote stamps like 23:83:20; the
         * grammar refused those and the test reported the truncation as 2,639 seconds, which is
         * the minute the stamps stopped parsing rather than anything the buffer did. A fixture
         * broken in the same DIRECTION as the defect is the worst kind. */
        const PER_SEC: usize = 20;
        const SECS: usize = (LIVE_LINES / PER_SEC) * 2;
        let mut buf: VecDeque<String> = VecDeque::new();
        let mut before = Party::new();
        for s in 0..SECS {
            let mm = s / 60;
            let ss = s % 60;
            for _ in 0..PER_SEC {
                push_recent(
                    &mut buf,
                    &mut before,
                    [format!(
                        "[Wed Jul 15 23:{mm:02}:{ss:02} 2026] You slash a haunted chest for 10 \
                         points of damage."
                    )],
                );
            }
        }

        let (rows, open, _) = fold_recent(&buf, &before, Some("Reviir"));
        let last = rows.last().expect("one fight");
        assert!(
            open,
            "the log stopped mid fight, so the fight is still going"
        );

        /* THE WHOLE FIGHT IS ONE PULL AND ITS SPAN IS THE WHOLE RUN. `SECS - 1` because the span
         * is last stamp minus first, and both ends are stamps rather than boundaries. */
        assert_eq!(
            last.secs as usize,
            SECS - 1,
            "the fight is reported as {} seconds of a {} second pull: the window slid off the \
             front of it and took the opening with it",
            last.secs,
            SECS - 1
        );

        /* AND EVERY POINT OF IT IS COUNTED. This is the half a reader would never notice: a
         * duration that stalls is visible, a damage total that is quietly a fraction is not. */
        assert_eq!(
            last.damage as usize,
            SECS * PER_SEC * 10,
            "the meter is drawing {} of {} points dealt in this fight",
            last.damage,
            SECS * PER_SEC * 10
        );
    }

    /// The log's own stamp text, `s` seconds after 22:00:00, so a fixture can run for two hours of
    /// the log's clock without writing a stamp the grammar refuses (see the long-fight test above
    /// for what a stamp like `23:83:20` does to a test).
    fn clock(s: usize) -> String {
        let t = 22 * 3600 + s;
        format!(
            "Wed Jul 15 {:02}:{:02}:{:02} 2026",
            t / 3600,
            (t / 60) % 60,
            t % 60
        )
    }

    fn line_at(s: usize, body: &str) -> String {
        format!("[{}] {body}", clock(s))
    }

    /// The reader forms his own group with Zarmin, in the one-second shape the owner's logs print.
    fn zarmin_joins(s: usize) -> Vec<String> {
        vec![
            line_at(s, "You invite Zarmin to join your group."),
            line_at(s + 1, "You have joined the group."),
            line_at(s + 1, "You are now the leader of your group."),
            line_at(s + 1, "Zarmin has joined the group."),
        ]
    }

    /// DEFECT: THE LIVE FOLD FORGETS THE GROUP ON EVERY POLL.
    ///
    /// # WHY IT WOULD
    ///
    /// `fold_recent` folds the last `LIVE_LINES` lines. A group is formed once and then says
    /// nothing for hours, so on any real evening the lines that formed it are long gone from the
    /// window, and a party folded from the window alone is `Unknown` for every live fight. The DPS
    /// overlay and the Live page would say "group not known" all night, while the Fights table,
    /// folded by the bootstrap over the very same bytes, named the group.
    ///
    /// # WHAT IS PLANTED
    ///
    /// Zarmin joins, then `LIVE_LINES` lines of the reader talking to himself, then a pull with
    /// Zarmin in it. The bootstrap's window therefore genuinely does not contain the formation, and
    /// that is ASSERTED: with the formation inside the window this passes on a fold that carries
    /// nothing at all.
    ///
    /// # BOTH CALL SITES, AND THE LINES THAT ARRIVE AFTER
    ///
    /// `adopt`'s fold at launch, then `tail`'s after the game appends a new pull, then a removal and
    /// one more pull, which must come out solo: the carried party is where the fold STARTS, and the
    /// lines written after launch still move it.
    ///
    /// WHAT MUTATION MAKES THIS RED: `seed_live` leaving `before` as `Party::new()`; `fold_recent`
    /// folding from `Party::new()` instead of a clone of `before`; `seed_live` pushing the WHOLE
    /// text into `before`, which replays the window and reads as the clock stepping back.
    #[test]
    fn a_live_fold_after_the_bootstrap_keeps_the_group_the_bootstrap_saw_form() {
        let mut lines = zarmin_joins(0);
        for i in 0..LIVE_LINES {
            lines.push(line_at(2 + i / 20, "You say, 'still here'"));
        }
        lines.push(line_at(
            1100,
            "You slash a dry bone skeleton for 20 points of damage.",
        ));
        lines.push(line_at(
            1102,
            "Zarmin hits a dry bone skeleton for 10 points of damage.",
        ));
        let mut text = lines.join("\n");
        text.push('\n');
        let dir = probe::planted("live-group-carried", &text);
        let mut ing = probe::booted(&dir);
        let path = dir.join(format!("eqlog_{}_freeport.txt", probe::OWNER));
        let append = |text: &str| {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open the planted log to append");
            std::io::Write::write_all(&mut f, text.as_bytes()).expect("append to the planted log");
        };
        let zarmin = Some(vec!["Zarmin".to_owned()]);

        /* THE PREMISE, TWICE. The window must not hold the formation, and the bootstrap fold over
         * the whole file must name the group, or nothing below is about carrying it. */
        assert!(
            !ing.recent_tail(usize::MAX)
                .any(|l| l.contains("has joined the group.")),
            "the formation is inside the live window, so this cannot tell a carried party from a \
             fresh one"
        );
        assert_eq!(
            ing.fights().last().map(|r| r.group.clone()),
            Some(zarmin.clone()),
            "the bootstrap's fold over the whole file did not name the group"
        );

        assert_eq!(
            ing.current_fight().map(|r| r.group.clone()),
            Some(zarmin.clone()),
            "the live fold at launch forgot the group the bootstrap fold named for the same pull"
        );

        append(&format!(
            "{}\n",
            line_at(
                1200,
                "You slash a dry bone skeleton for 30 points of damage."
            )
        ));
        ing.last_poll = None;
        assert!(ing.tail() > 0, "the appended pull was not read");
        let now = ing.current_fight().expect("a fight");
        assert_eq!(
            now.damage, 30,
            "this is the new pull and not the one before it"
        );
        assert_eq!(
            now.group, zarmin,
            "the first poll after launch folded the window from nothing and forgot the group"
        );

        append(&format!(
            "{}\n{}\n",
            line_at(1300, "You have been removed from the group."),
            line_at(1400, "You slash a fire beetle for 4 points of damage.")
        ));
        ing.last_poll = None;
        assert!(ing.tail() > 0, "the removal and the pull were not read");
        let now = ing.current_fight().expect("a fight");
        assert_eq!(now.headline.as_deref(), Some("a fire beetle"));
        assert_eq!(
            now.group,
            Some(Vec::new()),
            "a removal written after launch did not reach the live fold, so a group that has ended \
             is still the group"
        );
    }

    /// DEFECT: THE TRIM THROWS THE GROUP AWAY WITH THE LINES.
    ///
    /// `trim_recent` is the way out of the front of the window that fires on an ordinary evening:
    /// past `LIVE_LINES` it drops the closed fights, and the lines that formed the group go with
    /// them. Unless each dropped line reaches `before`, the next fold starts from a party that never
    /// saw the formation, and every pull after the trim is not known.
    ///
    /// WHAT MUTATION MAKES THIS RED: `trim_recent` popping a line without `before.push`.
    #[test]
    fn a_group_formed_in_lines_the_trim_dropped_is_still_known_after_it() {
        let mut buf: VecDeque<String> = VecDeque::new();
        let mut before = Party::new();
        let mut lines = zarmin_joins(0);
        lines.push(line_at(
            5,
            "You slash a dry bone skeleton for 20 points of damage.",
        ));
        for i in 0..LIVE_LINES {
            lines.push(line_at(50 + i / 20, "You say, 'still here'"));
        }
        lines.push(line_at(
            1100,
            "You slash a fire beetle for 4 points of damage.",
        ));
        push_recent(&mut buf, &mut before, lines);
        let zarmin = Some(vec!["Zarmin".to_owned()]);

        let (rows, _, _) = fold_recent(&buf, &before, Some("Reviir"));
        assert_eq!(rows.len(), 2, "the pull before and the pull in progress");
        assert_eq!(
            rows[1].group, zarmin,
            "before any trim the fold knows the group"
        );

        trim_recent(&mut buf, &mut before, &rows);
        assert!(
            !buf.iter().any(|l| l.contains("has joined the group.")),
            "the trim kept the formation, so this proves nothing about the lines it dropped"
        );
        let (rows, _, _) = fold_recent(&buf, &before, Some("Reviir"));
        assert_eq!(
            rows.last().map(|r| r.group.clone()),
            Some(zarmin),
            "the trim dropped the lines that formed the group without handing them to `before`, so \
             the pull in progress is no longer known to have Zarmin in it"
        );
    }

    /// DEFECT: THE CEILING THROWS THE GROUP AWAY WITH THE LINES.
    ///
    /// The other way out of the front. `LIVE_CAP` fires on one endless raid fight, and what it
    /// drops is that fight's opening, which is where a group formed for the pull is announced.
    ///
    /// THE PARTY IS ASKED DIRECTLY AND THE WINDOW IS NOT FOLDED. Four hundred thousand lines
    /// through the combat fold is seconds of a debug build for an answer that is only ever read off
    /// `before`, which is what `fold_recent` clones; the trim test above folds, and the arithmetic
    /// of what reaches the fold is the same.
    ///
    /// WHAT MUTATION MAKES THIS RED: `push_recent`'s ceiling popping a line without `before.push`.
    #[test]
    fn a_group_formed_in_lines_the_ceiling_dropped_is_still_known_after_it() {
        let mut buf: VecDeque<String> = VecDeque::new();
        let mut before = Party::new();
        let mut lines = zarmin_joins(0);
        lines.extend((0..LIVE_CAP).map(|i| line_at(10 + i / 100, "You say, 'still here'")));
        assert!(
            push_recent(&mut buf, &mut before, lines),
            "the ceiling did not fire, so this is not the case under test"
        );
        assert!(
            !buf.iter().any(|l| l.contains("has joined the group.")),
            "the ceiling kept the formation, so this proves nothing about the lines it dropped"
        );
        assert_eq!(
            before.during(&clock(10), &clock(20)),
            Some(vec!["Zarmin".to_owned()]),
            "the ceiling dropped the lines that formed the group without handing them to `before`"
        );
    }

    /// Append `text` to the planted log in `dir`.
    fn append_to(dir: &Path, text: &str) {
        let path = dir.join(format!("eqlog_{}_freeport.txt", probe::OWNER));
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open the planted log to append");
        std::io::Write::write_all(&mut f, text.as_bytes()).expect("append to the planted log");
    }

    /// DEFECT: THE TRIM IN `tail`, THE PATH THAT FIRES ON AN ORDINARY EVENING, THROWING THE GROUP AWAY.
    ///
    /// `a_group_formed_in_lines_the_trim_dropped_is_still_known_after_it` calls `trim_recent`
    /// itself, so the CALL in `tail` was unguarded: handing it `&mut Party::new()` in place of
    /// `a.before` stayed green across the whole crate. This drives the call. The formation is in the
    /// window at launch; one poll takes the window past `LIVE_LINES` so the trim drops it; the
    /// NEXT poll is the first fold that starts from `before` alone, and it must still know Zarmin.
    ///
    /// WHAT MUTATION MAKES THIS RED: `tail` trimming into `&mut Party::new()` instead of `a.before`.
    #[test]
    fn tails_trim_hands_the_formation_to_the_party_before_it_drops_it() {
        let mut lines = zarmin_joins(0);
        lines.push(line_at(
            5,
            "You slash a dry bone skeleton for 20 points of damage.",
        ));
        for i in 0..LIVE_LINES - 100 {
            lines.push(line_at(60 + i / 20, "You say, 'still here'"));
        }
        let mut text = lines.join("\n");
        text.push('\n');
        let dir = probe::planted("live-group-trimmed", &text);
        let mut ing = probe::booted(&dir);
        assert!(
            ing.recent_tail(usize::MAX)
                .any(|l| l.contains("Zarmin has joined the group.")),
            "the formation is not in the window at launch, so no trim in `tail` is under test"
        );

        let base = 60 + LIVE_LINES / 20 + 60;
        let mut more = String::new();
        for i in 0..400 {
            more.push_str(&line_at(base + i / 20, "You say, 'still here'"));
            more.push('\n');
        }
        more.push_str(&line_at(
            base + 100,
            "You slash a fire beetle for 4 points of damage.",
        ));
        more.push('\n');
        append_to(&dir, &more);
        ing.last_poll = None;
        assert!(ing.tail() > 0, "the first appended lines were not read");
        assert!(
            !ing.recent_tail(usize::MAX)
                .any(|l| l.contains("Zarmin has joined the group.")),
            "the trim in `tail` kept the formation, so this proves nothing about the lines it drops"
        );

        append_to(
            &dir,
            &format!(
                "{}\n",
                line_at(
                    base + 200,
                    "You slash a fire beetle for 5 points of damage."
                )
            ),
        );
        ing.last_poll = None;
        assert!(ing.tail() > 0, "the second pull was not read");
        let now = ing.current_fight().expect("a fight");
        assert_eq!(now.damage, 5, "this is the pull after the trim");
        assert_eq!(
            now.group,
            Some(vec!["Zarmin".to_owned()]),
            "`tail`'s trim dropped the lines that formed the group without handing them to \
             `before`, so every fight after it lost the group"
        );
    }

    /// DEFECT: THE CEILING IN `tail` THROWING THE GROUP AWAY.
    ///
    /// The other call site `tail` has, and like the trim it was only ever tested by calling
    /// `push_recent` directly. One poll appends more than `LIVE_CAP` lines, so the ceiling (and not
    /// the trim, which runs after the fold) is what takes the formation off the front before the
    /// fold reads the window.
    ///
    /// ONE FOLD OF `LIVE_CAP` LINES, WHICH IS THE PRICE. It is paid once, and it is the only way the
    /// call in `tail` is ever reached.
    ///
    /// WHAT MUTATION MAKES THIS RED: `tail` pushing into `&mut Party::new()` instead of `a.before`.
    #[test]
    fn tails_ceiling_hands_the_formation_to_the_party_before_it_drops_it() {
        let mut lines = zarmin_joins(0);
        lines.push(line_at(
            5,
            "You slash a dry bone skeleton for 20 points of damage.",
        ));
        let mut text = lines.join("\n");
        text.push('\n');
        let dir = probe::planted("live-group-ceiling", &text);
        let mut ing = probe::booted(&dir);

        let mut more = String::with_capacity(LIVE_CAP * 48);
        for i in 0..LIVE_CAP {
            more.push_str(&line_at(60 + i / 100, "You say, 'still here'"));
            more.push('\n');
        }
        let end = 60 + LIVE_CAP / 100 + 60;
        more.push_str(&line_at(
            end,
            "You slash a fire beetle for 4 points of damage.",
        ));
        more.push('\n');
        append_to(&dir, &more);
        ing.last_poll = None;
        assert!(ing.tail() > 0, "the appended lines were not read");
        assert!(
            !ing.recent_tail(usize::MAX)
                .any(|l| l.contains("Zarmin has joined the group.")),
            "the ceiling did not drop the formation, so this proves nothing about it"
        );
        let now = ing.current_fight().expect("a fight");
        assert_eq!(now.damage, 4, "this is the pull after the ceiling fired");
        assert_eq!(
            now.group,
            Some(vec!["Zarmin".to_owned()]),
            "`tail`'s ceiling dropped the lines that formed the group without handing them to \
             `before`"
        );
    }

    /// DEFECT: THE LINE AT `seed_live`'s SPLIT GOING INTO BOTH THE PARTY AND THE WINDOW, OR INTO
    /// NEITHER.
    ///
    /// The live test above replayed the whole text, and its split line was `You say`, which no
    /// party reads. Here a group line sits at the split and on either side of it. Pushed twice,
    /// `Zarmin has left the group.` is a stranger leaving a complete group and revokes it; skipped,
    /// Zarmin never left. Either way the live fold's last fight disagrees with the whole text's.
    ///
    /// WHAT MUTATION MAKES THIS RED: `seed_live` also pushing the window's first line into `before`
    /// (the group assertion); `seed_live` dropping that line from both (the window assertion).
    #[test]
    fn seed_live_hands_the_line_at_the_split_to_the_window_and_to_nothing_else() {
        const HEAD: usize = 40;
        for pos in [HEAD - 1, HEAD, HEAD + 1] {
            let mut lines = zarmin_joins(0);
            lines.push(line_at(2, "You invite Hert to join your group."));
            lines.push(line_at(3, "Hert has joined the group."));
            while lines.len() < LIVE_LINES + HEAD - 1 {
                let i = lines.len();
                lines.push(line_at(10 + i / 20, "You say, 'still here'"));
            }
            lines[pos] = line_at(10 + pos / 20, "Zarmin has left the group.");
            lines.push(line_at(
                10 + (LIVE_LINES + HEAD) / 20 + 60,
                "You slash a fire beetle for 4 points of damage.",
            ));
            let mut text = lines.join("\n");
            text.push('\n');

            let (recent, before) = seed_live(&text);
            assert_eq!(
                recent.front().map(String::as_str),
                Some(lines[HEAD].as_str()),
                "the window does not start at the split, so the line there went into the party or \
                 into nothing"
            );
            let whole = fold_text(&text, quiet_window(), Some(probe::OWNER)).0;
            let live = fold_recent(&recent, &before, Some(probe::OWNER)).0;
            assert_eq!(
                whole.last().map(|r| r.group.clone()),
                Some(Some(vec!["Hert".to_owned()])),
                "the whole text does not end with Hert alone in the group, so this compares nothing"
            );
            assert_eq!(
                live.last().map(|r| r.group.clone()),
                whole.last().map(|r| r.group.clone()),
                "with Zarmin's departure at line {pos} and the split at {HEAD}, the live fold and \
                 the whole text disagree: the line at the split was pushed twice or not at all"
            );
        }
    }

    /// DEFECT: `IN COMBAT` NEVER GOING OUT.
    ///
    /// # WHAT A READER SAW
    ///
    /// The Live page header and the DPS overlay's mark both read `Ingest::fight_is_live`. After a
    /// pull finished it stayed true, so both kept saying IN COMBAT, in live green, over the
    /// finished fight's frozen totals, for as long as the reader sat medding or reading chat. It
    /// is the one fact a person checks before believing any number on that window.
    ///
    /// # WHY THE OLD RULE COULD NOT ANSWER
    ///
    /// `still_going` tested `last.ended != "the log stopped"` and nothing else. The aggregator
    /// emits `Ended::Quiet` only on a fight it closes to open a NEW one, and `Fights::finish`
    /// closes the last fight as `EndOfLog` however long the silence before it. So that test is
    /// true for every fold that did not end on a zone line. `fold_recent`'s own doc had the right
    /// rule written down and the code never read a stamp.
    ///
    /// # AND THE GAP IS AGAINST THE NEWEST LINE, NOT THE NEWEST BLOW
    ///
    /// Chat, casts and loot are `Beat::Idle`: they never reach `FightRow::end`. They are also
    /// exactly how the file proves the reader is alive and not swinging, so the second half of
    /// this test feeds nothing but chat after the pull and expects the fight to go cold on the
    /// log's own clock.
    ///
    /// WHAT MUTATION MAKES THIS RED: returning `true` after the `ended` check, which is the
    /// shipped behaviour this replaces.
    #[test]
    fn a_pull_that_ended_stops_saying_in_combat() {
        /* THE COMBAT WINDOW, WHICH IS NOT THE QUIET ONE. This read `quiet_window` while one constant
         * did both jobs, and thirty seconds is an age to watch IN COMBAT burn over a corpse. The
         * boundary constant still decides whether the NEXT blow is a new fight; this decides
         * whether the reader is swinging, and the window it reads is the one `still_going` reads. */
        let quiet = i64::from(combat_window());
        let at = |s: i64| format!("[Wed Jul 15 23:{:02}:{:02} 2026]", 10 + s / 60, s % 60);
        let blow = |s: i64| {
            format!(
                "{} You slash a dry bone skeleton for 20 points of damage.",
                at(s)
            )
        };
        let chat = |s: i64| format!("{} Losumyda says, 'oom'", at(s));

        /* MID PULL: two blows, the last of them just now. */
        let mut buf: VecDeque<String> = VecDeque::new();
        let nobody = Party::new();
        push_recent(&mut buf, &mut nobody.clone(), [blow(0), blow(1)]);
        let (_, open, _) = fold_recent(&buf, &nobody, Some("Reviir"));
        assert!(
            open,
            "a fight whose last blow is the newest line is not over"
        );

        /* STILL INSIDE THE WINDOW, with only chat since: the reader is between swings. */
        let mut warm = buf.clone();
        push_recent(&mut warm, &mut nobody.clone(), [chat(1 + quiet - 1)]);
        let (_, open, _) = fold_recent(&warm, &nobody, Some("Reviir"));
        assert!(
            open,
            "a pull went cold {} seconds after its last blow, inside a {quiet} second window",
            quiet - 1
        );

        /* PAST THE WINDOW: the pull is over and the header must say so. */
        let mut cold = buf.clone();
        push_recent(&mut cold, &mut nobody.clone(), [chat(1 + quiet + 1)]);
        let (_, open, _) = fold_recent(&cold, &nobody, Some("Reviir"));
        assert!(
            !open,
            "the page still says IN COMBAT {} seconds after the last blow, past a {quiet} second \
             window",
            quiet + 1
        );
    }

    /// DEFECT: A TEST WRITING THE OWNER'S REAL FIGHT HISTORY.
    ///
    /// # THIS TREE HAS ALREADY DONE IT ONCE, WITH SETTINGS
    ///
    /// `settings`'s module note records a valet test writing its fixture over the operator's saved
    /// folders in `%APPDATA%\eql-grimoire\settings.json`. The fix there was `Settings::save`
    /// refusing under `cfg(test)`.
    ///
    /// `cfg(test)` IS NOT ENOUGH HERE AND THAT IS THE WHOLE POINT OF THIS TEST. Integration tests
    /// compile this crate WITHOUT that flag, and `tests/live_twitch.rs` constructs a real
    /// `Ingest`. The first cut of the fight store opened `Store::app_data()` inside `Ingest::new`,
    /// so that one file would have written a real history from a fixture and no unit-test guard
    /// would have seen it.
    ///
    /// SO THE STORE IS OPT IN. A fresh `Ingest` has nowhere to write, production hands it one in
    /// `main`, and a test cannot forget to avoid a thing it never asked for.
    ///
    /// WHAT MUTATION MAKES THIS RED: `store: Store::app_data()` back in the constructor.
    #[test]
    fn a_fresh_ingest_has_nowhere_to_write_fights() {
        let dir = crate::fights::probe::logs_dir("store-optin");
        let s = crate::settings::Settings {
            log_dir: Some(dir),
            ..crate::settings::Settings::default()
        };
        let ig = Ingest::new(&s);
        assert!(
            ig.store().is_none(),
            "a newly built Ingest already points at a fight store, so any test that builds one \
             writes the owner's real history"
        );
        assert_eq!(ig.stored(), crate::store::Wrote::default());
    }

    /// AND WITH A STORE IT KEEPS THE CLOSED FIGHTS AND HOLDS THE OPEN ONE BACK.
    ///
    /// The last row of a tail read is the fight in progress: `fold_text` closes it with
    /// `Ended::EndOfLog` because the text ran out, which from a file still being appended to is
    /// what an open fight looks like. Storing it would write a half fight whose start stamp then
    /// BLOCKS the whole one, because the stamp is the dedupe key and the half got there first.
    ///
    /// WHAT MUTATION MAKES THIS RED: storing `&self.fights` whole.
    #[test]
    fn the_fight_still_running_is_not_written_to_the_store() {
        let dir = crate::fights::probe::planted("store-keep", crate::fights::probe::CAPTURE);
        let root = std::env::temp_dir().join(format!("grimoire-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let s = crate::settings::Settings {
            log_dir: Some(dir),
            ..crate::settings::Settings::default()
        };
        let mut ig = Ingest::new(&s);
        ig.use_store(Some(crate::store::Store::at(&root)));
        /* THE SAME PUMP `probe::booted` USES, inline because that helper builds its own
         * `Ingest` and this test has to hand one a store BEFORE the bootstrap lands. */
        for _ in 0..1_000 {
            let _ = ig.tail();
            if !ig.scanning() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!ig.fights().is_empty(), "the bootstrap never landed");

        let folded = ig.fights().len();
        assert!(folded >= 2, "the capture folds several fights");
        assert_eq!(
            ig.stored().added,
            folded - 1,
            "every fight was stored, including the one still running"
        );
    }

    /// DEFECT: ONE STAT THAT FAILED, AND THE LOGS PAGE STAYS RED FOR THE LIFE OF THE PROCESS.
    ///
    /// The poll's error arm wrote `active_problem` and no path anywhere cleared it. A file handle
    /// lost for a moment to an antivirus scan or to the client's own writer is a completely normal
    /// event on Windows, and after one of them the Logs page and the pop-out Parser said the log
    /// could not be read for the rest of the session, while the lines a few lines further down went
    /// on being read and every number on every other screen went on moving.
    ///
    /// THE FILE IS TAKEN AWAY AND PUT BACK, which is the smallest thing that reaches the arm: the
    /// stat fails on a path that is not there and succeeds when it is. The poll clock is stepped by
    /// hand rather than slept through, because `TAIL_POLL` is a second and this test is about an
    /// order of operations, not about a duration.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting `self.active_problem = None` from the `Ok(md)` arm of
    /// `tail`, which is the code as it stood.
    #[test]
    fn a_stat_that_failed_once_does_not_leave_the_page_red_for_ever() {
        let dir = probe::planted("problem-clears", TWO_FIGHTS);
        let mut ig = probe::booted(&dir);
        assert!(
            ig.active_problem().is_none(),
            "the planted log read cleanly, so nothing should be complaining: {:?}",
            ig.active_problem()
        );

        let path = ig
            .active
            .as_ref()
            .map(|a| a.file.path.clone())
            .expect("the planted log is the active file");
        let bytes = std::fs::read(&path).expect("read the planted log back");

        /* THE FAILURE. Nothing else in the folder, so no newer log takes over and no rescan is
         * queued: the poll goes straight to the stat, which is the arm under test. */
        std::fs::remove_file(&path).expect("take the log away");
        ig.last_poll = None;
        let _ = ig.tail();
        assert!(
            ig.active_problem().is_some(),
            "a log that cannot be stat'd must be said out loud"
        );

        /* AND THE RECOVERY, which is the half that was never written. */
        std::fs::write(&path, &bytes).expect("put the log back");
        ig.last_poll = None;
        let _ = ig.tail();
        assert_eq!(
            ig.active_problem(),
            None,
            "the log is readable again and the page still says it is not"
        );
    }

    /// Three kills of one mob that agree, one fight where a name died TWICE, and an open fight.
    ///
    /// THE DOUBLE DEATH IS THE POINT OF IT. `hp::read` throws that fight away (the fold shares one
    /// damage row between two mobs of one name) and still leaves a row in the book for the mummy,
    /// which is what made the book's LENGTH a count of mobs seen rather than mobs measured.
    const A_NIGHT_OF_KILLS: &str = concat!(
        "[Wed Jul 15 23:16:50 2026] You slash a dry bone skeleton for 1000 points of damage.\n",
        "[Wed Jul 15 23:16:51 2026] You have slain a dry bone skeleton!\n",
        "[Wed Jul 15 23:20:00 2026] You slash a dry bone skeleton for 1020 points of damage.\n",
        "[Wed Jul 15 23:20:01 2026] You have slain a dry bone skeleton!\n",
        "[Wed Jul 15 23:24:00 2026] You slash a dry bone skeleton for 1040 points of damage.\n",
        "[Wed Jul 15 23:24:01 2026] You have slain a dry bone skeleton!\n",
        "[Wed Jul 15 23:28:00 2026] You slash a lurking mummy for 500 points of damage.\n",
        "[Wed Jul 15 23:28:02 2026] You have slain a lurking mummy!\n",
        "[Wed Jul 15 23:28:04 2026] You slash a lurking mummy for 500 points of damage.\n",
        "[Wed Jul 15 23:28:06 2026] You have slain a lurking mummy!\n",
        "[Wed Jul 15 23:40:00 2026] You slash a fire beetle for 3 points of damage.\n",
    );

    /// An [`Ingest`] whose store was handed over BEFORE the bootstrap landed.
    ///
    /// `probe::booted` cannot do this: it pumps until the scan lands, and `keep_fights` (which is
    /// what fills the hit point book) runs inside that landing. A test that called `use_store`
    /// afterwards would be testing an ingest that had already thrown the bootstrap's fights away.
    ///
    /// THE ROOT IS A TEMP PATH AND `Store::at` IS THE ONLY CONSTRUCTOR USED. `Store::app_data`
    /// points at the owner's own `%APPDATA%\eql-grimoire\fights`.
    fn booted_with_store(dir: &Path, root: &Path) -> Ingest {
        let _ = std::fs::remove_dir_all(root);
        let settings = crate::settings::Settings {
            log_dir: Some(dir.to_path_buf()),
            data_root: Some(dir.to_path_buf()),
            ..crate::settings::Settings::default()
        };
        let mut ig = Ingest::new(&settings);
        ig.use_store(Some(crate::store::Store::at(root)));
        for _ in 0..1_000 {
            let _ = ig.tail();
            if !ig.scanning() {
                return ig;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the bootstrap never landed for {}", dir.display());
    }

    /// DEFECT: A MOB PROVEN IN ONE FIGHT WAS A PLAYER IN THE NEXT.
    ///
    /// The owner's store: `Xicotl` punched him at 16:11 on Sep 7, and at 16:42 hit `Xeoning` with
    /// the reader not in the fight at all. The second fight proves nothing alone, and it put the mob
    /// back on the roster. The premise is asserted: the second fight's OWN evidence is empty.
    ///
    /// WHAT MUTATION MAKES THIS RED: `refresh_foes` learning nothing from other fights, or
    /// `apply_foes` stamping only a row's own evidence.
    #[test]
    fn a_mob_proven_in_one_fight_is_a_mob_in_every_fight() {
        let lines = [
            line_at(0, "Xicotl punches YOU for 30 points of damage."),
            line_at(1, "You slash Xicotl for 40 points of damage."),
            line_at(300, "Xicotl punches Xeoning for 20 points of damage."),
            line_at(301, "Xeoning hits Xicotl for 25 points of damage."),
            line_at(
                700,
                "You slash a dry bone skeleton for 20 points of damage.",
            ),
        ];
        let mut text = lines.join("\n");
        text.push('\n');
        let dir = probe::planted("foe-book", &text);
        let ig = probe::booted(&dir);
        let second = ig
            .fights()
            .iter()
            .find(|r| r.start == clock(300))
            .unwrap_or_else(|| {
                panic!(
                    "no fight opened at the second pull: {:?}",
                    ig.fights().iter().map(|r| &r.start).collect::<Vec<_>>()
                )
            });
        assert!(
            crate::fights::proven_foes(&second.fighters, second.group.as_deref(), &second.pets)
                .is_empty(),
            "the second fight proves the mob on its own, so this is not about the book"
        );
        let xicotl = crate::fights::Who::Named("Xicotl".to_owned());
        let xeoning = crate::fights::Who::Named("Xeoning".to_owned());
        assert!(
            !second.ours(&xicotl),
            "a mob proven in the first fight is a player on the second fight's roster"
        );
        assert!(
            second.ours(&xeoning),
            "the player it hit was taken off the roster"
        );
    }

    /// DEFECT: EVERY FIGHT STORED BEFORE THE GROUP READER SAID `GROUP NOT KNOWN` FOR GOOD.
    ///
    /// [`crate::store::Store::append`] refuses a fight it already holds, and the bootstrap only
    /// folds the tail, so a fight stored by an older build was never offered its group again. On
    /// the owner's machine that was all 327 stored fights.
    ///
    /// # WHAT IS PLANTED
    ///
    /// A log in which the reader forms a group with Zarmin and fights three times beside him, and
    /// a store holding the first two fights exactly as an older build wrote them: the same rows
    /// with no group. The whole-log fold is asserted to know the group first, or nothing below is
    /// about refilling it.
    ///
    /// # AND THE SECOND LAUNCH
    ///
    /// A marked log is not read whole again. Without that, every launch re-reads hundreds of
    /// megabytes to learn nothing.
    ///
    /// WHAT MUTATION MAKES THIS RED: `adopt` not calling `start_refill`; `apply_refill`
    /// writing the store without rereading the history; `start_refill` ignoring the marker.
    #[test]
    fn stored_fights_from_before_the_group_reader_are_given_the_group_the_whole_log_proves() {
        let mut lines = zarmin_joins(0);
        for s in [10, 400, 900] {
            lines.push(line_at(
                s,
                "You slash a dry bone skeleton for 20 points of damage.",
            ));
            lines.push(line_at(
                s + 1,
                "Zarmin hits a dry bone skeleton for 10 points of damage.",
            ));
        }
        let mut text = lines.join("\n");
        text.push('\n');
        let dir = probe::planted("refill-old-store", &text);
        let root =
            std::env::temp_dir().join(format!("grimoire-refill-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = crate::store::Store::at(&root);
        let who = crate::store::Owner {
            character: probe::OWNER.to_owned(),
            server: "freeport".to_owned(),
        };
        let log = format!("eqlog_{}_freeport.txt", probe::OWNER);
        let zarmin = Some(vec!["Zarmin".to_owned()]);

        /* WHAT AN OLDER BUILD STORED: the same fights, with no group and no pets. */
        let (mut rows, _) = fold_text(&text, quiet_window(), Some(probe::OWNER));
        assert_eq!(rows.len(), 3, "the fixture is not three fights");
        assert_eq!(
            rows[0].group, zarmin,
            "the whole log does not prove the group, so there is nothing to refill"
        );
        for r in &mut rows {
            r.group = None;
            r.pets.clear();
        }
        store.append(&who, &rows[..2]).expect("store the old rows");

        let settings = crate::settings::Settings {
            log_dir: Some(dir.clone()),
            data_root: Some(dir.clone()),
            ..crate::settings::Settings::default()
        };
        let mut ig = Ingest::new(&settings);
        ig.use_store(Some(crate::store::Store::at(&root)));
        for _ in 0..1_000 {
            let _ = ig.tail();
            if !ig.scanning() && !ig.refilling() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !ig.scanning() && !ig.refilling(),
            "the bootstrap or the refill never landed"
        );

        let groups: Vec<Option<Vec<String>>> =
            ig.history().iter().map(|r| r.group.clone()).collect();
        assert_eq!(
            groups,
            vec![zarmin.clone(); 2],
            "the stored fights still do not know the group the whole log proves"
        );
        assert!(
            store.refilled(&who, crate::store::GROUP_REFILL, &log),
            "the log was refilled and not marked, so every launch would read it whole again"
        );

        /* THE NEXT LAUNCH: the scan lands and no refill starts. */
        let mut again = Ingest::new(&settings);
        again.use_store(Some(crate::store::Store::at(&root)));
        for _ in 0..1_000 {
            let _ = again.tail();
            if !again.scanning() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!again.scanning(), "the second bootstrap never landed");
        assert!(
            !again.refilling(),
            "a log already refilled is being read whole again on the next launch"
        );
    }

    /// DEFECT, AND IT IS AN INVENTED NUMBER: "N MOBS MEASURED" WAS A COUNT OF MOBS SEEN.
    ///
    /// `hp_known` returned the book's length, and `hp::read` leaves a row for every name that ever
    /// died, including names whose every kill it had to throw away. The Dashboards tile prints that
    /// number big, in the words "mobs measured", under a tooltip promising the kills agreed on a
    /// figure. This night has two names in the book and exactly one of them has been measured: the
    /// mummy died twice in one fight, so the fold shares one damage row between two mobs and there
    /// is nothing here anybody measured.
    ///
    /// THE BOOK STILL HOLDS THE MUMMY and that is deliberate, asserted here so a later reader does
    /// not "fix" this by dropping the row: "killed, and not measurable" is a different sentence
    /// from "never killed", and a screen has to be able to say it.
    ///
    /// WHAT MUTATION MAKES THIS RED: `hp_known` going back to `self.hp.len()`.
    #[test]
    fn mobs_measured_is_not_mobs_seen() {
        let dir = probe::planted("hp-known", A_NIGHT_OF_KILLS);
        let root = std::env::temp_dir().join(format!("grimoire-hp-known-{}", std::process::id()));
        let ig = booted_with_store(&dir, &root);

        assert_eq!(
            ig.fights().len(),
            5,
            "three skeletons, the double mummy pull and the open beetle fight"
        );
        assert_eq!(
            ig.hp.len(),
            2,
            "both names are in the book, which is the state the count has to survive: {:?}",
            ig.hp
        );
        /* A FIGHT WITH TWO OF A NAME IN IT CANNOT SAY WHAT EITHER OF THEM HAD, so the mob is seen and
         * skipped rather than measured.
         *
         * AND THAT IS THE LOG'S LIMIT, NOT THE BOUNDARY'S. Two mobs sharing a name can be different
         * levels with different maximums, and no line in an EverQuest Legends log states a level or
         * a maximum: the figure is only ever inferred from damage taken by one mob across one fight,
         * so two of them in one fight leaves nothing to infer from. Merging the pull back together
         * costs this reading and nothing else. WHEN the fight ended and WHETHER a mob was the
         * reader's charmed pet are both things the log states outright, and both survive the merge.
         *
         * `Ended::Killed` briefly split this pull in two and made it measurable, and `HOLD_SECONDS`
         * put it back: the second mummy engages the reader inside the hold, which is the definition
         * of the same encounter continuing. The split was a better HP reading of a worse fight. */
        assert_eq!(
            ig.hp_of("a lurking mummy").map(|r| (r.n(), r.skipped)),
            Some((0, 1)),
            "the double pull is seen and unmeasured"
        );
        assert!(
            ig.hp_of("a dry bone skeleton")
                .and_then(|r| r.expect())
                .is_some(),
            "three kills inside a few percent should stand a figure up"
        );
        assert_eq!(
            ig.hp_known(),
            1,
            "the tile would print two mobs measured over a night that measured one"
        );
    }

    /// DEFECT: THE HIT POINT BOOK WAS BUILT AT LAUNCH AND NEVER AGAIN.
    ///
    /// `keep_fights` runs on a bootstrap, and a bootstrap happens at launch and when a different
    /// log file takes over. So a reader playing one character all evening had exactly one: the
    /// header's `of ~N` denominator and the "from 11 of 26 kills" count beside it were whatever
    /// they had been at launch, however many times he killed the thing that night, and the fights
    /// themselves did not reach the store until the next launch read them back off the log.
    ///
    /// THE MOB IS KILLED IN TWO STEPS ON PURPOSE. A fight is stored only once this app has WATCHED
    /// it open (see `Ingest::live_whole`), because the live window is a slice out of the middle of
    /// a file and its oldest fight is nearly always clipped; a clipped row would put a floor in the
    /// store under a start stamp that is not the fight's start, and that stamp is the dedupe key,
    /// so the whole fight could never be written afterwards. The first poll sees the pull begin,
    /// the second sees it end.
    ///
    /// WHAT MUTATION MAKES THIS RED: removing the `keep_live_fights` call from `tail`, which is the
    /// code as it stood.
    /// DEFECT: THE FIRST FIGHT OF THE NIGHT WAS NEVER KEPT, EVERY NIGHT.
    ///
    /// # WHY THE OLD RULE MISSED IT
    ///
    /// A fight may only be stored once this app has WATCHED it open, and the proof `watch_open_fight`
    /// took was that a row exists BEFORE it in the same fold. That proof is sound and it was the
    /// only one accepted, so a fold with ONE row in it proved nothing.
    ///
    /// A fold has one row in it exactly when the reader has just started logging: `/log on` on a
    /// fresh character, or the client rolling the file. So the FIRST pull of a session was never
    /// watched, never whole, and never written until the next bootstrap read it back off the disk,
    /// which for a reader who plays all evening and quits is never. The hit point book behind the
    /// Live header's `of ~N` learned nothing from the fight the reader was most likely watching.
    ///
    /// # THE SECOND PROOF
    ///
    /// `fold_recent` clips because the live window is a slice out of the MIDDLE of a file. When
    /// `tail_start` is zero it is not: the fold began at byte 0, so nothing exists in front of the
    /// first row to have been cut off it, and its start stamp is the fight's own. That is a fact
    /// about the FILE rather than a guess about the fold, which is why it is allowed to stand in
    /// for the preceding row.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// A log that begins with its first pull, folded from byte 0, reaches the store as soon as that
    /// pull closes, with no second bootstrap. And the figure that comes back is the fight's OWN
    /// total, which is the half that would break if the row were clipped after all.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting `self.live.len() < 2` back as the only test.
    #[test]
    fn the_first_fight_of_a_fresh_log_is_kept_without_waiting_for_a_bootstrap() {
        /* A LOG THAT IS ONLY ITS FIRST PULL. Planted whole so the bootstrap folds it from byte 0,
         * which is what `/log on` looks like: `tail_start` is zero and the window is the file. */
        const FIRST: &str =
            "[Wed Jul 15 23:16:50 2026] You slash a practice dummy for 700 points of damage.\n";
        let dir = probe::planted("first-fight-kept", FIRST);
        let root =
            std::env::temp_dir().join(format!("grimoire-first-fight-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut ig = booted_with_store(&dir, &root);
        let path = dir.join(format!("eqlog_{}_freeport.txt", probe::OWNER));

        /* THE VALIDITY CHECK, and it is the whole premise: this must be the ONE row case, read
         * from the top of the file. With a second row the old rule would have kept the fight and
         * this test would pass on the code it exists to fail. */
        assert_eq!(
            ig.tail_start(),
            Some(0),
            "the fold did not start at byte 0, so this is not the fresh log case"
        );
        assert_eq!(
            ig.stored().added,
            0,
            "a fight was stored before the first pull had closed"
        );

        let append = |text: &str| {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open the planted log to append");
            std::io::Write::write_all(&mut f, text.as_bytes()).expect("append to the planted log");
        };

        /* THE PULL ENDS, AND THE NEXT ONE FOUR MINUTES LATER IS WHAT TELLS THE FOLD IT WENT
         * QUIET. Same two step shape as the test above it and for the same reason. */
        append("[Wed Jul 15 23:16:52 2026] You have slain a practice dummy!\n");
        append("[Wed Jul 15 23:21:00 2026] You slash a fire beetle for 4 points of damage.\n");
        ig.last_poll = None;
        assert!(ig.tail() > 0, "the appended lines were not read");

        assert_eq!(
            ig.stored().added,
            1,
            "the first fight of the log closed and was not kept, so it waits for a bootstrap that \
             may never come"
        );
        /* AND IT IS THE WHOLE FIGHT AND NOT A FLOOR. A clipped row would carry less than the 700
         * the log states, which is the failure the old rule was guarding against. */
        assert_eq!(
            ig.hp_of("a practice dummy").map(|r| (r.n(), r.low())),
            Some((1, Some(700))),
            "the kept row is not the fight's own total, so a floor reached the hit point book"
        );
    }

    /// DEFECT: THE DASHBOARD KEPT DRAWING THE PREVIOUS CHARACTER'S NIGHTS.
    ///
    /// `keep_fights` returned early with no owner and left `history` as it was. Reconfigure onto a
    /// folder with no log in it: the fights list is cleared by `adopt` (it always was) and the
    /// history must go with it, or the page shows a store that belongs to somebody else.
    ///
    /// WHAT MUTATION MAKES THIS RED: the bare `return` in the no-owner path.
    #[test]
    fn switching_to_a_folder_with_no_log_clears_the_stored_history_too() {
        let dir = probe::planted("history-owner-change", A_NIGHT_OF_KILLS);
        let root =
            std::env::temp_dir().join(format!("grimoire-history-owner-{}", std::process::id()));
        let mut ig = booted_with_store(&dir, &root);
        assert_eq!(ig.stored().added, 4, "the night was not stored");
        assert_eq!(ig.history().len(), 4, "the history did not take the night");

        /* ANOTHER FOLDER, WITH NOTHING IN IT. */
        let empty = probe::logs_dir("history-owner-change-empty");
        let settings = crate::settings::Settings {
            log_dir: Some(empty.clone()),
            data_root: Some(empty),
            ..crate::settings::Settings::default()
        };
        ig.reconfigure(&settings);
        for _ in 0..1_000 {
            let _ = ig.tail();
            if !ig.scanning() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            ig.fights().is_empty(),
            "adopt did not clear the fights list"
        );
        assert!(
            ig.history().is_empty(),
            "the previous character's {} stored fights are still the history under a folder \
             with no log in it",
            ig.history().len()
        );
        assert_eq!(
            ig.hp_known(),
            0,
            "the previous character's hit point book survived"
        );
    }

    /// DEFECT: TWO WINDOWS ON ONE STORE PRINTED DIFFERENT FIGHT COUNTS FOR ONE NIGHT.
    ///
    /// The main window and the pop-out each run an `Ingest` over the same log and the same
    /// store. When a fight finishes both try to write it; one gets `added` and the other gets
    /// `already`. The history push was under `added`, so the loser dropped the fight for good.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting the push back under `if w.added > 0`.
    #[test]
    fn the_ingest_that_loses_the_write_still_keeps_the_fight_in_its_history() {
        const FIRST: &str =
            "[Wed Jul 15 23:16:50 2026] You slash a practice dummy for 700 points of damage.\n";
        let dir = probe::planted("history-two-windows", FIRST);
        let root =
            std::env::temp_dir().join(format!("grimoire-history-two-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut a = booted_with_store(&dir, &root);
        /* THE SECOND WINDOW: its own ingest, the same store. `booted_with_store` clears the
         * root, so it is opened by hand here. */
        let settings = crate::settings::Settings {
            log_dir: Some(dir.clone()),
            data_root: Some(dir.clone()),
            ..crate::settings::Settings::default()
        };
        let mut b = Ingest::new(&settings);
        b.use_store(Some(crate::store::Store::at(&root)));
        for _ in 0..1_000 {
            let _ = b.tail();
            if !b.scanning() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let path = dir.join(format!("eqlog_{}_freeport.txt", probe::OWNER));
        let append = |text: &str| {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open the planted log to append");
            std::io::Write::write_all(&mut f, text.as_bytes()).expect("append to the planted log");
        };
        append("[Wed Jul 15 23:16:52 2026] You have slain a practice dummy!\n");
        append("[Wed Jul 15 23:21:00 2026] You slash a fire beetle for 4 points of damage.\n");
        /* A WINS THE WRITE, B LOSES IT. */
        a.last_poll = None;
        assert!(a.tail() > 0);
        b.last_poll = None;
        assert!(b.tail() > 0);
        assert_eq!(a.stored().added, 1, "A did not write the fight");
        assert_eq!(
            b.stored().already,
            1,
            "B did not see the fight as already there, so this is not the losing case"
        );
        assert_eq!(
            (a.history().len(), b.history().len()),
            (1, 1),
            "the two windows disagree about how many fights the night holds"
        );
        /* AND THE LOSER FOLDED NOTHING INTO ITS BOOK TWICE: one kill, one sample. */
        assert_eq!(b.hp_of("a practice dummy").map(|r| r.n()), Some(1));
    }

    #[test]
    fn a_kill_made_while_the_app_runs_reaches_the_book() {
        let dir = probe::planted("hp-live", A_NIGHT_OF_KILLS);
        let root = std::env::temp_dir().join(format!("grimoire-hp-live-{}", std::process::id()));
        let mut ig = booted_with_store(&dir, &root);
        let path = dir.join(format!("eqlog_{}_freeport.txt", probe::OWNER));

        /* FOUR OF THE FIVE FOLDED FIGHTS ARE ON DISK: the last is the beetle pull, still open. */
        assert_eq!(ig.stored().added, 4);
        assert!(
            ig.hp_of("a practice dummy").is_none(),
            "nothing has killed one yet"
        );

        let append = |text: &str| {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open the planted log to append");
            std::io::Write::write_all(&mut f, text.as_bytes()).expect("append to the planted log");
        };

        /* THE PULL BEGINS. The beetle fight is four minutes back now, so the fold closes it, and
         * this app watched THAT one open at the bootstrap, so it is the one that gets written. */
        append("[Wed Jul 15 23:44:00 2026] You slash a practice dummy for 700 points of damage.\n");
        ig.last_poll = None;
        assert!(ig.tail() > 0, "the appended line was not read");
        assert_eq!(
            ig.stored().added,
            5,
            "the fight that was open at launch closed and should have been kept"
        );
        assert!(
            ig.hp_of("a practice dummy").is_none(),
            "a fight still in progress must never be stored: its totals are still growing and its \
             start stamp would then block the whole fight for ever"
        );

        /* AND IT ENDS. The later line is what tells the fold the pull went quiet. */
        append("[Wed Jul 15 23:44:02 2026] You have slain a practice dummy!\n");
        append("[Wed Jul 15 23:48:00 2026] You slash a fire beetle for 4 points of damage.\n");
        ig.last_poll = None;
        assert!(ig.tail() > 0, "the appended lines were not read");

        assert_eq!(
            ig.stored().added,
            6,
            "the kill that finished mid-session was not written"
        );
        assert_eq!(
            ig.hp_of("a practice dummy").map(|r| (r.n(), r.low())),
            Some((1, Some(700))),
            "a mob killed while the app was running never reached the book"
        );
    }

    /* ---- what a bootstrap costs, what a roll costs, and what two windows share ---- */

    /// Settings pointed at one folder, as every caller of `reconfigure` builds them.
    fn settings_for(dir: &Path) -> crate::settings::Settings {
        crate::settings::Settings {
            log_dir: Some(dir.to_path_buf()),
            data_root: Some(dir.to_path_buf()),
            ..crate::settings::Settings::default()
        }
    }

    /// DEFECT: EVERY SETTINGS EDIT OF ANY KIND COST A FULL RE-BOOTSTRAP IN EVERY POP-OUT.
    ///
    /// `Windows::sync` answers ANY changed settings JSON by calling `reconfigure` on the child's
    /// ingest, and the comment above that call asserted this function "compares the resolved folder
    /// and returns without a rescan when it is the same". It did not: it copied both paths, dropped
    /// the roster and called `rescan` unconditionally, folding the last forty megabytes of the log
    /// on a worker. So a fight note, an overlay width or an LFG post typed in the main window
    /// re-read the whole log inside every open tool window, which on this owner's machine is while
    /// he is raiding. The comment was the intention written down as if it had happened.
    ///
    /// AND THE COUNTING FILTERS STILL COME THROUGH, which is the half an early return is most
    /// likely to break: a Kills checkbox is a settings edit that moves no folder at all, so seeding
    /// them after the guard would leave the tick in the file, back down through `sync`, and one
    /// field short of the ingest that does the counting.
    ///
    /// WHAT MUTATION MAKES THIS RED: deleting the guard (the first half goes red), or moving
    /// `self.tracker.settings = ...` below it (the second half does).
    #[test]
    fn reconfigure_re_reads_only_when_a_folder_actually_moved() {
        let dir = probe::planted("reconfigure-guard", TWO_FIGHTS);
        let mut ig = probe::booted(&dir);
        assert_eq!(ig.fights().len(), 2, "this must run with a history in hand");
        let stamp = ig.scanned_at();

        /* THE SAME TWO PATHS. Nothing moved, so nothing may be re-read. */
        ig.reconfigure(&settings_for(&dir));
        assert!(
            !ig.scanning(),
            "an unchanged folder started a fold of the whole log"
        );
        assert_eq!(ig.scanned_at(), stamp, "the history was thrown away");
        assert_eq!(ig.fights().len(), 2);

        /* A CHECKBOX, WHICH MOVES NO FOLDER AND MUST STILL LAND. */
        let mut filtered = settings_for(&dir);
        filtered.tracker.witnessed = true;
        filtered.tracker.ignore_cities = false;
        ig.reconfigure(&filtered);
        assert!(!ig.scanning(), "a counting filter is not a folder");
        assert!(
            ig.tracker().settings.witnessed,
            "the tick reached the file and stopped short of the ingest that counts the kills"
        );
        assert!(!ig.tracker().settings.ignore_cities);

        /* AND A FOLDER THAT REALLY MOVED IS READ AGAIN, which is what the function is for. */
        let moved = probe::planted("reconfigure-guard-moved", TWO_FIGHTS);
        let mut elsewhere = settings_for(&dir);
        elsewhere.log_dir = Some(moved);
        ig.reconfigure(&elsewhere);
        assert!(
            ig.scanning(),
            "a changed Logs folder left every window tailing the old one"
        );
    }

    /// DEFECT: WHEN THE CLIENT ROLLED THE LOG, EVERY NUMBER A BOOTSTRAP OWNS WENT ON DESCRIBING THE
    /// FILE THAT WAS GONE.
    ///
    /// A file that shrank is one the client rotated or reset. The cursor was restarted at byte zero
    /// and the skipped-bytes claim was cleared, and that was all: `fights` was still the fold of
    /// the old bytes, `fights_unreadable` was still that fold's denominator and `scanned_at` still
    /// dated it. The Fights table paged through a history of a file that no longer exists under a
    /// stamp saying when the file that no longer exists was read, and the only thing on screen that
    /// had noticed the roll was the byte count.
    ///
    /// THE FIX IS THE ARM THIS FILE ALREADY HAD FOR THE OTHER HALF OF THE SAME EVENT. A DIFFERENT
    /// log taking over is answered forty lines above with `self.rescan(); return 0;`, because the
    /// history in hand is about another file. A rotation is that same event with the same path.
    ///
    /// AND NO BYTES ARE FED ON THE WAY PAST. `tail` returns zero for this poll: feeding the new
    /// file's opening lines to a kill stream and a live window that describe the old file would put
    /// two files' lines in one fold, which is the merge `adopt` refuses by name.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `rolled` rescan and leaving the cursor reset on
    /// its own, which is the code as it stood.
    #[test]
    fn a_rolled_log_is_folded_again_and_not_described_by_the_old_one() {
        const AFTER_THE_ROLL: &str = "\
[Thu Jul 16 09:00:00 2026] You slash a gnoll pup for 12 points of damage.
[Thu Jul 16 09:00:01 2026] You have slain a gnoll pup!
";
        let dir = probe::planted("rolled-log", TWO_FIGHTS);
        let path = dir.join(format!("eqlog_{}_freeport.txt", probe::OWNER));
        let mut ig = probe::booted(&dir);
        assert_eq!(ig.fights().len(), 2, "the old file's two fights");
        let before = ig.scanned_at().expect("the bootstrap stamped itself");

        /* THE CLIENT ROLLS IT: the same path, fewer bytes, different content. */
        assert!(
            AFTER_THE_ROLL.len() < TWO_FIGHTS.len(),
            "the fixture has to shrink the file or this is not the branch under test"
        );
        std::fs::write(&path, AFTER_THE_ROLL).expect("roll the planted log");

        ig.last_poll = None;
        assert_eq!(
            ig.tail(),
            0,
            "lines from the new file were fed to a stream that describes the old one"
        );
        assert!(ig.scanning(), "the roll did not start a re-read");

        for _ in 0..1_000 {
            let _ = ig.tail();
            if !ig.scanning() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!ig.scanning(), "the re-read never landed");

        assert_eq!(
            ig.fights().len(),
            1,
            "the history still describes the file that is gone: {:?}",
            ig.fights()
                .iter()
                .map(|f| f.headline.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(ig.fights()[0].headline.as_deref(), Some("a gnoll pup"));
        assert!(
            ig.scanned_at().is_some_and(|t| t > before),
            "the stamp still dates a read of bytes that are not there any more"
        );
        assert_eq!(
            ig.tail_start(),
            Some(0),
            "a rolled file is read whole, so nothing before its first byte was skipped"
        );
    }

    /// DEFECT: TWO WINDOWS OF ONE APPLICATION PRINTING TWO TOTALS FOR ONE LOG.
    ///
    /// A pop-out builds its own `Ingest` and points it at the same folder, so each side's `fights`
    /// is a fold of a growing file taken at a different moment. Neither is wrong about the bytes it
    /// read and they are not the same bytes, so the two Fights counts differ all evening with
    /// nothing on either screen saying why. `Windows::sync` holds both and can put them in step,
    /// and could not: `fights` had no writer but `adopt`.
    ///
    /// THE STAMP IS PART OF THE PAYLOAD AND NOT A DETAIL. The caller adopts when the two stamps
    /// differ, so copying the source's stamp is what makes the next comparison say "in step".
    /// Leaving this ingest's own stamp behind would re-adopt on every frame, and whichever side
    /// rescans next has a stamp the other does not, so the newer history wins from either
    /// direction.
    ///
    /// WHAT MUTATION MAKES THIS RED: `adopt_history` leaving `scanned_at` alone, leaving
    /// `fights_unreadable` alone, or reaching into anything the live fold owns (the last two
    /// assertions are the child's own file, which must not move).
    #[test]
    fn one_ingest_can_take_another_ingests_history() {
        const ONE_FIGHT: &str = "\
[Wed Jul 15 22:00:00 2026] You slash a fire beetle for 9 points of damage.
[Wed Jul 15 22:00:01 2026] You have slain a fire beetle!
";
        let root_dir = probe::planted("adopt-history-root", TWO_FIGHTS);
        let child_dir = probe::planted("adopt-history-child", ONE_FIGHT);
        let root = probe::booted(&root_dir);
        let mut child = probe::booted(&child_dir);
        assert_eq!(root.fights().len(), 2);
        assert_eq!(child.fights().len(), 1);
        let childs_own_live = child.current_fight().and_then(|f| f.headline.clone());
        assert_eq!(childs_own_live.as_deref(), Some("a fire beetle"));

        /* A STAMP OF ITS OWN, so this pins the copy rather than a clock two `Utc::now()` calls
         * milliseconds apart might make equal by accident. */
        let stamp = DateTime::<Utc>::from_timestamp(1_752_600_000, 0);
        assert!(stamp.is_some(), "the constant is a real instant");
        child.adopt_history(root.fights().to_vec(), 7, stamp);

        assert_eq!(
            child.fights().len(),
            2,
            "the child kept folding its own history"
        );
        assert_eq!(
            child.fights()[0].headline.as_deref(),
            Some("a dry bone skeleton")
        );
        assert_eq!(
            child.fights_unreadable(),
            7,
            "the denominator did not travel with the rows it qualifies"
        );
        assert_eq!(
            child.scanned_at(),
            stamp,
            "the stamp stayed behind, so the caller re-adopts on every frame"
        );

        /* AND NOTHING THE LIVE FOLD OWNS MOVED. This ingest is still tailing its own file. */
        assert_eq!(
            child.current_fight().and_then(|f| f.headline.clone()),
            childs_own_live,
            "the live end took another file's fight"
        );
        assert_eq!(
            child
                .active_log()
                .and_then(|f| f.path.parent().map(Path::to_path_buf)),
            Some(child_dir),
            "the child stopped tailing its own log"
        );
    }

    /// DEFECT: THE CANDIDATES TABLE WAS SORTED ON A NUMBER IT COULD NOT PRINT, AND NOTHING SAID HOW
    /// OLD THE TABLE ITSELF WAS.
    ///
    /// `list_logs` orders the candidates newest first and that order is the whole reason one of
    /// them is tailed, so "why that one" was answered by a column the page had no way to draw:
    /// `Source` carries no modified time and `Ingest::logs` is private. And the folder is re-listed
    /// on every poll while `scanned_at` dates the last FOLD, so the page could say how old the
    /// forty megabyte read was and could say nothing about the table above it, which is newer by up
    /// to a whole session.
    ///
    /// THE TIME COMES OFF THE INGEST'S OWN LISTING, which is the listing the sort ran on. A page
    /// that stat'd each file again would print times measured at a different moment from the ones
    /// the order rests on, and they disagree exactly when a file is being written, which is always.
    ///
    /// WHAT MUTATION MAKES THIS RED: `log_modified` reading the disk instead of `self.logs` (the
    /// path that is not in the listing then answers), or `listed_at` being set only in `adopt`.
    #[test]
    fn the_candidate_listing_carries_a_write_time_and_a_stamp_of_its_own() {
        let dir = probe::planted("listed-at", TWO_FIGHTS);
        let path = dir.join(format!("eqlog_{}_freeport.txt", probe::OWNER));
        let mut ig = probe::booted(&dir);

        let want = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .expect("the planted log has a modified time");
        assert_eq!(
            ig.log_modified(&path),
            Some(want),
            "the table cannot print the number it is sorted on"
        );
        assert_eq!(
            ig.log_modified(&dir.join("eqlog_Nobody_freeport.txt")),
            None,
            "a path that is in no listing and on no disk was answered for"
        );

        /* A REAL FILE THAT IS NOT A CANDIDATE, which is what tells the listing apart from the
         * disk. `list_logs` takes `eqlog_*.txt` and nothing else, so this file exists, has a
         * modified time, and has no row in the table the sort ran on. An accessor that stat'd the
         * path would answer for it; this one has nothing to say and says so. */
        let stranger = dir.join("notes.txt");
        std::fs::write(&stranger, b"not a log").expect("write a file that is not a candidate");
        assert!(
            std::fs::metadata(&stranger)
                .and_then(|m| m.modified())
                .is_ok(),
            "the fixture must have a modified time on disk or it proves nothing"
        );
        assert_eq!(
            ig.log_modified(&stranger),
            None,
            "the time came off the disk rather than off the listing the order rests on"
        );

        let scanned = ig.scanned_at().expect("the bootstrap stamped itself");
        assert!(
            ig.listed_at().is_some_and(|t| t <= scanned),
            "the bootstrap's own listing was never stamped, so the table reads as unread"
        );

        /* A POLL RE-LISTS THE FOLDER AND FOLDS NOTHING. The two stamps have to part here or the
         * page is dating the candidates with the age of the fold. Two milliseconds is not a
         * measurement, it is headroom over the clock. */
        std::thread::sleep(Duration::from_millis(2));
        ig.last_poll = None;
        let _ = ig.tail();
        assert!(
            ig.listed_at().is_some_and(|t| t > scanned),
            "a poll re-listed the folder and left the stamp where the bootstrap put it"
        );
        assert_eq!(
            ig.scanned_at(),
            Some(scanned),
            "a poll is not a bootstrap and must not claim to be one"
        );
    }

    /// DEFECT: THE KILL TRACKER'S THREE FILTERS WERE PER WINDOW AND PER RUN.
    ///
    /// They live on `TrackerState::settings`, which lives on the `Ingest`, and there is one
    /// `Ingest` per window. Ticking "count witnessed kills" on the Kills view moved that window's
    /// completion percentage and left the other window's alone, and nothing wrote the answer
    /// anywhere, so all three were back at their defaults on the next launch.
    ///
    /// SEEDED AT CONSTRUCTION AND NOT ON THE FIRST FRAME THE KILLS VIEW IS DRAWN, which is what
    /// makes both windows agree from the first frame: a percentage on a dashboard tile is computed
    /// off this state whether or not anybody has opened the Kills view in that window.
    ///
    /// WHAT MUTATION MAKES THIS RED: `Ingest::new` building its tracker with
    /// `TrackerState::default()`.
    #[test]
    fn the_counting_filters_are_seeded_from_the_saved_settings() {
        let dir = probe::planted("tracker-seed", TWO_FIGHTS);
        let mut settings = settings_for(&dir);
        settings.tracker.witnessed = true;
        settings.tracker.ignore_cities = false;
        settings.tracker.ignored_zones.push("oggok".to_owned());

        let ig = Ingest::new(&settings);
        assert!(
            ig.tracker().settings.witnessed,
            "a reader who counts witnessed kills had to say so again on every launch"
        );
        assert!(!ig.tracker().settings.ignore_cities);
        assert_eq!(ig.tracker().settings.ignored_zones, vec!["oggok"]);
        assert_eq!(
            ig.tracker().settings,
            settings.tracker,
            "the whole rule travels, not the fields somebody remembered"
        );
    }
}
