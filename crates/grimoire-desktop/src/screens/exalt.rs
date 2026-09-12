//! Screen: exaltations. Decision D9: the stones a character owns, where each one can go and what
//! it gives, plus the Exalt reading on the Inventory screen.
//!
//! WHAT IS HERE, IN THREE PARTS.
//!   The sockets.    What a socket id means, which sockets an item of a given tier should have,
//!                   and what a stone sitting in one grants.
//!   The catalogue.  Every effect an item could yield as a stone, which classes can use it, and
//!                   whether it fits a given host and at what cost.
//!   The fires.      Which exaltations the log has actually seen go off.
//! Every rule carries a test below that fails without it.
//!
//! WHAT AN EXALTATION IS. Every item's Focus / Click / Worn / Proc effect becomes a removable stone
//! once the item reaches +1 / +2 / +3 / +4, and the stone can be socketed into any other item that
//! has that socket open. The `/outputfile inventory` dump nests a socketed stone under its host as
//! `<Slot>-Slot<N>` where N is the socket TYPE (7 focus, 8 click, 9 worn, 10 proc; 1 or 2 an
//! ornament), lists loose stones under `Augmentation`, and names every stone after the item it was
//! rendered from with ` (Exaltation)` appended. A stone is ONE effect of its source; a loose one
//! does not say which, so it is shown with every effect the source could yield and says so.
//!
//! THE FITTING RULE. A stone keeps its source item's class list and slot list, and the host takes
//! on the INTERSECTION of both when the stone goes in. So a stone fits a host when the class sets
//! meet and the slot sets meet; whether YOU can wear the result is a separate question, answered
//! by `usable_by` against the trio.
//!
//! WHAT THIS SCREEN WILL NOT DO. The gear catalogue's `effects` map (spell text per effect name)
//! is not in the typed snapshot, so an effect is named and never described. A count of fires is
//! "does this one do anything", never a rate: the client prints a line when an effect goes off and
//! nothing when it does not, so a rate has no denominator.

use crate::chrome::State;
use crate::data::Item;
use crate::ingest::{InvRow, Section};
use crate::screens::gear::{self, item_key, norm_name, strip_decor, CharState, CLASSES};
use crate::screens::Cx;
use crate::theme::*;
use egui::{FontId, RichText, Stroke, StrokeKind, Ui, Vec2};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/* ------------------------------------------------------------- socket types -- */

/// The four effect kinds, in socket unlock order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Focus,
    Click,
    Worn,
    Proc,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Focus, Kind::Click, Kind::Worn, Kind::Proc];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Focus => "Focus",
            Kind::Click => "Click",
            Kind::Worn => "Worn",
            Kind::Proc => "Proc",
        }
    }

    /// The tier that opens the socket AND the tier the source needs before the effect can be
    /// pulled out.
    pub fn at(self) -> u32 {
        match self {
            Kind::Focus => 1,
            Kind::Click => 2,
            Kind::Worn => 3,
            Kind::Proc => 4,
        }
    }

    /// The dump's socket id.
    pub fn n(self) -> u32 {
        match self {
            Kind::Focus => 7,
            Kind::Click => 8,
            Kind::Worn => 9,
            Kind::Proc => 10,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Kind::Focus => 0,
            Kind::Click => 1,
            Kind::Worn => 2,
            Kind::Proc => 3,
        }
    }

    /// The word this effect kind is shown as.
    pub fn what(self) -> &'static str {
        match self {
            Kind::Focus => "A passive focus effect: spell damage, cast speed, duration, healing, an instrument's resonance.",
            Kind::Click => "A clickable spell. Some need the item worn to click; some work from a bag or the Activated Items storage.",
            Kind::Worn => "A passive effect that is on while the item is worn.",
            Kind::Proc => "A chance-on-hit spell.",
        }
    }
}

/// A socket id read as a thing with a meaning. Ornaments are a fifth kind
/// that carries no effect and no restriction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketType {
    Ornament,
    Effect(Kind),
}

impl SocketType {
    pub fn label(self) -> &'static str {
        match self {
            SocketType::Ornament => "Ornament",
            SocketType::Effect(k) => k.label(),
        }
    }

    /// The item level that unlocks the type.
    pub fn at(self) -> u32 {
        match self {
            SocketType::Ornament => 0,
            SocketType::Effect(k) => k.at(),
        }
    }

    /// Display order, which follows the unlock order rather than the raw id.
    pub fn sort(self) -> u32 {
        match self {
            SocketType::Ornament => 0,
            SocketType::Effect(k) => k.at(),
        }
    }

    /// The word this socket kind is shown as.
    pub fn what(self) -> &'static str {
        match self {
            SocketType::Ornament => "The look of another item. Visible equipment only.",
            SocketType::Effect(Kind::Focus) => "A focus effect carried over from another item.",
            SocketType::Effect(Kind::Click) => "A clickable effect carried over from another item.",
            SocketType::Effect(Kind::Worn) => {
                "A passive worn effect carried over from another item."
            }
            SocketType::Effect(Kind::Proc) => "A combat proc carried over from another item.",
        }
    }
}

/// What a socket id means: the dump's socket ids are 1 or 2 (Ornamentation) and 7, 8, 9, 10. Ids
/// 3 to 6 have never been seen and are reported as unknown rather than invented.
pub fn type_of(n: u32) -> Option<SocketType> {
    match n {
        1 | 2 => Some(SocketType::Ornament),
        7 => Some(SocketType::Effect(Kind::Focus)),
        8 => Some(SocketType::Effect(Kind::Click)),
        9 => Some(SocketType::Effect(Kind::Worn)),
        10 => Some(SocketType::Effect(Kind::Proc)),
        _ => None,
    }
}

/// A location read as a socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketAt {
    pub n: u32,
    pub host: String,
    pub kind: Option<SocketType>,
}

/// `loc` minus one trailing `-SlotN`, with N.
fn split_slot(loc: &str) -> Option<(&str, u32)> {
    let at = loc.rfind("-Slot")?;
    let digits = &loc[at + 5..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((&loc[..at], digits.parse().ok()?))
}

/// Is this location a socket, and which one. None for a bag position and for anything with no
/// `-SlotN` at all. Sockets are not bag slots: `General 1-Slot14` is bag 1's fourteenth slot; the
/// two are told apart by what the suffix hangs off (a worn slot name, or an item that is itself
/// inside something), never by the number.
pub fn socket_at(loc: &str) -> Option<SocketAt> {
    let (host, n) = split_slot(loc)?;
    if gear::worn_slot(host).is_none() && split_slot(host).is_none() {
        return None;
    }
    Some(SocketAt {
        n,
        host: host.to_owned(),
        kind: type_of(n),
    })
}

/// The socket ids an item of this tier should have. The ornament
/// id is not predictable from the tier (it depends on the slot) so it is passed in when known.
pub fn expected_sockets(tier: u32, ornament_id: Option<u32>) -> Vec<u32> {
    let mut out: Vec<u32> = ornament_id.into_iter().collect();
    for k in Kind::ALL {
        if tier >= k.at() {
            out.push(k.n());
        }
    }
    out
}

/* --------------------------------------------------------- reading a record -- */

/// The effect kind of a wiki effect record. None when the wiki tagged
/// nothing, which is shown and never claimed.
pub fn kind_of(e: &Value) -> Option<Kind> {
    let s = |k: &str| e.get(k).and_then(Value::as_str);
    let k = s("k").unwrap_or("");
    let m = s("m").unwrap_or("");
    if k == "focus" {
        return Some(Kind::Focus);
    }
    if m == "combat" || k == "combat_eff" {
        return Some(Kind::Proc);
    }
    if m == "worn" || k == "worn_eff" {
        return Some(Kind::Worn);
    }
    if m == "clicky"
        || m == "must_equip"
        || k == "click"
        || e.get("ct")
            .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
    {
        return Some(Kind::Click);
    }
    None
}

fn effs(item: &Item) -> Vec<&Value> {
    match item.extra.get("eff") {
        Some(Value::Array(a)) => a.iter().filter(|e| e.is_object()).collect(),
        _ => Vec::new(),
    }
}

fn foc(item: &Item) -> Option<&str> {
    item.extra
        .get("foc")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// What a stone in this socket actually gives you, read off the record
/// of the item it was rendered from. None when the data cannot say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub text: String,
    pub kind: Kind,
    /// Cast time, click effects only.
    pub ct: Option<String>,
    /// Level, click and proc effects.
    pub lvl: Option<String>,
}

pub fn grants(item: &Item, socket_n: u32) -> Option<Grant> {
    let SocketType::Effect(kind) = type_of(socket_n)? else {
        return None;
    };
    let list = effs(item);
    let s = |e: &Value, k: &str| e.get(k).and_then(Value::as_str).map(str::to_owned);
    let name = |e: &Value| s(e, "n").unwrap_or_default();
    match kind {
        Kind::Focus => {
            if let Some(f) = foc(item) {
                return Some(Grant {
                    text: f.to_owned(),
                    kind,
                    ct: None,
                    lvl: None,
                });
            }
            list.iter()
                .find(|e| e.get("k").and_then(Value::as_str) == Some("focus"))
                .map(|e| Grant {
                    text: name(e),
                    kind,
                    ct: None,
                    lvl: None,
                })
        }
        Kind::Click => list
            .iter()
            .find(|e| {
                let m = e.get("m").and_then(Value::as_str);
                let k = e.get("k").and_then(Value::as_str);
                m == Some("clicky")
                    || m == Some("must_equip")
                    || k == Some("click")
                    || (m.is_none() && e.get("ct").is_some_and(|v| !v.is_null()))
            })
            .map(|e| Grant {
                text: name(e),
                kind,
                ct: s(e, "ct"),
                lvl: s(e, "l"),
            }),
        Kind::Worn => list
            .iter()
            .find(|e| {
                e.get("m").and_then(Value::as_str) == Some("worn")
                    || e.get("k").and_then(Value::as_str) == Some("worn_eff")
            })
            .map(|e| Grant {
                text: name(e),
                kind,
                ct: None,
                lvl: None,
            }),
        Kind::Proc => list
            .iter()
            .find(|e| {
                e.get("m").and_then(Value::as_str) == Some("combat")
                    || e.get("k").and_then(Value::as_str) == Some("combat_eff")
            })
            .map(|e| Grant {
                text: name(e),
                kind,
                ct: None,
                lvl: s(e, "l"),
            }),
    }
}

/// One line for a hover: what this stone is and what it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Described {
    pub title: String,
    pub body: String,
}

/// A stone sitting loose: every transferable property listed and none claimed by a host.
pub fn describe_loose(item: Option<&Item>) -> Vec<(&'static str, String)> {
    let mut rows = Vec::new();
    if let Some(it) = item {
        for k in Kind::ALL {
            if let Some(g) = grants(it, k.n()) {
                rows.push((k.label(), g.text));
            }
        }
    }
    rows
}

pub fn describe(item: Option<&Item>, socket_n: Option<u32>, src_name: Option<&str>) -> Described {
    let Some(t) = socket_n.and_then(type_of) else {
        let rows = describe_loose(item);
        let body = if rows.is_empty() {
            format!(
                "Rendered from {}. The wiki records no transferable effect on it.",
                src_name.unwrap_or("an item")
            )
        } else {
            rows.iter()
                .map(|(l, t)| format!("{l}: {t}"))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        return Described {
            title: "Exaltation".into(),
            body,
        };
    };
    if t == SocketType::Ornament {
        return Described {
            title: "Ornament".into(),
            body: match src_name {
                Some(n) => format!("Makes this item look like {n}."),
                None => t.what().to_owned(),
            },
        };
    }
    let g = item.and_then(|it| grants(it, socket_n.unwrap_or(0)));
    let Some(g) = g else {
        return Described {
            title: t.label().into(),
            body: format!(
                "{}{} The wiki does not record which effect this one carries.",
                t.what(),
                src_name
                    .map(|n| format!(" Rendered from {n}."))
                    .unwrap_or_default()
            ),
        };
    };
    let tail = match &g.ct {
        Some(ct) if ct != "Instant" => format!(" ({ct})"),
        _ => String::new(),
    };
    let lvl = g
        .lvl
        .as_ref()
        .map(|l| format!(" (level {l})"))
        .unwrap_or_default();
    let from = src_name.map(|n| format!(" from {n}")).unwrap_or_default();
    Described {
        title: t.label().into(),
        body: format!(
            "{}{tail}{lvl}, {} effect{from}.",
            g.text,
            t.label().to_lowercase()
        ),
    }
}

/* --------------------------------------------------------- the class rule -- */

/// Two class lists intersected. None is "any class".
pub fn cls_inter(
    a: Option<&[&'static str]>,
    b: Option<&[&'static str]>,
) -> Option<Vec<&'static str>> {
    match (a, b) {
        (None, None) => None,
        (None, Some(b)) => Some(b.to_vec()),
        (Some(a), None) => Some(a.to_vec()),
        (Some(a), Some(b)) => Some(a.iter().copied().filter(|k| b.contains(k)).collect()),
    }
}

/// Can this trio wear an item with this class list.
pub fn usable_by(cls: Option<&[&'static str]>, trio: &[String]) -> bool {
    match cls {
        None => true,
        Some(list) => trio.iter().any(|t| list.contains(&t.as_str())),
    }
}

/// A class list as words: "ALL", "none", or the codes in canonical order.
pub fn cls_text(cls: Option<&[&'static str]>) -> String {
    match cls {
        None => "ALL".into(),
        Some([]) => "none".into(),
        Some(l) => {
            let mut v = l.to_vec();
            v.sort_by_key(|c| CLASSES.iter().position(|k| k == c));
            v.join(" ")
        }
    }
}

fn slot_inter(a: &[String], b: &[String]) -> Vec<String> {
    a.iter().filter(|s| b.contains(s)).cloned().collect()
}

/* ------------------------------------------------------------ the catalog -- */

/// One effect one item could yield as a stone.
#[derive(Clone, Debug)]
pub struct Stone {
    pub id: String,
    /// Index into the snapshot's items.
    pub item: usize,
    pub item_name: String,
    /// None for an effect the wiki lists without saying what kind it is: shown, filed under no
    /// socket, never counted as fitting one.
    pub kind: Option<Kind>,
    pub effect: String,
    pub cls: Option<Vec<&'static str>>,
    pub sl: Vec<String>,
    pub two_h: bool,
    pub oe: bool,
    pub era: Option<String>,
    pub deity: bool,
}

impl Stone {
    pub fn at(&self) -> Option<u32> {
        self.kind.map(Kind::at)
    }
}

/// Every effect every item could yield, and the name index the dump resolves through.
pub struct ExaltCat {
    pub stones: Vec<Stone>,
    /// Snapshot item index to its stones (indices into `stones`).
    pub by_item: HashMap<usize, Vec<usize>>,
    exact: HashMap<String, usize>,
    loose: HashMap<String, usize>,
    pub items_read: usize,
}

fn is_two_h(item: &Item) -> bool {
    gear::two_h(item.extra.get("skill").and_then(Value::as_str))
}

impl ExaltCat {
    /// Built over the snapshot's items. Items with a class list of "none" and items
    /// with no slot are skipped (nothing can wear them, so nothing can hold their stone), and so
    /// are conjured items ("Summoned: Staff of Runes").
    pub fn build(items: &[Item]) -> ExaltCat {
        let mut stones = Vec::new();
        let mut by_item: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut exact: HashMap<String, usize> = HashMap::new();
        for (i, it) in items.iter().enumerate() {
            exact.entry(item_key(&it.name)).or_insert(i);
            if !it.key.is_empty() {
                exact.entry(item_key(&it.key)).or_insert(i);
            }
            let cls = it.classes();
            if it.sl.is_empty() || cls.as_ref().is_some_and(|c| c.is_empty()) {
                continue;
            }
            if it.name.to_ascii_lowercase().starts_with("summoned:") {
                continue;
            }
            let mut list: Vec<usize> = Vec::new();
            let mut push = |kind: Option<Kind>, effect: String, stones: &mut Vec<Stone>| {
                let id = format!(
                    "{}|{}|{}",
                    it.key,
                    kind.map(|k| k.label()).unwrap_or("?"),
                    effect
                );
                stones.push(Stone {
                    id,
                    item: i,
                    item_name: it.name.clone(),
                    kind,
                    effect,
                    cls: cls.clone(),
                    sl: it.sl.clone(),
                    two_h: is_two_h(it),
                    oe: it.out_of_era(),
                    era: it.era.clone(),
                    deity: truthy(it.extra.get("deity")),
                });
                list.push(stones.len() - 1);
            };
            let f = foc(it).map(str::to_owned);
            if let Some(f) = &f {
                push(Some(Kind::Focus), f.clone(), &mut stones);
            }
            /* an instrument's resonance is a focus effect; one the focus field already names is
             * not counted twice. "Brass Instruments: 10" is a tradeskill line, not a resonance. */
            if let Some(Value::Array(inst)) = it.extra.get("inst") {
                for kind in inst.iter().filter_map(Value::as_str) {
                    if !kind.ends_with("Resonance") {
                        continue;
                    }
                    let v = it
                        .extra
                        .get("ex")
                        .and_then(|ex| ex.get(kind))
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        });
                    let Some(v) = v else { continue };
                    if v.is_empty() || v == "0" {
                        continue;
                    }
                    let name = format!("{kind} {v}");
                    if f.as_deref() == Some(name.as_str()) {
                        continue;
                    }
                    push(Some(Kind::Focus), name, &mut stones);
                }
            }
            for e in effs(it) {
                let n = e.get("n").and_then(Value::as_str).unwrap_or("").to_owned();
                if n.is_empty() {
                    continue;
                }
                let kind = kind_of(e);
                if kind == Some(Kind::Focus) && f.as_deref() == Some(n.as_str()) {
                    continue;
                }
                push(kind, n, &mut stones);
            }
            if !list.is_empty() {
                by_item.insert(i, list);
            }
        }
        /* the article-stripped key, an article-less title winning any tie */
        let mut loose: HashMap<String, usize> = HashMap::new();
        let mut keys: Vec<&String> = exact.keys().collect();
        keys.sort();
        for k in keys {
            let lk = norm_name(k);
            let i = exact[k];
            if &lk == k {
                loose.insert(lk, i);
            } else {
                loose.entry(lk).or_insert(i);
            }
        }
        ExaltCat {
            stones,
            by_item,
            exact,
            loose,
            items_read: items.len(),
        }
    }

    /// Resolve a dump name to a snapshot item: exact, exact on the decoration stripped name, then
    /// the article stripped fallbacks (the same ladder the gear catalogue climbs).
    pub fn resolve(&self, name: &str) -> Option<usize> {
        let stripped = strip_decor(name);
        self.exact
            .get(&item_key(name))
            .or_else(|| self.exact.get(&item_key(&stripped)))
            .or_else(|| self.loose.get(&norm_name(name)))
            .or_else(|| self.loose.get(&norm_name(&stripped)))
            .copied()
    }

    pub fn yields_of(&self, item: Option<usize>) -> Vec<usize> {
        item.and_then(|i| self.by_item.get(&i))
            .cloned()
            .unwrap_or_default()
    }
}

/* ------------------------------------------------------------ the fitting -- */

/// A host for the fitting rule: its class list, slot list and tier (None for a catalogue item,
/// whose tier is unknown).
pub struct Host<'a> {
    pub cls: Option<&'a [&'static str]>,
    pub sl: &'a [String],
    pub tier: Option<u32>,
}

/// The fitting verdict, and the item that would result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fit {
    pub ok: bool,
    pub why: Vec<&'static str>,
    pub cls: Option<Vec<&'static str>>,
    pub sl: Vec<String>,
    /// None when the host's tier is unknown.
    pub open: Option<bool>,
    pub narrows_cls: bool,
    pub narrows_sl: bool,
}

pub fn fit(stone: &Stone, host: &Host) -> Fit {
    let cls = cls_inter(stone.cls.as_deref(), host.cls);
    let sl = slot_inter(&stone.sl, host.sl);
    let mut why = Vec::new();
    if cls.as_ref().is_some_and(|c| c.is_empty()) {
        why.push("no shared class");
    }
    if sl.is_empty() {
        why.push("no shared slot");
    }
    let open = match (stone.at(), host.tier) {
        (None, _) => Some(false),
        (Some(_), None) => None,
        (Some(at), Some(t)) => Some(t >= at),
    };
    if stone.kind.is_none() {
        why.push("kind of effect not stated on the wiki");
    }
    let narrows_cls = match (&cls, host.cls) {
        (Some(_), None) => true,
        (Some(c), Some(h)) => c.len() < h.len(),
        (None, _) => false,
    };
    Fit {
        ok: why.is_empty(),
        why,
        narrows_cls,
        narrows_sl: host.sl.len() > sl.len(),
        cls,
        sl,
        open,
    }
}

/// What socketing this stone would COST the host, in words: the classes and slots the result is
/// narrowed to, or nothing when it takes the host as it is. The fit computes `narrows`
/// for exactly this warning (an any-class item that becomes a five-class item is a real loss),
/// and the Exaltations screen prints it after every home a stone is offered.
pub fn narrows_text(f: &Fit) -> String {
    let mut parts: Vec<String> = Vec::new();
    if f.narrows_cls {
        match &f.cls {
            Some(c) if !c.is_empty() => parts.push(format!("classes to {}", c.join("/"))),
            _ => parts.push("classes".to_owned()),
        }
    }
    if f.narrows_sl {
        if f.sl.is_empty() {
            parts.push("slots".to_owned());
        } else {
            parts.push(format!("slots to {}", f.sl.join("/")));
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" (narrows its {})", parts.join(" and "))
    }
}

/* --------------------------------------------------------------- the dump -- */

/// One of a worn item's four sockets, read off its `-SlotN` children.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Socket {
    /// The item's tier has opened it.
    pub open: bool,
    pub filled: bool,
    pub stone: Option<SocketStone>,
    /// The dump printed it as Empty: the only evidence an effect was pulled out.
    pub empty: bool,
    /// Holding the item's own rendered effect.
    pub own: bool,
}

/// The stone in a filled socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketStone {
    /// Index into the dump rows.
    pub row: usize,
    /// The source item in the snapshot.
    pub item: Option<usize>,
    /// The catalogue stone of this socket's type, when the source yields one.
    pub stone: Option<usize>,
    pub yields: Vec<usize>,
}

/// An ornament socket, as the dump reports it: its id, and the row of the ornament in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ornament {
    pub n: u32,
    pub row: Option<usize>,
}

/// A worn item with its sockets.
#[derive(Clone, Debug)]
pub struct WornItem {
    pub row: usize,
    pub slot: String,
    pub item: Option<usize>,
    pub tier: u32,
    pub cls: Option<Vec<&'static str>>,
    pub sl: Vec<String>,
    pub sockets: [Socket; 4],
    pub ornament: Option<Ornament>,
    pub yields: Vec<usize>,
}

impl WornItem {
    pub fn socket(&self, k: Kind) -> &Socket {
        &self.sockets[k.index()]
    }
}

/// A loose stone: the Augmentation bin, a bag, the bank (readDump `loose`).
#[derive(Clone, Debug)]
pub struct Loose {
    pub row: usize,
    pub item: Option<usize>,
    pub yields: Vec<usize>,
}

/// An item you own anywhere whose record yields a stone, or that holds one (readDump `sources`).
#[derive(Clone, Debug)]
pub struct Source {
    pub row: usize,
    pub item: Option<usize>,
    pub tier: u32,
    pub worn: bool,
    pub cls: Option<Vec<&'static str>>,
    pub sl: Vec<String>,
    pub yields: Vec<usize>,
    pub sockets: [Socket; 4],
    pub sockets_known: bool,
}

/// What reading a dump gives back.
#[derive(Clone, Debug, Default)]
pub struct Dump {
    pub worn: Vec<WornItem>,
    pub loose: Vec<Loose>,
    pub sources: Vec<Source>,
}

/// Read over the dump's rows (with the Empty placeholders kept).
///
/// A socket row belongs to the row it FOLLOWS. Location strings are not unique: both worn
/// earrings are "Ear" and their stones "Ear-Slot7", so a lookup by location handed the second
/// earring's stones to both earrings and lost the first one's. Each socket row is filed under the
/// most recent row whose location is its parent's, in dump order. Only SOCKET rows are filed:
/// "General 6-Slot7" is bag 6's seventh position, not a focus socket of the bag.
pub fn read_dump(rows: &[InvRow], items: &[Item], cat: &ExaltCat) -> Dump {
    let mut kids: HashMap<usize, BTreeMap<u32, usize>> = HashMap::new();
    {
        let mut last: HashMap<&str, usize> = HashMap::new();
        for (i, r) in rows.iter().enumerate() {
            if let Some(s) = socket_at(&r.loc) {
                if let Some(&p) = last.get(s.host.as_str()) {
                    kids.entry(p).or_default().insert(s.n, i);
                }
            }
            if !r.empty {
                last.insert(r.loc.as_str(), i);
            }
        }
    }
    let resolve = |base: &str| cat.resolve(base);
    let sockets_of = |ri: usize, r: &InvRow| -> ([Socket; 4], bool, Option<Ornament>) {
        let mut out: [Socket; 4] = Default::default();
        let mut seen = false;
        let empty = BTreeMap::new();
        let ks = kids.get(&ri).unwrap_or(&empty);
        let own_item = resolve(&r.base);
        for k in Kind::ALL {
            let sub = ks.get(&k.n()).map(|&i| (i, &rows[i]));
            if sub.is_some() {
                seen = true;
            }
            let filled = sub.filter(|(_, s)| s.exalt);
            let stone = filled.map(|(i, s)| {
                let item = resolve(&s.base);
                let yields = cat.yields_of(item);
                let stone = yields
                    .iter()
                    .copied()
                    .find(|&si| cat.stones[si].kind == Some(k));
                SocketStone {
                    row: i,
                    item,
                    stone,
                    yields,
                }
            });
            let own = stone
                .as_ref()
                .is_some_and(|s| s.item.is_some() && s.item == own_item);
            out[k.index()] = Socket {
                open: u32::from(r.tier) >= k.at(),
                filled: stone.is_some(),
                stone,
                empty: sub.is_some_and(|(_, s)| s.empty),
                own,
            };
        }
        let ornament = ks
            .iter()
            .find(|(n, _)| matches!(type_of(**n), Some(SocketType::Ornament)))
            .map(|(n, &i)| Ornament {
                n: *n,
                row: if rows[i].empty { None } else { Some(i) },
            });
        (out, seen, ornament)
    };
    let mut out = Dump::default();
    for (i, r) in rows.iter().enumerate() {
        if r.empty || socket_at(&r.loc).is_some() {
            continue;
        }
        let is_worn = gear::worn_slot(&r.loc).is_some();
        let item = resolve(&r.base);
        if r.exalt {
            out.loose.push(Loose {
                row: i,
                item,
                yields: cat.yields_of(item),
            });
            continue;
        }
        let yields = cat.yields_of(item);
        let (sockets, known, ornament) = sockets_of(i, r);
        let (cls, sl) = match item {
            Some(ix) => (items[ix].classes(), items[ix].sl.clone()),
            None => (None, Vec::new()),
        };
        if is_worn {
            out.worn.push(WornItem {
                row: i,
                slot: gear::worn_slot(&r.loc).unwrap_or("").to_owned(),
                item,
                tier: r.tier.into(),
                cls: cls.clone(),
                sl: sl.clone(),
                sockets: sockets.clone(),
                ornament,
                yields: yields.clone(),
            });
        }
        if !yields.is_empty() || sockets.iter().any(|s| s.filled) {
            out.sources.push(Source {
                row: i,
                item,
                tier: r.tier.into(),
                worn: is_worn,
                cls,
                sl,
                yields,
                sockets,
                sockets_known: known,
            });
        }
    }
    /* The dump's own "sockets unknown" case: a worn item with no record has cls None and sl empty,
     * which fit() reads as "no shared slot". That is the honest answer for it. */
    out
}

/// Where a stone can go among the worn items.
#[derive(Clone, Debug, Default)]
pub struct Homes {
    /// (worn index, fit, occupied)
    pub now: Vec<(usize, Fit, bool)>,
    /// (worn index, fit, tier needed)
    pub later: Vec<(usize, Fit, u32)>,
    pub never: Vec<(usize, Fit)>,
}

pub fn homes_for(stone: &Stone, worn: &[WornItem]) -> Homes {
    let mut h = Homes::default();
    for (wi, w) in worn.iter().enumerate() {
        if w.item.is_none() {
            continue;
        }
        let f = fit(
            stone,
            &Host {
                cls: w.cls.as_deref(),
                sl: &w.sl,
                tier: Some(w.tier),
            },
        );
        if !f.ok {
            h.never.push((wi, f));
            continue;
        }
        let sock = stone.kind.map(|k| w.socket(k));
        match sock {
            Some(s) if s.open => h.now.push((wi, f, s.filled)),
            _ => h.later.push((wi, f, stone.at().unwrap_or(0))),
        }
    }
    h
}

/// Stones that could sit in an item worn in this slot. `trio` narrows to
/// stones one of your classes could still use; empty keeps everything. Untyped effects are left
/// out, because they fit no socket.
pub fn for_slot<'a>(
    cat: &'a ExaltCat,
    slot: &str,
    trio: &[String],
    hide_oe: bool,
) -> Vec<&'a Stone> {
    cat.stones
        .iter()
        .filter(|s| s.kind.is_some())
        .filter(|s| s.sl.iter().any(|x| x == slot))
        .filter(|s| !(hide_oe && s.oe))
        .filter(|s| trio.is_empty() || usable_by(s.cls.as_deref(), trio))
        .collect()
}

/* --------------------------------------------------------------- fires -- */

/// The client prints a line whenever an exaltation's effect goes off, and it names the item the
/// stone was rendered from: `Your <item> (Exaltation) <what it did>.`
///
/// THE ENDING IS NOT ENUMERATED, AND THAT IS THE WHOLE POINT OF THIS PATTERN. Measured on
/// `tests/fixtures/princess-night.txt`, thirty hours of one character's log: 962 lines carry
/// `(Exaltation)`, all 962 fit the shape above, and they end three different ways, "feels alive
/// with power" (630), "shimmers briefly" (324) and "sparkles" (8). An earlier version of this
/// pattern listed the endings it expected and silently dropped every line ending any other way,
/// which cost it all 8 "sparkles" lines in that file. A count of fires is a count of whether an
/// effect did anything, so a wording nobody has seen yet must still count; what identifies the
/// line is `Your`, the parenthesised `(Exaltation)` and the closing period, none of which the
/// ending is needed for. `fires_count_every_ending_the_real_log_carries` below pins that against
/// the real file.
///
/// The leading `Your` is load bearing: the client prints the same sentence about other people's
/// items, and those are not yours to count.
static FIRE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[[^\]]+\] Your (.+?) \(Exaltation\) [^.]+\.\s*$").expect("constant")
});

/// The source item named on a fire line, or None when the line is not one.
pub fn fire_line(line: &str) -> Option<&str> {
    FIRE.captures(line)
        .and_then(|m| m.get(1))
        .map(|g| g.as_str())
}

/// Fires counted per source item name, over the bootstrap tail plus everything live since.
#[derive(Default)]
pub struct Fires {
    pub counts: BTreeMap<String, u32>,
    pub file: Option<PathBuf>,
    /// Bytes the bootstrap covered, and how many were skipped before it.
    pub seeded: Option<(u64, u64)>,
    pub lines: u64,
    pub problem: Option<String>,
    tail: Option<crate::ingest::Tail>,
    rx: Option<mpsc::Receiver<Seed>>,
    last_poll: Option<Instant>,
}

struct Seed {
    counts: BTreeMap<String, u32>,
    end: u64,
    start: u64,
    lines: u64,
    problem: Option<String>,
}

impl Fires {
    pub fn feed(&mut self, lines: &[String]) {
        for l in lines {
            self.lines += 1;
            if let Some(n) = fire_line(l) {
                *self.counts.entry(n.to_owned()).or_insert(0) += 1;
            }
        }
    }

    /// Follow the active log: a new file seeds a fresh count on a worker thread, then the bytes
    /// appended since are read once a second. The read is the ingest's own cursor, so it never
    /// competes with the ingest's for the same lines.
    pub fn follow(&mut self, path: Option<&Path>, size: u64) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(s) => {
                    self.rx = None;
                    self.counts = s.counts;
                    self.lines = s.lines;
                    self.seeded = Some((s.end, s.start));
                    self.problem = s.problem;
                    self.tail = Some(crate::ingest::Tail::at(s.end));
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.problem = Some("the log reader thread ended without a result".into());
                }
            }
        }
        let Some(path) = path else {
            self.file = None;
            self.tail = None;
            return;
        };
        if self.file.as_deref() != Some(path) {
            self.file = Some(path.to_path_buf());
            self.counts.clear();
            self.lines = 0;
            self.seeded = None;
            self.tail = None;
            let (tx, rx) = mpsc::channel();
            let p = path.to_path_buf();
            let spawned = std::thread::Builder::new()
                .name("grimoire-exalt-fires".into())
                .spawn(move || {
                    let seed = match crate::ingest::read_tail(&p, size) {
                        Ok(t) => {
                            let mut counts = BTreeMap::new();
                            let mut lines = 0;
                            for l in t.text.split('\n') {
                                lines += 1;
                                if let Some(n) = fire_line(l) {
                                    *counts.entry(n.to_owned()).or_insert(0) += 1;
                                }
                            }
                            Seed {
                                counts,
                                end: t.end,
                                start: t.start,
                                lines,
                                problem: None,
                            }
                        }
                        Err(e) => Seed {
                            counts: BTreeMap::new(),
                            end: 0,
                            start: 0,
                            lines: 0,
                            problem: Some(format!("{}: {e}", p.display())),
                        },
                    };
                    let _ = tx.send(seed);
                });
            match spawned {
                Ok(_) => self.rx = Some(rx),
                Err(e) => {
                    self.problem = Some(format!("could not start the log reader thread: {e}"))
                }
            }
            return;
        }
        let due = self
            .last_poll
            .map_or(true, |t| t.elapsed() >= Duration::from_secs(1));
        if !due {
            return;
        }
        self.last_poll = Some(Instant::now());
        if let Some(tail) = &mut self.tail {
            if size < tail.offset {
                *tail = crate::ingest::Tail::at(0);
            }
            if size > tail.offset {
                let lines = crate::ingest::read_appended(path, tail, size);
                self.feed(&lines);
            }
        }
    }

    pub fn seeding(&self) -> bool {
        self.rx.is_some()
    }

    pub fn of(&self, source_name: &str) -> Option<u32> {
        self.counts.get(source_name).copied()
    }
}

/* ------------------------------------------------------------------ screen -- */

/// The exaltation state one screen keeps: the catalogue built once per snapshot, the dump read
/// once per dump, and the fires followed off the active log.
#[derive(Default)]
pub struct ExaltState {
    pub cat: Option<ExaltCat>,
    cat_key: Option<(usize, usize)>,
    pub dump: Option<Dump>,
    dump_key: Option<(PathBuf, chrono::DateTime<chrono::Utc>)>,
    pub rows: Vec<InvRow>,
    pub fires: Fires,
}

impl ExaltState {
    /// Rebuild what changed. Returns true when the dump was re-read this call.
    pub fn refresh(&mut self, cx: &mut Cx) -> bool {
        let snap = cx.data;
        let key = snap.map(|s| (s as *const _ as usize, s.items.len()));
        if key != self.cat_key {
            self.cat_key = key;
            self.cat = snap.map(|s| ExaltCat::build(&s.items));
            self.dump_key = None;
        }
        let mut reread = false;
        match (cx.ingest.inventory(), &self.cat, snap) {
            (Some(d), Some(cat), Some(s)) => {
                let k = (d.path.clone(), d.read_at);
                if self.dump_key.as_ref() != Some(&k) {
                    self.dump_key = Some(k);
                    self.rows = d.all.clone();
                    self.dump = Some(read_dump(&self.rows, &s.items, cat));
                    reread = true;
                }
            }
            _ => {
                self.dump = None;
                self.dump_key = None;
                self.rows.clear();
            }
        }
        let (path, size) = match cx.ingest.active_log() {
            Some(f) => (Some(f.path.clone()), f.size),
            None => (None, 0),
        };
        self.fires.follow(path.as_deref(), size);
        reread
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Worn,
    Loose,
    Sources,
    Fired,
    BySlot,
}

pub struct ExaltScreen {
    state: ExaltState,
    view: View,
    slot: String,
    hide_oe: bool,
    filter: String,
}

impl Default for ExaltScreen {
    fn default() -> Self {
        ExaltScreen {
            state: ExaltState::default(),
            view: View::Worn,
            slot: "Head".into(),
            hide_oe: true,
            filter: String::new(),
        }
    }
}

const BY_SLOT_CAP: usize = 200;

impl ExaltScreen {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        self.state.refresh(cx);
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        let trio = CharState::read(cx.settings);

        heading(ui, "EXALTATIONS");
        ui.label(RichText::new("Every socket on what you wear, every stone you own and where it fits, and which ones the log has seen go off.").color(TEXT_2));
        ui.add_space(6.0);

        /* the two sources, each named */
        match (cx.data, cx.data_err) {
            (Some(s), _) => {
                let n = self.state.cat.as_ref().map(|c| c.stones.len()).unwrap_or(0);
                mark(
                    ui,
                    State::Settled,
                    &format!(
                        "{} items read from {}, {n} effects a stone could carry",
                        s.items.len(),
                        s.root.join(crate::data::GEAR_FILE).display()
                    ),
                );
            }
            (None, Some(e)) => {
                mark(ui, State::Wrong, e);
            }
            (None, None) => {
                mark(ui, State::Idle, "no item data loaded; put the snapshot's gear-data.json in data/ beside the executable");
            }
        }
        match cx.ingest.inventory() {
            Some(d) => {
                let who = d
                    .character
                    .as_deref()
                    .map(|c| format!(" · {c}'s dump"))
                    .unwrap_or_default();
                mark(
                    ui,
                    State::Settled,
                    &format!("{}{who} · {} rows", d.path.display(), d.all.len()),
                );
            }
            None => {
                mark(
                    ui,
                    State::You,
                    cx.ingest
                        .inventory_problem()
                        .unwrap_or("no inventory dump read yet; in game: /outputfile inventory"),
                );
            }
        }
        ui.add_space(8.0);

        let (Some(dump), Some(cat), Some(snap)) = (&self.state.dump, &self.state.cat, cx.data)
        else {
            return;
        };
        let rows = &self.state.rows;
        let items = &snap.items;

        ui.horizontal(|ui| {
            for (v, label) in [
                (View::Worn, "Worn"),
                (View::Loose, "Loose stones"),
                (View::Sources, "Sources"),
                (View::Fired, "Fired"),
                (View::BySlot, "By slot"),
            ] {
                if ui.selectable_label(self.view == v, label).clicked() {
                    self.view = v;
                }
            }
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("exalt-body")
            .show(ui, |ui| match self.view {
                View::Worn => worn_view(ui, dump, rows, items, cat, &self.state.fires),
                View::Loose => loose_view(ui, dump, rows, items, cat, &trio.classes),
                View::Sources => sources_view(ui, dump, rows, cat),
                View::Fired => fired_view(
                    ui,
                    &self.state.fires,
                    dump,
                    rows,
                    cx.ingest.active_log().map(|f| f.path.clone()),
                ),
                View::BySlot => {
                    let mut slot = self.slot.clone();
                    let mut hide_oe = self.hide_oe;
                    let mut filter = self.filter.clone();
                    by_slot_view(
                        ui,
                        cat,
                        dump,
                        &trio.classes,
                        &mut slot,
                        &mut hide_oe,
                        &mut filter,
                    );
                    self.slot = slot;
                    self.hide_oe = hide_oe;
                    self.filter = filter;
                }
            });
    }
}

fn row_name(rows: &[InvRow], i: usize) -> String {
    let r = &rows[i];
    if r.tier > 0 {
        format!("{} +{}", r.base, r.tier)
    } else {
        r.base.clone()
    }
}

fn worn_view(
    ui: &mut Ui,
    dump: &Dump,
    rows: &[InvRow],
    items: &[Item],
    cat: &ExaltCat,
    fires: &Fires,
) {
    if dump.worn.is_empty() {
        mark(ui, State::Idle, "the dump lists nothing worn");
        return;
    }
    let open_empty: usize = dump
        .worn
        .iter()
        .map(|w| w.sockets.iter().filter(|s| s.open && !s.filled).count())
        .sum();
    let filled: usize = dump
        .worn
        .iter()
        .map(|w| w.sockets.iter().filter(|s| s.filled).count())
        .sum();
    ui.label(
        RichText::new(format!(
            "{} worn items · {filled} sockets filled · {open_empty} open and empty",
            dump.worn.len()
        ))
        .color(TEXT_2),
    );
    ui.add_space(4.0);
    egui::Grid::new("exalt-worn")
        .num_columns(7)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for h in ["slot", "item", "ornament"] {
                ui.label(RichText::new(h).color(TEXT_3));
            }
            /* The four effect columns, from the kinds table: label, the tier that opens the socket,
             * and the kind's own word under the pointer. */
            for k in Kind::ALL {
                ui.label(
                    RichText::new(format!("{} +{}", k.label().to_lowercase(), k.at()))
                        .color(TEXT_3),
                )
                .on_hover_text(k.what());
            }
            ui.end_row();
            for w in &dump.worn {
                ui.label(RichText::new(&w.slot).color(TEXT_2));
                let name = row_name(rows, w.row);
                /* the expected sockets against what the dump printed: the socket ids an
                 * item of this tier should carry, beside the ids the dump listed as open. */
                let expected = expected_sockets(w.tier, w.ornament.as_ref().map(|o| o.n));
                let mut listed: Vec<u32> = w.ornament.as_ref().map(|o| o.n).into_iter().collect();
                listed.extend(
                    Kind::ALL
                        .iter()
                        .filter(|k| w.socket(**k).open)
                        .map(|k| k.n()),
                );
                let sockets_tip = format!(
                    "at +{} the wiki expects socket ids {:?}; the dump lists {:?}",
                    w.tier, expected, listed
                );
                match w.item {
                    Some(_) => {
                        ui.label(RichText::new(name).color(TEXT))
                            .on_hover_text(sockets_tip);
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(name).color(TEXT))
                                .on_hover_text(sockets_tip);
                            /* a data gap, not a refusal: no state colour on it */
                            ui.label(RichText::new("no wiki page").color(TEXT_3));
                        });
                    }
                }
                match &w.ornament {
                    Some(Ornament { n, row: Some(r) }) => {
                        ui.label(
                            RichText::new(format!("{} (id {n})", strip_decor(&rows[*r].name)))
                                .color(TEXT),
                        );
                    }
                    Some(Ornament { n, row: None }) => {
                        ui.label(RichText::new(format!("empty (id {n})")).color(TEXT_3));
                    }
                    None => {
                        ui.label(RichText::new("none listed").color(TEXT_3));
                    }
                }
                for k in Kind::ALL {
                    socket_cell(ui, w.socket(k), k, rows, items, cat, fires);
                }
                ui.end_row();
            }
        });
    ui.add_space(6.0);
    /* The unlock order, from the socket types themselves (their `at` ids, in `sort`
     * order), so this sentence cannot drift from the table it describes. */
    let mut types: Vec<SocketType> = vec![SocketType::Ornament];
    types.extend(Kind::ALL.iter().map(|k| SocketType::Effect(*k)));
    types.sort_by_key(|t| t.sort());
    let order: Vec<String> = types
        .iter()
        .map(|t| {
            if t.at() == 0 {
                format!("{} at any tier", t.label().to_lowercase())
            } else {
                format!("{} at +{}", t.label().to_lowercase(), t.at())
            }
        })
        .collect();
    ui.label(RichText::new(format!("Sockets open by tier alone: {}. An open socket with an Empty row is one the dump printed as empty; a closed one has not been reached by the item's tier.", order.join(", "))).color(TEXT_3));
}

fn socket_cell(
    ui: &mut Ui,
    s: &Socket,
    k: Kind,
    rows: &[InvRow],
    items: &[Item],
    cat: &ExaltCat,
    fires: &Fires,
) {
    if !s.open {
        ui.label(RichText::new("closed").color(TEXT_3));
        return;
    }
    let Some(st) = &s.stone else {
        /* An empty open socket is something a person can act on, so it carries the act state:
         * on a leading square, where the vocabulary puts every state colour, and never on the
         * word itself (the words stay in the text colours). */
        if s.empty {
            mark(ui, State::You, "empty");
        } else {
            ui.label(RichText::new("open, not listed").color(TEXT_3));
        }
        return;
    };
    let src = strip_decor(&rows[st.row].name);
    let effect = st.stone.map(|si| cat.stones[si].effect.clone());
    let text = match (&effect, st.item) {
        (Some(e), _) => e.clone(),
        (None, Some(ix)) => match grants(&items[ix], k.n()) {
            Some(g) => g.text,
            None => format!("{src} (effect not on the wiki)"),
        },
        (None, None) => format!("{src} (no wiki page)"),
    };
    let fired = fires
        .of(&src)
        .map(|n| format!(" · fired {n}x"))
        .unwrap_or_default();
    let own = if s.own { " · its own" } else { "" };
    let r = ui.label(RichText::new(format!("{text}{own}{fired}")).color(TEXT));
    let d = describe(st.item.map(|i| &items[i]), Some(k.n()), Some(&src));
    r.on_hover_text(format!("{}: {}", d.title, d.body));
}

/// readDump `sources`: every item you own anywhere whose wiki record yields a stone, or that
/// holds one. Where it is, its tier, whether it is worn, what it could be rendered into, and
/// whether the dump printed its sockets at all (a three column storage row does not).
fn sources_view(ui: &mut Ui, dump: &Dump, rows: &[InvRow], cat: &ExaltCat) {
    if dump.sources.is_empty() {
        mark(
            ui,
            State::Idle,
            "nothing in the dump yields an exaltation or holds one",
        );
        return;
    }
    let worn = dump.sources.iter().filter(|s| s.worn).count();
    let unknown = dump.sources.iter().filter(|s| !s.sockets_known).count();
    ui.label(RichText::new(format!("{} items could be rendered into a stone or hold one · {worn} worn · {unknown} with sockets the dump does not print (storage rows)", dump.sources.len())).color(TEXT_2));
    ui.add_space(4.0);
    egui::Grid::new("exalt-sources")
        .num_columns(6)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for h in ["where", "item", "tier", "classes", "could yield", "sockets"] {
                ui.label(RichText::new(h).color(TEXT_3));
            }
            ui.end_row();
            for src in &dump.sources {
                let r = &rows[src.row];
                ui.label(
                    RichText::new(if src.worn {
                        "worn".to_owned()
                    } else {
                        where_word(r)
                    })
                    .color(TEXT_2),
                );
                match src.item {
                    Some(_) => {
                        ui.label(RichText::new(row_name(rows, src.row)).color(TEXT))
                            .on_hover_text(format!(
                                "slots {}",
                                if src.sl.is_empty() {
                                    "none listed".to_owned()
                                } else {
                                    src.sl.join(", ")
                                }
                            ));
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(row_name(rows, src.row)).color(TEXT));
                            ui.label(RichText::new("no wiki page").color(TEXT_3));
                        });
                    }
                }
                ui.label(
                    RichText::new(format!("+{}", src.tier))
                        .font(FontId::monospace(11.5))
                        .color(TEXT_2),
                );
                ui.label(
                    RichText::new(cls_text(src.cls.as_deref()))
                        .font(FontId::monospace(11.0))
                        .color(TEXT_2),
                );
                let yields: Vec<String> = src
                    .yields
                    .iter()
                    .map(|&i| {
                        format!(
                            "{} {}",
                            cat.stones[i].kind.map(Kind::label).unwrap_or("?"),
                            cat.stones[i].effect
                        )
                    })
                    .collect();
                ui.label(
                    RichText::new(if yields.is_empty() {
                        "nothing of a known kind".to_owned()
                    } else {
                        yields.join(" · ")
                    })
                    .color(if yields.is_empty() { TEXT_3 } else { TEXT }),
                );
                let filled = src.sockets.iter().filter(|s| s.filled).count();
                let open = src.sockets.iter().filter(|s| s.open).count();
                ui.label(
                    RichText::new(if src.sockets_known {
                        format!("{filled} filled of {open} open")
                    } else {
                        "not printed for this row".to_owned()
                    })
                    .color(if src.sockets_known { TEXT_2 } else { TEXT_3 }),
                );
                ui.end_row();
            }
        });
}

fn loose_view(
    ui: &mut Ui,
    dump: &Dump,
    rows: &[InvRow],
    items: &[Item],
    cat: &ExaltCat,
    trio: &[String],
) {
    if dump.loose.is_empty() {
        mark(ui, State::Idle, "no loose exaltation stones in the dump (the Augmentation bin, bags and bank are all empty of them)");
        return;
    }
    ui.label(
        RichText::new(format!(
            "{} loose stones · homes are judged for {}",
            dump.loose.len(),
            trio.join("/")
        ))
        .color(TEXT_2),
    );
    ui.add_space(4.0);
    egui::Grid::new("exalt-loose")
        .num_columns(5)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for h in [
                "where",
                "stone",
                "could carry",
                "fits now",
                "later or never",
            ] {
                ui.label(RichText::new(h).color(TEXT_3));
            }
            ui.end_row();
            for l in &dump.loose {
                let r = &rows[l.row];
                ui.label(RichText::new(where_word(r)).color(TEXT_2));
                let src = strip_decor(&r.name);
                ui.label(RichText::new(&src).color(TEXT));
                let typed: Vec<&Stone> = l
                    .yields
                    .iter()
                    .map(|&i| &cat.stones[i])
                    .filter(|s| s.kind.is_some())
                    .collect();
                match (l.item, typed.is_empty()) {
                    (None, _) => {
                        ui.label(RichText::new("no wiki page for its source").color(TEXT_3));
                    }
                    (Some(_), true) => {
                        ui.label(
                            RichText::new("the wiki lists no effect of a known kind on its source")
                                .color(TEXT_3),
                        );
                    }
                    (Some(_), false) => {
                        let mut s = typed
                            .iter()
                            .map(|t| {
                                format!("{} {}", t.kind.map(Kind::label).unwrap_or("?"), t.effect)
                            })
                            .collect::<Vec<_>>()
                            .join(" · ");
                        if typed.len() > 1 {
                            s.push_str(" (one of these; the dump does not say which)");
                        }
                        ui.label(RichText::new(s).color(TEXT));
                    }
                }
                let mut now = Vec::new();
                let mut later = Vec::new();
                let mut never: Option<&'static str> = None;
                for t in &typed {
                    let h = homes_for(t, &dump.worn);
                    for (wi, f, occupied) in &h.now {
                        if !usable_by(f.cls.as_deref(), trio) {
                            never = Some("none of your classes could wear the result");
                            continue;
                        }
                        let w = &dump.worn[*wi];
                        now.push(format!(
                            "{}: {}{}{}",
                            w.slot,
                            row_name(rows, w.row),
                            if *occupied {
                                " (replacing what is in it)"
                            } else {
                                ""
                            },
                            narrows_text(f)
                        ));
                    }
                    for (wi, f, needs) in &h.later {
                        if !usable_by(f.cls.as_deref(), trio) {
                            continue;
                        }
                        let w = &dump.worn[*wi];
                        later.push(format!(
                            "{}: {} at +{needs}{}",
                            w.slot,
                            row_name(rows, w.row),
                            narrows_text(f)
                        ));
                    }
                    if h.now.is_empty() && h.later.is_empty() && never.is_none() {
                        never = Some(
                            if h.never
                                .iter()
                                .all(|(_, f)| f.why.contains(&"no shared slot"))
                            {
                                "no worn item shares a slot with it"
                            } else {
                                "no worn item in its slot shares a class with it"
                            },
                        );
                    }
                }
                if now.is_empty() {
                    ui.label(RichText::new("nothing worn takes it now").color(TEXT_3));
                } else {
                    ui.label(RichText::new(now.join("; ")).color(TEXT));
                }
                if !later.is_empty() {
                    ui.label(RichText::new(later.join("; ")).color(TEXT_2));
                } else if let Some(n) = never {
                    ui.label(RichText::new(n).color(TEXT_3));
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    let _ = items;
}

/// Where a row is, as a word: the section and the number.
fn where_word(r: &InvRow) -> String {
    match r.section {
        Section::Worn => format!("worn · {}", r.root),
        Section::Bags => r.root.to_lowercase(),
        Section::Bank => r.root.to_lowercase(),
        Section::Exalts => "exaltation bin".into(),
        Section::Storage => "storage".into(),
        Section::Shared => "shared bank".into(),
        Section::Depot => "depot".into(),
        Section::Hoard => "hoard".into(),
        Section::KeyRing => "key ring".into(),
        Section::Other => r.root.to_lowercase(),
    }
}

fn fired_view(ui: &mut Ui, fires: &Fires, dump: &Dump, rows: &[InvRow], log: Option<PathBuf>) {
    match (&fires.file, log) {
        (Some(f), _) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("log").color(TEXT_3));
                ui.label(
                    RichText::new(f.display().to_string())
                        .font(FontId::monospace(11.5))
                        .color(TEXT_2),
                );
            });
        }
        (None, _) => {
            mark(
                ui,
                State::Idle,
                "no log is being tailed, so nothing can be seen going off",
            );
            return;
        }
    }
    if let Some(p) = &fires.problem {
        mark(ui, State::Wrong, p);
    }
    if fires.seeding() {
        mark(ui, State::Working, "reading the log's tail");
        return;
    }
    match fires.seeded {
        Some((_, start)) if start > 0 => {
            ui.label(RichText::new(format!("counted over the last {} of the log ({} lines read), plus everything live since", human_bytes(crate::ingest::TAIL_CAP), fires.lines)).color(TEXT_3));
        }
        _ => {
            ui.label(
                RichText::new(format!(
                    "counted over the whole log ({} lines read), plus everything live since",
                    fires.lines
                ))
                .color(TEXT_3),
            );
        }
    }
    ui.add_space(4.0);
    if fires.counts.is_empty() {
        mark(
            ui,
            State::Idle,
            "no exaltation has been seen going off in this log",
        );
        return;
    }
    let owned: HashSet<String> = dump
        .worn
        .iter()
        .flat_map(|w| {
            w.sockets
                .iter()
                .filter_map(|s| s.stone.as_ref().map(|st| strip_decor(&rows[st.row].name)))
        })
        .collect();
    egui::Grid::new("exalt-fired")
        .num_columns(3)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new("exaltation").color(TEXT_3));
            ui.label(
                RichText::new("fired")
                    .font(FontId::monospace(11.5))
                    .color(TEXT_3),
            );
            ui.label(RichText::new("socketed into something worn").color(TEXT_3));
            ui.end_row();
            let mut list: Vec<(&String, &u32)> = fires.counts.iter().collect();
            list.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            for (n, c) in list {
                ui.label(RichText::new(n).color(TEXT));
                ui.label(
                    RichText::new(format!("{c:>6}"))
                        .font(FontId::monospace(11.5))
                        .color(TEXT),
                );
                ui.label(
                    RichText::new(if owned.contains(n) {
                        "yes"
                    } else {
                        "not in this dump"
                    })
                    .color(if owned.contains(n) { TEXT } else { TEXT_3 }),
                );
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.label(RichText::new("A count says the effect does something. It is not a rate: the client prints nothing when an effect does not go off.").color(TEXT_3));
}

fn human_bytes(b: u64) -> String {
    if b >= 1 << 20 {
        format!("{}MB", b >> 20)
    } else if b >= 1 << 10 {
        format!("{}KB", b >> 10)
    } else {
        format!("{b}B")
    }
}

fn by_slot_view(
    ui: &mut Ui,
    cat: &ExaltCat,
    dump: &Dump,
    trio: &[String],
    slot: &mut String,
    hide_oe: &mut bool,
    filter: &mut String,
) {
    ui.horizontal_wrapped(|ui| {
        for s in gear::SLOTS {
            if ui.selectable_label(slot == s, s).clicked() {
                *slot = s.to_owned();
            }
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(hide_oe, "hide out of era");
        ui.label(RichText::new("filter").color(TEXT_3));
        ui.add(egui::TextEdit::singleline(filter).desired_width(200.0));
    });
    ui.add_space(4.0);
    let all = for_slot(cat, slot, trio, *hide_oe);
    let needle = filter.trim().to_lowercase();
    let list: Vec<&Stone> = all
        .iter()
        .copied()
        .filter(|s| {
            needle.is_empty()
                || s.effect.to_lowercase().contains(&needle)
                || s.item_name.to_lowercase().contains(&needle)
        })
        .collect();
    let worn_here: Vec<&WornItem> = dump.worn.iter().filter(|w| w.slot == *slot).collect();
    ui.label(
        RichText::new(format!(
            "{} effects could sit in something worn at {slot} for {}{}",
            list.len(),
            trio.join("/"),
            if list.len() > BY_SLOT_CAP {
                format!(" · the first {BY_SLOT_CAP} are below")
            } else {
                String::new()
            }
        ))
        .color(TEXT_2),
    );
    if worn_here.is_empty() {
        ui.label(RichText::new(format!("nothing is worn at {slot} in this dump")).color(TEXT_3));
    }
    ui.add_space(4.0);
    egui::Grid::new("exalt-byslot")
        .num_columns(5)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for h in [
                "kind",
                "effect",
                "from",
                "classes",
                "into what you wear here",
            ] {
                ui.label(RichText::new(h).color(TEXT_3));
            }
            ui.end_row();
            for s in list.iter().take(BY_SLOT_CAP) {
                ui.label(RichText::new(s.kind.map(Kind::label).unwrap_or("?")).color(TEXT_2));
                ui.label(RichText::new(&s.effect).color(TEXT));
                /* the stone's own record under the pointer: catalogue id, era, and whether the
                 * source is a two-hander (which decides the off-hand it can never sit in) */
                ui.label(
                    RichText::new(format!(
                        "{}{}",
                        s.item_name,
                        if s.oe { " (out of era)" } else { "" }
                    ))
                    .color(TEXT_2),
                )
                .on_hover_text(format!(
                    "{} · {}{}",
                    s.id,
                    s.era.as_deref().unwrap_or("no era on the wiki"),
                    if s.two_h { " · two-handed source" } else { "" }
                ));
                ui.label(
                    RichText::new(cls_text(s.cls.as_deref()))
                        .font(FontId::monospace(11.0))
                        .color(TEXT_2),
                );
                let mut into = Vec::new();
                for w in &worn_here {
                    let f = fit(
                        s,
                        &Host {
                            cls: w.cls.as_deref(),
                            sl: &w.sl,
                            tier: Some(w.tier),
                        },
                    );
                    if f.ok && usable_by(f.cls.as_deref(), trio) {
                        let open = f.open.unwrap_or(false);
                        into.push(format!(
                            "{}{}{}",
                            w.slot,
                            if open { "" } else { " (after an upgrade)" },
                            narrows_text(&f)
                        ));
                    }
                }
                ui.label(
                    RichText::new(if into.is_empty() {
                        "nothing worn here takes it".to_owned()
                    } else {
                        into.join(", ")
                    })
                    .color(if into.is_empty() { TEXT_3 } else { TEXT }),
                );
                ui.end_row();
            }
        });
}

fn heading(ui: &mut Ui, s: &str) {
    ui.label(
        RichText::new(s)
            .font(crate::fonts::display(12.0))
            .color(GOLD),
    );
    ui.add_space(4.0);
}

/// A leading square in a state colour and a line of text; idle is a hollow ring.
fn mark(ui: &mut Ui, st: State, text: &str) -> egui::Response {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
        let sq = egui::Rect::from_center_size(rect.center(), Vec2::splat(6.0));
        if st == State::Idle {
            ui.painter()
                .rect_stroke(sq, 0.0, Stroke::new(1.0, IDLE), StrokeKind::Middle);
        } else {
            ui.painter().rect_filled(sq, 0.0, st.color());
        }
        let col = if st == State::Wrong { WRONG } else { TEXT };
        ui.label(RichText::new(text).color(col))
    })
    .inner
}

/* ------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::testdata;
    use crate::ingest::parse_rows;

    fn item(json: &str) -> Item {
        serde_json::from_str(json).expect("test item parses")
    }

    /// EVERY ENDING THE REAL LOG CARRIES IS COUNTED AS A FIRE, including one that an enumerated
    /// pattern drops on the floor.
    ///
    /// This reads the tracked fixture rather than hand-written lines deliberately. What is being
    /// pinned is that recognising a fire does NOT depend on knowing how the sentence ends, and
    /// only a real file can carry a wording nobody thought to hand-write. That file holds 962
    /// `(Exaltation)` lines across five items, and one of them, Clawed Knuckle-Ring, only ever
    /// "sparkles": a pattern that lists the endings it expects reports that item as having never
    /// fired at all, which is exactly the silence this assertion exists to break.
    #[test]
    fn fires_count_every_ending_the_real_log_carries() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/princess-night.txt");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} is a tracked fixture: {e}", path.display()));
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();

        let mut f = Fires::default();
        f.feed(&lines);

        assert_eq!(
            f.of("Clawed Knuckle-Ring"),
            Some(8),
            "the 8 lines that end in 'sparkles' are fires like any other"
        );
        assert_eq!(f.of("Serpentine Bracer"), Some(442));
        assert_eq!(f.of("Djarn's Amethyst Ring"), Some(324));
        assert_eq!(f.of("Polished Mithril Mask"), Some(121));
        assert_eq!(f.of("Idol of the Underking"), Some(67));

        let counted: u32 = [
            "Clawed Knuckle-Ring",
            "Serpentine Bracer",
            "Djarn's Amethyst Ring",
            "Polished Mithril Mask",
            "Idol of the Underking",
        ]
        .iter()
        .map(|n| f.of(n).unwrap_or(0))
        .sum();
        assert_eq!(
            counted, 962,
            "every (Exaltation) line in the fixture is accounted for by one of the five items"
        );
    }

    /* ---- sockets ---- */

    #[test]
    fn socket_at_table() {
        let cases: &[(&str, Option<(u32, &str)>)] = &[
            ("Head-Slot7", Some((7, "Head"))),
            ("Ear-Slot10", Some((10, "Ear"))),
            ("Any Slot-Slot2", Some((2, "Any Slot"))),
            ("Fingers-Slot8", Some((8, "Fingers"))),
            ("Finger-Slot7", Some((7, "Finger"))),
            ("Ring-Slot9", Some((9, "Ring"))),
            /* a bag position is not a socket */
            ("General 1-Slot14", None),
            ("Bank3-Slot2", None),
            /* a thing nested inside a bag IS a socket of that thing */
            ("General 1-Slot14-Slot7", Some((7, "General 1-Slot14"))),
            ("Head", None),
            ("", None),
            ("Head-Slot", None),
            ("Head-SlotX", None),
        ];
        for (loc, want) in cases {
            let got = socket_at(loc);
            match want {
                None => assert!(got.is_none(), "{loc} should not be a socket, got {got:?}"),
                Some((n, host)) => {
                    let g = got.unwrap_or_else(|| panic!("{loc} should be a socket"));
                    assert_eq!(g.n, *n, "{loc}");
                    assert_eq!(g.host, *host, "{loc}");
                    assert_eq!(g.kind, type_of(*n), "{loc}");
                }
            }
        }
    }

    #[test]
    fn expected_sockets_table() {
        let cases: &[(u32, Option<u32>, &[u32])] = &[
            (0, None, &[]),
            (0, Some(2), &[2]),
            (1, None, &[7]),
            (1, Some(2), &[2, 7]),
            (2, None, &[7, 8]),
            (3, Some(1), &[1, 7, 8, 9]),
            (4, None, &[7, 8, 9, 10]),
            (6, Some(2), &[2, 7, 8, 9, 10]),
            (10, None, &[7, 8, 9, 10]),
        ];
        for (tier, orn, want) in cases {
            assert_eq!(
                expected_sockets(*tier, *orn),
                want.to_vec(),
                "tier {tier} ornament {orn:?}"
            );
        }
    }

    #[test]
    fn type_of_ids_and_unknowns() {
        assert_eq!(type_of(1), Some(SocketType::Ornament));
        assert_eq!(type_of(2), Some(SocketType::Ornament));
        assert_eq!(type_of(7), Some(SocketType::Effect(Kind::Focus)));
        assert_eq!(type_of(8), Some(SocketType::Effect(Kind::Click)));
        assert_eq!(type_of(9), Some(SocketType::Effect(Kind::Worn)));
        assert_eq!(type_of(10), Some(SocketType::Effect(Kind::Proc)));
        for n in [0, 3, 4, 5, 6, 11] {
            assert_eq!(
                type_of(n),
                None,
                "id {n} has never been seen and must not be invented"
            );
        }
        assert_eq!(SocketType::Effect(Kind::Proc).sort(), 4);
        assert_eq!(SocketType::Ornament.sort(), 0);
    }

    #[test]
    fn grants_reads_each_socket_off_the_record() {
        let it = item(
            r#"{"n":"T","foc":"Summoning Haste I","eff":[{"n":"Hug","m":"clicky","ct":"1.0","l":"5"},{"n":"Vision","m":"worn"},{"n":"Stun","m":"combat","l":"20"}]}"#,
        );
        assert_eq!(grants(&it, 7).unwrap().text, "Summoning Haste I");
        let c = grants(&it, 8).unwrap();
        assert_eq!(
            (c.text.as_str(), c.ct.as_deref(), c.lvl.as_deref()),
            ("Hug", Some("1.0"), Some("5"))
        );
        assert_eq!(grants(&it, 9).unwrap().text, "Vision");
        let p = grants(&it, 10).unwrap();
        assert_eq!((p.text.as_str(), p.lvl.as_deref()), ("Stun", Some("20")));
        /* ornament: the look IS the source item, so nothing is granted */
        assert_eq!(grants(&it, 2), None);
        /* a focus with no foc field falls back to a k:focus effect */
        let f = item(r#"{"n":"T","eff":[{"n":"Haste II","k":"focus"}]}"#);
        assert_eq!(grants(&f, 7).unwrap().text, "Haste II");
        assert_eq!(grants(&f, 8), None);
    }

    #[test]
    fn describe_says_what_it_cannot_say() {
        let it = item(r#"{"n":"T","eff":[{"n":"Hug","m":"clicky","ct":"6.0"}]}"#);
        let d = describe(Some(&it), Some(8), Some("Doll"));
        assert_eq!(d.title, "Click");
        assert_eq!(d.body, "Hug (6.0), click effect from Doll.");
        let d = describe(Some(&it), Some(10), Some("Doll"));
        assert!(
            d.body
                .ends_with("The wiki does not record which effect this one carries."),
            "{}",
            d.body
        );
        let d = describe(Some(&it), Some(1), Some("Doll"));
        assert_eq!(d.body, "Makes this item look like Doll.");
        let d = describe(None, None, Some("Ghost"));
        assert_eq!(
            d.body,
            "Rendered from Ghost. The wiki records no transferable effect on it."
        );
        let d = describe(Some(&it), None, None);
        assert_eq!(d.body, "Click: Hug");
    }

    /* ---- catalogue and fitting ---- */

    #[test]
    fn kind_of_follows_the_wiki_tags() {
        let k = |s: &str| kind_of(&serde_json::from_str::<Value>(s).unwrap());
        assert_eq!(k(r#"{"k":"focus"}"#), Some(Kind::Focus));
        assert_eq!(k(r#"{"m":"combat"}"#), Some(Kind::Proc));
        assert_eq!(k(r#"{"k":"combat_eff"}"#), Some(Kind::Proc));
        assert_eq!(k(r#"{"m":"worn"}"#), Some(Kind::Worn));
        assert_eq!(k(r#"{"k":"worn_eff"}"#), Some(Kind::Worn));
        assert_eq!(k(r#"{"m":"clicky"}"#), Some(Kind::Click));
        assert_eq!(k(r#"{"m":"must_equip"}"#), Some(Kind::Click));
        assert_eq!(k(r#"{"k":"click"}"#), Some(Kind::Click));
        assert_eq!(k(r#"{"k":"effect","ct":"1.0"}"#), Some(Kind::Click));
        assert_eq!(
            k(r#"{"k":"effect"}"#),
            None,
            "the wiki tagged nothing: not claimed"
        );
    }

    #[test]
    fn fit_is_the_intersection_of_class_and_slot() {
        let stone = Stone {
            id: "s".into(),
            item: 0,
            item_name: "Spiky Splintmail".into(),
            kind: Some(Kind::Click),
            effect: "Spike".into(),
            cls: Some(vec!["WAR", "RNG", "SHD", "SHM", "BER"]),
            sl: vec!["Chest".into()],
            two_h: false,
            oe: false,
            era: None,
            deity: false,
        };
        let host_cls = [
            "WAR", "CLR", "PAL", "RNG", "SHD", "DRU", "MNK", "BRD", "ROG", "SHM", "BST", "BER",
        ];
        let f = fit(
            &stone,
            &Host {
                cls: Some(&host_cls),
                sl: &["Chest".to_owned()],
                tier: Some(3),
            },
        );
        assert!(f.ok);
        assert_eq!(f.cls, Some(vec!["WAR", "RNG", "SHD", "SHM", "BER"]));
        assert!(f.narrows_cls);
        assert!(!f.narrows_sl);
        /* the narrowing is a cost the screen prints after the home, in these words */
        assert_eq!(
            narrows_text(&f),
            " (narrows its classes to WAR/RNG/SHD/SHM/BER)"
        );
        assert_eq!(f.open, Some(true));
        /* below the click tier the socket is not open yet */
        let f = fit(
            &stone,
            &Host {
                cls: Some(&host_cls),
                sl: &["Chest".to_owned()],
                tier: Some(1),
            },
        );
        assert_eq!(f.open, Some(false));
        /* no shared slot */
        let f = fit(
            &stone,
            &Host {
                cls: Some(&host_cls),
                sl: &["Legs".to_owned()],
                tier: Some(5),
            },
        );
        assert!(!f.ok);
        assert_eq!(f.why, vec!["no shared slot"]);
        /* no shared class */
        let f = fit(
            &stone,
            &Host {
                cls: Some(&["CLR", "DRU"]),
                sl: &["Chest".to_owned()],
                tier: Some(5),
            },
        );
        assert_eq!(f.why, vec!["no shared class"]);
        /* an any-class host takes the stone's list */
        let f = fit(
            &stone,
            &Host {
                cls: None,
                sl: &["Chest".to_owned()],
                tier: None,
            },
        );
        assert!(f.ok);
        assert!(f.narrows_cls);
        assert_eq!(f.open, None, "a catalogue host has no tier");
        /* a host with two slots and a stone with one: the slot narrowing is said too */
        let f = fit(
            &stone,
            &Host {
                cls: Some(&host_cls),
                sl: &["Chest".to_owned(), "Legs".to_owned()],
                tier: Some(5),
            },
        );
        assert!(f.narrows_sl);
        assert_eq!(
            narrows_text(&f),
            " (narrows its classes to WAR/RNG/SHD/SHM/BER and slots to Chest)"
        );
        let same = fit(
            &stone,
            &Host {
                cls: Some(&["WAR", "RNG", "SHD", "SHM", "BER"]),
                sl: &["Chest".to_owned()],
                tier: Some(5),
            },
        );
        assert_eq!(
            narrows_text(&same),
            "",
            "a stone that takes the host as it is costs nothing to say"
        );
        /* an untyped stone never fits a socket */
        let mut untyped = stone.clone();
        untyped.kind = None;
        let f = fit(
            &untyped,
            &Host {
                cls: None,
                sl: &["Chest".to_owned()],
                tier: Some(9),
            },
        );
        assert!(!f.ok);
        assert_eq!(f.open, Some(false));
    }

    #[test]
    fn usable_by_and_class_text() {
        let trio = vec!["WAR".to_owned(), "CLR".to_owned(), "WIZ".to_owned()];
        assert!(usable_by(None, &trio));
        assert!(usable_by(Some(&["CLR"]), &trio));
        assert!(!usable_by(Some(&["DRU", "SHM"]), &trio));
        assert!(!usable_by(Some(&[]), &trio));
        assert_eq!(cls_text(None), "ALL");
        assert_eq!(cls_text(Some(&[])), "none");
        assert_eq!(
            cls_text(Some(&["WIZ", "WAR"])),
            "WAR WIZ",
            "canonical order, not given order"
        );
        assert_eq!(
            cls_inter(Some(&["WAR", "CLR"]), Some(&["CLR", "WIZ"])),
            Some(vec!["CLR"])
        );
        assert_eq!(cls_inter(None, None), None);
    }

    fn fixture_items() -> Vec<Item> {
        vec![
            item(
                r#"{"n":"Fishbone Earring","t":"Fishbone_Earring","cls":{"all":1},"sl":["Ear"],"eff":[{"n":"Water Breathing","m":"worn"}]}"#,
            ),
            item(
                r#"{"n":"Black Sapphire Electrum Earring","t":"BSEE","cls":{"all":1},"sl":["Ear"],"foc":"Mana Preservation I"}"#,
            ),
            item(
                r#"{"n":"Razing Sword of Skarlon","t":"RSS","cls":{"c":["WAR","PAL","SHD"]},"sl":["Primary"],"skill":"2H Slashing","eff":[{"n":"Raze","m":"combat"},{"n":"Slash","m":"worn"}]}"#,
            ),
            item(
                r#"{"n":"Crazy Cleric Doll","t":"CCD","cls":{"all":1},"sl":["Primary","Secondary"],"eff":[{"n":"Hug","m":"clicky","ct":"1.0"}]}"#,
            ),
            item(
                r#"{"n":"Summoned: Staff of Runes","t":"SSR","cls":{"all":1},"sl":["Primary"],"eff":[{"n":"Rune","m":"clicky"}]}"#,
            ),
            item(
                r#"{"n":"Nobody Ring","t":"NR","cls":{"none":1},"sl":["Fingers"],"eff":[{"n":"Nothing","m":"worn"}]}"#,
            ),
            item(r#"{"n":"Slotless Thing","t":"ST","cls":{"all":1},"sl":[],"foc":"Void"}"#),
            item(
                r#"{"n":"Agilmente's Flute","t":"AF","cls":{"all":1},"sl":["Primary"],"inst":["Wind Resonance","Brass Instruments"],"ex":{"Wind Resonance":"12","Brass Instruments":"10"}}"#,
            ),
            item(
                r#"{"n":"Untyped Charm","t":"UC","cls":{"all":1},"sl":["Charm"],"eff":[{"n":"Mystery","k":"effect"}]}"#,
            ),
            item(r#"{"n":"Plain Helm","t":"PH","cls":{"c":["WAR"]},"sl":["Head"],"st":{"ac":5}}"#),
        ]
    }

    #[test]
    fn build_skips_what_nothing_can_hold_and_files_each_effect_once() {
        let items = fixture_items();
        let cat = ExaltCat::build(&items);
        let names: Vec<(&str, Option<Kind>, &str)> = cat
            .stones
            .iter()
            .map(|s| (s.item_name.as_str(), s.kind, s.effect.as_str()))
            .collect();
        assert!(names.contains(&("Fishbone Earring", Some(Kind::Worn), "Water Breathing")));
        assert!(names.contains(&(
            "Black Sapphire Electrum Earring",
            Some(Kind::Focus),
            "Mana Preservation I"
        )));
        assert!(names.contains(&("Razing Sword of Skarlon", Some(Kind::Proc), "Raze")));
        assert!(names.contains(&("Razing Sword of Skarlon", Some(Kind::Worn), "Slash")));
        assert!(names.contains(&("Crazy Cleric Doll", Some(Kind::Click), "Hug")));
        /* an instrument's resonance is a focus; a tradeskill line is not */
        assert!(names.contains(&("Agilmente's Flute", Some(Kind::Focus), "Wind Resonance 12")));
        assert!(!names
            .iter()
            .any(|(n, _, e)| *n == "Agilmente's Flute" && e.starts_with("Brass")));
        /* untyped: shown, filed under no socket */
        assert!(names.contains(&("Untyped Charm", None, "Mystery")));
        /* skipped outright */
        assert!(
            !names.iter().any(|(n, _, _)| n.starts_with("Summoned:")),
            "conjured items yield nothing"
        );
        assert!(
            !names.iter().any(|(n, _, _)| *n == "Nobody Ring"),
            "a none class item cannot be held"
        );
        assert!(
            !names.iter().any(|(n, _, _)| *n == "Slotless Thing"),
            "no slot, nothing holds it"
        );
        assert!(
            !names.iter().any(|(n, _, _)| *n == "Plain Helm"),
            "no effect, no stone"
        );
        assert!(!cat.by_item.contains_key(&9));
        assert!(
            cat.stones
                .iter()
                .find(|s| s.item_name == "Razing Sword of Skarlon")
                .unwrap()
                .two_h
        );
        /* resolution through the dump's spellings */
        assert_eq!(cat.resolve("Fishbone Earring (Exaltation)"), Some(0));
        assert_eq!(cat.resolve("Razing Sword of Skarlon +10"), Some(2));
        assert_eq!(cat.resolve("fishbone earring"), Some(0));
        assert_eq!(cat.resolve("Nothing Here"), None);
    }

    /// The rows an `/outputfile inventory` dump prints for two earrings and a two-hander.
    const DUMP: &str = "Location\tName\tID\tCount\tSlots\r\n\
Ear\tBlack Sapphire Electrum Earring +4\t14701\t1\t10\r\n\
Ear-Slot7\tEmpty\t0\t0\t0\r\n\
Ear-Slot8\tEmpty\t0\t0\t0\r\n\
Ear-Slot9\tFishbone Earring (Exaltation)\t10313\t1\t10\r\n\
Ear-Slot10\tEmpty\t0\t0\t0\r\n\
Head\tPlain Helm +1\t4851\t1\t10\r\n\
Head-Slot2\tEmpty\t0\t0\t0\r\n\
Head-Slot7\tEmpty\t0\t0\t0\r\n\
Ear\tBlack Sapphire Electrum Earring +5\t14701\t1\t10\r\n\
Ear-Slot7\tBlack Sapphire Electrum Earring (Exaltation)\t14701\t1\t10\r\n\
Ear-Slot8\tEmpty\t0\t0\t0\r\n\
Ear-Slot9\tEmpty\t0\t0\t0\r\n\
Ear-Slot10\tEmpty\t0\t0\t0\r\n\
Primary\tRazing Sword of Skarlon +10\t5412\t1\t10\r\n\
Primary-Slot2\tEmpty\t0\t0\t0\r\n\
Primary-Slot7\tEmpty\t0\t0\t0\r\n\
Primary-Slot8\tCrazy Cleric Doll (Exaltation)\t99\t1\t10\r\n\
Primary-Slot9\tRazing Sword of Skarlon (Exaltation)\t5412\t1\t10\r\n\
Primary-Slot10\tEmpty\t0\t0\t0\r\n\
General 1\tBackpack*\t7\t1\t10\r\n\
General 1-Slot7\tFishbone Earring\t10313\t1\t0\r\n\
Augmentation\tCrazy Cleric Doll (Exaltation)\t99\t1\t0\r\n\
Bank1\tRazing Sword of Skarlon (Exaltation)\t5412\t1\t0\r\n\
Equipment\tFishbone Earring\t10313\r\n";

    #[test]
    fn read_dump_files_sockets_under_the_row_they_follow() {
        let items = fixture_items();
        let cat = ExaltCat::build(&items);
        let rows = parse_rows(DUMP, true);
        let d = read_dump(&rows, &items, &cat);
        assert_eq!(d.worn.len(), 4, "two ears, a head, a primary");
        let ear1 = &d.worn[0];
        let ear2 = &d.worn[2];
        assert_eq!((ear1.slot.as_str(), ear1.tier), ("Ear", 4));
        assert_eq!((ear2.slot.as_str(), ear2.tier), ("Ear", 5));
        /* the first earring's worn socket holds the fishbone; the second's does not */
        assert!(ear1.socket(Kind::Worn).filled);
        assert_eq!(
            ear1.socket(Kind::Worn).stone.as_ref().unwrap().item,
            Some(0)
        );
        assert_eq!(
            cat.stones[ear1
                .socket(Kind::Worn)
                .stone
                .as_ref()
                .unwrap()
                .stone
                .unwrap()]
            .effect,
            "Water Breathing"
        );
        assert!(!ear2.socket(Kind::Worn).filled);
        assert!(ear2.socket(Kind::Worn).empty, "the dump printed Empty");
        /* the second earring holds its OWN focus */
        assert!(ear2.socket(Kind::Focus).filled);
        assert!(ear2.socket(Kind::Focus).own);
        assert!(!ear1.socket(Kind::Focus).own);
        /* open by tier alone */
        let head = &d.worn[1];
        assert!(head.socket(Kind::Focus).open);
        assert!(!head.socket(Kind::Click).open);
        assert!(head.socket(Kind::Focus).empty);
        assert_eq!(
            head.ornament.as_ref().map(|o| (o.n, o.row)),
            Some((2, None))
        );
        /* a bag position is not a socket: the bag has no sockets and the earring in it is a source */
        let prim = &d.worn[3];
        assert!(prim.socket(Kind::Click).filled);
        assert_eq!(
            cat.stones[prim
                .socket(Kind::Click)
                .stone
                .as_ref()
                .unwrap()
                .stone
                .unwrap()]
            .effect,
            "Hug"
        );
        assert!(
            prim.socket(Kind::Worn).own,
            "the sword holds its own worn effect"
        );
        assert!(prim.socket(Kind::Proc).open && prim.socket(Kind::Proc).empty);
        /* loose: the bin and the bank, never the socketed ones */
        assert_eq!(d.loose.len(), 2);
        assert!(d.loose.iter().all(|l| rows[l.row].exalt));
        /* sources: worn items with yields or filled sockets, the bag earring, the storage earring */
        assert!(d.sources.iter().any(|s| s.row == 20 && !s.worn));
        let storage = d
            .sources
            .iter()
            .find(|s| s.row == 23)
            .expect("storage row is a source");
        assert!(
            !storage.sockets_known,
            "a three column storage row reports sockets unknown"
        );
    }

    #[test]
    fn homes_for_now_later_never() {
        let items = fixture_items();
        let cat = ExaltCat::build(&items);
        let rows = parse_rows(DUMP, true);
        let d = read_dump(&rows, &items, &cat);
        let fishbone = cat
            .stones
            .iter()
            .position(|s| s.effect == "Water Breathing")
            .unwrap();
        let h = homes_for(&cat.stones[fishbone], &d.worn);
        /* both earrings are +4 or better: the worn socket is open on both; ear 1 is occupied */
        assert_eq!(h.now.len(), 2);
        assert!(h.now[0].2, "ear 1's worn socket is occupied");
        assert!(!h.now[1].2);
        assert!(h.later.is_empty());
        assert_eq!(h.never.len(), 2, "head and primary share no slot");
        /* the doll's click fits the primary (open at +10) and nothing else */
        let hug = cat.stones.iter().position(|s| s.effect == "Hug").unwrap();
        let h = homes_for(&cat.stones[hug], &d.worn);
        assert_eq!(h.now.len(), 1);
        assert_eq!(d.worn[h.now[0].0].slot, "Primary");
        /* a focus onto the +1 helm is open; a proc onto it waits for +4 */
        let raze = cat.stones.iter().position(|s| s.effect == "Raze").unwrap();
        let h = homes_for(&cat.stones[raze], &d.worn);
        assert!(h.now.iter().any(|(w, _, _)| d.worn[*w].slot == "Primary"));
        let mut sword_focus = cat.stones[raze].clone();
        sword_focus.kind = Some(Kind::Focus);
        sword_focus.sl = vec!["Head".into()];
        sword_focus.cls = None;
        let h = homes_for(&sword_focus, &d.worn);
        assert_eq!(h.now.len(), 1);
        sword_focus.kind = Some(Kind::Proc);
        let h = homes_for(&sword_focus, &d.worn);
        assert_eq!(h.later.len(), 1);
        assert_eq!(h.later[0].2, 4);
    }

    #[test]
    fn for_slot_filters_by_slot_trio_era_and_type() {
        let mut items = fixture_items();
        items.push(item(
            r#"{"n":"Old Ear","t":"OE","cls":{"c":["DRU"]},"sl":["Ear"],"oe":1,"foc":"Ancient"}"#,
        ));
        let cat = ExaltCat::build(&items);
        let none: Vec<String> = Vec::new();
        let ears = for_slot(&cat, "Ear", &none, false);
        assert_eq!(ears.len(), 3);
        let ears = for_slot(&cat, "Ear", &none, true);
        assert_eq!(ears.len(), 2, "out of era hidden");
        let trio = vec!["WAR".to_owned(), "CLR".to_owned(), "WIZ".to_owned()];
        let ears = for_slot(&cat, "Ear", &trio, false);
        assert_eq!(ears.len(), 2, "the druid-only stone is out for this trio");
        assert!(
            for_slot(&cat, "Charm", &none, false).is_empty(),
            "an untyped effect fits no socket"
        );
        assert!(for_slot(&cat, "Primary", &trio, false)
            .iter()
            .any(|s| s.effect == "Raze"));
    }

    #[test]
    fn fire_lines_are_counted_and_nothing_else_is() {
        let lines = vec![
            "[Sat Aug 15 20:11:02 2026] Your Idol of the Underking (Exaltation) feels alive with power.".to_owned(),
            "[Sat Aug 15 20:11:03 2026] Your Fishbone Earring (Exaltation) shimmers briefly.".to_owned(),
            "[Sat Aug 15 20:11:04 2026] Your Fishbone Earring (Exaltation) pulses with light as your vision sharpens.".to_owned(),
            "[Sat Aug 15 20:11:05 2026] Your Fishbone Earring feels alive with power.".to_owned(),
            "[Sat Aug 15 20:11:06 2026] Someone's Fishbone Earring (Exaltation) shimmers briefly.".to_owned(),
            "Your Idol of the Underking (Exaltation) feels alive with power.".to_owned(),
        ];
        assert_eq!(fire_line(&lines[0]), Some("Idol of the Underking"));
        assert_eq!(fire_line(&lines[3]), None);
        assert_eq!(fire_line(&lines[4]), None);
        assert_eq!(fire_line(&lines[5]), None, "no timestamp, not a log line");
        let mut f = Fires::default();
        f.feed(&lines);
        assert_eq!(f.of("Fishbone Earring"), Some(2));
        assert_eq!(f.of("Idol of the Underking"), Some(1));
        assert_eq!(f.of("Nothing"), None);
        assert_eq!(f.lines, 6);
    }

    /* ---- the real file ---- */

    #[test]
    fn the_real_catalogue_yields_the_measured_stones() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let cat = ExaltCat::build(&s.items);
        assert!(
            cat.stones.len() > 1000,
            "{} stones; the snapshot has 1093 items with effects and 133 with a focus",
            cat.stones.len()
        );
        let doll = cat
            .resolve("Crazy Cleric Doll")
            .expect("Crazy Cleric Doll is in gear-data");
        let y = cat.yields_of(Some(doll));
        assert!(
            y.iter()
                .any(|&i| cat.stones[i].kind == Some(Kind::Click) && cat.stones[i].effect == "Hug"),
            "the doll's click is Hug"
        );
        let orb = cat.resolve("A Shimmering Orb").expect("in gear-data");
        assert!(cat
            .yields_of(Some(orb))
            .iter()
            .any(|&i| cat.stones[i].kind == Some(Kind::Focus)
                && cat.stones[i].effect == "Summoning Haste I"));
        let flute = cat
            .resolve("Agilmente's Flute of Flight")
            .expect("in gear-data");
        assert!(
            cat.yields_of(Some(flute))
                .iter()
                .any(|&i| cat.stones[i].effect == "Wind Resonance 12"),
            "an instrument's resonance is a focus stone"
        );
        assert!(!cat
            .stones
            .iter()
            .any(|st| st.item_name.starts_with("Summoned:")));
        let kinds: HashSet<Option<Kind>> = cat.stones.iter().map(|st| st.kind).collect();
        assert!(
            kinds.contains(&Some(Kind::Proc))
                && kinds.contains(&Some(Kind::Worn))
                && kinds.contains(&Some(Kind::Click))
                && kinds.contains(&Some(Kind::Focus))
        );
    }
}
