//! THE MONSTERS & MEMORIES WIKI BUNDLE: zones, NPCs, items, quests and spells, read from disk.
//!
//! The bundle is built outside this repository (mnm-lore) from the Monsters & Memories community wiki,
//! licensed CC BY-SA 4.0, and delivered as data, never compiled in. Every record keeps the URL of the
//! page it came from, and every screen that shows a record shows that credit.
//!
//! RECORDS ARE KEPT AS THE JSON THEY ARRIVED AS. Each kind has a handful of fields that most records
//! carry and a long tail that some do (the wiki's own info boxes), and the shapes vary: a drop list is
//! an object, a slot is a list, stats are a map. A typed struct per kind would either drop the tail or
//! fail on the record that disagrees; a map keeps everything and the screens choose what to draw.
//!
//! WHERE IT IS LOOKED FOR, in order: `GRIMOIRE_LORE` (a folder, for development), the updater's data
//! folder (where an update delivers it), and `mnm-corpus` beside the executable. Nothing tied to one
//! machine.

use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;

/// The file every bundle carries; its presence is what makes a folder a bundle.
pub const META_FILE: &str = "meta.json";

/// The five kinds the Compendium browses.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Zone,
    Npc,
    Item,
    Quest,
    Spell,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Zone, Kind::Npc, Kind::Item, Kind::Quest, Kind::Spell];

    /// The bundle file holding this kind.
    pub fn file(self) -> &'static str {
        match self {
            Kind::Zone => "zones.json",
            Kind::Npc => "npcs.json",
            Kind::Item => "items.json",
            Kind::Quest => "quests.json",
            Kind::Spell => "spells.json",
        }
    }

    /// What one of these is called, for headings and empty states.
    pub fn noun(self) -> &'static str {
        match self {
            Kind::Zone => "zone",
            Kind::Npc => "NPC",
            Kind::Item => "item",
            Kind::Quest => "quest",
            Kind::Spell => "spell",
        }
    }

    pub fn plural(self) -> &'static str {
        match self {
            Kind::Zone => "zones",
            Kind::Npc => "NPCs",
            Kind::Item => "items",
            Kind::Quest => "quests",
            Kind::Spell => "spells",
        }
    }
}

/// One record: its name, its source page, and everything else the bundle said about it.
#[derive(Clone, Debug)]
pub struct Record {
    pub name: String,
    /// The wiki page it came from; the credit every screen shows.
    pub url: Option<String>,
    pub fields: Map<String, Value>,
    /// Lower-cased name, for search.
    pub fold: String,
}

impl Record {
    fn from_value(v: Value) -> Option<Record> {
        let Value::Object(fields) = v else {
            return None;
        };
        let name = fields.get("name").and_then(Value::as_str)?.to_owned();
        let url = fields.get("url").and_then(Value::as_str).map(str::to_owned);
        Some(Record {
            fold: name.to_lowercase(),
            name,
            url,
            fields,
        })
    }

    /// A field as display text: strings as they are, numbers trimmed, lists joined, objects as
    /// `key value` pairs. None when absent or empty.
    pub fn text(&self, key: &str) -> Option<String> {
        self.fields.get(key).and_then(value_text)
    }

    /// A field as a list of strings (a list stays a list; a single string is a list of one).
    pub fn list(&self, key: &str) -> Vec<String> {
        match self.fields.get(key) {
            Some(Value::Array(a)) => a.iter().filter_map(value_text).collect(),
            Some(v) => value_text(v).into_iter().collect(),
            None => Vec::new(),
        }
    }
}

/// Any JSON value as one line of text, or None when it says nothing.
pub fn value_text(v: &Value) -> Option<String> {
    let s = match v {
        Value::Null => return None,
        Value::Bool(b) => if *b { "yes" } else { "no" }.to_owned(),
        Value::Number(n) => {
            let f = n.as_f64().unwrap_or(0.0);
            if f.fract() == 0.0 {
                format!("{}", f as i64)
            } else {
                format!("{f}")
            }
        }
        Value::String(s) => s.trim().to_owned(),
        Value::Array(a) => a
            .iter()
            .filter_map(value_text)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(o) => o
            .iter()
            .filter_map(|(k, v)| value_text(v).map(|t| format!("{} {t}", k.replace('_', " "))))
            .collect::<Vec<_>>()
            .join(", "),
    };
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// The loaded bundle.
pub struct Lore {
    pub root: PathBuf,
    pub data_version: String,
    pub licence: String,
    pub source: String,
    kinds: HashMap<Kind, Vec<Record>>,
    /// lower-cased name, or redirect, to (kind, index). The first kind to claim a name keeps it, in
    /// `Kind::ALL` order, so a zone and an NPC sharing a name resolve to the zone.
    index: HashMap<String, (Kind, usize)>,
}

impl Lore {
    pub fn records(&self, kind: Kind) -> &[Record] {
        self.kinds.get(&kind).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.records(kind).len()
    }

    /// Where a name points, if anywhere: exact, case-insensitive, then through the wiki's redirects.
    pub fn find(&self, name: &str) -> Option<(Kind, usize)> {
        self.index.get(&name.trim().to_lowercase()).copied()
    }

    /// Where a name points within one kind (an item named in a drop list is an item).
    pub fn find_in(&self, kind: Kind, name: &str) -> Option<usize> {
        let want = name.trim().to_lowercase();
        if let Some((k, i)) = self.index.get(&want) {
            if *k == kind {
                return Some(*i);
            }
        }
        self.records(kind).iter().position(|r| r.fold == want)
    }

    /// Read a bundle folder. Every kind's file must be there; a missing or malformed one is an error
    /// naming the file, never a half-loaded Compendium.
    pub fn load(root: &Path) -> Result<Lore, String> {
        let read = |file: &str| -> Result<Value, String> {
            let p = root.join(file);
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            serde_json::from_str(&text).map_err(|e| format!("{}: not valid JSON: {e}", p.display()))
        };
        let meta = read(META_FILE)?;
        let mut kinds = HashMap::new();
        let mut index = HashMap::new();
        for kind in Kind::ALL {
            let Value::Array(rows) = read(kind.file())? else {
                return Err(format!("{}: not a list of records", kind.file()));
            };
            let records: Vec<Record> = rows.into_iter().filter_map(Record::from_value).collect();
            for (i, r) in records.iter().enumerate() {
                index.entry(r.fold.clone()).or_insert((kind, i));
                if let Some(page) = r.fields.get("page").and_then(Value::as_str) {
                    index.entry(page.to_lowercase()).or_insert((kind, i));
                }
            }
            kinds.insert(kind, records);
        }
        /* The wiki's redirects: alternative names for a page, resolved through the index. */
        if let Ok(Value::Array(rs)) = read("redirects.json") {
            for r in rs {
                let (Some(from), Some(to)) = (
                    r.get("from").and_then(Value::as_str),
                    r.get("to").and_then(Value::as_str),
                ) else {
                    continue;
                };
                if let Some(hit) = index.get(&to.to_lowercase()).copied() {
                    index.entry(from.to_lowercase()).or_insert(hit);
                }
            }
        }
        let s = |k: &str| {
            meta.pointer(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        Ok(Lore {
            root: root.to_path_buf(),
            data_version: s("/data_version"),
            licence: s("/source/licence"),
            source: s("/source/url"),
            kinds,
            index,
        })
    }
}

/// The folders a bundle is looked for in, in order.
pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dev) = std::env::var_os("GRIMOIRE_LORE") {
        out.push(PathBuf::from(dev));
    }
    if let Some(l) = crate::updater::install::Layout::platform() {
        out.push(l.data_dir());
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        out.push(dir.join("mnm-corpus"));
    }
    out
}

/// The first candidate that holds a bundle.
pub fn locate() -> Option<PathBuf> {
    candidates()
        .into_iter()
        .find(|d| d.join(META_FILE).is_file())
}

/// Where the bundle stands, for the screens and the rail.
pub enum Status {
    /// No folder holds a bundle; the words name where it was looked for.
    Absent(String),
    Loading(mpsc::Receiver<Result<Lore, String>>),
    Loaded(Arc<Lore>),
    Failed(String),
}

impl Status {
    /// Start loading in the background, so the first frame is not held up by a few megabytes of JSON.
    pub fn start() -> Status {
        let Some(root) = locate() else {
            let tried: Vec<String> = candidates()
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            return Status::Absent(format!(
                "The Monsters & Memories wiki data is not on this PC yet. Grimoire looked in: {}.",
                tried.join("; ")
            ));
        };
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("grimoire-lore".to_owned())
            .spawn(move || {
                let _ = tx.send(Lore::load(&root));
            });
        match spawned {
            Ok(_) => Status::Loading(rx),
            Err(e) => Status::Failed(format!("could not start the wiki data loader: {e}")),
        }
    }

    /// Collect a finished load. Cheap when nothing is pending.
    pub fn poll(&mut self) {
        if let Status::Loading(rx) = self {
            match rx.try_recv() {
                Ok(Ok(lore)) => *self = Status::Loaded(Arc::new(lore)),
                Ok(Err(e)) => *self = Status::Failed(e),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    *self =
                        Status::Failed("the wiki data loader ended without answering".to_owned())
                }
            }
        }
    }

    pub fn lore(&self) -> Option<&Arc<Lore>> {
        match self {
            Status::Loaded(l) => Some(l),
            _ => None,
        }
    }

    /// Why there is no lore, in words, or None when it is loaded or loading.
    pub fn why_not(&self) -> Option<&str> {
        match self {
            Status::Absent(w) | Status::Failed(w) => Some(w),
            _ => None,
        }
    }

    pub fn loading(&self) -> bool {
        matches!(self, Status::Loading(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        let w = |f: &str, v: Value| std::fs::write(dir.join(f), v.to_string()).unwrap();
        w(
            META_FILE,
            serde_json::json!({"data_version": "2026-10-08", "source": {"url": "https://example.wiki", "licence": "CC BY-SA 4.0"}}),
        );
        w(
            "zones.json",
            serde_json::json!([{"name": "Night Harbor", "url": "u/nh", "level": {"min": 1, "max": 50}, "adjacent": ["Underdocks"]}, {"name": "Underdocks", "url": "u/ud"}]),
        );
        w(
            "npcs.json",
            serde_json::json!([{"name": "a cultist", "zones": ["Night Harbor"]}, {"name": "Night Harbor"}]),
        );
        w(
            "items.json",
            serde_json::json!([{"name": "Aashir's Journal", "slot": ["BAG"], "weight": 0.3, "drops": {"zones": ["Night Harbor"], "mobs": ["a cultist"]}}]),
        );
        w(
            "quests.json",
            serde_json::json!([{"name": "A Sweet Treat", "giver": "Guard Pippi Pinecone"}]),
        );
        w(
            "spells.json",
            serde_json::json!([{"name": "Lift Bones", "classes": [{"class": "Necromancer", "level": 1}]}]),
        );
        w(
            "redirects.json",
            serde_json::json!([{"from": "NH", "to": "Night Harbor"}]),
        );
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("grimoire-lore-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_bundle_loads_every_kind_and_resolves_names_and_redirects() {
        let d = scratch("load");
        bundle(&d);
        let l = Lore::load(&d).expect("loads");
        assert_eq!(l.data_version, "2026-10-08");
        assert_eq!(l.licence, "CC BY-SA 4.0");
        assert_eq!(l.count(Kind::Zone), 2);
        assert_eq!(
            l.find("night harbor"),
            Some((Kind::Zone, 0)),
            "the zone wins over an NPC of the same name"
        );
        assert_eq!(l.find("NH"), Some((Kind::Zone, 0)), "a redirect resolves");
        assert_eq!(l.find_in(Kind::Npc, "A Cultist"), Some(0));
        assert_eq!(
            l.find_in(Kind::Npc, "Night Harbor"),
            Some(1),
            "within a kind, the kind's own record"
        );
        assert_eq!(l.find("nowhere"), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_kind_is_an_error_naming_the_file() {
        let d = scratch("missing");
        bundle(&d);
        std::fs::remove_file(d.join("spells.json")).unwrap();
        let e = Lore::load(&d).err().expect("refused");
        assert!(e.contains("spells.json"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn values_read_as_plain_text() {
        let d = scratch("text");
        bundle(&d);
        let l = Lore::load(&d).unwrap();
        let journal = &l.records(Kind::Item)[0];
        assert_eq!(journal.text("weight").as_deref(), Some("0.3"));
        assert_eq!(journal.list("slot"), vec!["BAG"]);
        assert_eq!(
            journal.text("drops").as_deref(),
            Some("mobs a cultist, zones Night Harbor"),
            "object keys read in sorted order: serde_json keeps no insertion order here"
        );
        assert_eq!(value_text(&serde_json::json!(50)).as_deref(), Some("50"));
        assert_eq!(value_text(&serde_json::json!("  ")), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
