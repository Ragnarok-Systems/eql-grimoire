//! Snapshot data, loaded at runtime. Decision D6.
//!
//! WHY RUNTIME AND NOT include_bytes!.
//! The snapshot is 18.8MB across six JSON files and 122 atlas pages. Baked into the binary, every
//! data refresh would be a rebuild and every build 18.8MB heavier for a table of item names. So the
//! app LOOKS for the data (see [`Snapshot::locate`]) and the FIND screens say which path they read
//! and how many records it held, or that it is absent and where to put it.
//!
//! THE TWO FILES THAT NOTHING READ NOW HAVE READERS, AND NEITHER GOT A RAIL ROW.
//! This paragraph used to say that `factions.json` (255 pages, 0.6MB) and `merchants.json` (1373
//! pages, 1.2MB) sat in the directory with no loader, no screen and no place in [`FILES`], costing
//! disk and nothing else, and that wiring either one meant a Factions screen and a Merchants
//! screen that did not exist. The second half was wrong, which is why it took a second look.
//!
//! A merchant record read forwards is a shop window and wants a screen. Read BACKWARDS it is the
//! missing sibling of the drop table: Items already answers "who drops this" and 369 item pages
//! carry the wiki's "sold by a vendor" flag with no vendor on them. So merchants.json is loaded as
//! an INVERSE index, item name to seller ([`merchants::SellersIndex`]), and it hangs under the drop
//! table on a pane the reader already opens. Factions cross link the same way: a faction names the
//! zones, quests and mobs that move it, and zone and quest pages are where those belong. Two files
//! wired, four panes richer, zero rows added to a rail the owner has been cutting rows out of.
//!
//! Both files stay OUT of [`FILES`], with spells.json, because a root without them still loads
//! every screen that does not need them, and a root that loads fine without them is not a broken
//! root.
//!
//! NEITHER IS IN THE FINDER, and that is the same argument that retired `d:`. A prefixed hit is
//! routed to the screen that OWNS its kind (see [`HitKind`]); there is no Factions screen and no
//! Merchants screen, so a faction hit would have nowhere to land. The records are reached from the
//! panes that draw them, which is exactly what happened to the drop table.
//!
//! WHY TYPED RECORDS WITH `extra`, AND NOT serde_json::Value.
//! A Value is a tree the caller walks with string keys at every use site, which is how a table
//! column ends up reaching three keys deep with a fallback at every step. Typed records put
//! the shape in one place. But the wiki scrape adds fields, and a typed record that DROPS what it
//! does not know is a record that silently loses data on the next refresh. So every record carries
//! `#[serde(flatten)] pub extra`, which keeps every unknown field, and a test proves it is non-empty
//! on the first item of the real file.
//!
//! THE MAP KEYS ARE NOT THE NAMES.
//! gear-data.json indexes items by the case folded name ("a bone necklace") and carries the display
//! spelling in `n` ("A Bone Necklace"). The contract says `name` is the key; the honest reading is
//! that `name` is what a person calls the item, so `name` is `n` (present on every one of the 6891
//! measured records, key as the fallback) and the key is kept beside it as `key`. Search and the
//! exact lookups accept either.
//!
//! LOAD TIME, MEASURED ON THE REAL FILES: see [`LOAD_BUDGET`]. The integrator loads on a
//! background thread; nothing here touches the UI thread or the network.
//!
//! No em dashes and no en dashes anywhere in this module, by house rule.
//!
//! This module once carried a scoped `#![allow(dead_code)]` because it was a library surface
//! inside a binary crate while its callers were still being written. The crate now has a lib
//! target (`lib.rs`), where a `pub` item is API rather than dead code, so the allow is gone and a
//! private item nobody calls is a warning again, as it should be.

pub mod drops;
pub mod factions;
pub mod items;
pub mod merchants;
pub mod quests;
pub mod sky;
pub mod spells;
pub mod zones;

/* Only the five record types the contract names are re-exported. The finer grained types
 * (DropSource, ItemDrop, ZoneMob, AtlasMob, Isle, SkyTest, QuestStep, ...) are reached through
 * their module, `crate::data::zones::AtlasMob`, so a screen that needs one says where it came
 * from and an unused re-export never trips the unused_imports lint on a build nobody broke. */
pub use drops::Drop;
pub use factions::Faction;
pub use items::{Item, Tooltip};
pub use merchants::Merchant;
pub use quests::Quest;
pub use sky::Sky;
pub use spells::Spell;
pub use zones::Zone;

use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/* ------------------------------------------------------------------ the files -- */

pub const GEAR_FILE: &str = "gear-data.json";
pub const TOOLTIPS_FILE: &str = "item-tooltips.json";
pub const QUEST_ITEMS_FILE: &str = "quest-items.json";
pub const KILLS_FILE: &str = "kills-data.json";
pub const SKY_FILE: &str = "sky.json";
/// spells.json. NOT in [`FILES`]: see [`spells::load`] for why an absent one is an empty list
/// rather than a broken snapshot.
pub const SPELLS_FILE: &str = "spells.json";
/// factions.json. NOT in [`FILES`], same reason as spells.json; see [`factions::load`].
pub const FACTIONS_FILE: &str = "factions.json";
/// merchants.json. NOT in [`FILES`], same reason as spells.json; see [`merchants::load`].
pub const MERCHANTS_FILE: &str = "merchants.json";
pub const ATLAS_DIR: &str = "atlas-wiki";
/// The five files a snapshot root must hold, plus the `atlas-wiki/` directory beside them.
pub const FILES: &[&str] = &[
    GEAR_FILE,
    TOOLTIPS_FILE,
    QUEST_ITEMS_FILE,
    KILLS_FILE,
    SKY_FILE,
];

/* The one phrase a screen prints beside each probed directory. They live here, next to the
 * function that builds the list, so a screen cannot label a path something the loader does not
 * call it. */

/// `data/` in the directory the running executable sits in.
pub const EXE_SIBLING: &str = "data/ beside the executable";
/// `data/` in the per-user folder this app already writes settings.json into.
pub const USER_FOLDER: &str = "the per-user folder beside settings.json";
pub const UPDATER_FOLDER: &str = "the folder the updater installs data into";
/// `crates/grimoire-desktop/data` in the source tree a development build was compiled from.
pub const SOURCE_TREE: &str = "the copy in the source tree (development builds only)";

/// The per-user snapshot folder: `data/` beside the settings file this app already writes, which
/// on Windows is `%APPDATA%\eql-grimoire\data`.
///
/// WHY A SECOND PLACE AT ALL, when `data/` beside the executable is the shipped layout. An
/// installed binary can sit in a directory the reader cannot write to, and the snapshot is
/// content he refreshes rather than something the installer owns. The app already has one folder
/// per user that is always writable, because settings.json is in it; the snapshot gets the same
/// folder rather than a new one to explain. None only on a platform that offers no config
/// directory, and then the list is simply one entry shorter.
pub fn user_data_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(crate::settings::APP_DIR).join("data"))
}

/// The load budget on the shipped binary, and the measurement behind it.
///
/// Measured on James's machine, `Snapshot::load` end to end, warm cache, via
/// `cargo test -p grimoire-desktop --release --lib load_is_fast_enough -- --nocapture`.
///
/// 2026-09-02, six files, 16.7MB of JSON (6891 items, 11253 tooltips, 122 zones, 1637 drops,
/// 3057 sky items, 924 quests):
///   release profile (opt-level z, lto, the shipped profile):  96 to 106ms  (four runs: 106, 99, 97, 96)
///   dev profile (unoptimized, what `cargo test` runs):          432ms
///
/// 2026-09-03, with spells.json (2001 spells, 1.6MB), kills-data grown from 76 zones and 3843
/// mobs to 122 and 6833 (+0.4MB), and 203 tooltips appended for the Category:Items members that
/// had none (11253 to 11456, +0.06MB), 18.8MB of JSON:
///   release profile:  115ms
/// So about 10ms for the 2001 spells and the 2990 extra mobs, and still twenty six times under the
/// budget. A background thread stays a courtesy to the first frame rather than a necessity.
///
/// The `load_is_fast_enough` test prints the number every run and asserts the budget only in a
/// release build, because the dev profile is not what a user runs.
pub const LOAD_BUDGET: Duration = Duration::from_secs(3);

/* ----------------------------------------------------------------- the errors -- */

/// A file that could not be read or was not the shape this build was measured against. Carries
/// the path so the screen can say WHICH file, and serde's reason, which names the line and column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataError {
    pub path: PathBuf,
    pub reason: String,
}

impl DataError {
    pub fn new(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.reason)
    }
}

impl std::error::Error for DataError {}

/// Read a whole file and parse it. `from_str` over a `read_to_string` is the fast path; a buffered
/// reader is measurably slower in serde_json and the files fit in memory many times over.
pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, DataError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| DataError::new(path, format!("cannot read: {e}")))?;
    serde_json::from_str(&text)
        .map_err(|e| DataError::new(path, format!("not the shape this build expects: {e}")))
}

/// A schema number other than the one this build was measured against is not a refusal, because
/// the record shapes may still match and refusing would hide data that loads fine. It is logged,
/// so a mismatch that DOES break something has its cause on record.
pub(crate) fn note_schema(path: &Path, seen: Option<u64>, measured: u64) {
    match seen {
        Some(s) if s == measured => {}
        Some(s) => log::warn!(
            "{}: schema {s}, this build was measured against schema {measured}; loading what matches",
            path.display()
        ),
        None => log::warn!(
            "{}: no schema field; this build was measured against schema {measured}",
            path.display()
        ),
    }
}

/// Case folding for search and lookups: trimmed and lower cased, Unicode aware. The same function
/// is applied to both sides so a name and a query can never fold differently.
pub(crate) fn fold(s: &str) -> String {
    s.trim().to_lowercase()
}

/* ----------------------------------------------------------------- the report -- */

/// What was actually loaded, for the FIND screens. Counts come from the vectors, never from the
/// file's own claims about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataReport {
    pub root: PathBuf,
    pub items: usize,
    pub zones: usize,
    pub drops: usize,
    pub sky_items: usize,
    pub quests: usize,
    /// factions.json records. Not one of the contract's five; the SOURCES ledger prints it so a
    /// file that is loaded is a file the app can be asked to account for.
    pub factions: usize,
    /// merchants.json records.
    pub merchants: usize,
}

/* ----------------------------------------------------------------- the search -- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HitKind {
    Item,
    Zone,
    Quest,
    /// A sky.json item window: the Plane of Sky pieces, which are their own list in the
    /// snapshot (3057 of them) with their own drop tables and slot alternatives.
    SkyItem,
    /// A spells.json page: 2001 of them, pulled from the wiki's Category:Spells.
    Spell,
}

impl HitKind {
    pub const ALL: [HitKind; 5] = [
        HitKind::Item,
        HitKind::Zone,
        HitKind::Quest,
        HitKind::SkyItem,
        HitKind::Spell,
    ];

    /// The one letter typed before the colon: `i:`, `z:`, `q:`, `s:`, `p:`. Decision D6 names
    /// items, zones and drops; quests, the sky list and the spell list are the others the snapshot
    /// carries. Spells take `p` because `s` was already the sky list's and a prefix that means two
    /// things means neither.
    ///
    /// `d:` IS GONE, AND IT IS ONE D6 NAMED, so its absence is argued rather than assumed. A
    /// prefixed hit is routed by `main::ask_for` to the screen that OWNS its kind, and the Drops
    /// screen has left the rail (`nav::NAV` carries the owner's reason and what it cost). With no
    /// owner, a drop hit had two possible ends and both were worse than not offering it: sending
    /// it to Items dead ends 955 of the table's 1637 names on "no item under this name", and
    /// leaving it unrouted is a row in the finder that swallows the click. The drop RECORDS are
    /// untouched and still drawn, by the Items detail pane under the item they belong to and by
    /// the Zones detail pane under the zone they drop in.
    pub fn prefix(self) -> char {
        match self {
            HitKind::Item => 'i',
            HitKind::Zone => 'z',
            HitKind::Quest => 'q',
            HitKind::SkyItem => 's',
            HitKind::Spell => 'p',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HitKind::Item => "item",
            HitKind::Zone => "zone",
            HitKind::Quest => "quest",
            HitKind::SkyItem => "sky",
            HitKind::Spell => "spell",
        }
    }

    fn from_prefix(c: char) -> Option<Self> {
        HitKind::ALL
            .iter()
            .copied()
            .find(|k| k.prefix() == c.to_ascii_lowercase())
    }
}

/// One search result: what kind of thing, what it is called, and one computed line about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub kind: HitKind,
    pub name: String,
    pub detail: String,
}

/// The most hits a search returns. Two hundred rows is already more than a person reads; past that
/// the answer is "type more", and the screen can say so when it sees exactly this many.
pub const SEARCH_CAP: usize = 200;

/// Split a query into its optional type prefix and its case folded needle.
///
/// `i:bone` restricts to items and searches "bone". `bone` searches everything. The prefix is
/// exactly one letter and a colon, so a name that happens to contain a colon deeper in is never
/// mistaken for a prefixed query. An empty needle yields no hits: listing everything is the
/// screen's job, and it has the vectors for that.
pub fn parse_query(q: &str) -> (Option<HitKind>, String) {
    let q = q.trim();
    let mut chars = q.chars();
    if let (Some(p), Some(':')) = (chars.next(), chars.next()) {
        if let Some(kind) = HitKind::from_prefix(p) {
            return (Some(kind), fold(chars.as_str()));
        }
    }
    (None, fold(q))
}

/* --------------------------------------------------------------- the snapshot -- */

/// Everything the snapshot holds, typed, plus the indexes that make an exact lookup O(1).
#[derive(Debug)]
pub struct Snapshot {
    /// The directory the files were read from. The FIND screens print it.
    pub root: PathBuf,
    pub items: Vec<Item>,
    pub zones: Vec<Zone>,
    pub drops: Vec<Drop>,
    pub sky: Sky,
    pub quests: Vec<Quest>,
    /// spells.json: the wiki's Category:Spells, 2001 pages. Empty when the snapshot root has no
    /// spells.json, which is not an error; see [`spells::load`].
    pub spells: Vec<Spell>,
    /// factions.json: the wiki's faction pages, 255 of them. Empty when the root has none.
    pub factions: Vec<Faction>,
    /// merchants.json: the wiki's merchant pages, 1373 of them. Empty when the root has none.
    pub merchants: Vec<Merchant>,
    /// item-tooltips.json: the wiki's stat block lines per item. Not in the report because the
    /// contract's report has five counts, but it is loaded and [`Snapshot::tooltip`] reaches it.
    pub tooltips: Vec<Tooltip>,
    /// Item name to the merchants selling it. Public because it carries its own counts and the
    /// SOURCES ledger prints them; the lookups go through [`Snapshot::sellers_of`].
    pub sellers: merchants::SellersIndex,
    /// How long `load` took, so the Settings screen's SOURCES ledger can say it rather than guess.
    pub load_time: Duration,
    /* Exact lookup indexes, folded name (and key) to position. Private: they are derived from the
     * vectors, and a caller that could edit one without the other would have a lying index. */
    item_index: HashMap<String, usize>,
    zone_index: HashMap<String, usize>,
    drop_index: HashMap<String, usize>,
    tooltip_index: HashMap<String, usize>,
    spell_index: HashMap<String, usize>,
    faction_index: HashMap<String, usize>,
    /* THESE THREE ARE ONE TO MANY, and that is not an accident of the data: a zone holds many
     * merchants, and a zone or a quest is named by many factions. A one to one index here would
     * have kept the first and dropped the rest, silently, which is the same defect as a name keyed
     * merchant index keeping one of thirty two clockwork merchants. */
    merchants_by_zone: HashMap<String, Vec<u32>>,
    factions_by_zone: HashMap<String, Vec<(u32, factions::Way)>>,
    factions_by_quest: HashMap<String, Vec<(u32, factions::Way)>>,
}

/// The directories EVERY build probes, in order, each with the phrase a screen prints beside it.
///
/// Both are worked out when this function RUNS: one from `current_exe()`, one from the platform's
/// own config directory. Neither carries anything from the machine that compiled the binary, and
/// `no_release_candidate_is_a_compile_time_absolute_path` is the test that keeps it so.
fn runtime_candidates() -> Vec<(PathBuf, &'static str)> {
    let mut out: Vec<(PathBuf, &'static str)> = Vec::new();
    /* data/ beside the executable: the shipped layout. */
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push((dir.join("data"), EXE_SIBLING));
        }
    }
    /* THE FOLDER THE UPDATER MANAGES, AND IT COMES FIRST BECAUSE THE UPDATER OWNS IT.
     *
     * THE DEFECT THIS FIXES: two modules each picked "the per-user folder" and picked different
     * ones. `updater::install::Layout::platform` roots at `dirs::data_local_dir()`, so a data
     * bundle it installs lands in `%LOCALAPPDATA%\eql-grimoire\data`; this list only ever named
     * `dirs::config_dir()`, which on Windows is `%APPDATA%` under Roaming. Local and Roaming are
     * different directories, so an updated app looked straight past the snapshot the updater had
     * just put down and reported no data at all. A reader met it as `No snapshot found` on a
     * version he had done nothing to but let update.
     *
     * LOCAL IS THE RIGHT HOME FOR IT, which is why the updater is followed rather than changed:
     * a roaming profile copies its contents between machines at sign-in, and 32 MB of game data
     * that any install can re-fetch has no business travelling.
     *
     * `updater_data_dir` IS DERIVED FROM THE UPDATER'S OWN LAYOUT and not from a second copy of
     * the same path arithmetic, so the two cannot drift apart again; a test asserts they name the
     * same directory. */
    if let Some(d) = updater_data_dir() {
        out.push((d, UPDATER_FOLDER));
    }
    /* The folder this app already owns per user; see `user_data_dir` for why there are two. */
    if let Some(d) = user_data_dir() {
        out.push((d, USER_FOLDER));
    }
    out
}

/// The snapshot folder the updater installs into, asked of the updater rather than rebuilt here.
pub fn updater_data_dir() -> Option<PathBuf> {
    crate::updater::install::Layout::platform().map(|l| l.data_dir())
}

/// Every directory [`Snapshot::locate`] will try, in order, each with the one phrase a screen
/// prints beside it. Public so the screens can list them when the data is absent: "put it at one
/// of these" is a sentence the user can act on, and taking the WORDS from here as well as the
/// paths means no screen can call a directory something the loader does not.
///
/// A SHIPPED BUILD PROBES ONLY WHAT IT CAN FIND AT RUN TIME, and that is the whole reason this
/// list is built rather than written down. Two absolute paths used to be `const`s in this module
/// and the last of them named a directory that existed on one developer's disk; a release binary
/// therefore probed it, and PRINTED it, on every "put the data here" screen a stranger would
/// ever read. The source tree copy below is the one entry that survives, because `cargo run` and
/// the tests do want the checkout's own data, and it is compiled out of a release build.
pub fn candidates_labelled() -> Vec<(PathBuf, &'static str)> {
    let mut out = runtime_candidates();
    /* The copy in the source tree this binary was built from. The manifest directory is a
     * compile time fact, which is exactly why it is gated: it is right for a build run out of a
     * checkout and meaningless anywhere else. */
    #[cfg(any(test, debug_assertions))]
    out.push((
        Path::new(env!("CARGO_MANIFEST_DIR")).join("data"),
        SOURCE_TREE,
    ));
    out.dedup();
    out
}

/// The paths of [`candidates_labelled`], without the words, for callers that only list them.
pub fn candidates() -> Vec<PathBuf> {
    candidates_labelled().into_iter().map(|(p, _)| p).collect()
}

impl Snapshot {
    /// Where the data is: the first directory of [`candidates_labelled`] that holds
    /// gear-data.json, else None. A shipped build asks two questions, `data/` beside the
    /// executable and then the per-user folder; a development build asks the source tree copy
    /// after those. The Settings screen's `data_root` is consulted by the integrator ahead of all
    /// of them and is not in this list, because a path the user typed is an instruction rather
    /// than a guess: a wrong one is reported as a failure naming it, never fallen through.
    ///
    /// The probe is for the one file every screen needs; a root that has it and lacks another
    /// fails in `load`, loudly, with that file's path.
    pub fn locate() -> Option<PathBuf> {
        candidates()
            .into_iter()
            .find(|p| p.join(GEAR_FILE).is_file())
    }

    /// Parse the five files and the atlas pages under `root`. Synchronous; the integrator puts it
    /// on a thread. Any file that cannot be read or is not the measured shape is an `Err` naming
    /// the file, and a file that parses to zero records is an `Err` too: a snapshot with no items
    /// is a broken snapshot, not an empty one.
    pub fn load(root: &Path) -> Result<Snapshot, DataError> {
        let t0 = Instant::now();
        if !root.is_dir() {
            return Err(DataError::new(root, "not a directory"));
        }
        let items = items::load(root)?;
        let tooltips = items::load_tooltips(root)?;
        let zones = zones::load(root)?;
        let quest_file = quests::load_file(root)?;
        let drops = drops::from_table(quest_file.drops);
        let quests = quest_file.quests;
        let sky = sky::load(root)?;
        let spells = spells::load(root)?;
        let factions = factions::load(root)?;
        let merchants = merchants::load(root)?;

        let mut item_index = HashMap::with_capacity(items.len() * 2);
        for (i, it) in items.iter().enumerate() {
            item_index.entry(fold(&it.key)).or_insert(i);
            item_index.entry(fold(&it.name)).or_insert(i);
        }
        let mut zone_index = HashMap::with_capacity(zones.len() * 3);
        for (i, z) in zones.iter().enumerate() {
            zone_index.entry(fold(&z.key)).or_insert(i);
            zone_index.entry(fold(&z.name)).or_insert(i);
            if let Some(w) = &z.wiki_title {
                zone_index.entry(fold(w)).or_insert(i);
            }
        }
        let mut drop_index = HashMap::with_capacity(drops.len());
        for (i, d) in drops.iter().enumerate() {
            drop_index.entry(fold(&d.name)).or_insert(i);
        }
        let mut spell_index = HashMap::with_capacity(spells.len() * 2);
        for (i, sp) in spells.iter().enumerate() {
            spell_index.entry(fold(&sp.key)).or_insert(i);
            spell_index.entry(fold(&sp.name)).or_insert(i);
        }
        let mut tooltip_index = HashMap::with_capacity(tooltips.len() * 2);
        for (i, t) in tooltips.iter().enumerate() {
            tooltip_index.entry(fold(&t.key)).or_insert(i);
            tooltip_index.entry(fold(&t.name)).or_insert(i);
        }
        let mut faction_index = HashMap::with_capacity(factions.len() * 2);
        for (i, f) in factions.iter().enumerate() {
            faction_index.entry(fold(&f.key)).or_insert(i);
            faction_index.entry(fold(&f.name)).or_insert(i);
        }
        /* Every zone string a merchant names, and the pieces of a two zone string. The keys are
         * the merchant's own spelling, folded; `merchants_in` folds the ZONE's key, name and wiki
         * title against them, so no alias table is invented on either side. */
        let mut merchants_by_zone: HashMap<String, Vec<u32>> = HashMap::new();
        for (i, m) in merchants.iter().enumerate() {
            for piece in m.zone_pieces() {
                if piece.is_empty() {
                    continue;
                }
                merchants_by_zone
                    .entry(fold(piece))
                    .or_default()
                    .push(i as u32);
            }
        }
        let mut factions_by_zone: HashMap<String, Vec<(u32, factions::Way)>> = HashMap::new();
        let mut factions_by_quest: HashMap<String, Vec<(u32, factions::Way)>> = HashMap::new();
        for (i, f) in factions.iter().enumerate() {
            let file = |map: &mut HashMap<String, Vec<(u32, factions::Way)>>,
                        names: &[String],
                        way: factions::Way| {
                for n in names {
                    let k = fold(n);
                    if k.is_empty() {
                        continue;
                    }
                    let slot = map.entry(k).or_default();
                    /* A faction that names the same zone under BOTH lists (107 of them do, which
                     * is ordinary: you raise and lower the same faction in the same zone by
                     * killing different things) gets one row per direction and never two of one. */
                    if !slot.contains(&(i as u32, way)) {
                        slot.push((i as u32, way));
                    }
                }
            };
            file(&mut factions_by_zone, &f.zones_raise, factions::Way::Raise);
            file(&mut factions_by_zone, &f.zones_lower, factions::Way::Lower);
            file(
                &mut factions_by_quest,
                &f.quests_raise,
                factions::Way::Raise,
            );
            file(
                &mut factions_by_quest,
                &f.quests_lower,
                factions::Way::Lower,
            );
        }
        let sellers = merchants::SellersIndex::build(&merchants);

        Ok(Snapshot {
            root: root.to_path_buf(),
            items,
            zones,
            drops,
            sky,
            quests,
            spells,
            factions,
            merchants,
            tooltips,
            sellers,
            load_time: t0.elapsed(),
            item_index,
            zone_index,
            drop_index,
            tooltip_index,
            spell_index,
            faction_index,
            merchants_by_zone,
            factions_by_zone,
            factions_by_quest,
        })
    }

    /// Counts of what is in memory. See [`DataReport`].
    pub fn report(&self) -> DataReport {
        DataReport {
            root: self.root.clone(),
            items: self.items.len(),
            zones: self.zones.len(),
            drops: self.drops.len(),
            sky_items: self.sky.items.len(),
            quests: self.quests.len(),
            factions: self.factions.len(),
            merchants: self.merchants.len(),
        }
    }

    /// Substring search, case folded, over items, zones, quests, spells and the sky list, in that
    /// order, capped at [`SEARCH_CAP`]. A type prefix (`i:` `z:` `q:` `p:` `s:`) restricts to one
    /// kind. No fuzzy matching, decision D6: a wrong fuzzy hit on an item name is worse than a miss.
    ///
    /// THE DROP TABLE IS NOT SEARCHED. It has no screen to land on any more; see [`HitKind`].
    pub fn search(&self, q: &str) -> Vec<Hit> {
        let (kind, needle) = parse_query(q);
        let mut out = Vec::new();
        if needle.is_empty() {
            return out;
        }
        let wants = |k: HitKind| match kind {
            None => true,
            Some(w) => w == k,
        };
        if wants(HitKind::Item) {
            for it in &self.items {
                if out.len() >= SEARCH_CAP {
                    return out;
                }
                if it.matches(&needle) {
                    out.push(it.hit());
                }
            }
        }
        if wants(HitKind::Zone) {
            for z in &self.zones {
                if out.len() >= SEARCH_CAP {
                    return out;
                }
                if z.matches(&needle) {
                    out.push(z.hit());
                }
            }
        }
        if wants(HitKind::Quest) {
            for qu in &self.quests {
                if out.len() >= SEARCH_CAP {
                    return out;
                }
                if qu.matches(&needle) {
                    out.push(qu.hit());
                }
            }
        }
        if wants(HitKind::Spell) {
            for sp in &self.spells {
                if out.len() >= SEARCH_CAP {
                    return out;
                }
                if sp.matches(&needle) {
                    out.push(sp.hit());
                }
            }
        }
        if wants(HitKind::SkyItem) {
            for it in &self.sky.items {
                if out.len() >= SEARCH_CAP {
                    return out;
                }
                if it.matches(&needle) {
                    out.push(Hit {
                        kind: HitKind::SkyItem,
                        name: it.name.clone(),
                        detail: self.sky_detail(it),
                    });
                }
            }
        }
        out
    }

    /// One computed line for a sky item: its own drop table (the item window's `src`), the isle
    /// rosters that name it, and the slots it stands in for. Says so when the file has none.
    fn sky_detail(&self, it: &sky::SkyItem) -> String {
        let mut parts: Vec<String> = Vec::new();
        let rows = it.drop_rows();
        if !rows.is_empty() {
            let mut mobs: Vec<&str> = Vec::new();
            for r in &rows {
                if !mobs.contains(&r.mob) {
                    mobs.push(r.mob);
                }
            }
            let shown: Vec<&str> = mobs.iter().take(3).copied().collect();
            let more = mobs.len().saturating_sub(shown.len());
            parts.push(if more > 0 {
                format!("drops from {} (+{more} more)", shown.join(", "))
            } else {
                format!("drops from {}", shown.join(", "))
            });
        }
        let on = self.sky.dropped_on(&it.name);
        if !on.is_empty() {
            parts.push(on.join("; "));
        }
        let alts = self.sky.alt_slots(&it.name);
        if !alts.is_empty() {
            parts.push(format!("stands in for {}", alts.join(", ")));
        }
        if parts.is_empty() {
            "sky.json item window with no drop table, isle roster or slot alternative".to_owned()
        } else {
            parts.join(" · ")
        }
    }

    /// An item by name, exact after case folding. Accepts the display spelling or the map key.
    pub fn item(&self, name: &str) -> Option<&Item> {
        self.item_index.get(&fold(name)).map(|&i| &self.items[i])
    }

    /// A zone by short key ("befallen"), tracker name ("Befallen") or wiki title, case folded.
    pub fn zone(&self, key_or_name: &str) -> Option<&Zone> {
        self.zone_index
            .get(&fold(key_or_name))
            .map(|&i| &self.zones[i])
    }

    /// Who drops an item, by item name, case folded. None means the wiki lists no droppers, which
    /// is different from the item not existing; check [`Snapshot::item`] for that.
    pub fn drop_sources(&self, item: &str) -> Option<&Drop> {
        self.drop_index.get(&fold(item)).map(|&i| &self.drops[i])
    }

    /// A spell by name or by map key, exact after case folding.
    pub fn spell(&self, name: &str) -> Option<&Spell> {
        self.spell_index.get(&fold(name)).map(|&i| &self.spells[i])
    }

    /// The wiki's stat block for an item, by name, case folded.
    pub fn tooltip(&self, name: &str) -> Option<&Tooltip> {
        self.tooltip_index
            .get(&fold(name))
            .map(|&i| &self.tooltips[i])
    }

    /// A faction by name or by map key, exact after case folding. None when the wiki has no page
    /// under that name, which is what a zone or merchant line naming an unknown faction gets, and
    /// is why those lines are drawn as text rather than as a broken link.
    pub fn faction(&self, name: &str) -> Option<&Faction> {
        self.faction_index
            .get(&fold(name))
            .map(|&i| &self.factions[i])
    }

    /// Every merchant selling this item, by item name, case folded. Empty means the wiki lists no
    /// vendor, which is different from the item not existing.
    ///
    /// THE SIBLING OF [`Snapshot::drop_sources`]. That answers who drops it; this answers who
    /// sells it, and the Items detail pane draws them one under the other.
    pub fn sellers_of(&self, item: &str) -> &[merchants::Posting] {
        self.sellers.sellers_of(item)
    }

    /// Resolve a posting into the merchant and the one line of its stock list that matched. None
    /// only if a posting were ever built against a different vector than the one it is read from,
    /// which `Snapshot::load` makes impossible; the Option is here so a caller cannot panic on it.
    pub fn sale(&self, p: merchants::Posting) -> Option<(&Merchant, merchants::Sale<'_>)> {
        let m = self.merchants.get(p.merchant as usize)?;
        Some((m, m.sale_at(p.line as usize)?))
    }

    /// The merchants standing in a zone. Asked with the zone's key, tracker name or wiki title;
    /// all three are tried, because merchants.json spells zones its own way and neither side gets
    /// an invented alias table.
    pub fn merchants_in(&self, zone: &Zone) -> Vec<&Merchant> {
        let mut idx: Vec<u32> = Vec::new();
        let mut names: Vec<String> = vec![fold(&zone.key), fold(&zone.name)];
        if let Some(w) = &zone.wiki_title {
            names.push(fold(w));
        }
        names.sort();
        names.dedup();
        for n in names {
            if let Some(v) = self.merchants_by_zone.get(&n) {
                for i in v {
                    if !idx.contains(i) {
                        idx.push(*i);
                    }
                }
            }
        }
        idx.into_iter()
            .map(|i| &self.merchants[i as usize])
            .collect()
    }

    /// Merchants whose `zone` string matches no zone in this snapshot, under any of the three
    /// spellings [`Snapshot::merchants_in`] tries.
    ///
    /// WHY THIS EXISTS AT ALL. Zone detail is the only home merchants got, so a merchant filed
    /// under a zone with no page is a record the app holds and never draws. Rather than let that
    /// be silent, the SOURCES ledger counts them and names them. Measured on the shipped file: 9
    /// merchants over 7 zone strings, every one of them the wiki's own variant spelling ("Kerra
    /// Isle" where the atlas has "Kerra Island", "West Karana" and "Western Karana" where it has
    /// "Western Plains of Karana", "North Felwithe" where it has "Northern Felwithe"). An alias
    /// table would place all nine and would be seven guesses; the ledger naming them is the honest
    /// version, and it is the thing a person can act on by fixing the wiki or the puller.
    pub fn merchants_without_a_zone_page(&self) -> Vec<&Merchant> {
        let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();
        for z in &self.zones {
            known.insert(fold(&z.key));
            known.insert(fold(&z.name));
            if let Some(w) = &z.wiki_title {
                known.insert(fold(w));
            }
        }
        self.merchants
            .iter()
            .filter(|m| !m.zone_pieces().iter().any(|p| known.contains(&fold(p))))
            .collect()
    }

    /// The factions a zone moves, and which way. Asked the same three ways as
    /// [`Snapshot::merchants_in`], and sorted by faction name so the pane is stable frame to frame.
    pub fn factions_of_zone(&self, zone: &Zone) -> Vec<factions::Move<'_>> {
        let mut names: Vec<String> = vec![fold(&zone.key), fold(&zone.name)];
        if let Some(w) = &zone.wiki_title {
            names.push(fold(w));
        }
        names.sort();
        names.dedup();
        let mut out: Vec<(u32, factions::Way)> = Vec::new();
        for n in names {
            if let Some(v) = self.factions_by_zone.get(&n) {
                for e in v {
                    if !out.contains(e) {
                        out.push(*e);
                    }
                }
            }
        }
        self.moves(out)
    }

    /// The factions a quest moves, and which way, by the quest's title, case folded.
    pub fn factions_of_quest(&self, quest: &str) -> Vec<factions::Move<'_>> {
        let out = self
            .factions_by_quest
            .get(&fold(quest))
            .cloned()
            .unwrap_or_default();
        self.moves(out)
    }

    fn moves(&self, mut raw: Vec<(u32, factions::Way)>) -> Vec<factions::Move<'_>> {
        raw.sort_by(|a, b| {
            self.factions[a.0 as usize]
                .name
                .cmp(&self.factions[b.0 as usize].name)
                .then_with(|| a.1.label().cmp(b.1.label()))
        });
        raw.into_iter()
            .map(|(i, way)| factions::Move {
                faction: &self.factions[i as usize],
                way,
            })
            .collect()
    }
}

/* ------------------------------------------------------------------ the tests -- */

/// Shared access to the real snapshot for tests, in this module and its children.
///
/// A TEST THAT NEEDS THE DATA AND FINDS NONE FAILS. It does not skip, because a suite that goes
/// green on an empty machine certifies nothing. The one way out is explicit: GRIMOIRE_NO_DATA=1,
/// which prints SKIPPED ON PURPOSE so the log shows a decision rather than an absence.
#[cfg(test)]
pub(crate) mod testdata {
    use super::*;
    use std::sync::OnceLock;

    pub const OPT_OUT: &str = "GRIMOIRE_NO_DATA";

    /// Say SKIPPED ON PURPOSE where the operator can see it: on the process's real stderr, NOT
    /// through `eprintln!`. The test harness captures the print macros of a passing test and
    /// shows them only under `--nocapture`, so a marker printed that way is invisible under the
    /// exact command the goal names (`GRIMOIRE_NO_DATA=1 cargo test -p grimoire-desktop`), which
    /// makes the opt-out look like the silent skip the goal forbids. A direct write to the
    /// stderr handle is not captured; it lands on the console in the run's output.
    pub fn say_skipped(why: &str) {
        use std::io::Write;
        let line = format!("SKIPPED ON PURPOSE: {why}\n");
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(line.as_bytes());
        let _ = err.flush();
    }

    /// The snapshot root, or None only when the operator opted out explicitly.
    pub fn root() -> Option<PathBuf> {
        if std::env::var(OPT_OUT).map(|v| v == "1").unwrap_or(false) {
            say_skipped(&format!(
                "{OPT_OUT}=1, this test reads the real snapshot and was told not to"
            ));
            return None;
        }
        match Snapshot::locate() {
            Some(p) => Some(p),
            None => {
                let tried: Vec<String> = candidates()
                    .iter()
                    .map(|p| format!("  {}", p.display()))
                    .collect();
                panic!(
                    "the real data files are absent and this test refuses to pass on nothing.\n\
                     Put the five JSON files plus {ATLAS_DIR}/ at one of:\n{}\n\
                     or set {OPT_OUT}=1 to skip on purpose.",
                    tried.join("\n")
                )
            }
        }
    }

    static SHARED: OnceLock<Snapshot> = OnceLock::new();

    /// The real snapshot, loaded once per test binary. None only under the opt out.
    pub fn snapshot() -> Option<&'static Snapshot> {
        let root = root()?;
        Some(SHARED.get_or_init(|| {
            Snapshot::load(&root)
                .unwrap_or_else(|e| panic!("the real snapshot failed to load: {e}"))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /* ---- no data needed ---- */

    #[test]
    fn prefix_parsing_restricts_kind_and_folds_the_rest() {
        assert_eq!(parse_query("i:Bone"), (Some(HitKind::Item), "bone".into()));
        assert_eq!(
            parse_query("Z: Befal "),
            (Some(HitKind::Zone), "befal".into())
        );
        assert_eq!(parse_query("q:ring"), (Some(HitKind::Quest), "ring".into()));
        assert_eq!(parse_query("p:gate"), (Some(HitKind::Spell), "gate".into()));
        /* `d:` was the drop table's and the drop table has no screen to land on any more, so it
         * is an ordinary letter again and the whole string is the needle. */
        assert_eq!(parse_query("d:crown"), (None, "d:crown".into()));
        assert_eq!(
            parse_query("  Bone Necklace "),
            (None, "bone necklace".into())
        );
        /* an unknown prefix letter is part of the needle, not a filter */
        assert_eq!(parse_query("x:thing"), (None, "x:thing".into()));
        /* a colon deeper in the string is not a prefix */
        assert_eq!(parse_query("ring: of x"), (None, "ring: of x".into()));
        assert_eq!(parse_query("i:"), (Some(HitKind::Item), String::new()));
    }

    #[test]
    fn every_kind_has_a_distinct_prefix() {
        let mut seen = std::collections::HashSet::new();
        for k in HitKind::ALL {
            assert!(seen.insert(k.prefix()), "{:?} shares a prefix", k);
            assert_eq!(HitKind::from_prefix(k.prefix()), Some(k));
            assert_eq!(
                HitKind::from_prefix(k.prefix().to_ascii_uppercase()),
                Some(k)
            );
        }
    }

    /// The integrator loads on a background thread and hands the result to the UI thread, and the
    /// screens borrow it through `Cx` for the life of a frame. Both need `Snapshot: Send`, and the
    /// shared borrow across egui's viewport callbacks wants `Sync`. This fails to COMPILE, not at
    /// runtime, the day a field (an `Rc`, a `Cell`) breaks either, so the handoff can never
    /// silently turn into a per-frame clone.
    #[test]
    fn snapshot_crosses_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Snapshot>();
        assert_send_sync::<DataReport>();
        assert_send_sync::<DataError>();
        assert_send_sync::<Hit>();
    }

    #[test]
    fn a_missing_root_is_an_error_naming_the_root() {
        let p = PathBuf::from("Z:/grimoire/definitely/not/here");
        let e = Snapshot::load(&p).unwrap_err();
        assert_eq!(e.path, p);
        assert!(e.reason.contains("not a directory"), "{e}");
    }

    #[test]
    fn malformed_json_is_an_error_naming_the_file_and_line() {
        let dir = std::env::temp_dir().join(format!("grimoire-data-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(GEAR_FILE),
            "{ \"schema\": 2, \"items\": { \"x\": [ }",
        )
        .unwrap();
        let e = Snapshot::load(&dir).unwrap_err();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(e.path.ends_with(GEAR_FILE), "{e}");
        assert!(
            e.reason.contains("line"),
            "serde's reason should carry the position: {e}"
        );
    }

    #[test]
    fn a_root_missing_one_file_names_that_file() {
        let dir =
            std::env::temp_dir().join(format!("grimoire-data-partial-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        /* a valid but tiny gear file, and nothing else */
        std::fs::write(
            dir.join(GEAR_FILE),
            "{\"schema\":2,\"items\":{\"a thing\":{\"n\":\"A Thing\",\"sl\":[],\"t\":\"A_Thing\"}}}",
        )
        .unwrap();
        let e = Snapshot::load(&dir).unwrap_err();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            e.path.ends_with(TOOLTIPS_FILE),
            "the next file in load order is named: {e}"
        );
        assert!(e.reason.contains("cannot read"), "{e}");
    }

    /// THE PROBE ORDER, AND THAT THE SOURCE TREE COPY IS THE LAST WORD RATHER THAN THE FIRST.
    ///
    /// The shipped layout is asked first so an installed app answers out of its own directory
    /// without consulting anything about the reader's machine; the per-user folder is the place
    /// he can always write to; the checkout's copy is the convenience that makes `cargo run`
    /// work, and it comes last so it can never shadow a real install.
    #[test]
    fn candidates_probe_the_shipped_layout_first_and_the_source_copy_last() {
        let c = candidates_labelled();
        assert!(c.len() >= 2, "{c:?}");
        assert_eq!(c[0].1, EXE_SIBLING);
        assert!(
            c[0].0.ends_with("data"),
            "the first candidate is data/ beside the exe: {}",
            c[0].0.display()
        );
        /* THE UPDATER'S FOLDER COMES BEFORE THE SETTINGS FOLDER, because the updater is what
         * installs a snapshot and a version it just delivered must not be shadowed by whatever a
         * reader once dropped next to settings.json. */
        if let Some(u) = updater_data_dir() {
            assert_eq!(c[1], (u, UPDATER_FOLDER));
        }
        if let Some(u) = user_data_dir() {
            assert_eq!(c[2], (u, USER_FOLDER));
        }
        /* This is a test binary, so the gated entry IS compiled in and is last. */
        assert_eq!(c.last().unwrap().1, SOURCE_TREE);
        assert_eq!(
            c.last().unwrap().0,
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data")
        );
        /* The path-only list the screens call is the same list, in the same order. */
        assert_eq!(
            candidates(),
            c.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>()
        );
    }

    /// NOTHING A SHIPPED BUILD PROBES CAME FROM THE MACHINE THAT COMPILED IT.
    ///
    /// This module carried two absolute paths as `const &str` and pushed both onto the probe
    /// list. One of them was a checkout on one developer's disk. A release binary therefore
    /// probed directories that exist on exactly one machine and, worse, PRINTED them: the empty
    /// state on every FIND screen lists this function's output as "put the data here", so a
    /// stranger's install told him to go to a folder he could not have.
    ///
    /// The rule that replaced them is that every entry a shipped build probes is derived when
    /// the function runs, and this test is that rule stated as an assertion. It reads
    /// `runtime_candidates` rather than the public list precisely so it needs no exemption: the
    /// one compile time entry is not in this set at all, in any profile.
    #[test]
    fn no_release_candidate_is_a_compile_time_absolute_path() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf));
        let user = user_data_dir();
        let updater = updater_data_dir();
        let set = runtime_candidates();
        assert!(!set.is_empty(), "a build with no candidates finds nothing");
        for (p, label) in &set {
            assert!(
                !p.starts_with(manifest),
                "{label}: {} is under the manifest directory this build was compiled from",
                p.display()
            );
            /* THE UPDATER'S FOLDER IS A THIRD LEGITIMATE ROOT, and it is named here rather than
             * the check being loosened: it comes from `dirs::data_local_dir()` at run time, the
             * same way the other two come from `current_exe` and `dirs::config_dir()`. What this
             * test refuses is a path that could only have been typed into the source, which is
             * what `C:\Repos\...` and `C:\Users\revii\...` once were here. */
            let derived_at_run_time = exe_dir.as_deref().is_some_and(|d| p.starts_with(d))
                || user.as_ref() == Some(p)
                || updater.as_ref() == Some(p);
            assert!(
                derived_at_run_time,
                "{label}: {} is rooted in neither the running executable's folder nor the \
                 per-user folder, so it can only have come from a literal in the source",
                p.display()
            );
        }
        /* And the public list differs from it by exactly the gated source tree entry, which is
         * what lets the loop above stand for what a release build does. */
        let public = candidates_labelled();
        let extra: Vec<&'static str> = public
            .iter()
            .map(|(_, l)| *l)
            .filter(|l| !set.iter().any(|(_, s)| s == l))
            .collect();
        assert_eq!(extra, vec![SOURCE_TREE], "{public:?}");
    }

    /* ---- the real files ---- */

    #[test]
    fn locate_finds_the_snapshot() {
        let Some(root) = testdata::root() else { return };
        assert!(root.join(GEAR_FILE).is_file());
        for f in FILES {
            assert!(
                root.join(f).is_file(),
                "{f} missing under {}",
                root.display()
            );
        }
        assert!(root.join(ATLAS_DIR).is_dir());
    }

    #[test]
    fn counts_clear_the_floor_the_goal_sets() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let r = s.report();
        assert!(r.items > 6000, "items {}", r.items);
        assert!(r.zones > 70, "zones {}", r.zones);
        assert!(r.drops > 1500, "drops {}", r.drops);
        assert!(r.sky_items > 3000, "sky items {}", r.sky_items);
        assert!(r.quests > 500, "quests {}", r.quests);
        assert!(s.tooltips.len() > 10000, "tooltips {}", s.tooltips.len());
        assert_eq!(r.root, s.root);
    }

    #[test]
    fn report_counts_what_is_actually_in_the_vectors() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let r = s.report();
        assert_eq!(r.items, s.items.len());
        assert_eq!(r.zones, s.zones.len());
        assert_eq!(r.drops, s.drops.len());
        assert_eq!(r.sky_items, s.sky.items.len());
        assert_eq!(r.quests, s.quests.len());
    }

    #[test]
    fn flatten_keeps_the_fields_this_build_did_not_type() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let first = &s.items[0];
        assert!(
            !first.extra.is_empty(),
            "extra is empty on {}: flatten is not keeping unknown fields",
            first.name
        );
        /* cls and rc are measured on 6890 of 6891 records and deliberately left untyped; see
         * items.rs. If this stops holding, flatten stopped working or the file changed shape. */
        assert!(
            first.extra.contains_key("cls"),
            "{:?}",
            first.extra.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn i_bone_finds_a_bone_necklace() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let hits = s.search("i:bone");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.kind == HitKind::Item));
        assert!(
            hits.iter().any(|h| h.name == "A Bone Necklace"),
            "{:?}",
            hits.iter().map(|h| &h.name).take(10).collect::<Vec<_>>()
        );
        /* the detail line is computed, never blank */
        let h = hits.iter().find(|h| h.name == "A Bone Necklace").unwrap();
        assert!(h.detail.contains("Neck"), "{}", h.detail);
        assert!(h.detail.contains("Droga"), "{}", h.detail);
    }

    #[test]
    fn z_befal_finds_befallen() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let hits = s.search("z:befal");
        assert!(hits.iter().all(|h| h.kind == HitKind::Zone));
        assert!(hits.iter().any(|h| h.name == "Befallen"), "{hits:?}");
    }

    /// THE q PREFIX RESTRICTS TO QUESTS, and no prefix at all still reaches every kind the
    /// finder offers.
    ///
    /// IT USED TO BE `d_and_q_prefixes_restrict_to_drops_and_quests` and the `d:` half is not
    /// merely deleted: the drop table has left the search, so what took its place is the positive
    /// claim that a bare needle is answered by the kinds that ARE searched. Asserting that a drop
    /// hit no longer comes back would be asserting the absence of something now absent by
    /// construction, which is green whatever anyone does to `search`.
    #[test]
    fn the_q_prefix_restricts_to_quests_and_a_bare_needle_reaches_every_kind() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        /* A NEEDLE MORE THAN ONE KIND ANSWERS, FOUND RATHER THAN GUESSED. "q:coldain ring" was
         * here and it was not a discriminating case: nothing but a quest is named anything like
         * "coldain ring", so a `wants` that had stopped restricting at all still returned quests
         * only and the assertion below held. Measured, that exact mutation did not fail. The
         * needle is now chosen for the property the test needs, and the search is asked which
         * needle has it. */
        let needle = ["bone", "ring", "crown", "shield", "gate", "wolf"]
            .into_iter()
            .find(|n| {
                let all = s.search(n);
                all.iter().any(|h| h.kind == HitKind::Quest)
                    && all.iter().any(|h| h.kind != HitKind::Quest)
            })
            .expect(
                "no candidate needle is answered by quests AND by something else, so a prefix has \
                 nothing to restrict here and this test would prove nothing",
            );
        let q = s.search(&format!("q:{needle}"));
        assert!(!q.is_empty(), "q:{needle} found nothing");
        assert!(
            q.iter().all(|h| h.kind == HitKind::Quest),
            "q:{needle} let another kind through: {:?}",
            q.iter()
                .filter(|h| h.kind != HitKind::Quest)
                .map(|h| (h.kind, &h.name))
                .take(6)
                .collect::<Vec<_>>()
        );

        /* Every kind the finder offers is reachable without a prefix. One needle cannot hit all
         * five, so each is asked for by a needle its own list is known to answer, with no prefix
         * on it, and the KIND that comes back is what is asserted. */
        for (needle, want) in [
            ("bone necklace", HitKind::Item),
            ("befallen", HitKind::Zone),
            ("coldain ring", HitKind::Quest),
            ("crude stein", HitKind::SkyItem),
            ("gate", HitKind::Spell),
        ] {
            let hits = s.search(needle);
            assert!(
                hits.iter().any(|h| h.kind == want),
                "a bare \"{needle}\" found no {want:?}: {:?}",
                hits.iter()
                    .map(|h| (h.kind, &h.name))
                    .take(6)
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn s_prefix_finds_a_sky_item_with_a_computed_detail() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let hits = s.search("s:crude stein");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.kind == HitKind::SkyItem));
        let h = hits
            .iter()
            .find(|h| h.name == "A Crude Stein")
            .expect("the stein is a sky item");
        assert!(!h.detail.is_empty());
        /* a piece the isle rosters name: the isle and the island's name come through `dropped_on` */
        let named = s
            .sky
            .isles
            .iter()
            .flat_map(|i| i.mobs.iter().flat_map(|m| m.drops.iter()))
            .next()
            .expect("some isle roster lists a drop")
            .clone();
        let on = s.sky.dropped_on(&named);
        assert!(!on.is_empty(), "{named}");
        assert!(on[0].contains("(I"), "{}", on[0]);
        assert!(s.sky.dropped_on("no such piece, ever").is_empty());
        /* the alts table answers by item name */
        let (slot, first) = s
            .sky
            .alts
            .iter()
            .find_map(|(slot, v)| v.first().map(|n| (slot.clone(), n.clone())))
            .expect("alts has an entry");
        assert!(s.sky.alt_slots(&first).contains(&slot.as_str()));
    }

    #[test]
    fn no_prefix_searches_every_kind_and_folds_case() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let hits = s.search("BEFALLEN");
        assert!(hits
            .iter()
            .any(|h| h.kind == HitKind::Zone && h.name == "Befallen"));
        let mixed = s.search("bone");
        let kinds: std::collections::HashSet<HitKind> = mixed.iter().map(|h| h.kind).collect();
        assert!(
            kinds.len() >= 2,
            "expected more than one kind for 'bone': {kinds:?}"
        );
    }

    #[test]
    fn search_caps_at_two_hundred_and_empty_needle_returns_nothing() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        assert_eq!(s.search("i:a").len(), SEARCH_CAP);
        assert_eq!(s.search("a").len(), SEARCH_CAP);
        assert!(s.search("").is_empty());
        assert!(s.search("i:").is_empty());
        assert!(s.search("   ").is_empty());
    }

    #[test]
    fn exact_lookups_accept_key_or_display_spelling() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        assert_eq!(
            s.item("A Bone Necklace").map(|i| i.key.as_str()),
            Some("a bone necklace")
        );
        assert_eq!(
            s.item("a bone necklace").map(|i| i.name.as_str()),
            Some("A Bone Necklace")
        );
        assert!(s.item("no such item, ever").is_none());
        assert_eq!(
            s.zone("befallen").map(|z| z.name.as_str()),
            Some("Befallen")
        );
        assert_eq!(s.zone("Befallen").map(|z| z.key.as_str()), Some("befallen"));
        assert_eq!(
            s.zone("Plane of Sky").map(|z| z.key.as_str()),
            Some("airplane")
        );
        assert!(s.drop_sources("A Black Crown").is_some());
        assert!(s.tooltip("10 Dose Adrenaline Tap").is_some());
    }

    /// THE CROSS LINKS, ON THE REAL FILES. These four lookups are the whole reason factions.json
    /// and merchants.json are loaded at all, so the numbers behind them are asserted rather than
    /// described: a wiring that quietly resolved nothing would still compile, still draw an empty
    /// section, and still look like the wiki simply had nothing to say.
    #[test]
    fn the_zone_and_quest_cross_links_actually_resolve() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let placed: usize = s.zones.iter().map(|z| s.merchants_in(z).len()).sum();
        let with_merchants = s
            .zones
            .iter()
            .filter(|z| !s.merchants_in(z).is_empty())
            .count();
        let with_factions = s
            .zones
            .iter()
            .filter(|z| !s.factions_of_zone(z).is_empty())
            .count();
        let quests_moved = s
            .quests
            .iter()
            .filter(|q| !s.factions_of_quest(&q.name).is_empty())
            .count();
        eprintln!(
            "cross links: {placed} merchant placements over {with_merchants} zones, \
             {with_factions} zones name a faction, {quests_moved} quests move one, \
             {} merchants have no zone page",
            s.merchants_without_a_zone_page().len()
        );
        /* Pinned, not floored. A floor would stay green while a rewrite quietly halved the joins,
         * and these numbers ARE the feature: if one of them moves, either the data changed or the
         * matching did, and both are worth a person looking. */
        assert_eq!(
            with_merchants, 77,
            "zones with at least one merchant, of 122"
        );
        assert_eq!(
            placed, 1368,
            "merchant placements: more than the 1364 placed merchants, because four of them name \
             two zones and land on both pages"
        );
        assert_eq!(with_factions, 117, "zones a faction names");
        assert_eq!(quests_moved, 446, "quests a faction names, of 924");

        /* the worked example, both directions, on records the goal named */
        let lfay = s.zone("Lesser Faydark").expect("Lesser Faydark");
        let here = s.merchants_in(lfay);
        assert!(
            here.iter().any(|m| m.name == "a brownie merchant"),
            "{:?}",
            here.iter().map(|m| &m.name).take(8).collect::<Vec<_>>()
        );
        let cazic = s.zone("Cazic Thule").expect("Cazic Thule");
        let moves = s.factions_of_zone(cazic);
        assert!(
            moves.iter().any(|m| m.faction.name == "Allize Taeew"),
            "{:?}",
            moves
                .iter()
                .map(|m| (&m.faction.name, m.way))
                .take(8)
                .collect::<Vec<_>>()
        );
        /* a faction is listed once per direction and never twice in one */
        for m in &moves {
            let same = moves
                .iter()
                .filter(|o| o.faction.key == m.faction.key && o.way == m.way)
                .count();
            assert_eq!(
                same, 1,
                "{} {:?} appears {same} times",
                m.faction.name, m.way
            );
        }
    }

    /// EVERY MERCHANT LANDS ON A ZONE PAGE OR IS COUNTED AS ONE THAT DOES NOT. Zone detail is the
    /// only home merchants got, so the two sets have to add up to the file; anything else is a
    /// record the app holds and never draws, which is the defect this whole lane is about.
    #[test]
    fn no_merchant_is_both_unplaced_and_unaccounted_for() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let orphans = s.merchants_without_a_zone_page();
        let mut placed: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for z in &s.zones {
            for m in s.merchants_in(z) {
                placed.insert(m.key.as_str());
            }
        }
        assert_eq!(
            placed.len() + orphans.len(),
            s.merchants.len(),
            "{} placed plus {} orphans should be {}",
            placed.len(),
            orphans.len(),
            s.merchants.len()
        );
        for o in &orphans {
            assert!(
                !placed.contains(o.key.as_str()),
                "{} is in both sets",
                o.key
            );
        }
        eprintln!(
            "unplaced merchants: {:?}",
            orphans
                .iter()
                .map(|m| (&m.name, &m.zone))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn load_is_fast_enough() {
        let Some(root) = testdata::root() else { return };
        let t0 = Instant::now();
        let s = Snapshot::load(&root).expect("loads");
        let took = t0.elapsed();
        eprintln!(
            "snapshot load: {took:?} ({} items, {} tooltips, {} zones, {} drops, {} sky items, {} quests, {} spells) profile={}",
            s.items.len(),
            s.tooltips.len(),
            s.zones.len(),
            s.drops.len(),
            s.sky.items.len(),
            s.quests.len(),
            s.spells.len(),
            if cfg!(debug_assertions) { "dev" } else { "release" }
        );
        assert!(s.load_time <= took);
        /* The budget is on the shipped binary. A dev build is several times slower on serde and is
         * not what a user runs, so it reports the number and does not fail on it. */
        if !cfg!(debug_assertions) {
            assert!(
                took < LOAD_BUDGET,
                "load took {took:?}, budget {LOAD_BUDGET:?}"
            );
        }
    }
}


#[cfg(test)]
mod updater_agreement {
    /// DEFECT: THE APP LOOKING SOMEWHERE THE UPDATER NEVER WRITES.
    ///
    /// Both modules had a notion of "the per-user data folder" and they were different folders.
    /// `updater::install::Layout::platform` roots at `dirs::data_local_dir()`; this module's
    /// `user_data_dir` used `dirs::config_dir()`, which on Windows is Roaming. So the updater
    /// could install a snapshot correctly, report success correctly, and the next launch would
    /// still say `No snapshot found` while listing two paths, neither of which was the one the
    /// bundle was in. It reached a reader on a version he had done nothing to but let update.
    ///
    /// WHAT MUTATION MAKES THIS RED: drop the `updater_data_dir()` entry from `runtime_candidates`,
    /// or point either side at a different `dirs::` function. Nothing else in the suite compares
    /// the two, because until now nothing knew they were supposed to match.
    #[test]
    fn the_app_searches_the_folder_the_updater_installs_into() {
        let Some(updater) = crate::updater::install::Layout::platform() else {
            /* A platform with no local data directory has no agreement to keep. */
            return;
        };
        let wanted = updater.data_dir();
        let searched = super::candidates();
        assert!(
            searched.contains(&wanted),
            "the updater installs snapshots into {} and the app never looks there; it searches {:?}",
            wanted.display(),
            searched
        );
    }
}
