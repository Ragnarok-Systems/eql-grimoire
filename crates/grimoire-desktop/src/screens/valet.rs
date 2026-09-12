//! Screen: valet. Decision D9: "get dressed", one slot at a time, from what you own.
//!
//! WHAT IS HERE. The slot order, the owned corpus, the walk, the remembered-decision rule, the
//! loadout arithmetic, the by-slot view, the catalogue gap and the exaltation audit: the RULES and
//! the DATA MODEL, with the drawing below them. The scorer it prices with is the gear lane's
//! (`crate::screens::gear::Scorer`), so a number here is the number the Gear screen prints for
//! the same item, by construction. Every rule carries a test below that fails without it.
//!
//! THE WALK IS ORDER DEPENDENT ON PURPOSE. Each candidate prices its own AC against the softcap
//! position the loadout being built will actually reach, or the twelfth piece is credited at the
//! first piece's rate. That makes the answer depend on the order the slots are asked in, which is
//! why the order is fixed and written down (ORDER) rather than chosen per run. Any Slot goes last
//! because it carries no slot restriction: ranked earlier it would eat the best chest piece.
//!
//! WHAT IS ON YOUR BODY IS LEGAL. The class lists in gear-data are wrong often enough to matter,
//! so a worn item is a candidate whatever the wiki says. The game let you equip it; that outranks
//! a table.
//!
//! WHAT IS NOT HERE. The stat priorities panel lives on the Gear screen, in one place, and this
//! screen reads the same overrides. Race is not in the settings contract, so the scorer runs its
//! no-race path.

use crate::chrome::State;
use crate::ingest::{InvRow, InventoryDump, Section};
use crate::screens::exalt::{self, Dump as ExaltDump, ExaltCat, Kind};
use crate::screens::gear::{
    self, ac_of, read_overrides, stat_at, CatCache, Catalogue, CharState, EqEntry, Equipped,
    GearRec, Ranker, Scorer, ANY, SLOTS,
};
use crate::screens::Cx;
use crate::theme::*;
use egui::{FontId, RichText, Stroke, StrokeKind, Ui, Vec2};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::LazyLock;

/* ------------------------------------------------------------------ the order -- */

/// Body order down the character sheet. The weapon hands and the two Any
/// Slot positions follow it as their own steps.
pub const ORDER: [&str; 20] = [
    "Head",
    "Face",
    "Ear",
    "Ear",
    "Neck",
    "Shoulders",
    "Arms",
    "Back",
    "Wrist",
    "Wrist",
    "Hands",
    "Chest",
    "Legs",
    "Feet",
    "Waist",
    "Fingers",
    "Fingers",
    "Charm",
    "Range",
    "Ammo",
];

/// Reading order for a fetch list: the trip first (bags, storage, bank), the things already on
/// your body last, because those are the rows with nothing to do. The key ring is not a place you
/// walk to, so it sorts ahead of everything.
pub const SEC_ORDER: [Section; 9] = [
    Section::Bags,
    Section::Storage,
    Section::Exalts,
    Section::Bank,
    Section::Shared,
    Section::Depot,
    Section::Hoard,
    Section::Other,
    Section::Worn,
];

pub fn sec_rank(sec: Section) -> i32 {
    SEC_ORDER
        .iter()
        .position(|s| *s == sec)
        .map(|p| p as i32)
        .unwrap_or(-1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    Slot,
    Primary,
    Secondary,
    Any,
}

/// One question the walk asks. `n` is the position for a paired slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub key: String,
    pub kind: StepKind,
    pub slot: String,
    pub label: String,
    pub n: usize,
}

/// The steps: twenty body steps, then the two hands as two steps (a card holding
/// two items reads as one recommendation), then Any Slot twice.
pub fn build_steps() -> Vec<Step> {
    let mut steps = Vec::new();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for slot in ORDER {
        let n = seen.entry(slot).or_insert(0);
        *n += 1;
        steps.push(Step {
            key: format!("{slot}#{n}"),
            kind: StepKind::Slot,
            slot: slot.to_owned(),
            label: if *n > 1 {
                format!("{slot} ({n})")
            } else {
                slot.to_owned()
            },
            n: *n,
        });
    }
    steps.push(Step {
        key: "Primary#1".into(),
        kind: StepKind::Primary,
        slot: "Primary".into(),
        label: "Main hand".into(),
        n: 1,
    });
    steps.push(Step {
        key: "Secondary#1".into(),
        kind: StepKind::Secondary,
        slot: "Secondary".into(),
        label: "Off hand".into(),
        n: 1,
    });
    steps.push(Step {
        key: format!("{ANY}#1"),
        kind: StepKind::Any,
        slot: ANY.into(),
        label: "Any Slot (1)".into(),
        n: 1,
    });
    steps.push(Step {
        key: format!("{ANY}#2"),
        kind: StepKind::Any,
        slot: ANY.into(),
        label: "Any Slot (2)".into(),
        n: 2,
    });
    steps
}

/// The by-slot picker's list: twenty slots in sheet order, the hands
/// named as the walk names them. Power Source is not offered: no dump has carried one.
pub fn browse() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for s in ORDER {
        if !out.iter().any(|(k, _)| k == s) {
            out.push((s.to_owned(), s.to_owned()));
        }
    }
    out.push(("Primary".into(), "Main hand".into()));
    out.push(("Secondary".into(), "Off hand".into()));
    out.push((ANY.into(), ANY.into()));
    out
}

/* ------------------------------------------------------------- the owned corpus -- */

/// One physical copy you own, with its gear record attached.
/// Two Six Note Blades in storage are two rows and can fill two slots. A stone never resolves:
/// it is named after the item it was rendered from and would otherwise be offered as a chest
/// piece you do not own.
#[derive(Clone, Debug)]
pub struct VRow {
    /// Index into the dump's `all` rows (the exaltation reader's row identity).
    pub i: usize,
    pub loc: String,
    pub root: String,
    pub sec: Section,
    pub name: String,
    pub base: String,
    pub tier: u32,
    pub exalt: bool,
    pub rec: Option<usize>,
}

impl VRow {
    /// Which worn slot this row sits in, when it is worn.
    pub fn worn_slot(&self) -> Option<&'static str> {
        gear::worn_slot(&self.root)
    }
}

/// The nth thing worn in a slot, paired with ITS dump row.
#[derive(Clone, Debug)]
pub struct WornRef {
    pub slot: String,
    /// Index into `Corpus::rows`.
    pub row: usize,
}

/// What reading the dump into owned rows gives back.
#[derive(Clone, Debug, Default)]
pub struct Corpus {
    /// Every non-empty row, dump order.
    pub rows: Vec<VRow>,
    /// The worn set, for the scorer.
    pub equipped: Equipped,
    /// Worn entries in dump order, each pointing at its row.
    pub worn: Vec<WornRef>,
    /// Rows the wiki has no record of, so they cannot be candidates for anything.
    pub unmatched: usize,
    /// The dangerous ones: worn pieces with no record, (slot, name).
    pub worn_unknown: Vec<(String, String)>,
}

impl Corpus {
    pub fn read(dump: &InventoryDump, cat: &Catalogue) -> Corpus {
        let mut rows = Vec::new();
        let mut by_all: HashMap<usize, usize> = HashMap::new();
        for (i, r) in dump.all.iter().enumerate() {
            if r.empty {
                continue;
            }
            let rec = if r.exalt { None } else { cat.find(&r.base) };
            by_all.insert(i, rows.len());
            rows.push(VRow {
                i,
                loc: r.loc.clone(),
                root: r.root.clone(),
                sec: r.section,
                name: r.name.clone(),
                base: r.base.clone(),
                tier: u32::from(r.tier),
                exalt: r.exalt,
                rec,
            });
        }
        let mut equipped = Equipped::empty();
        let mut worn = Vec::new();
        let mut worn_unknown = Vec::new();
        for w in &dump.worn {
            let rec = cat.find(&w.base);
            equipped.push(
                &w.slot,
                EqEntry {
                    name: w.name.clone(),
                    base: w.base.clone(),
                    tier: u32::from(w.tier),
                    rec,
                },
            );
            if let Some(&row) = by_all.get(&w.row) {
                worn.push(WornRef {
                    slot: w.slot.clone(),
                    row,
                });
            }
            if rec.is_none() {
                worn_unknown.push((w.slot.clone(), w.name.clone()));
            }
        }
        let unmatched = rows.iter().filter(|r| !r.exalt && r.rec.is_none()).count();
        Corpus {
            rows,
            equipped,
            worn,
            unmatched,
            worn_unknown,
        }
    }

    /// The nth (1-based) worn thing in a slot: its entry and its row.
    pub fn wearing_at(&self, slot: &str, n: usize) -> Option<(&EqEntry, usize)> {
        let e = self.equipped.get(slot).get(n.checked_sub(1)?)?;
        let row = self.worn.iter().filter(|w| w.slot == slot).nth(n - 1)?.row;
        Some((e, row))
    }
}

/// Identity for a remembered: the ITEM at its tier. Row order
/// changes with every new dump; "Belt of Virtue +3" does not.
pub fn item_id(row: &VRow, cat: &Catalogue) -> String {
    let key = match row.rec {
        Some(r) => cat.recs[r].key.clone(),
        None => row.base.to_lowercase(),
    };
    format!("{key}+{}", row.tier)
}

/* ------------------------------------------------------------------ the walk -- */

/// One thing an option puts on, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct OptItem {
    pub row: usize,
    pub slot: String,
}

/// One candidate for a step.
#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub id: String,
    pub items: Vec<OptItem>,
    pub score: f64,
}

/// What the walk settled without asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Auto {
    /// Nothing you own fits.
    NoneFit,
    /// One candidate.
    Only,
    /// A remembered decision stood.
    Mem,
    /// Left empty on purpose.
    Skip,
    /// Nothing the scorer reads separates the candidates: kept what is on.
    Flat,
    /// The off hand is held by the two-hander.
    TwoH,
}

impl Auto {
    pub const ALL: [Auto; 6] = [
        Auto::Mem,
        Auto::Only,
        Auto::Flat,
        Auto::TwoH,
        Auto::Skip,
        Auto::NoneFit,
    ];
}

/// What a step was priced in.
#[derive(Clone, Debug, Default)]
pub struct StepCtx {
    pub taken: HashSet<usize>,
    pub lore: HashSet<usize>,
    pub base_ac: f64,
}

/// One remembered decision: the pick, the four it beat, and the fourth
/// one's score.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemRec {
    pub pick: String,
    pub set: Vec<String>,
    pub floor: f64,
}

/// The remembered decisions for one trio. The screen persists it in settings.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Memory {
    pub map: BTreeMap<String, MemRec>,
    #[serde(skip)]
    pub dirty: bool,
}

impl Memory {
    pub fn get(&self, key: &str) -> Option<&MemRec> {
        self.map.get(key)
    }
    pub fn set(&mut self, key: &str, rec: MemRec) {
        self.map.insert(key.to_owned(), rec);
        self.dirty = true;
    }
    pub fn del(&mut self, key: &str) {
        if self.map.remove(key).is_some() {
            self.dirty = true;
        }
    }
    pub fn clear(&mut self) {
        if !self.map.is_empty() {
            self.dirty = true;
        }
        self.map.clear();
    }
}

/// What the walk prices with.
pub struct Env<'a> {
    pub corpus: &'a Corpus,
    pub cat: &'a Catalogue,
    pub sc: &'a Scorer,
}

impl Env<'_> {
    fn rec(&self, row: usize) -> Option<&GearRec> {
        self.corpus.rows[row].rec.map(|r| &self.cat.recs[r])
    }
}

/// Two copies of the same item at the same tier are the same offer; keep one. Ties break toward
/// what you already have on: nothing in the scorer reads an arrow, so every quiver scores 0 and
/// the sort order alone decided which one to go dig out of the bank. A tie is not a reason to
/// get up.
fn dedupe(env: &Env, mut list: Vec<Opt>) -> Vec<Opt> {
    let on_you = |o: &Opt| {
        o.items
            .iter()
            .any(|it| env.corpus.rows[it.row].sec == Section::Worn)
    };
    list.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| on_you(b).cmp(&on_you(a)))
            .then_with(|| {
                env.corpus.rows[b.items[0].row]
                    .tier
                    .cmp(&env.corpus.rows[a.items[0].row].tier)
            })
    });
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for o in list {
        if seen.insert(o.id.clone()) {
            out.push(o);
        }
    }
    out
}

fn mk(env: &Env, items: Vec<OptItem>, base_ac: f64) -> Opt {
    let mut ac = base_ac;
    let mut score = 0.0;
    for it in &items {
        if let Some(rec) = env.rec(it.row) {
            let tier = env.corpus.rows[it.row].tier;
            score += env.sc.score(rec, tier, ac, &it.slot);
            ac += ac_of(rec, tier);
        }
    }
    let mut ids: Vec<String> = items
        .iter()
        .map(|it| item_id(&env.corpus.rows[it.row], env.cat))
        .collect();
    ids.sort();
    Opt {
        id: ids.join(" | "),
        items,
        score,
    }
}

/// Options for one step, given what earlier steps already took.
pub fn options_for(
    env: &Env,
    step: &Step,
    taken: &HashSet<usize>,
    lore: &HashSet<usize>,
    base_ac: f64,
) -> Vec<Opt> {
    let dw = env.sc.dual_wield_cap() > 0;
    let usable = |i: usize, r: &VRow| -> Option<&GearRec> {
        let rec = env.rec(i)?;
        if taken.contains(&i) {
            return None;
        }
        if r.rec.is_some_and(|k| lore.contains(&k)) {
            return None;
        }
        if env.sc.legal(rec) || r.sec == Section::Worn {
            Some(rec)
        } else {
            None
        }
    };
    let mut pool: Vec<Opt> = Vec::new();
    for (i, r) in env.corpus.rows.iter().enumerate() {
        let Some(rec) = usable(i, r) else { continue };
        let fits = match step.kind {
            StepKind::Primary => rec.sl.iter().any(|s| s == "Primary"),
            /* The off hand's damage gate has the same worn escape as the class one: the game
             * lets a hand hold what it cannot swing. */
            StepKind::Secondary => {
                rec.sl.iter().any(|s| s == "Secondary")
                    && !rec.is_two_h()
                    && (dw || rec.dmg.is_none() || r.worn_slot() == Some("Secondary"))
            }
            /* Any Slot takes anything the trio can equip. It is scored AS Any Slot, so a weapon
             * parked there gets no ratio: it is not in a hand and does not swing. */
            StepKind::Any => !rec.sl.is_empty(),
            StepKind::Slot => rec.sl.contains(&step.slot),
        };
        if fits {
            pool.push(mk(
                env,
                vec![OptItem {
                    row: i,
                    slot: step.slot.clone(),
                }],
                base_ac,
            ));
        }
    }
    dedupe(env, pool)
}

/// The two-hander question: the best 2H on its own against the
/// best main+off pair, both priced at the same AC base.
#[derive(Clone, Debug, Default)]
pub struct WeaponCompare {
    pub best_2h: Option<Opt>,
    /// (total, main row, off row)
    pub best_pair: Option<(f64, usize, usize)>,
}

fn weapon_compare(
    env: &Env,
    taken: &HashSet<usize>,
    lore: &HashSet<usize>,
    base_ac: f64,
) -> WeaponCompare {
    let prim_step = Step {
        key: "Primary#1".into(),
        kind: StepKind::Primary,
        slot: "Primary".into(),
        label: "Main hand".into(),
        n: 1,
    };
    let sec_step = Step {
        key: "Secondary#1".into(),
        kind: StepKind::Secondary,
        slot: "Secondary".into(),
        label: "Off hand".into(),
        n: 1,
    };
    let prim = options_for(env, &prim_step, taken, lore, base_ac);
    let is_2h = |o: &Opt| env.rec(o.items[0].row).is_some_and(GearRec::is_two_h);
    let best_2h = prim.iter().find(|o| is_2h(o)).cloned();
    let mut best_pair = None;
    for p in &prim {
        if is_2h(p) {
            continue;
        }
        let mut after = taken.clone();
        after.insert(p.items[0].row);
        let ac = env
            .rec(p.items[0].row)
            .map(|r| ac_of(r, env.corpus.rows[p.items[0].row].tier))
            .unwrap_or(0.0);
        let secs = options_for(env, &sec_step, &after, lore, base_ac + ac);
        if secs.is_empty() {
            continue;
        }
        best_pair = Some((
            p.score + secs[0].score,
            p.items[0].row,
            secs[0].items[0].row,
        ));
        break; /* prim is score-sorted; the best main hand fixes the best pair */
    }
    WeaponCompare { best_2h, best_pair }
}

/// The walk itself.
pub struct Walk {
    pub steps: Vec<Step>,
    pub i: usize,
    pub picks: HashMap<String, Option<Opt>>,
    pub opts: HashMap<String, Vec<Opt>>,
    pub auto: HashMap<String, Auto>,
    pub ctx: HashMap<String, StepCtx>,
    pub skipped: HashSet<String>,
    pub mem: Memory,
}

/// One piece the plan puts on, with the number it was priced at and the AC base it was priced
/// against, so a host can say what the swap is worth without re-walking the loadout.
#[derive(Clone, Debug)]
pub struct PlanItem {
    pub step: usize,
    pub slot: String,
    pub row: usize,
    pub score: f64,
    pub base_ac: f64,
}

/// A step the walk could not fill that already has something on it: almost always a worn piece
/// with no wiki page. It carries no score, because an item with no record cannot be priced.
#[derive(Clone, Debug)]
pub struct Kept {
    pub step: usize,
    pub slot: String,
    pub row: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Loadout {
    pub score: f64,
    pub st: BTreeMap<String, i32>,
    pub items: Vec<PlanItem>,
    pub kept: Vec<Kept>,
}

/// What is worn now, priced the same way.
#[derive(Clone, Debug)]
pub struct CurItem {
    pub slot: String,
    pub name: String,
    pub rec: usize,
    pub tier: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Current {
    pub score: f64,
    pub st: BTreeMap<String, i32>,
    pub items: Vec<CurItem>,
}

impl Walk {
    pub fn new(env: &Env, mem: Memory) -> Walk {
        let mut w = Walk {
            steps: build_steps(),
            i: 0,
            picks: HashMap::new(),
            opts: HashMap::new(),
            auto: HashMap::new(),
            ctx: HashMap::new(),
            skipped: HashSet::new(),
            mem,
        };
        w.advance(env, 0);
        w
    }

    pub fn done(&self) -> bool {
        self.i >= self.steps.len()
    }

    pub fn step(&self) -> Option<&Step> {
        self.steps.get(self.i)
    }

    pub fn options(&self, key: &str) -> &[Opt] {
        self.opts.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn ctx(&self, key: &str) -> Option<&StepCtx> {
        self.ctx.get(key)
    }

    fn pick_of(&self, key: &str) -> Option<&Opt> {
        self.picks.get(key).and_then(|p| p.as_ref())
    }

    /// The 2H-vs-pair line for the Main hand step, priced in that step's own context. None
    /// anywhere else.
    pub fn weapon_compare(&self, env: &Env, key: &str) -> Option<WeaponCompare> {
        if key != "Primary#1" {
            return None;
        }
        let c = self.ctx.get(key)?;
        Some(weapon_compare(env, &c.taken, &c.lore, c.base_ac))
    }

    /// Remembering a pick means "this item beats the others": the slot stops asking until an
    /// item that beats at least one of those four turns up.
    fn remember(&mut self, key: &str, opt: &Opt) {
        let top: Vec<&Opt> = self.options(key).iter().take(4).collect();
        let rec = MemRec {
            pick: opt.id.clone(),
            set: top.iter().map(|o| o.id.clone()).collect(),
            floor: top.last().map(|o| o.score).unwrap_or(0.0),
        };
        self.mem.set(key, rec);
    }

    /// A decision stands while the pick is still owned and nothing outside the remembered four
    /// scores above that floor. Anything else re-opens the slot.
    fn mem_apply(&self, key: &str, list: &[Opt]) -> Option<Opt> {
        let m = self.mem.get(key)?;
        let pick = list.iter().find(|o| o.id == m.pick)?;
        if list
            .iter()
            .any(|o| !m.set.contains(&o.id) && o.score > m.floor)
        {
            return None;
        }
        Some(pick.clone())
    }

    /// Recompute every step from the first, because a pick changes what is left for every later
    /// one. Steps that need no decision are settled in passing. The walk stops at the first step
    /// that is a real, still-open choice.
    pub fn advance(&mut self, env: &Env, from: usize) {
        let mut taken: HashSet<usize> = HashSet::new();
        let mut lore: HashSet<usize> = HashSet::new();
        let mut base_ac = 0.0;
        self.opts.clear();
        self.auto.clear();
        self.ctx.clear();
        for i in 0..self.steps.len() {
            let st = self.steps[i].clone();
            let list = options_for(env, &st, &taken, &lore, base_ac);
            self.ctx.insert(
                st.key.clone(),
                StepCtx {
                    taken: taken.clone(),
                    lore: lore.clone(),
                    base_ac,
                },
            );
            let mut chosen: Option<Opt> = if i < from {
                self.pick_of(&st.key).cloned()
            } else {
                None
            };
            if chosen
                .as_ref()
                .is_some_and(|c| !list.iter().any(|o| o.id == c.id))
            {
                chosen = None;
            }
            /* A two-hander occupies both hands, so the off-hand step is not a question, and it
             * must not read as one the walk forgot. */
            let main_2h = self
                .pick_of("Primary#1")
                .is_some_and(|p| env.rec(p.items[0].row).is_some_and(GearRec::is_two_h));
            if st.kind == StepKind::Secondary && main_2h {
                self.opts.insert(st.key.clone(), list);
                self.auto.insert(st.key.clone(), Auto::TwoH);
                self.picks.insert(st.key.clone(), None);
                continue;
            }
            if chosen.is_none() {
                if list.is_empty() {
                    self.auto.insert(st.key.clone(), Auto::NoneFit);
                } else if list[0].score <= 0.0 && env.corpus.wearing_at(&st.slot, st.n).is_some() {
                    /* Nothing the scorer reads: every candidate comes out at zero, so the ranking
                     * is arbitrary and a swap cannot be an improvement. Ammo is the whole class of
                     * this. Keep what is on and say so. */
                    self.auto.insert(st.key.clone(), Auto::Flat);
                } else if list.len() == 1 {
                    self.auto.insert(st.key.clone(), Auto::Only);
                    chosen = Some(list[0].clone());
                } else if let Some(m) = self.mem_apply(&st.key, &list) {
                    self.auto.insert(st.key.clone(), Auto::Mem);
                    chosen = Some(m);
                }
            }
            if chosen.is_none() && self.skipped.contains(&st.key) {
                self.auto.insert(st.key.clone(), Auto::Skip);
            }
            /* Stop only on a step nothing above settled. */
            if chosen.is_none() && !self.auto.contains_key(&st.key) && list.len() > 1 {
                self.opts.insert(st.key.clone(), list);
                self.picks.insert(st.key.clone(), None);
                self.i = i;
                return;
            }
            self.opts.insert(st.key.clone(), list);
            if let Some(c) = &chosen {
                for it in &c.items {
                    taken.insert(it.row);
                    let row = &env.corpus.rows[it.row];
                    if let Some(r) = row.rec {
                        if env.cat.recs[r].fl.iter().any(|f| f == "lore") {
                            lore.insert(r);
                        }
                        base_ac += ac_of(&env.cat.recs[r], row.tier);
                    }
                }
            }
            self.picks.insert(st.key.clone(), chosen);
        }
        self.i = self.steps.len();
    }

    pub fn pick(&mut self, env: &Env, key: &str, opt: Opt, keep: bool) {
        let Some(i) = self.steps.iter().position(|s| s.key == key) else {
            return;
        };
        if keep {
            self.remember(key, &opt);
        } else {
            self.mem.del(key);
        }
        self.picks.insert(key.to_owned(), Some(opt));
        self.advance(env, i + 1);
    }

    /// Leaving a slot empty is a decision the walk has to honour, so the skipped step is recorded
    /// and the walk resumes AFTER it.
    pub fn skip(&mut self, env: &Env) {
        let at = self.i;
        let Some(st) = self.steps.get(at) else { return };
        self.skipped.insert(st.key.clone());
        self.advance(env, at + 1);
        if self.i == at {
            self.i = at + 1;
        }
    }

    /// Back goes to the previous step that was a real choice. Landing on a remembered step
    /// forgets it; you came back to change it.
    pub fn back(&mut self, env: &Env) -> bool {
        for j in (0..self.i.min(self.steps.len())).rev() {
            let key = self.steps[j].key.clone();
            if self.options(&key).len() > 1 {
                self.mem.del(&key);
                self.advance(env, j);
                return true;
            }
        }
        false
    }

    pub fn restart(&mut self, env: &Env) {
        self.picks.clear();
        self.skipped.clear();
        self.advance(env, 0);
    }

    /// What the walk settled without asking, so a slot it never showed you does not read as a
    /// slot it forgot.
    pub fn auto_summary(&self) -> Vec<(Auto, Vec<String>)> {
        Auto::ALL
            .iter()
            .map(|a| {
                (
                    *a,
                    self.steps
                        .iter()
                        .filter(|s| self.auto.get(&s.key) == Some(a))
                        .map(|s| s.label.clone())
                        .collect(),
                )
            })
            .collect()
    }

    /// Both loadouts walk the same slot list with AC accumulating in the same order, which is
    /// what makes "wearing now" and "this loadout" comparable numbers rather than two sums.
    pub fn plan_loadout(&self, env: &Env) -> Loadout {
        let mut out = Loadout::default();
        let mut ac = 0.0;
        for (si, step) in self.steps.iter().enumerate() {
            let Some(p) = self.pick_of(&step.key) else {
                if let Some((_, row)) = env.corpus.wearing_at(&step.slot, step.n) {
                    out.kept.push(Kept {
                        step: si,
                        slot: step.slot.clone(),
                        row,
                    });
                }
                continue;
            };
            for it in &p.items {
                let row = &env.corpus.rows[it.row];
                let Some(rec) = env.rec(it.row) else { continue };
                let s = env.sc.score(rec, row.tier, ac, &it.slot);
                out.score += s;
                out.items.push(PlanItem {
                    step: si,
                    slot: it.slot.clone(),
                    row: it.row,
                    score: s,
                    base_ac: ac,
                });
                ac += ac_of(rec, row.tier);
                for (k, v) in rec.stats_at(row.tier) {
                    *out.st.entry(k.to_owned()).or_insert(0) += v;
                }
            }
        }
        out
    }

    pub fn current_loadout(&self, env: &Env) -> Current {
        let mut out = Current::default();
        let mut ac = 0.0;
        let mut used: HashMap<&str, usize> = HashMap::new();
        let slots: Vec<&str> = ORDER
            .iter()
            .copied()
            .chain(["Primary", "Secondary", ANY, ANY])
            .collect();
        for slot in slots {
            let idx = used.entry(slot).or_insert(0);
            let e = env.corpus.equipped.get(slot).get(*idx).cloned();
            *idx += 1;
            let Some(e) = e else { continue };
            let Some(r) = e.rec else { continue };
            let rec = &env.cat.recs[r];
            out.score += env.sc.score(rec, e.tier, ac, slot);
            ac += ac_of(rec, e.tier);
            out.items.push(CurItem {
                slot: slot.to_owned(),
                name: e.name.clone(),
                rec: r,
                tier: e.tier,
            });
            for (k, v) in rec.stats_at(e.tier) {
                *out.st.entry(k.to_owned()).or_insert(0) += v;
            }
        }
        out
    }
}

/* -------------------------------------------------------------- the breakdown -- */

/// One line of why a number is that number.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub k: String,
    pub n: f64,
    pub w: f64,
    pub v: f64,
    /// AC that lands past the softcap.
    pub cap: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Breakdown {
    pub parts: Vec<Part>,
    pub total: f64,
}

pub fn breakdown(rec: &GearRec, tier: u32, base_ac: f64, slot: &str, sc: &Scorer) -> Breakdown {
    let mut out = Vec::new();
    let mut total = 0.0;
    let st = rec.stats_at(tier);
    for (k, n) in &st {
        if *k == "ac" {
            continue;
        }
        let w = sc.w.get(k);
        let v = w * *n as f64;
        if v != 0.0 {
            out.push(Part {
                k: k.to_uppercase(),
                n: *n as f64,
                w,
                v,
                cap: false,
            });
            total += v;
        } else if *n != 0 {
            out.push(Part {
                k: k.to_uppercase(),
                n: *n as f64,
                w: 0.0,
                v: 0.0,
                cap: false,
            });
        }
    }
    if let Some((_, ac)) = st.iter().find(|(k, _)| *k == "ac") {
        if *ac != 0 {
            let v = sc.w.get("ac") * (sc.eff_ac(base_ac + *ac as f64) - sc.eff_ac(base_ac));
            out.push(Part {
                k: "AC".into(),
                n: *ac as f64,
                w: sc.w.get("ac"),
                v,
                cap: base_ac + *ac as f64 > sc.cap_v,
            });
            total += v;
        }
    }
    for (k, n) in &rec.sv {
        if k == "v" {
            continue;
        }
        let v = sc.w.get("sv") * *n as f64;
        out.push(Part {
            k: format!("SV{}", k.to_uppercase()),
            n: *n as f64,
            w: sc.w.get("sv"),
            v,
            cap: false,
        });
        total += v;
    }
    if rec.haste != 0 {
        let v = sc.w.get("haste") * rec.haste as f64;
        out.push(Part {
            k: "haste".into(),
            n: rec.haste as f64,
            w: sc.w.get("haste"),
            v,
            cap: false,
        });
        total += v;
    }
    if let (Some(dmg), Some(dly)) = (rec.dmg, rec.dly) {
        if dly > 0 && matches!(slot, "Primary" | "Secondary" | "Range") {
            let ratio = stat_at(dmg, tier) as f64 / dly as f64;
            let v = sc.w.get("ratio") * 10.0 * ratio * if rec.is_two_h() { 2.0 } else { 1.0 };
            out.push(Part {
                k: "DMG/DLY".into(),
                n: (ratio * 100.0).round() / 100.0,
                w: sc.w.get("ratio"),
                v,
                cap: false,
            });
            total += v;
        }
    }
    out.sort_by(|a, b| b.v.partial_cmp(&a.v).unwrap_or(std::cmp::Ordering::Equal));
    Breakdown { parts: out, total }
}

/* ---------------------------------------------------------------- the fetch -- */

static LOC_NUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:General ?|Bank ?)(\d+)(?:-Slot(\d+))?").expect("constant"));

/// The container and position numbers of a location, 0 when
/// the place has no number.
pub fn loc_numbers(loc: &str) -> (u32, u32) {
    match LOC_NUM.captures(loc) {
        Some(m) => (
            m[1].parse().unwrap_or(0),
            m.get(2).and_then(|g| g.as_str().parse().ok()).unwrap_or(0),
        ),
        None => (0, 0),
    }
}

/// Where to walk, as words.
pub fn where_text(row: &VRow) -> String {
    let (n, sub) = loc_numbers(&row.loc);
    match row.sec {
        Section::Bags if n > 0 => format!(
            "bag {n}{}",
            if sub > 0 {
                format!(", slot {sub}")
            } else {
                String::new()
            }
        ),
        Section::Bank if n > 0 => format!(
            "bank {n}{}",
            if sub > 0 {
                format!(", slot {sub}")
            } else {
                String::new()
            }
        ),
        Section::Worn => "worn".into(),
        Section::Shared => "shared bank".into(),
        Section::Depot => "depot".into(),
        Section::Hoard => "hoard".into(),
        Section::KeyRing => "key ring".into(),
        Section::Storage => {
            if row.root.starts_with("Activated") {
                "activated".into()
            } else {
                "storage".into()
            }
        }
        Section::Exalts => "exaltation".into(),
        _ => row.loc.to_lowercase(),
    }
}

/// One container at a time: everything in bank slot 12 comes out in
/// one reach, so the rows for it sit together.
pub fn fetch_order(items: &mut [PlanItem], corpus: &Corpus) {
    items.sort_by(|a, b| {
        let ra = &corpus.rows[a.row];
        let rb = &corpus.rows[b.row];
        let (na, sa) = loc_numbers(&ra.loc);
        let (nb, sb) = loc_numbers(&rb.loc);
        sec_rank(ra.sec)
            .cmp(&sec_rank(rb.sec))
            .then_with(|| na.cmp(&nb))
            .then_with(|| sa.cmp(&sb))
    });
}

/* -------------------------------------------------------------------- by slot -- */

fn scorable(rec: &GearRec) -> bool {
    rec.scorable()
}

fn fits_slot(rec: &GearRec, slot: &str) -> bool {
    if slot == ANY {
        !rec.sl.is_empty()
    } else {
        rec.sl.iter().any(|s| s == slot)
    }
}

/// The off hand's gate, the same one the walk and spare apply: never a two-hander, and a damaging
/// one-hander only with Dual Wield.
fn placeable(rec: &GearRec, slot: &str, dw: bool) -> bool {
    slot != "Secondary" || (!rec.is_two_h() && (dw || rec.dmg.is_none()))
}

/// What the score cannot see, chipped on the item.
pub fn unscored_of(rec: &GearRec) -> Vec<&'static str> {
    let mut out = Vec::new();
    if rec.inst {
        out.push("inst");
    }
    if rec.charges > 0 {
        out.push("charges");
    }
    out
}

/// One occupant of a slot, priced at the table's base.
#[derive(Clone, Debug)]
pub struct Now {
    pub name: String,
    pub tier: u32,
    pub rec: Option<usize>,
    pub ac: f64,
    pub score: Option<f64>,
}

/// The AC base for a slot is the AC you are wearing minus the AC of
/// what this slot holds, so every candidate (including the one already on) is priced as the
/// thing that would sit there against the rest of the kit. A paired slot subtracts its weaker
/// occupant, the one a new piece would displace.
pub struct SlotCtx {
    pub worn_ac: f64,
    pub now: Vec<Now>,
    pub vs: Option<usize>,
    pub base_ac: f64,
}

pub fn slot_context(corpus: &Corpus, cat: &Catalogue, sc: &Scorer, slot: &str) -> SlotCtx {
    let worn_ac = gear::worn_ac(&corpus.equipped, cat);
    let mut now: Vec<Now> = corpus
        .equipped
        .get(slot)
        .iter()
        .map(|e| {
            let ac = e.rec.map(|r| ac_of(&cat.recs[r], e.tier)).unwrap_or(0.0);
            Now {
                name: e.name.clone(),
                tier: e.tier,
                rec: e.rec,
                ac,
                score: e
                    .rec
                    .map(|r| sc.score(&cat.recs[r], e.tier, worn_ac - ac, slot)),
            }
        })
        .collect();
    let vs = now
        .iter()
        .enumerate()
        .filter(|(_, n)| n.score.is_some())
        .min_by(|a, b| {
            a.1.score
                .partial_cmp(&b.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i);
    let base_ac = worn_ac - vs.map(|i| now[i].ac).unwrap_or(0.0);
    /* Every occupant is then re-priced at the table's base, so the number in "on you now" is the
     * number on the same item's row below it. */
    for n in &mut now {
        if let (Some(_), Some(r)) = (n.score, n.rec) {
            n.score = Some(sc.score(&cat.recs[r], n.tier, base_ac, slot));
        }
    }
    SlotCtx {
        worn_ac,
        now,
        vs,
        base_ac,
    }
}

/// Where the item stands in the whole in-era catalogue for the best of your classes, at +0.
#[derive(Clone, Debug, PartialEq)]
pub struct CatRank {
    pub cls: &'static str,
    pub slot: String,
    pub rank: usize,
    pub of: usize,
}

/// The catalogue standing. "Best" is the best PERCENTILE, not the lowest number: 36 of 340 is
/// a stronger standing than 14 of 91. Any Slot has no catalogue of its own; an item parked there
/// is ranked in its own best slot.
pub fn catalog_rank(
    ranker: &mut Ranker,
    cat: &Catalogue,
    rec_i: usize,
    slot: &str,
    tier: u32,
    trio: &[String],
) -> Option<CatRank> {
    let rec = &cat.recs[rec_i];
    let slots: Vec<String> = if slot == ANY {
        rec.sl
            .iter()
            .filter(|s| SLOTS.contains(&s.as_str()))
            .cloned()
            .collect()
    } else {
        vec![slot.to_owned()]
    };
    let mut best: Option<CatRank> = None;
    for cls in trio {
        let Some(code) = gear::class_code(cls) else {
            continue;
        };
        for s in &slots {
            /* The ranker answers for any slot it is asked about, pool or no pool; an item is only
             * ranked in a slot its record names, and never "1 of 0". */
            if !rec.sl.iter().any(|x| x == s) {
                continue;
            }
            let Some(r) = ranker.rank(cat, rec_i, code, s, tier) else {
                continue;
            };
            if r.of == 0 {
                continue;
            }
            let pct = r.rank as f64 / r.of as f64;
            let better = match &best {
                None => true,
                Some(b) => {
                    let bp = b.rank as f64 / b.of as f64;
                    pct < bp || (pct == bp && r.of > b.of)
                }
            };
            if better {
                best = Some(CatRank {
                    cls: code,
                    slot: s.clone(),
                    rank: r.rank,
                    of: r.of,
                });
            }
        }
    }
    best
}

/// One copy you own that fits a slot.
#[derive(Clone, Debug)]
pub struct Owned {
    pub row: usize,
    pub score: f64,
    pub worn_here: bool,
    pub worn_in: Option<&'static str>,
    pub two_h: bool,
    pub catalog: Option<CatRank>,
    /// The worn copy's sockets, when the exaltation reader saw them.
    pub sockets: Option<[exalt::Socket; 4]>,
    pub unscored: Vec<&'static str>,
    pub legal: bool,
    pub placeable: bool,
    /// Ties share.
    pub rank: usize,
}

pub struct SlotView {
    pub slot: String,
    pub now: Vec<Now>,
    pub vs: Option<usize>,
    pub base_ac: f64,
    pub worn_ac: f64,
    pub owned: Vec<Owned>,
    /// Main + off together, the number a two-hander has to beat: (main, off, total).
    pub hands_now: Option<(Option<f64>, Option<f64>, f64)>,
    /// The rank the catalogue gap is priced at: the highest you hold in this slot.
    pub par_tier: u32,
}

/// Pick a slot and see the whole field for it, every copy you own that
/// fits, where each one is, and how they rank, with no decision attached.
pub fn slot_view(
    corpus: &Corpus,
    cat: &Catalogue,
    sc: &Scorer,
    slot: &str,
    trio: &[String],
    mut ranker: Option<&mut Ranker>,
    xd: Option<&ExaltDump>,
) -> SlotView {
    let dw = sc.dual_wield_cap() > 0;
    let ctx = slot_context(corpus, cat, sc, slot);
    let mut sockets_by_all: HashMap<usize, &[exalt::Socket; 4]> = HashMap::new();
    if let Some(x) = xd {
        for w in &x.worn {
            sockets_by_all.insert(w.row, &w.sockets);
        }
    }
    let mut owned: Vec<Owned> = Vec::new();
    for (i, r) in corpus.rows.iter().enumerate() {
        let Some(ri) = r.rec else { continue };
        if r.exalt {
            continue;
        }
        let rec = &cat.recs[ri];
        let worn_in = if r.sec == Section::Worn {
            r.worn_slot()
        } else {
            None
        };
        /* What is worn IN THIS SLOT is a candidate for it whatever the wiki's class list, slot
         * list or the Dual Wield gate say: the game put it there. Worn elsewhere, it is gated
         * like anything else. */
        let usable = worn_in == Some(slot)
            || (fits_slot(rec, slot)
                && placeable(rec, slot, dw)
                && (sc.legal(rec) || r.sec == Section::Worn));
        if !usable {
            continue;
        }
        let catalog = ranker
            .as_deref_mut()
            .and_then(|rk| catalog_rank(rk, cat, ri, slot, 0, trio));
        owned.push(Owned {
            row: i,
            score: sc.score(rec, r.tier, ctx.base_ac, slot),
            worn_here: worn_in == Some(slot),
            worn_in,
            two_h: rec.is_two_h(),
            catalog,
            sockets: sockets_by_all.get(&r.i).map(|s| (*s).clone()),
            unscored: unscored_of(rec),
            legal: sc.legal(rec),
            placeable: fits_slot(rec, slot) && placeable(rec, slot, dw),
            rank: 0,
        });
    }
    owned.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.worn_here.cmp(&a.worn_here))
            .then_with(|| corpus.rows[b.row].tier.cmp(&corpus.rows[a.row].tier))
            .then_with(|| corpus.rows[a.row].name.cmp(&corpus.rows[b.row].name))
    });
    let mut rank = 0;
    let mut prev: Option<f64> = None;
    for (i, c) in owned.iter_mut().enumerate() {
        if prev.map_or(true, |p| c.score < p) {
            rank = i + 1;
        }
        prev = Some(c.score);
        c.rank = rank;
    }
    let hands_now = if slot == "Primary" {
        let off = slot_context(corpus, cat, sc, "Secondary");
        let m = ctx.now.first().and_then(|n| n.score);
        let o = off.now.first().and_then(|n| n.score);
        Some((m, o, m.unwrap_or(0.0) + o.unwrap_or(0.0)))
    } else {
        None
    };
    let par_tier = owned
        .iter()
        .map(|c| corpus.rows[c.row].tier)
        .max()
        .unwrap_or(0);
    SlotView {
        slot: slot.to_owned(),
        now: ctx.now,
        vs: ctx.vs,
        base_ac: ctx.base_ac,
        worn_ac: ctx.worn_ac,
        owned,
        hands_now,
        par_tier,
    }
}

/// One catalogue item you do not own that your trio could wear in the slot.
#[derive(Clone, Debug)]
pub struct GapItem {
    pub rec: usize,
    /// At +0, which is what a drop is when it lands.
    pub s0: f64,
    /// At `par_tier`, the rank your best owned copy is at.
    pub s_par: f64,
    pub two_h: bool,
    pub unscored: Vec<&'static str>,
    /// The page's own qualifier when the wiki files several records under one name.
    pub variant: Option<String>,
    pub variants: usize,
}

pub struct Gap {
    pub slot: String,
    pub par_tier: u32,
    pub items: Vec<GapItem>,
}

/// The catalogue gap: every in-era item your trio can wear there that no row of your dump
/// is, scored at +0 and at par. Items the wiki gives no stats are not listed. Out-of-era records
/// never appear.
pub fn slot_gap(
    cat: &Catalogue,
    sc: &Scorer,
    slot: &str,
    owned: &HashSet<usize>,
    base_ac: f64,
    par_tier: u32,
) -> Gap {
    let dw = sc.dual_wield_cap() > 0;
    let mut items = Vec::new();
    for (i, rec) in cat.recs.iter().enumerate() {
        if rec.oe || owned.contains(&i) || !scorable(rec) {
            continue;
        }
        if !fits_slot(rec, slot) || !placeable(rec, slot, dw) || !sc.legal(rec) {
            continue;
        }
        let s0 = sc.score(rec, 0, base_ac, slot);
        let s_par = if par_tier > 0 {
            sc.score(rec, par_tier, base_ac, slot)
        } else {
            s0
        };
        items.push(GapItem {
            rec: i,
            s0,
            s_par,
            two_h: rec.is_two_h(),
            unscored: unscored_of(rec),
            variant: None,
            variants: 1,
        });
    }
    items.sort_by(|a, b| {
        b.s_par
            .partial_cmp(&a.s_par)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.s0.partial_cmp(&a.s0).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| cat.recs[a.rec].name.cmp(&cat.recs[b.rec].name))
    });
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, it) in items.iter().enumerate() {
        by_name
            .entry(cat.recs[it.rec].name.as_str())
            .or_default()
            .push(idx);
    }
    for group in by_name.values() {
        if group.len() < 2 {
            continue;
        }
        for &idx in group {
            let rec = &cat.recs[items[idx].rec];
            items[idx].variant = Some(rec.key.clone());
            items[idx].variants = group.len();
        }
    }
    Gap {
        slot: slot.to_owned(),
        par_tier,
        items,
    }
}

/// Two records of one name are the same item when nothing the scorer or the fitting rule reads
/// differs, and different items when the stats do (deity variants of a crafted piece).
pub fn same_item(a: &GearRec, b: &GearRec) -> bool {
    a.st == b.st
        && a.sv == b.sv
        && a.haste == b.haste
        && a.dmg == b.dmg
        && a.dly == b.dly
        && a.sl == b.sl
        && a.cls == b.cls
}

/// Every item a dump holds at least one copy of, by record. A stone is
/// named after an item and is not one, so it never counts as owning it. A name the wiki files
/// under more than one page counts as owning every record of that name that is the same item.
pub fn owned_keys(corpus: &Corpus, cat: &Catalogue) -> HashSet<usize> {
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, r) in cat.recs.iter().enumerate() {
        by_name.entry(gear::item_key(&r.name)).or_default().push(i);
    }
    let mut out = HashSet::new();
    for r in &corpus.rows {
        if r.exalt {
            continue;
        }
        let Some(k) = r.rec else { continue };
        out.insert(k);
        if let Some(group) = by_name.get(&gear::item_key(&cat.recs[k].name)) {
            for &other in group {
                if other != k && same_item(&cat.recs[k], &cat.recs[other]) {
                    out.insert(other);
                }
            }
        }
    }
    out
}

/* ------------------------------------------------------------ the exalt audit -- */

/// An open socket on a worn item with nothing in it, and the idle stones that fit it.
#[derive(Clone, Debug)]
pub struct EmptySocket {
    /// Index into the exaltation dump's `worn`.
    pub w: usize,
    pub kind: Kind,
    /// Indices into `Audit::idle`.
    pub fits: Vec<usize>,
}

/// One place an idle stone could go.
#[derive(Clone, Debug)]
pub struct Home {
    pub w: usize,
    pub kind: Kind,
    /// The catalogue stone that would go in.
    pub def: usize,
    /// What is in the socket now, for a swap.
    pub current: Option<String>,
    /// The tier the host needs first, for a later home.
    pub needs: Option<u32>,
}

/// An exaltation that is NOT on your body.
#[derive(Clone, Debug)]
pub struct Idle {
    /// Index into the dump rows.
    pub row: usize,
    pub item: Option<usize>,
    /// Every effect the stone could be, as catalogue stones.
    pub defs: Vec<usize>,
    /// The row of the item it is socketed into, when it is not loose.
    pub host: Option<usize>,
    pub kind: Option<Kind>,
    /// The source yields more than one effect and the dump does not say which.
    pub maybe: bool,
    /// The source has no wiki page.
    pub unknown: bool,
    pub empty: Vec<Home>,
    pub occupied: Vec<Home>,
    pub later: Vec<Home>,
    pub why: Option<&'static str>,
    pub deity: bool,
    pub sec: Section,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditSummary {
    pub empty: usize,
    pub worn_with_empty: usize,
    /// Sockets that can be filled from what you own.
    pub fillable: usize,
    pub idle: usize,
    /// Idle stones that fit an empty socket right now.
    pub usable: usize,
    pub swaps: usize,
    pub later: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Audit {
    pub empty: Vec<EmptySocket>,
    pub idle: Vec<Idle>,
    pub summary: AuditSummary,
}

/// The exaltation audit: the two flags, "empty sockets on what you wear" and "exaltations
/// somewhere you could be using". A home must leave the item wearable by one of your classes.
pub fn exalt_audit(xd: &ExaltDump, rows: &[InvRow], xcat: &ExaltCat, trio: &[String]) -> Audit {
    let mut audit = Audit::default();
    let mut empty_at: HashMap<(usize, Kind), usize> = HashMap::new();
    for (wi, w) in xd.worn.iter().enumerate() {
        for k in Kind::ALL {
            let s = w.socket(k);
            if !s.filled && (s.empty || s.open) {
                empty_at.insert((wi, k), audit.empty.len());
                audit.empty.push(EmptySocket {
                    w: wi,
                    kind: k,
                    fits: Vec::new(),
                });
            }
        }
    }
    let worn_with_empty: HashSet<usize> = audit.empty.iter().map(|e| e.w).collect();

    let mk = |row: usize,
              item: Option<usize>,
              defs: Vec<usize>,
              host: Option<usize>,
              kind: Option<Kind>,
              audit: &mut Audit| {
        let mut empty = Vec::new();
        let mut occupied = Vec::new();
        let mut later = Vec::new();
        /* Why a worn item refuses it, for the row that ends with no home at all, read in order:
         * nothing worn shares its slot; or those that do all lack a shared class; or the result
         * would be wearable by none of your three. */
        let (mut slot_ok, mut cls_ok, mut trio_no, mut same_fx) = (0, 0, 0, 0);
        for &d in &defs {
            let def = &xcat.stones[d];
            let Some(dk) = def.kind else { continue };
            let h = exalt::homes_for(def, &xd.worn);
            for (_, f) in &h.never {
                if !f.why.contains(&"no shared slot") {
                    slot_ok += 1;
                }
            }
            for (_, f, _) in &h.now {
                slot_ok += 1;
                cls_ok += 1;
                if !exalt::usable_by(f.cls.as_deref(), trio) {
                    trio_no += 1;
                }
            }
            for (_, f, _) in &h.later {
                slot_ok += 1;
                cls_ok += 1;
                if !exalt::usable_by(f.cls.as_deref(), trio) {
                    trio_no += 1;
                }
            }
            for (wi, f, occ) in &h.now {
                if !exalt::usable_by(f.cls.as_deref(), trio) {
                    continue;
                }
                let sock = xd.worn[*wi].socket(dk);
                if *occ {
                    /* Replacing an effect with the same effect is not a swap. */
                    let cur = sock
                        .stone
                        .as_ref()
                        .and_then(|s| s.stone)
                        .map(|si| xcat.stones[si].effect.clone());
                    if cur.as_deref() == Some(def.effect.as_str()) {
                        same_fx += 1;
                        continue;
                    }
                    let current = cur.or_else(|| {
                        sock.stone
                            .as_ref()
                            .map(|s| gear::strip_decor(&rows[s.row].name))
                    });
                    occupied.push(Home {
                        w: *wi,
                        kind: dk,
                        def: d,
                        current,
                        needs: None,
                    });
                } else {
                    empty.push(Home {
                        w: *wi,
                        kind: dk,
                        def: d,
                        current: None,
                        needs: None,
                    });
                }
            }
            for (wi, f, needs) in &h.later {
                if exalt::usable_by(f.cls.as_deref(), trio) {
                    later.push(Home {
                        w: *wi,
                        kind: dk,
                        def: d,
                        current: None,
                        needs: Some(*needs),
                    });
                }
            }
        }
        let why = if defs.is_empty() {
            Some(if item.is_some() {
                "the wiki lists no effect of a known kind on its source"
            } else {
                "no wiki page for its source"
            })
        } else if empty.is_empty() && occupied.is_empty() && later.is_empty() {
            Some(if slot_ok == 0 {
                "no worn item shares a slot with it"
            } else if cls_ok == 0 {
                "no worn item in its slot shares a class with it"
            } else if trio_no > 0 {
                "none of your classes could wear the result"
            } else if same_fx > 0 {
                "the same effect is already in every socket it fits"
            } else {
                "nothing worn takes it"
            })
        } else {
            None
        };
        let idx = audit.idle.len();
        for h in &empty {
            if let Some(&e) = empty_at.get(&(h.w, h.kind)) {
                if !audit.empty[e].fits.contains(&idx) {
                    audit.empty[e].fits.push(idx);
                }
            }
        }
        let r = &rows[row];
        audit.idle.push(Idle {
            row,
            item,
            kind: kind.or_else(|| {
                if defs.len() == 1 {
                    xcat.stones[defs[0]].kind
                } else {
                    None
                }
            }),
            maybe: defs.len() > 1,
            unknown: defs.is_empty(),
            deity: defs.iter().any(|&d| xcat.stones[d].deity),
            defs,
            host,
            empty,
            occupied,
            later,
            why,
            sec: r.section,
            name: r.name.clone(),
        });
    };

    for l in &xd.loose {
        let defs: Vec<usize> = l
            .yields
            .iter()
            .copied()
            .filter(|&i| xcat.stones[i].kind.is_some())
            .collect();
        mk(l.row, l.item, defs, None, None, &mut audit);
    }
    for src in &xd.sources {
        if src.worn {
            continue;
        }
        for k in Kind::ALL {
            let sk = &src.sockets[k.index()];
            let Some(st) = &sk.stone else { continue };
            let defs: Vec<usize> = st.stone.into_iter().collect();
            mk(st.row, st.item, defs, Some(src.row), Some(k), &mut audit);
        }
    }
    /* The ones you could be using first, then the ones that would replace something, then the
     * rest; within a group, the bin before a bag before the bank. */
    let tier = |it: &Idle| {
        if !it.empty.is_empty() {
            0
        } else if !it.occupied.is_empty() {
            1
        } else if !it.later.is_empty() {
            2
        } else {
            3
        }
    };
    let mut order: Vec<usize> = (0..audit.idle.len()).collect();
    order.sort_by(|&a, &b| {
        let (ia, ib) = (&audit.idle[a], &audit.idle[b]);
        tier(ia)
            .cmp(&tier(ib))
            .then_with(|| sec_rank(ia.sec).cmp(&sec_rank(ib.sec)))
            .then_with(|| ia.name.cmp(&ib.name))
    });
    let remap: HashMap<usize, usize> = order
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();
    let mut idle = Vec::with_capacity(order.len());
    for &old in &order {
        idle.push(audit.idle[old].clone());
    }
    audit.idle = idle;
    for e in &mut audit.empty {
        for f in &mut e.fits {
            *f = remap[f];
        }
        e.fits.sort_unstable();
    }
    audit.summary = AuditSummary {
        empty: audit.empty.len(),
        worn_with_empty: worn_with_empty.len(),
        fillable: audit.empty.iter().filter(|e| !e.fits.is_empty()).count(),
        idle: audit.idle.len(),
        usable: audit.idle.iter().filter(|it| !it.empty.is_empty()).count(),
        swaps: audit
            .idle
            .iter()
            .filter(|it| it.empty.is_empty() && !it.occupied.is_empty())
            .count(),
        later: audit
            .idle
            .iter()
            .filter(|it| it.empty.is_empty() && it.occupied.is_empty() && !it.later.is_empty())
            .count(),
    };
    audit
}

/* ------------------------------------------------------------------ settings -- */

/// The settings key this screen's remembered decisions and view state live under.
pub const VALET_KEY: &str = "valet";

fn trio_key(classes: &[String]) -> String {
    let mut c = classes.to_vec();
    c.sort();
    c.join("/")
}

fn load_mem(settings: &crate::settings::Settings, trio: &str) -> Memory {
    settings
        .extra
        .get(VALET_KEY)
        .and_then(|v| v.get("mem"))
        .and_then(|m| m.get(trio))
        .and_then(|m| serde_json::from_value::<BTreeMap<String, MemRec>>(m.clone()).ok())
        .map(|map| Memory { map, dirty: false })
        .unwrap_or_default()
}

/// Put the memory for one trio into settings, under `valet.mem.<trio>`. Pure over the struct:
/// nothing here touches the disk, which is what lets the round trip be tested without writing
/// anywhere. `save_mem` is the one caller that then writes.
fn put_mem(
    settings: &mut crate::settings::Settings,
    trio: &str,
    mem: &Memory,
) -> Result<(), String> {
    let mut root = settings
        .extra
        .get(VALET_KEY)
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }
    let obj = root.as_object_mut().expect("just made an object");
    let mems = obj.entry("mem").or_insert_with(|| serde_json::json!({}));
    if !mems.is_object() {
        *mems = serde_json::json!({});
    }
    mems.as_object_mut().expect("object").insert(
        trio.to_owned(),
        serde_json::to_value(&mem.map).map_err(|e| e.to_string())?,
    );
    settings.extra.insert(VALET_KEY.to_owned(), root);
    Ok(())
}

/// `put_mem`, then the operator's settings file. Only the screen calls this: a test that reached
/// it would write the operator's file, and `Settings::save` refuses under test for that reason.
fn save_mem(
    settings: &mut crate::settings::Settings,
    trio: &str,
    mem: &Memory,
) -> Result<(), String> {
    put_mem(settings, trio, mem)?;
    settings.save()
}

/* -------------------------------------------------------------------- screen -- */

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Dress,
    Slot,
}

enum Action {
    Pick(usize, bool),
    Skip,
    Back,
    Restart,
    ClearMem,
}

/// The Valet screen: the get-dressed walk and the by-slot field, over the same owned corpus.
pub struct ValetScreen {
    cat: Option<CatCache>,
    xcat: Option<ExaltCat>,
    xcat_key: Option<(usize, usize)>,
    rows: Vec<InvRow>,
    corpus: Option<Corpus>,
    dump_key: Option<(PathBuf, chrono::DateTime<chrono::Utc>)>,
    dump_line: String,
    scorer: Option<Scorer>,
    scorer_key: String,
    walk: Option<Walk>,
    xdump: Option<ExaltDump>,
    audit: Option<Audit>,
    ranker: Option<Ranker>,
    view: View,
    slot: String,
    remember: bool,
    all_open: bool,
    gap_all: bool,
    mem_problem: Option<String>,
    trio: CharState,
}

impl Default for ValetScreen {
    fn default() -> Self {
        ValetScreen {
            cat: None,
            xcat: None,
            xcat_key: None,
            rows: Vec::new(),
            corpus: None,
            dump_key: None,
            dump_line: String::new(),
            scorer: None,
            scorer_key: String::new(),
            walk: None,
            xdump: None,
            audit: None,
            ranker: None,
            view: View::Dress,
            slot: "Head".into(),
            remember: false,
            all_open: false,
            gap_all: false,
            mem_problem: None,
            trio: CharState::default(),
        }
    }
}

impl ValetScreen {
    /// Rebuild whatever changed: the catalogue per snapshot, the corpus per dump, the scorer and
    /// walk per trio, level, overrides or dump.
    fn refresh(&mut self, cx: &mut Cx) {
        CatCache::refresh(&mut self.cat, cx.data);
        let xkey = cx.data.map(|s| (s as *const _ as usize, s.items.len()));
        if xkey != self.xcat_key {
            self.xcat_key = xkey;
            self.xcat = cx.data.map(|s| ExaltCat::build(&s.items));
            self.dump_key = None;
        }
        let dump = cx.ingest.inventory();
        let key = dump.map(|d| (d.path.clone(), d.read_at));
        if key != self.dump_key || (key.is_some() && self.corpus.is_none()) {
            self.dump_key = key;
            self.corpus = None;
            self.xdump = None;
            self.rows.clear();
            self.scorer_key.clear();
            if let (Some(d), Some(cat)) = (dump, &self.cat) {
                self.corpus = Some(Corpus::read(d, &cat.cat));
                self.rows = d.all.clone();
                let who = d
                    .character
                    .as_deref()
                    .map(|c| format!(" · {c}'s dump"))
                    .unwrap_or_default();
                self.dump_line = format!(
                    "{}{who} · {} items",
                    d.path.display(),
                    self.rows.iter().filter(|r| !r.empty).count()
                );
                if let (Some(x), Some(s)) = (&self.xcat, cx.data) {
                    self.xdump = Some(exalt::read_dump(&self.rows, &s.items, x));
                }
            }
        }
        self.trio = CharState::read(cx.settings);
        let overrides = read_overrides(cx.settings);
        let mut ov: Vec<(&String, &f64)> = overrides.iter().collect();
        ov.sort_by(|a, b| a.0.cmp(b.0));
        let skey = format!(
            "{}|{}|{:?}|{:?}",
            self.trio.classes.join("/"),
            self.trio.level,
            ov,
            self.dump_key
        );
        if skey != self.scorer_key {
            self.scorer_key = skey;
            self.walk = None;
            self.audit = None;
            self.scorer = None;
            if let (Some(corpus), Some(cat)) = (&self.corpus, &self.cat) {
                let sc = Scorer::make(
                    &self.trio.classes,
                    self.trio.level,
                    Some(&corpus.equipped),
                    Some(&cat.cat),
                    &overrides,
                );
                let mem = load_mem(cx.settings, &trio_key(&self.trio.classes));
                let env = Env {
                    corpus,
                    cat: &cat.cat,
                    sc: &sc,
                };
                self.walk = Some(Walk::new(&env, mem));
                self.scorer = Some(sc);
                if let (Some(xd), Some(x)) = (&self.xdump, &self.xcat) {
                    self.audit = Some(exalt_audit(xd, &self.rows, x, &self.trio.classes));
                }
            }
            if self
                .ranker
                .as_ref()
                .map_or(true, |r| r.level != self.trio.level)
            {
                self.ranker = Some(Ranker::new(self.trio.level));
            }
        }
    }

    fn persist_mem(&mut self, cx: &mut Cx) {
        let Some(w) = &mut self.walk else { return };
        if !w.mem.dirty {
            return;
        }
        w.mem.dirty = false;
        self.mem_problem = save_mem(cx.settings, &trio_key(&self.trio.classes), &w.mem).err();
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        self.refresh(cx);
        heading(ui, "VALET");
        ui.label(RichText::new("Get dressed from what you own, one slot at a time, priced for your trio. Or pick a slot and see the whole field for it.").color(TEXT_2));
        ui.add_space(6.0);

        match (cx.data, cx.data_err) {
            (Some(s), _) => {
                let n = self.cat.as_ref().map(|c| c.cat.len()).unwrap_or(0);
                mark(
                    ui,
                    State::Settled,
                    &format!(
                        "{n} item records from {}",
                        s.root.join(crate::data::GEAR_FILE).display()
                    ),
                );
                if let Some(p) = self.cat.as_ref().and_then(|c| c.problem.as_deref()) {
                    mark(ui, State::Wrong, p);
                }
            }
            (None, Some(e)) => {
                mark(ui, State::Wrong, e);
            }
            (None, None) => {
                mark(ui, State::Idle, "no item data loaded; put the snapshot's gear-data.json in data/ beside the executable");
            }
        }
        match cx.ingest.inventory() {
            Some(_) => {
                mark(ui, State::Settled, &self.dump_line.clone());
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
        ui.horizontal(|ui| {
            ui.label(RichText::new("trio").color(TEXT_3));
            ui.label(
                RichText::new(format!(
                    "{} at {}",
                    self.trio.classes.join("/"),
                    self.trio.level
                ))
                .font(FontId::monospace(11.5))
                .color(TEXT),
            );
            let note = CharState::default_note(cx.settings);
            ui.label(
                RichText::new(if note.is_empty() {
                    "set on the Gear screen, with the stat priorities"
                } else {
                    note
                })
                .color(TEXT_3),
            );
        });
        if let Some(p) = &self.mem_problem {
            mark(
                ui,
                State::Wrong,
                &format!("could not save the remembered decision: {p}"),
            );
        }
        ui.add_space(8.0);

        if self.corpus.is_none() || self.walk.is_none() {
            return;
        }

        ui.horizontal(|ui| {
            for (v, label) in [(View::Dress, "Get dressed"), (View::Slot, "By slot")] {
                if ui.selectable_label(self.view == v, label).clicked() {
                    self.view = v;
                }
            }
            if let Some(a) = &self.audit {
                let s = &a.summary;
                /* exaltAudit's `swaps`: idle stones whose only worn home is a socket something
                 * else already sits in. Not an act (the socket is not empty), so it is said in
                 * the dim line and never on the gold square. */
                let swaps = if s.swaps > 0 { format!("; {} idle stone{} fit only an occupied socket (a swap, not a fill)", s.swaps, if s.swaps == 1 { "" } else { "s" }) } else { String::new() };
                if s.fillable > 0 {
                    mark(ui, State::You, &format!("{} empty socket{} on what you wear can be filled from what you own ({} fit); the list is below{swaps}", s.fillable, if s.fillable == 1 { "" } else { "s" }, s.usable));
                } else if s.empty > 0 || s.swaps > 0 {
                    ui.label(RichText::new(format!("{} empty socket{} on {} worn item{}{swaps}", s.empty, if s.empty == 1 { "" } else { "s" }, s.worn_with_empty, if s.worn_with_empty == 1 { "" } else { "s" })).color(TEXT_3));
                }
            }
        });
        if let (Some(a), Some(xd), Some(xc)) = (&self.audit, &self.xdump, &self.xcat) {
            idle_list(ui, a, xd, &self.rows, xc);
        }
        ui.add_space(6.0);

        let mut actions: Vec<Action> = Vec::new();
        egui::ScrollArea::vertical()
            .id_salt("valet-body")
            .show(ui, |ui| match self.view {
                View::Dress => {
                    let (Some(corpus), Some(cat), Some(sc), Some(walk)) =
                        (&self.corpus, &self.cat, &self.scorer, &self.walk)
                    else {
                        return;
                    };
                    let env = Env {
                        corpus,
                        cat: &cat.cat,
                        sc,
                    };
                    if walk.done() {
                        done_view(ui, walk, &env, self.xdump.as_ref(), &mut actions);
                    } else {
                        run_view(
                            ui,
                            walk,
                            &env,
                            self.xdump.as_ref(),
                            &mut self.remember,
                            &mut self.all_open,
                            &mut actions,
                        );
                    }
                }
                View::Slot => {
                    let (Some(corpus), Some(cat), Some(sc)) =
                        (&self.corpus, &self.cat, &self.scorer)
                    else {
                        return;
                    };
                    slot_view_ui(
                        ui,
                        corpus,
                        &cat.cat,
                        sc,
                        &self.trio.classes,
                        self.ranker.as_mut(),
                        self.xdump.as_ref(),
                        self.audit.as_ref(),
                        &mut self.slot,
                        &mut self.gap_all,
                    );
                }
            });

        if !actions.is_empty() {
            let (Some(corpus), Some(cat), Some(sc), Some(walk)) =
                (&self.corpus, &self.cat, &self.scorer, &mut self.walk)
            else {
                return;
            };
            let env = Env {
                corpus,
                cat: &cat.cat,
                sc,
            };
            for a in actions {
                match a {
                    Action::Pick(n, keep) => {
                        let Some(step) = walk.step().cloned() else {
                            continue;
                        };
                        let Some(opt) = walk.options(&step.key).get(n).cloned() else {
                            continue;
                        };
                        walk.pick(&env, &step.key, opt, keep);
                    }
                    Action::Skip => walk.skip(&env),
                    Action::Back => {
                        walk.back(&env);
                    }
                    Action::Restart => walk.restart(&env),
                    Action::ClearMem => {
                        walk.mem.clear();
                        walk.restart(&env);
                    }
                }
            }
            self.persist_mem(cx);
        }
    }
}

/// The AC already assigned when the walk reaches a step.
fn base_ac_at(walk: &Walk, env: &Env, step: usize) -> f64 {
    let mut ac = 0.0;
    for s in walk.steps.iter().take(step) {
        if let Some(Some(p)) = walk.picks.get(&s.key) {
            for it in &p.items {
                if let Some(rec) = env.rec(it.row) {
                    ac += ac_of(rec, env.corpus.rows[it.row].tier);
                }
            }
        }
    }
    ac
}

/// Whoever is in the slot right now, priced in the candidates' own context.
struct Occupant {
    name: String,
    row: usize,
    rec: Option<usize>,
    score: Option<f64>,
}

fn occupant(walk: &Walk, env: &Env, si: usize) -> Option<Occupant> {
    let step = &walk.steps[si];
    let (e, row) = env.corpus.wearing_at(&step.slot, step.n)?;
    let base = base_ac_at(walk, env, si);
    Some(Occupant {
        name: e.name.clone(),
        row,
        rec: e.rec,
        score: e
            .rec
            .map(|r| env.sc.score(&env.cat.recs[r], e.tier, base, &step.slot)),
    })
}

/// A worn piece the plan puts on again at another step has not come off; it moves.
fn moved_to(walk: &Walk, si: usize, occ_row: usize) -> Option<&Step> {
    for (j, s) in walk.steps.iter().enumerate() {
        if j == si {
            continue;
        }
        if let Some(Some(p)) = walk.picks.get(&s.key) {
            if p.items.iter().any(|it| it.row == occ_row) {
                return Some(s);
            }
        }
    }
    None
}

/// An item coming off takes its exaltations with it.
fn strip_effects(xd: Option<&ExaltDump>, env: &Env, row: usize) -> Vec<String> {
    let Some(xd) = xd else { return Vec::new() };
    let all_i = env.corpus.rows[row].i;
    let Some(w) = xd.worn.iter().find(|w| w.row == all_i) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for k in Kind::ALL {
        let s = w.socket(k);
        if let Some(st) = &s.stone {
            let name = gear::strip_decor(
                env.corpus
                    .rows
                    .iter()
                    .find(|r| r.i == st.row)
                    .map(|r| r.name.as_str())
                    .unwrap_or("stone"),
            );
            out.push(match st.stone {
                Some(_) => format!("{name} ({})", k.label()),
                None => name,
            });
        }
    }
    out
}

/// The stat line under a card.
fn stat_line(rec: &GearRec, tier: u32) -> String {
    let st = rec.stats_at(tier);
    let mut out = Vec::new();
    for k in [
        "ac", "hp", "mana", "end", "atk", "str", "sta", "agi", "dex", "wis", "int", "cha",
    ] {
        if let Some((_, v)) = st.iter().find(|(s, _)| *s == k) {
            if *v != 0 {
                out.push(format!("{v} {}", k.to_uppercase()));
            }
        }
    }
    for (k, v) in &rec.sv {
        if k != "v" && *v != 0 {
            out.push(format!("{v} SV{}", k.to_uppercase()));
        }
    }
    if rec.haste != 0 {
        out.push(format!("{}% haste", rec.haste));
    }
    if let (Some(dmg), Some(dly)) = (rec.dmg, rec.dly) {
        if dly > 0 {
            let d = stat_at(dmg, tier);
            out.push(format!(
                "{d}/{dly} = {:.2}{}",
                d as f64 / dly as f64,
                if rec.is_two_h() { " 2H" } else { "" }
            ));
        }
    }
    if out.is_empty() {
        "no stats on the wiki".into()
    } else {
        out.join(" · ")
    }
}

fn name_tier(row: &VRow) -> String {
    if row.tier > 0 {
        format!("{} +{}", row.base, row.tier)
    } else {
        row.base.clone()
    }
}

fn gain_text(gain: Option<f64>) -> (String, egui::Color32) {
    match gain {
        None => (
            "(no wiki page on you now, nothing to compare against)".into(),
            TEXT_3,
        ),
        Some(g) if g > 0.5 => (format!("+{}", g.round()), TEXT),
        Some(g) if g < -0.5 => (format!("{}", g.round()), TEXT_2),
        Some(_) => ("0".into(), TEXT_3),
    }
}

fn run_view(
    ui: &mut Ui,
    walk: &Walk,
    env: &Env,
    xd: Option<&ExaltDump>,
    remember: &mut bool,
    all_open: &mut bool,
    actions: &mut Vec<Action>,
) {
    let Some(step) = walk.step() else { return };
    let si = walk.i;
    let all = walk.options(&step.key);
    /* The AC under this step is the context the walk priced its options in (the walk keeps it
     * per step); the sum over earlier picks is the same number and stands in
     * only for a step the walk has not priced yet. */
    let base_ac = walk
        .ctx(&step.key)
        .map(|c| c.base_ac)
        .unwrap_or_else(|| base_ac_at(walk, env, si));
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&step.label)
                .font(crate::fonts::display(15.0))
                .color(GOLD_HI),
        );
        ui.label(
            RichText::new(format!("{} of {}", si + 1, walk.steps.len()))
                .font(FontId::monospace(11.5))
                .color(TEXT_3),
        );
        ui.label(
            RichText::new(format!(
                "{} you own fit here{}",
                all.len(),
                if all.len() > 4 {
                    " · the four best are below"
                } else {
                    ""
                }
            ))
            .color(TEXT_2),
        );
    });
    if let Some(wc) = walk.weapon_compare(env, &step.key) {
        let mut bits = Vec::new();
        if let Some(b) = &wc.best_2h {
            bits.push(format!(
                "{} two-handed, {}",
                b.score.round(),
                name_tier(&env.corpus.rows[b.items[0].row])
            ));
        }
        if let Some((total, m, o)) = &wc.best_pair {
            bits.push(format!(
                "{} main + off, {} + {}",
                total.round(),
                name_tier(&env.corpus.rows[*m]),
                name_tier(&env.corpus.rows[*o])
            ));
        }
        if !bits.is_empty() {
            ui.label(RichText::new(bits.join(" · ")).color(TEXT_2));
        }
    }
    let occ = occupant(walk, env, si);
    ui.horizontal_wrapped(|ui| match &occ {
        None => {
            ui.label(RichText::new("nothing in this slot").color(TEXT_3));
        }
        Some(o) => {
            ui.label(RichText::new("On you now:").color(TEXT_3));
            ui.label(RichText::new(&o.name).color(TEXT));
            match o.score {
                Some(s) => {
                    let r = ui.label(
                        RichText::new(format!("{}", s.round()))
                            .font(FontId::monospace(11.5))
                            .color(TEXT),
                    );
                    if let Some(rec) = o.rec {
                        r.on_hover_text(why_text(
                            &env.cat.recs[rec],
                            env.corpus.rows[o.row].tier,
                            base_ac,
                            &step.slot,
                            env.sc,
                        ));
                    }
                }
                None => {
                    ui.label(
                        RichText::new("no wiki page, nothing here can be compared against it")
                            .color(TEXT_3),
                    );
                }
            }
            if let Some(m) = moved_to(walk, si, o.row) {
                ui.label(RichText::new(format!("this plan moves it to {}", m.label)).color(TEXT_3));
            } else {
                let eff = strip_effects(xd, env, o.row);
                if !eff.is_empty() {
                    mark(
                        ui,
                        State::You,
                        &format!(
                            "holds {} exaltation{}, strip first: {}",
                            eff.len(),
                            if eff.len() == 1 { "" } else { "s" },
                            eff.join(", ")
                        ),
                    );
                }
            }
        }
    });
    ui.add_space(6.0);

    if all.is_empty() {
        ui.label(RichText::new("Nothing you own fits here.").color(TEXT_3));
    }
    for (n, o) in all.iter().take(4).enumerate() {
        card(ui, n, o, occ.as_ref(), env, base_ac, actions, *remember);
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Skip, leave it empty").clicked() {
            actions.push(Action::Skip);
        }
        if ui.add_enabled(si > 0, egui::Button::new("Back")).clicked() {
            actions.push(Action::Back);
        }
        if ui.button("Restart").clicked() {
            actions.push(Action::Restart);
        }
        ui.checkbox(remember, "remember this pick beats the others");
        if all.len() > 4
            && ui
                .button(if *all_open {
                    "Show the four best only".to_owned()
                } else {
                    format!("Show all {}", all.len())
                })
                .clicked()
        {
            *all_open = !*all_open;
        }
    });
    if *all_open && all.len() > 4 {
        ui.add_space(4.0);
        all_table(ui, all, occ.as_ref(), env, base_ac, actions, *remember);
    }
    ui.add_space(6.0);
    auto_line(ui, walk, actions);
}

#[allow(clippy::too_many_arguments)]
fn card(
    ui: &mut Ui,
    n: usize,
    o: &Opt,
    occ: Option<&Occupant>,
    env: &Env,
    base_ac: f64,
    actions: &mut Vec<Action>,
    keep: bool,
) {
    let gain = occ.and_then(|x| x.score).map(|s| o.score - s);
    egui::Frame::NONE
        .fill(PANEL)
        .stroke(Stroke::new(1.0, RULE))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let r = ui.label(
                    RichText::new(format!("{:>5}", o.score.round()))
                        .font(FontId::monospace(16.0))
                        .color(GOLD),
                );
                if let Some(rec) = env.rec(o.items[0].row) {
                    r.on_hover_text(why_text(
                        rec,
                        env.corpus.rows[o.items[0].row].tier,
                        base_ac,
                        &o.items[0].slot,
                        env.sc,
                    ));
                }
                if occ.is_some() {
                    let (t, c) = gain_text(gain);
                    ui.label(RichText::new(t).font(FontId::monospace(12.0)).color(c));
                }
                ui.vertical(|ui| {
                    for it in &o.items {
                        let row = &env.corpus.rows[it.row];
                        ui.label(RichText::new(name_tier(row)).color(TEXT));
                        if let Some(rec) = env.rec(it.row) {
                            ui.label(RichText::new(stat_line(rec, row.tier)).color(TEXT_2));
                        }
                        ui.label(
                            RichText::new(format!(
                                "{}{}",
                                where_text(row),
                                if o.items.len() > 1 {
                                    format!(" · {}", it.slot)
                                } else {
                                    String::new()
                                }
                            ))
                            .color(TEXT_3),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Pick").clicked() {
                        actions.push(Action::Pick(n, keep));
                    }
                });
            });
        });
    ui.add_space(4.0);
}

fn all_table(
    ui: &mut Ui,
    all: &[Opt],
    occ: Option<&Occupant>,
    env: &Env,
    base_ac: f64,
    actions: &mut Vec<Action>,
    keep: bool,
) {
    egui::Grid::new("valet-all")
        .num_columns(7)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for h in ["score", "gain", "item", "where", "tier", "stats", ""] {
                ui.label(RichText::new(h).color(TEXT_3));
            }
            ui.end_row();
            for (n, o) in all.iter().enumerate() {
                let row = &env.corpus.rows[o.items[0].row];
                ui.label(
                    RichText::new(format!("{:>5}", o.score.round()))
                        .font(FontId::monospace(11.5))
                        .color(TEXT),
                );
                let (t, c) = gain_text(occ.and_then(|x| x.score).map(|s| o.score - s));
                ui.label(
                    RichText::new(if occ.is_some() { t } else { String::new() })
                        .font(FontId::monospace(11.5))
                        .color(c),
                );
                ui.label(
                    RichText::new(
                        o.items
                            .iter()
                            .map(|it| env.corpus.rows[it.row].base.clone())
                            .collect::<Vec<_>>()
                            .join(" + "),
                    )
                    .color(TEXT),
                );
                ui.label(RichText::new(where_text(row)).color(TEXT_2));
                ui.label(
                    RichText::new(if row.tier > 0 {
                        format!("+{}", row.tier)
                    } else {
                        String::new()
                    })
                    .font(FontId::monospace(11.5))
                    .color(TEXT_2),
                );
                ui.label(
                    RichText::new(
                        env.rec(o.items[0].row)
                            .map(|r| stat_line(r, row.tier))
                            .unwrap_or_default(),
                    )
                    .color(TEXT_2),
                );
                if ui.small_button("Pick").clicked() {
                    actions.push(Action::Pick(n, keep));
                }
                ui.end_row();
            }
        });
    let _ = base_ac;
}

fn auto_line(ui: &mut Ui, walk: &Walk, actions: &mut Vec<Action>) {
    let summary = walk.auto_summary();
    ui.horizontal_wrapped(|ui| {
        for (a, labels) in &summary {
            if labels.is_empty() {
                continue;
            }
            let n = labels.len();
            let text = match a {
                Auto::Mem => format!("{n} remembered"),
                Auto::Only => format!("{n} had one candidate"),
                Auto::Flat => format!("{n} nothing the scorer reads, kept what you have on"),
                Auto::TwoH => "off hand held by your two-hander".to_owned(),
                Auto::Skip => format!("{n} left empty"),
                Auto::NoneFit => format!("{n} had nothing that fits"),
            };
            ui.label(RichText::new(text).color(TEXT_3))
                .on_hover_text(labels.join(", "));
            if *a == Auto::Mem && ui.small_button("ask me again").clicked() {
                actions.push(Action::ClearMem);
            }
        }
    });
}

/// Why a score is that score, as hover text. The printed parts add up to the
/// printed score: rounding residue lands on the largest part.
fn why_text(rec: &GearRec, tier: u32, base_ac: f64, slot: &str, sc: &Scorer) -> String {
    let b = breakdown(rec, tier, base_ac, slot, sc);
    let shown = b.total.round() as i64;
    let mut rv: Vec<i64> = b.parts.iter().map(|p| p.v.round() as i64).collect();
    let resid = shown - rv.iter().sum::<i64>();
    if !rv.is_empty() && resid != 0 {
        let mut big = 0;
        for (i, x) in rv.iter().enumerate() {
            if x.abs() > rv[big].abs() {
                big = i;
            }
        }
        rv[big] += resid;
    }
    let mut out = format!(
        "{}{} · score {shown}\n",
        rec.name,
        if tier > 0 {
            format!(" +{tier}")
        } else {
            String::new()
        }
    );
    for (i, p) in b.parts.iter().enumerate() {
        let note = if p.w == 0.0 {
            "  worth nothing to this trio"
        } else if p.cap {
            "  past the AC softcap"
        } else {
            ""
        };
        out.push_str(&format!(
            "{:<8} {:>7} x {:<5} = {:>5}{note}\n",
            p.k, p.n, p.w, rv[i]
        ));
    }
    out.push_str(&format!("HP-equivalents for {} at {}, against {} AC already on. Weights are set on the Gear screen.", sc.classes.join("/"), sc.level, base_ac.round()));
    out
}

fn done_view(
    ui: &mut Ui,
    walk: &Walk,
    env: &Env,
    xd: Option<&ExaltDump>,
    actions: &mut Vec<Action>,
) {
    let plan = walk.plan_loadout(env);
    let cur = walk.current_loadout(env);
    let ds = plan.score - cur.score;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Wearing now").color(TEXT_3));
        ui.label(
            RichText::new(format!("{}", cur.score.round()))
                .font(FontId::monospace(14.0))
                .color(TEXT),
        );
        ui.label(RichText::new("this loadout").color(TEXT_3));
        ui.label(
            RichText::new(format!("{}", plan.score.round()))
                .font(FontId::monospace(14.0))
                .color(GOLD),
        );
        let (t, c) = gain_text(Some(ds));
        ui.label(RichText::new(t).font(FontId::monospace(14.0)).color(c));
        if ui.button("Restart").clicked() {
            actions.push(Action::Restart);
        }
        if ui.button("Back").clicked() {
            actions.push(Action::Back);
        }
    });
    let mut notes = Vec::new();
    for (a, labels) in walk.auto_summary() {
        if labels.is_empty() {
            continue;
        }
        match a {
            Auto::NoneFit => notes.push(format!("Nothing you own fits: {}.", labels.join(", "))),
            Auto::Flat => notes.push(format!("Kept what you have on, because nothing the scorer reads separates the candidates: {}.", labels.join(", "))),
            _ => {}
        }
    }
    if env.corpus.unmatched > 0 {
        let mut s = format!(
            "{} items in your dump have no wiki page, so they were not considered",
            env.corpus.unmatched
        );
        if !env.corpus.worn_unknown.is_empty() {
            s.push_str(&format!(
                ", including what you are wearing on {}. Check those swaps yourself.",
                env.corpus
                    .worn_unknown
                    .iter()
                    .map(|(sl, n)| format!("{sl} ({n})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else {
            s.push('.');
        }
        notes.push(s);
    }
    for n in notes {
        ui.label(RichText::new(n).color(TEXT_2));
    }
    ui.add_space(6.0);
    heading(ui, "THE FETCH LIST");
    let mut items = plan.items.clone();
    fetch_order(&mut items, env.corpus);
    egui::Grid::new("valet-fetch")
        .num_columns(8)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for h in [
                "where it is",
                "slot",
                "item",
                "tier",
                "comes off",
                "gain",
                "do",
                "stats",
            ] {
                ui.label(RichText::new(h).color(TEXT_3));
            }
            ui.end_row();
            for it in &items {
                let row = &env.corpus.rows[it.row];
                let worn = row.sec == Section::Worn;
                let occ = occupant(walk, env, it.step);
                let same = occ.as_ref().is_some_and(|o| {
                    o.rec.is_some() && o.rec == row.rec && env.corpus.rows[o.row].tier == row.tier
                });
                let moved = if same {
                    None
                } else {
                    occ.as_ref().and_then(|o| moved_to(walk, it.step, o.row))
                };
                let dim = if worn { TEXT_3 } else { TEXT };
                ui.label(RichText::new(where_text(row)).color(dim));
                ui.label(RichText::new(&walk.steps[it.step].label).color(dim));
                ui.label(RichText::new(&row.base).color(dim));
                ui.label(
                    RichText::new(if row.tier > 0 {
                        format!("+{}", row.tier)
                    } else {
                        String::new()
                    })
                    .font(FontId::monospace(11.5))
                    .color(dim),
                );
                match (&occ, same) {
                    (Some(o), false) => {
                        let mut s = o.name.clone();
                        if let Some(m) = moved {
                            s.push_str(&format!(" (moves to {})", m.label));
                        } else {
                            let eff = strip_effects(xd, env, o.row);
                            if !eff.is_empty() {
                                s.push_str(&format!(
                                    " (holds {} exaltation{}, strip first)",
                                    eff.len(),
                                    if eff.len() == 1 { "" } else { "s" }
                                ));
                            }
                        }
                        ui.label(RichText::new(s).color(TEXT_2));
                    }
                    (None, _) => {
                        ui.label(RichText::new("nothing").color(TEXT_3));
                    }
                    _ => {
                        ui.label("");
                    }
                }
                let gain = if same {
                    None
                } else {
                    match &occ {
                        None => Some(it.score),
                        Some(o) => o.score.map(|s| it.score - s),
                    }
                };
                if same {
                    ui.label("");
                } else {
                    let (t, c) = gain_text(gain);
                    ui.label(RichText::new(t).font(FontId::monospace(11.5)).color(c));
                }
                ui.label(
                    RichText::new(if worn { "already on" } else { "fetch" }).color(if worn {
                        TEXT_3
                    } else {
                        GOLD
                    }),
                );
                ui.label(
                    RichText::new(
                        env.rec(it.row)
                            .map(|r| stat_line(r, row.tier))
                            .unwrap_or_default(),
                    )
                    .color(TEXT_2),
                );
                ui.end_row();
            }
            for k in &plan.kept {
                let row = &env.corpus.rows[k.row];
                ui.label(RichText::new("worn").color(TEXT_3));
                ui.label(RichText::new(&walk.steps[k.step].label).color(TEXT_3));
                ui.label(RichText::new(&row.base).color(TEXT_3));
                ui.label("");
                ui.label("");
                ui.label("");
                ui.label(RichText::new("keep").color(TEXT_3));
                ui.label(
                    RichText::new(if row.rec.is_some() {
                        "not scored here"
                    } else {
                        "no wiki page, cannot be scored"
                    })
                    .color(TEXT_3),
                );
                ui.end_row();
            }
        });
    ui.add_space(8.0);
    heading(ui, "BEFORE AND AFTER");
    let mut keys: Vec<&String> = cur.st.keys().chain(plan.st.keys()).collect();
    keys.sort();
    keys.dedup();
    if keys.is_empty() {
        ui.label(
            RichText::new("Nothing you are wearing or picking has a stat the wiki lists.")
                .color(TEXT_3),
        );
        return;
    }
    egui::Grid::new("valet-cmp")
        .num_columns(4)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for h in ["stat", "wearing now", "this loadout", "difference"] {
                ui.label(RichText::new(h).color(TEXT_3));
            }
            ui.end_row();
            for k in keys {
                let a = cur.st.get(k).copied().unwrap_or(0);
                let b = plan.st.get(k).copied().unwrap_or(0);
                /* a stat key in caps is a caps run, so it is set in the display face (D8) */
                ui.label(
                    RichText::new(k.to_uppercase())
                        .font(crate::fonts::display(10.5))
                        .color(TEXT_2),
                );
                ui.label(
                    RichText::new(format!("{a:>5}"))
                        .font(FontId::monospace(11.5))
                        .color(TEXT),
                );
                ui.label(
                    RichText::new(format!("{b:>5}"))
                        .font(FontId::monospace(11.5))
                        .color(TEXT),
                );
                let d = b - a;
                ui.label(
                    RichText::new(if d > 0 {
                        format!("{d:>+5}")
                    } else if d < 0 {
                        format!("{d:>5}")
                    } else {
                        String::new()
                    })
                    .font(FontId::monospace(11.5))
                    .color(if d > 0 { TEXT } else { TEXT_2 }),
                );
                ui.end_row();
            }
        });
}

/// The exaltation audit's idle half, drawn: every exaltation you own that is not on your
/// body, where it is, and the homes the audit found for it. `Home::def` names the catalogue
/// stone that would go in, `current` what the swap would displace, `needs` the tier a later home
/// waits on; an idle stone with none of the three carries the audit's one-line reason.
fn idle_list(ui: &mut Ui, a: &Audit, xd: &ExaltDump, rows: &[InvRow], xcat: &ExaltCat) {
    if a.idle.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(
        RichText::new(format!(
            "{} exaltation{} not on your body",
            a.idle.len(),
            if a.idle.len() == 1 { "" } else { "s" }
        ))
        .color(TEXT_2),
    )
    .id_salt("valet-idle")
    .icon(crate::chrome::fold_icon)
    .default_open(a.summary.usable > 0)
    .show(ui, |ui| {
        let slot_of = |h: &Home| xd.worn.get(h.w).map(|w| w.slot.clone()).unwrap_or_default();
        let effect_of = |def: usize| {
            xcat.stones
                .get(def)
                .map(|s| s.effect.clone())
                .unwrap_or_default()
        };
        egui::Grid::new("valet-idle-grid")
            .num_columns(4)
            .spacing([14.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                for h in ["stone", "where", "what it is", "where it could go"] {
                    ui.label(RichText::new(h).color(TEXT_3));
                }
                ui.end_row();
                for it in &a.idle {
                    let name = rows
                        .get(it.row)
                        .map(|r| r.name.clone())
                        .unwrap_or_else(|| it.name.clone());
                    ui.label(RichText::new(name).color(TEXT));
                    let host = it
                        .host
                        .and_then(|h| rows.get(h))
                        .map(|r| format!("socketed in {}", r.name))
                        .unwrap_or_else(|| it.sec.label().to_owned());
                    ui.label(RichText::new(host).color(TEXT_2));
                    let what = if it.unknown {
                        if it.item.is_some() {
                            "no effect of a known kind on the wiki".to_owned()
                        } else {
                            "no wiki page for its source".to_owned()
                        }
                    } else {
                        let mut w = it
                            .defs
                            .iter()
                            .map(|&d| {
                                format!(
                                    "{} {}",
                                    xcat.stones
                                        .get(d)
                                        .and_then(|s| s.kind)
                                        .map(Kind::label)
                                        .unwrap_or("?"),
                                    effect_of(d)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" · ");
                        if it.maybe {
                            w.push_str(" (one of these; the dump does not say which)");
                        }
                        if it.deity {
                            w.push_str(" · deity bound");
                        }
                        if let Some(k) = it.kind {
                            if it.defs.is_empty() {
                                w = k.label().to_owned();
                            }
                        }
                        w
                    };
                    ui.label(RichText::new(what).color(if it.unknown { TEXT_3 } else { TEXT_2 }));
                    let mut homes: Vec<String> = Vec::new();
                    for h in &it.empty {
                        homes.push(format!(
                            "{} {} now ({})",
                            slot_of(h),
                            h.kind.label().to_lowercase(),
                            effect_of(h.def)
                        ));
                    }
                    for h in &it.occupied {
                        homes.push(format!(
                            "{} {} replacing {}",
                            slot_of(h),
                            h.kind.label().to_lowercase(),
                            h.current.as_deref().unwrap_or("what is in it")
                        ));
                    }
                    for h in &it.later {
                        homes.push(format!(
                            "{} {} at +{}",
                            slot_of(h),
                            h.kind.label().to_lowercase(),
                            h.needs.unwrap_or(0)
                        ));
                    }
                    if homes.is_empty() {
                        ui.label(
                            RichText::new(it.why.unwrap_or("nothing worn takes it")).color(TEXT_3),
                        );
                    } else {
                        ui.label(
                            RichText::new(homes.join("; ")).color(if it.empty.is_empty() {
                                TEXT_2
                            } else {
                                TEXT
                            }),
                        );
                    }
                    ui.end_row();
                }
            });
    });
}

#[allow(clippy::too_many_arguments)]
fn slot_view_ui(
    ui: &mut Ui,
    corpus: &Corpus,
    cat: &Catalogue,
    sc: &Scorer,
    trio: &[String],
    ranker: Option<&mut Ranker>,
    xd: Option<&ExaltDump>,
    audit: Option<&Audit>,
    slot: &mut String,
    gap_all: &mut bool,
) {
    let flags: HashSet<String> = audit
        .map(|a| {
            a.empty
                .iter()
                .filter(|e| !e.fits.is_empty())
                .filter_map(|e| xd.map(|x| x.worn[e.w].slot.clone()))
                .collect()
        })
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        for (s, label) in browse() {
            let text = if flags.contains(&s) {
                format!("{label} ·")
            } else {
                label.clone()
            };
            let r = ui.selectable_label(*slot == s, text);
            if flags.contains(&s) {
                r.clone()
                    .on_hover_text("an exaltation you own fits an empty socket here");
            }
            if r.clicked() {
                *slot = s.clone();
            }
        }
    });
    ui.add_space(4.0);
    let sv = slot_view(corpus, cat, sc, slot, trio, ranker, xd);
    let label = browse()
        .into_iter()
        .find(|(s, _)| s == slot)
        .map(|(_, l)| l)
        .unwrap_or_else(|| slot.clone());
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(label)
                .font(crate::fonts::display(15.0))
                .color(GOLD_HI),
        );
        if sv.now.is_empty() {
            ui.label(RichText::new("Nothing in this slot.").color(TEXT_3));
        }
        for (i, n) in sv.now.iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("+").color(TEXT_3));
            }
            ui.label(RichText::new(&n.name).color(TEXT));
            if n.rec.is_none() {
                ui.label(RichText::new("no wiki page").color(TEXT_3));
            }
        }
        if let Some((_, Some(_), total)) = sv.hands_now {
            ui.label(
                RichText::new(format!(
                    "Main + off together: {}, the number a two-hander has to beat.",
                    total.round()
                ))
                .color(TEXT_3),
            );
        }
    });
    ui.label(
        RichText::new(format!(
            "{} cop{} fit · priced against {} AC already on",
            sv.owned.len(),
            if sv.owned.len() == 1 { "y" } else { "ies" },
            sv.base_ac.round()
        ))
        .color(TEXT_2),
    );
    ui.add_space(4.0);
    egui::Grid::new("valet-own")
        .num_columns(8)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for h in [
                "rank",
                "score",
                "item",
                "tier",
                "where",
                "sockets",
                "catalogue",
                "stats",
            ] {
                ui.label(RichText::new(h).color(TEXT_3));
            }
            ui.end_row();
            if sv.owned.is_empty() {
                ui.label(RichText::new("Nothing you own fits here.").color(TEXT_3));
                ui.end_row();
            }
            for c in &sv.owned {
                let row = &corpus.rows[c.row];
                let col = if c.worn_here { GOLD_HI } else { TEXT };
                ui.label(
                    RichText::new(format!("{:>3}", c.rank))
                        .font(FontId::monospace(11.5))
                        .color(col),
                );
                let r = ui.label(
                    RichText::new(format!("{:>5}", c.score.round()))
                        .font(FontId::monospace(11.5))
                        .color(col),
                );
                if let Some(rec) = row.rec {
                    r.on_hover_text(why_text(&cat.recs[rec], row.tier, sv.base_ac, slot, sc));
                }
                let mut chips = Vec::new();
                if c.two_h {
                    chips.push("2H");
                }
                if c.unscored.contains(&"charges") {
                    chips.push("charges");
                }
                if !c.legal {
                    chips.push("wiki: not your class");
                }
                if c.worn_here && !c.placeable {
                    chips.push("wiki: cannot hold it");
                }
                ui.label(
                    RichText::new(format!(
                        "{}{}",
                        row.base,
                        if chips.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", chips.join(", "))
                        }
                    ))
                    .color(col),
                );
                ui.label(
                    RichText::new(if row.tier > 0 {
                        format!("+{}", row.tier)
                    } else {
                        String::new()
                    })
                    .font(FontId::monospace(11.5))
                    .color(TEXT_2),
                );
                ui.label(
                    RichText::new(if c.worn_here {
                        "on you".to_owned()
                    } else if let Some(w) = c.worn_in {
                        format!("on you · {w}")
                    } else {
                        where_text(row)
                    })
                    .color(TEXT_2),
                );
                match &c.sockets {
                    Some(s) => {
                        let mut parts = Vec::new();
                        for k in Kind::ALL {
                            let so = &s[k.index()];
                            let state = if !so.open {
                                continue;
                            } else if so.filled {
                                "filled"
                            } else if so.empty {
                                "empty"
                            } else {
                                "open"
                            };
                            parts.push(format!("{} {state}", k.label().to_lowercase()));
                        }
                        ui.label(
                            RichText::new(if parts.is_empty() {
                                "no sockets yet".to_owned()
                            } else {
                                parts.join(", ")
                            })
                            .color(TEXT_2),
                        );
                    }
                    None => {
                        ui.label(
                            RichText::new(if c.worn_here { "" } else { "not listed" })
                                .color(TEXT_3),
                        );
                    }
                }
                match &c.catalog {
                    Some(k) => {
                        ui.label(
                            RichText::new(format!("{} of {} · {}", k.rank, k.of, k.cls))
                                .font(FontId::monospace(11.5))
                                .color(TEXT_2),
                        );
                    }
                    None => {
                        ui.label(RichText::new("").color(TEXT_3));
                    }
                }
                ui.label(
                    RichText::new(
                        row.rec
                            .map(|r| stat_line(&cat.recs[r], row.tier))
                            .unwrap_or_default(),
                    )
                    .color(TEXT_2),
                );
                ui.end_row();
            }
        });

    ui.add_space(8.0);
    heading(ui, "NOT IN YOUR DUMP");
    let owned = owned_keys(corpus, cat);
    let gap = slot_gap(cat, sc, slot, &owned, sv.base_ac, sv.par_tier);
    let best = sv.owned.first().map(|c| c.score).unwrap_or(0.0);
    let above: Vec<&GapItem> = gap.items.iter().filter(|g| g.s_par > best).collect();
    if gap.items.is_empty() {
        ui.label(RichText::new("The catalogue has nothing in era your trio can wear here that you do not already own.").color(TEXT_3));
        return;
    }
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{} of {} score above your best",
                above.len(),
                gap.items.len()
            ))
            .color(TEXT_2),
        );
        ui.label(
            RichText::new(format!(
                "In era, wearable by your trio, not in your dump.{}",
                if gap.par_tier > 0 {
                    format!(" +{} is the rank of your best copy.", gap.par_tier)
                } else {
                    String::new()
                }
            ))
            .color(TEXT_3),
        );
        if gap.items.len() > above.len() {
            let t = if *gap_all {
                format!("Show only the {} above your best", above.len())
            } else {
                format!("Show all {}", gap.items.len())
            };
            if ui.small_button(t).clicked() {
                *gap_all = !*gap_all;
            }
        }
    });
    let list: Vec<&GapItem> = if *gap_all {
        gap.items.iter().collect()
    } else {
        above
    };
    egui::Grid::new("valet-gap")
        .num_columns(5)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label(
                RichText::new(if gap.par_tier > 0 {
                    "score +0"
                } else {
                    "score"
                })
                .color(TEXT_3),
            );
            ui.label(
                RichText::new(if gap.par_tier > 0 {
                    format!("at +{}", gap.par_tier)
                } else {
                    String::new()
                })
                .color(TEXT_3),
            );
            ui.label(RichText::new("item").color(TEXT_3));
            ui.label(RichText::new("stats").color(TEXT_3));
            ui.label(RichText::new("source").color(TEXT_3));
            ui.end_row();
            if list.is_empty() {
                ui.label(
                    RichText::new(
                        "Nothing in the catalogue scores above your best copy at its rank.",
                    )
                    .color(TEXT_3),
                );
                ui.end_row();
            }
            for g in list.iter().take(300) {
                let rec = &cat.recs[g.rec];
                let r = ui.label(
                    RichText::new(format!("{:>5}", g.s0.round()))
                        .font(FontId::monospace(11.5))
                        .color(TEXT),
                );
                r.on_hover_text(why_text(rec, 0, sv.base_ac, slot, sc));
                if gap.par_tier > 0 {
                    let r = ui.label(
                        RichText::new(format!("{:>5}", g.s_par.round()))
                            .font(FontId::monospace(11.5))
                            .color(GOLD),
                    );
                    r.on_hover_text(why_text(rec, gap.par_tier, sv.base_ac, slot, sc));
                } else {
                    ui.label("");
                }
                let mut chips = Vec::new();
                if g.two_h {
                    chips.push("2H".to_owned());
                }
                if let Some(v) = &g.variant {
                    chips.push(format!("{} pages under this name: {v}", g.variants));
                }
                if g.unscored.contains(&"charges") {
                    chips.push("charges".into());
                }
                ui.label(
                    RichText::new(format!(
                        "{}{}",
                        rec.name,
                        if chips.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", chips.join(", "))
                        }
                    ))
                    .color(TEXT),
                );
                ui.label(RichText::new(stat_line(rec, gap.par_tier)).color(TEXT_2));
                let mut src = rec.src_zones.first().cloned().unwrap_or_default();
                if rec.quest {
                    if !src.is_empty() {
                        src.push_str(" · ");
                    }
                    src.push_str("quest");
                }
                ui.label(RichText::new(src).color(TEXT_2));
                ui.end_row();
            }
        });
    if list.len() > 300 {
        ui.label(RichText::new(format!("{} more not shown", list.len() - 300)).color(TEXT_3));
    }
}

fn heading(ui: &mut Ui, s: &str) {
    ui.label(
        RichText::new(s)
            .font(crate::fonts::display(12.0))
            .color(GOLD),
    );
    ui.add_space(4.0);
}

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

/* --------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::testdata;
    use std::path::Path;

    fn trio() -> Vec<String> {
        vec!["WAR".into(), "CLR".into(), "WIZ".into()]
    }

    fn cat_of(json: &[&str]) -> Catalogue {
        let vals: Vec<serde_json::Value> = json
            .iter()
            .map(|s| serde_json::from_str(s).expect("fixture json"))
            .collect();
        Catalogue::from_values(vals.iter())
    }

    fn dump_of(text: &str) -> InventoryDump {
        InventoryDump::parse(Path::new("Tester_qeynos-Inventory.txt"), text, None)
    }

    fn header() -> &'static str {
        "Location\tName\tID\tCount\tSlots\r\n"
    }

    /// A small catalogue with one of everything the rules need.
    fn fixture_cat() -> Catalogue {
        cat_of(&[
            r#"{"n":"Plain Helm","t":"Plain_Helm","cls":{"all":1},"sl":["Head"],"st":{"ac":10,"hp":20}}"#,
            r#"{"n":"Better Helm","t":"Better_Helm","cls":{"all":1},"sl":["Head"],"st":{"ac":20,"hp":40}}"#,
            r#"{"n":"Druid Helm","t":"Druid_Helm","cls":{"c":["DRU"]},"sl":["Head"],"st":{"ac":30,"hp":60}}"#,
            r#"{"n":"Lore Earring","t":"Lore_Earring","cls":{"all":1},"fl":["lore"],"sl":["Ear"],"st":{"hp":30}}"#,
            r#"{"n":"Plain Earring","t":"Plain_Earring","cls":{"all":1},"sl":["Ear"],"st":{"hp":10}}"#,
            r#"{"n":"Big Sword","t":"Big_Sword","cls":{"all":1},"sl":["Primary"],"skill":"2H Slashing","dmg":30,"dly":40,"st":{"str":5}}"#,
            r#"{"n":"Small Sword","t":"Small_Sword","cls":{"all":1},"sl":["Primary","Secondary"],"skill":"1H Slashing","dmg":10,"dly":20,"st":{"str":2}}"#,
            r#"{"n":"Shield","t":"Shield","cls":{"all":1},"sl":["Secondary"],"st":{"ac":15,"hp":15}}"#,
            r#"{"n":"Arrow","t":"Arrow","cls":{"all":1},"sl":["Ammo"]}"#,
            r#"{"n":"Other Arrow","t":"Other_Arrow","cls":{"all":1},"sl":["Ammo"]}"#,
            r#"{"n":"Old Helm","t":"Old_Helm","cls":{"all":1},"sl":["Head"],"oe":1,"st":{"ac":99,"hp":99}}"#,
            r#"{"n":"Charm Thing","t":"Charm_Thing","cls":{"all":1},"sl":["Charm"],"st":{"hp":5},"charges":3}"#,
            r#"{"n":"Twin Gorget","t":"Twin_Gorget_A","cls":{"all":1},"sl":["Neck"],"st":{"ac":5}}"#,
            r#"{"n":"Twin Gorget","t":"Twin_Gorget_B","cls":{"all":1},"sl":["Neck"],"st":{"ac":5}}"#,
            r#"{"n":"Twin Gorget","t":"Twin_Gorget_C","cls":{"all":1},"sl":["Neck"],"st":{"ac":85}}"#,
        ])
    }

    fn fixture_dump() -> InventoryDump {
        let text = format!(
            "{}{}",
            header(),
            [
                "Head\tPlain Helm +2\t1\t1\t10",
                "Ear\tLore Earring\t2\t1\t10",
                "Ear\tPlain Earring\t3\t1\t10",
                "Primary\tSmall Sword\t4\t1\t10",
                "Secondary\tSmall Sword\t4\t1\t10",
                "Ammo\tArrow\t5\t20\t0",
                "General 1\tBackpack*\t6\t1\t10",
                "General 1-Slot1\tBetter Helm\t7\t1\t0",
                "General 1-Slot2\tLore Earring\t2\t1\t0",
                "General 1-Slot3\tBig Sword\t8\t1\t0",
                "General 1-Slot4\tOther Arrow\t9\t20\t0",
                "Bank1\tDruid Helm\t10\t1\t0",
                "Bank2\tShield\t11\t1\t0",
                "Bank2-Slot1\tCharm Thing\t12\t1\t0",
                "Augmentation\tBetter Helm (Exaltation)\t7\t1\t0",
                "General 2\tMystery Thing\t13\t1\t0",
                "Neck\tTwin Gorget\t14\t1\t10",
            ]
            .join("\r\n")
        );
        dump_of(&text)
    }

    fn scorer(corpus: &Corpus, cat: &Catalogue) -> Scorer {
        Scorer::make(
            &trio(),
            50,
            Some(&corpus.equipped),
            Some(cat),
            &HashMap::new(),
        )
    }

    fn row_named(corpus: &Corpus, name: &str) -> usize {
        corpus
            .rows
            .iter()
            .position(|r| r.name == name)
            .unwrap_or_else(|| panic!("{name} in corpus"))
    }

    /* ---- the order ---- */

    #[test]
    fn build_steps_is_the_written_order_with_the_hands_and_any_slot_last() {
        let s = build_steps();
        assert_eq!(s.len(), 24);
        assert_eq!(s[0].key, "Head#1");
        assert_eq!(s[2].key, "Ear#1");
        assert_eq!(s[3].key, "Ear#2");
        assert_eq!(s[3].label, "Ear (2)");
        assert_eq!(s[3].n, 2);
        assert_eq!(s[19].key, "Ammo#1");
        assert_eq!(
            (s[20].key.as_str(), s[20].kind, s[20].label.as_str()),
            ("Primary#1", StepKind::Primary, "Main hand")
        );
        assert_eq!(
            (s[21].key.as_str(), s[21].kind, s[21].label.as_str()),
            ("Secondary#1", StepKind::Secondary, "Off hand")
        );
        assert_eq!(
            (s[22].key.as_str(), s[22].kind),
            ("Any Slot#1", StepKind::Any)
        );
        assert_eq!(
            (s[23].key.as_str(), s[23].label.as_str()),
            ("Any Slot#2", "Any Slot (2)")
        );
        let b = browse();
        assert_eq!(b.len(), 20);
        assert_eq!(b[0].0, "Head");
        assert_eq!(b[17].1, "Main hand");
        assert!(!b.iter().any(|(s, _)| s == "Power Source"));
    }

    #[test]
    fn fetch_order_reads_bags_first_and_worn_last_one_container_at_a_time() {
        assert!(sec_rank(Section::Bags) < sec_rank(Section::Storage));
        assert!(sec_rank(Section::Storage) < sec_rank(Section::Bank));
        assert!(sec_rank(Section::Bank) < sec_rank(Section::Worn));
        assert_eq!(sec_rank(Section::KeyRing), -1, "the key ring sorts first");
        let cat = fixture_cat();
        let text = format!(
            "{}{}",
            header(),
            [
                "Head\tPlain Helm\t1\t1\t10",
                "Bank12\tPlain Helm\t1\t1\t0",
                "Bank12-Slot3\tPlain Helm\t1\t1\t0",
                "Bank12-Slot1\tPlain Helm\t1\t1\t0",
                "Bank2\tPlain Helm\t1\t1\t0",
                "General 3-Slot2\tPlain Helm\t1\t1\t0",
                "Equipment\tPlain Helm\t1"
            ]
            .join("\r\n")
        );
        let corpus = Corpus::read(&dump_of(&text), &cat);
        let mut items: Vec<PlanItem> = (0..corpus.rows.len())
            .map(|i| PlanItem {
                step: 0,
                slot: "Head".into(),
                row: i,
                score: 0.0,
                base_ac: 0.0,
            })
            .collect();
        fetch_order(&mut items, &corpus);
        let locs: Vec<&str> = items
            .iter()
            .map(|it| corpus.rows[it.row].loc.as_str())
            .collect();
        assert_eq!(
            locs,
            vec![
                "General 3-Slot2",
                "Equipment",
                "Bank2",
                "Bank12",
                "Bank12-Slot1",
                "Bank12-Slot3",
                "Head"
            ]
        );
        assert_eq!(loc_numbers("General 8-Slot6"), (8, 6));
        assert_eq!(loc_numbers("Bank12"), (12, 0));
        assert_eq!(loc_numbers("Equipment"), (0, 0));
    }

    /* ---- the corpus ---- */

    #[test]
    fn read_inventory_never_resolves_a_stone_and_pairs_worn_entries_with_rows() {
        let cat = fixture_cat();
        let corpus = Corpus::read(&fixture_dump(), &cat);
        let stone = corpus.rows.iter().find(|r| r.exalt).expect("the stone row");
        assert_eq!(
            stone.rec, None,
            "a stone is named after an item and is not one"
        );
        assert_eq!(
            corpus.unmatched, 2,
            "Backpack and Mystery Thing have no page"
        );
        assert!(corpus.worn_unknown.is_empty());
        let (e, row) = corpus.wearing_at("Ear", 2).expect("second ear");
        assert_eq!(e.name, "Plain Earring");
        assert_eq!(corpus.rows[row].name, "Plain Earring");
        assert_eq!(corpus.rows[row].loc, "Ear");
        assert!(corpus.wearing_at("Ear", 3).is_none());
        assert!(corpus.wearing_at("Face", 1).is_none());
        assert_eq!(corpus.equipped.get("Head")[0].tier, 2);
        assert_eq!(
            item_id(&corpus.rows[row_named(&corpus, "Plain Helm +2")], &cat),
            "Plain_Helm+2"
        );
    }

    /* ---- the walk ---- */

    #[test]
    fn worn_illegal_item_is_still_a_candidate_and_a_bank_one_is_not() {
        let cat = fixture_cat();
        let text = format!(
            "{}{}",
            header(),
            [
                "Head\tDruid Helm\t10\t1\t10",
                "Bank1\tDruid Helm\t10\t1\t0",
                "Bank2\tPlain Helm\t1\t1\t0"
            ]
            .join("\r\n")
        );
        let corpus = Corpus::read(&dump_of(&text), &cat);
        let sc = scorer(&corpus, &cat);
        let env = Env {
            corpus: &corpus,
            cat: &cat,
            sc: &sc,
        };
        let step = build_steps()
            .into_iter()
            .find(|s| s.key == "Head#1")
            .unwrap();
        let opts = options_for(&env, &step, &HashSet::new(), &HashSet::new(), 0.0);
        let names: Vec<&str> = opts
            .iter()
            .map(|o| corpus.rows[o.items[0].row].loc.as_str())
            .collect();
        assert!(
            names.contains(&"Head"),
            "the worn druid helm is legal because it is on: {names:?}"
        );
        assert!(
            !names.contains(&"Bank1"),
            "the banked druid helm is gated by the wiki: {names:?}"
        );
        assert!(names.contains(&"Bank2"));
    }

    #[test]
    fn a_two_hander_settles_the_off_hand_without_asking() {
        let cat = fixture_cat();
        let text = format!(
            "{}{}",
            header(),
            [
                "General 1\tBig Sword\t8\t1\t0",
                "General 2\tSmall Sword\t4\t1\t0",
                "General 3\tShield\t11\t1\t0"
            ]
            .join("\r\n")
        );
        let corpus = Corpus::read(&dump_of(&text), &cat);
        let sc = scorer(&corpus, &cat);
        let env = Env {
            corpus: &corpus,
            cat: &cat,
            sc: &sc,
        };
        let mut w = Walk::new(&env, Memory::default());
        assert_eq!(
            w.step().map(|s| s.key.as_str()),
            Some("Primary#1"),
            "every body slot has nothing, the hands are the first question"
        );
        let big = w
            .options("Primary#1")
            .iter()
            .find(|o| corpus.rows[o.items[0].row].name == "Big Sword")
            .cloned()
            .expect("the 2H is offered");
        w.pick(&env, "Primary#1", big, false);
        assert_eq!(w.auto.get("Secondary#1"), Some(&Auto::TwoH));
        assert_eq!(w.picks.get("Secondary#1"), Some(&None));
        assert!(
            w.step().map(|s| s.key != "Secondary#1").unwrap_or(true),
            "the walk did not stop on the off hand"
        );
        /* the weapon compare on the main hand step says both numbers */
        w.restart(&env);
        let wc = w
            .weapon_compare(&env, "Primary#1")
            .expect("main hand context");
        assert!(wc.best_2h.is_some());
        let (_, m, o) = wc.best_pair.expect("a main+off pair exists");
        assert_eq!(corpus.rows[m].name, "Small Sword");
        assert_eq!(corpus.rows[o].name, "Shield");
        assert!(w.weapon_compare(&env, "Head#1").is_none());
    }

    #[test]
    fn a_lore_item_fills_one_position_only() {
        let cat = fixture_cat();
        let text = format!(
            "{}{}",
            header(),
            [
                "General 1\tLore Earring\t2\t1\t0",
                "General 2\tLore Earring\t2\t1\t0",
                "General 3\tPlain Earring\t3\t1\t0"
            ]
            .join("\r\n")
        );
        let corpus = Corpus::read(&dump_of(&text), &cat);
        let sc = scorer(&corpus, &cat);
        let env = Env {
            corpus: &corpus,
            cat: &cat,
            sc: &sc,
        };
        let mut w = Walk::new(&env, Memory::default());
        /* Ear#1: two candidates after dedupe (the two lore copies are one offer), so it asks */
        assert_eq!(w.step().map(|s| s.key.as_str()), Some("Ear#1"));
        let opts = w.options("Ear#1").to_vec();
        assert_eq!(
            opts.len(),
            2,
            "two copies of one item at one tier are one offer"
        );
        let lore = opts
            .iter()
            .find(|o| corpus.rows[o.items[0].row].name == "Lore Earring")
            .cloned()
            .unwrap();
        w.pick(&env, "Ear#1", lore, false);
        let names: Vec<&str> = w
            .options("Ear#2")
            .iter()
            .map(|o| corpus.rows[o.items[0].row].name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["Plain Earring"],
            "the second lore copy cannot fill the second ear"
        );
        assert_eq!(w.auto.get("Ear#2"), Some(&Auto::Only));
    }

    #[test]
    fn flat_and_only_settle_without_a_question() {
        let cat = fixture_cat();
        let text = format!(
            "{}{}",
            header(),
            [
                "Ammo\tArrow\t5\t20\t0",
                "General 1\tOther Arrow\t9\t20\t0",
                "General 2\tPlain Helm\t1\t1\t0"
            ]
            .join("\r\n")
        );
        let corpus = Corpus::read(&dump_of(&text), &cat);
        let sc = scorer(&corpus, &cat);
        let env = Env {
            corpus: &corpus,
            cat: &cat,
            sc: &sc,
        };
        let w = Walk::new(&env, Memory::default());
        /* Every body slot settles itself. The walk then stops on Any Slot (1), because the two
         * arrows are still unassigned, both fit "anything wearable", and nothing is worn there
         * for the flat rule to keep. */
        assert_eq!(
            w.step().map(|s| s.key.as_str()),
            Some("Any Slot#1"),
            "{:?}",
            w.auto
        );
        assert_eq!(
            w.auto.get("Ammo#1"),
            Some(&Auto::Flat),
            "two quivers at zero with one on: keep what is on"
        );
        assert_eq!(w.picks.get("Ammo#1"), Some(&None));
        assert_eq!(w.auto.get("Head#1"), Some(&Auto::Only));
        assert!(w.picks.get("Head#1").unwrap().is_some());
        assert_eq!(w.auto.get("Face#1"), Some(&Auto::NoneFit));
        let summary = w.auto_summary();
        assert!(summary
            .iter()
            .any(|(a, l)| *a == Auto::Flat && l == &vec!["Ammo".to_owned()]));
        /* the plan keeps the arrow it could not price */
        let plan = w.plan_loadout(&env);
        assert!(plan.kept.iter().any(|k| corpus.rows[k.row].name == "Arrow"));
        assert_eq!(plan.items.len(), 1);
    }

    #[test]
    fn a_remembered_decision_stands_until_something_new_beats_the_floor() {
        let cat = fixture_cat();
        let text = format!(
            "{}{}",
            header(),
            [
                "General 1\tPlain Helm\t1\t1\t0",
                "General 2\tBetter Helm\t7\t1\t0"
            ]
            .join("\r\n")
        );
        let corpus = Corpus::read(&dump_of(&text), &cat);
        let sc = scorer(&corpus, &cat);
        let env = Env {
            corpus: &corpus,
            cat: &cat,
            sc: &sc,
        };
        let mut w = Walk::new(&env, Memory::default());
        assert_eq!(w.step().map(|s| s.key.as_str()), Some("Head#1"));
        /* choose the WORSE one and remember it */
        let plain = w
            .options("Head#1")
            .iter()
            .find(|o| corpus.rows[o.items[0].row].name == "Plain Helm")
            .cloned()
            .unwrap();
        w.pick(&env, "Head#1", plain.clone(), true);
        assert!(w.mem.dirty);
        let m = w.mem.get("Head#1").expect("remembered");
        assert_eq!(m.pick, plain.id);
        assert_eq!(m.set.len(), 2);
        /* a fresh walk with that memory does not ask again */
        let mem = w.mem.clone();
        let w2 = Walk::new(&env, mem.clone());
        assert_eq!(w2.auto.get("Head#1"), Some(&Auto::Mem));
        assert_eq!(
            w2.picks
                .get("Head#1")
                .unwrap()
                .as_ref()
                .map(|o| o.id.clone()),
            Some(plain.id.clone())
        );
        /* a new helm that beats the floor re-opens the slot */
        let text = format!(
            "{}{}",
            header(),
            [
                "General 1\tPlain Helm\t1\t1\t0",
                "General 2\tBetter Helm\t7\t1\t0",
                "General 3\tBetter Helm +5\t7\t1\t0"
            ]
            .join("\r\n")
        );
        let corpus3 = Corpus::read(&dump_of(&text), &cat);
        let sc3 = scorer(&corpus3, &cat);
        let env3 = Env {
            corpus: &corpus3,
            cat: &cat,
            sc: &sc3,
        };
        let w3 = Walk::new(&env3, mem.clone());
        assert_eq!(
            w3.step().map(|s| s.key.as_str()),
            Some("Head#1"),
            "the field changed, ask again"
        );
        assert!(!w3.auto.contains_key("Head#1"));
        /* no longer owning the pick re-opens too */
        let text = format!(
            "{}{}",
            header(),
            [
                "General 2\tBetter Helm\t7\t1\t0",
                "General 3\tOld Helm\t7\t1\t0"
            ]
            .join("\r\n")
        );
        let corpus4 = Corpus::read(&dump_of(&text), &cat);
        let sc4 = scorer(&corpus4, &cat);
        let env4 = Env {
            corpus: &corpus4,
            cat: &cat,
            sc: &sc4,
        };
        let w4 = Walk::new(&env4, mem);
        assert!(w4.auto.get("Head#1") != Some(&Auto::Mem));
        /* back onto a remembered step forgets it */
        let mut w5 = Walk::new(&env, w.mem.clone());
        assert!(w5.done());
        assert!(w5.back(&env));
        assert_eq!(w5.step().map(|s| s.key.as_str()), Some("Head#1"));
        assert!(w5.mem.get("Head#1").is_none());
    }

    #[test]
    fn the_off_hand_gate_and_its_worn_escape() {
        let cat = fixture_cat();
        /* WAR/CLR/WIZ: the warrior can dual wield, so a damaging one-hander is allowed */
        let text = format!(
            "{}{}",
            header(),
            [
                "General 1\tBig Sword\t8\t1\t0",
                "General 2\tSmall Sword\t4\t1\t0",
                "General 3\tShield\t11\t1\t0"
            ]
            .join("\r\n")
        );
        let corpus = Corpus::read(&dump_of(&text), &cat);
        let sc = scorer(&corpus, &cat);
        let env = Env {
            corpus: &corpus,
            cat: &cat,
            sc: &sc,
        };
        assert!(sc.dual_wield_cap() > 0);
        let sec = build_steps()
            .into_iter()
            .find(|s| s.key == "Secondary#1")
            .unwrap();
        let names: Vec<&str> = options_for(&env, &sec, &HashSet::new(), &HashSet::new(), 0.0)
            .iter()
            .map(|o| corpus.rows[o.items[0].row].name.as_str())
            .collect();
        assert!(
            !names.contains(&"Big Sword"),
            "a two-hander is never an off hand: {names:?}"
        );
        assert!(names.contains(&"Small Sword"));
        assert!(names.contains(&"Shield"));
        /* CLR/DRU/WIZ: no dual wield, so the damaging one-hander is out unless it is already there */
        let no_dw = vec!["CLR".to_owned(), "DRU".to_owned(), "WIZ".to_owned()];
        let text = format!(
            "{}{}",
            header(),
            [
                "Secondary\tSmall Sword\t4\t1\t10",
                "General 2\tSmall Sword\t4\t1\t0",
                "General 3\tShield\t11\t1\t0"
            ]
            .join("\r\n")
        );
        let corpus = Corpus::read(&dump_of(&text), &cat);
        let sc = Scorer::make(
            &no_dw,
            50,
            Some(&corpus.equipped),
            Some(&cat),
            &HashMap::new(),
        );
        assert_eq!(sc.dual_wield_cap(), 0);
        let env = Env {
            corpus: &corpus,
            cat: &cat,
            sc: &sc,
        };
        let opts = options_for(&env, &sec, &HashSet::new(), &HashSet::new(), 0.0);
        let locs: Vec<&str> = opts
            .iter()
            .map(|o| corpus.rows[o.items[0].row].loc.as_str())
            .collect();
        assert!(
            locs.contains(&"Secondary"),
            "the sword in the off hand stays a candidate: {locs:?}"
        );
        assert!(
            !locs.contains(&"General 2"),
            "the bagged one is gated: {locs:?}"
        );
        assert!(locs.contains(&"General 3"));
        assert!(!placeable(
            &cat.recs[cat.find("Big Sword").unwrap()],
            "Secondary",
            true
        ));
        assert!(placeable(
            &cat.recs[cat.find("Shield").unwrap()],
            "Secondary",
            false
        ));
    }

    #[test]
    fn breakdown_parts_add_up_to_the_score() {
        let cat = cat_of(&[
            r#"{"n":"Kit","t":"Kit","cls":{"all":1},"sl":["Primary"],"skill":"1H Slashing","dmg":12,"dly":20,"st":{"ac":40,"hp":25,"int":10,"cha":3},"sv":{"f":5,"v":10},"haste":20}"#,
        ]);
        let corpus = Corpus::default();
        let sc = Scorer::make(&trio(), 50, None, None, &HashMap::new());
        let rec = &cat.recs[0];
        for (base, slot) in [(0.0, "Primary"), (300.0, "Primary"), (400.0, "Head")] {
            let b = breakdown(rec, 3, base, slot, &sc);
            let s = sc.score(rec, 3, base, slot);
            assert!(
                (b.total - s).abs() < 1e-6,
                "{slot} at {base}: {} vs {s}",
                b.total
            );
            let sum: f64 = b.parts.iter().map(|p| p.v).sum();
            assert!((sum - s).abs() < 1e-6);
            assert!(
                b.parts.windows(2).all(|w| w[0].v >= w[1].v),
                "sorted by value"
            );
            assert!(
                !b.parts.iter().any(|p| p.k == "SVV"),
                "SV VOID is never scored"
            );
            let ratio = b.parts.iter().any(|p| p.k == "DMG/DLY");
            assert_eq!(ratio, slot == "Primary");
            let ac = b.parts.iter().find(|p| p.k == "AC").expect("AC line");
            assert_eq!(ac.cap, base + stat_at(40, 3) as f64 > sc.cap_v);
        }
        let _ = corpus;
        /* a stat with a zero weight is still listed, at zero */
        let b = breakdown(rec, 0, 0.0, "Head", &sc);
        assert!(b.parts.iter().any(|p| p.k == "CHA"));
    }

    /* ---- by slot ---- */

    #[test]
    fn slot_view_prices_against_the_kit_minus_the_weaker_occupant() {
        let cat = fixture_cat();
        let corpus = Corpus::read(&fixture_dump(), &cat);
        let sc = scorer(&corpus, &cat);
        let sv = slot_view(&corpus, &cat, &sc, "Head", &trio(), None, None);
        let helm = &cat.recs[cat.find("Plain Helm").unwrap()];
        assert!((sv.worn_ac - gear::worn_ac(&corpus.equipped, &cat)).abs() < 1e-9);
        assert!(
            (sv.base_ac - (sv.worn_ac - ac_of(helm, 2))).abs() < 1e-9,
            "the occupant's own AC comes out of the base"
        );
        assert_eq!(sv.vs, Some(0));
        /* owned: the worn helm (+2), the bagged Better Helm, not the banked Druid Helm, not the stone */
        let names: Vec<&str> = sv
            .owned
            .iter()
            .map(|c| corpus.rows[c.row].name.as_str())
            .collect();
        assert!(names.contains(&"Plain Helm +2"));
        assert!(names.contains(&"Better Helm"));
        assert!(!names.contains(&"Druid Helm"));
        assert!(!names.contains(&"Better Helm (Exaltation)"));
        assert_eq!(sv.owned[0].rank, 1);
        assert!(sv.owned.iter().find(|c| c.worn_here).is_some());
        assert_eq!(sv.par_tier, 2);
        /* the occupant's row score is the "on you now" score */
        let worn = sv.owned.iter().find(|c| c.worn_here).unwrap();
        assert!((worn.score - sv.now[0].score.unwrap()).abs() < 1e-9);
        /* a paired slot subtracts its weaker occupant */
        let ears = slot_view(&corpus, &cat, &sc, "Ear", &trio(), None, None);
        assert_eq!(ears.now.len(), 2);
        assert_eq!(ears.vs, Some(1), "the plain earring is the weaker one");
        /* ties share a rank */
        let text = format!(
            "{}{}",
            header(),
            [
                "General 1\tPlain Helm\t1\t1\t0",
                "General 2\tPlain Helm\t1\t1\t0",
                "General 3\tBetter Helm\t7\t1\t0"
            ]
            .join("\r\n")
        );
        let c2 = Corpus::read(&dump_of(&text), &cat);
        let sc2 = scorer(&c2, &cat);
        let sv2 = slot_view(&c2, &cat, &sc2, "Head", &trio(), None, None);
        let ranks: Vec<usize> = sv2.owned.iter().map(|c| c.rank).collect();
        assert_eq!(ranks, vec![1, 2, 2]);
        /* main + off together on the Primary view */
        let hands = slot_view(&corpus, &cat, &sc, "Primary", &trio(), None, None);
        let (m, o, total) = hands.hands_now.expect("hands line");
        assert!(m.is_some() && o.is_some());
        assert!((total - (m.unwrap() + o.unwrap())).abs() < 1e-9);
    }

    #[test]
    fn slot_gap_lists_only_what_you_could_wear_and_do_not_own() {
        let cat = fixture_cat();
        let corpus = Corpus::read(&fixture_dump(), &cat);
        let sc = scorer(&corpus, &cat);
        let owned = owned_keys(&corpus, &cat);
        let gap = slot_gap(&cat, &sc, "Head", &owned, 0.0, 2);
        let names: Vec<&str> = gap
            .items
            .iter()
            .map(|g| cat.recs[g.rec].name.as_str())
            .collect();
        assert!(!names.contains(&"Plain Helm"), "owned");
        assert!(!names.contains(&"Better Helm"), "owned, in a bag");
        assert!(!names.contains(&"Druid Helm"), "not your class");
        assert!(!names.contains(&"Old Helm"), "out of era");
        assert!(names.is_empty(), "{names:?}");
        let empty = Corpus::default();
        let sc0 = Scorer::make(&trio(), 50, None, None, &HashMap::new());
        let gap = slot_gap(&cat, &sc0, "Head", &HashSet::new(), 0.0, 3);
        let names: Vec<&str> = gap
            .items
            .iter()
            .map(|g| cat.recs[g.rec].name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["Better Helm", "Plain Helm"],
            "best at par first"
        );
        assert!(
            gap.items[0].s_par > gap.items[0].s0,
            "scored at +3 as well as +0"
        );
        let ammo = slot_gap(&cat, &sc0, "Ammo", &HashSet::new(), 0.0, 0);
        assert!(ammo.items.is_empty(), "nothing on an arrow can be priced");
        let _ = empty;
        /* variants: several pages under one name */
        let neck = slot_gap(&cat, &sc0, "Neck", &HashSet::new(), 0.0, 0);
        assert_eq!(neck.items.len(), 3);
        assert!(neck
            .items
            .iter()
            .all(|g| g.variants == 3 && g.variant.is_some()));
    }

    #[test]
    fn owned_keys_counts_identical_pages_under_one_name_and_never_a_stone() {
        let cat = fixture_cat();
        let corpus = Corpus::read(&fixture_dump(), &cat);
        let owned = owned_keys(&corpus, &cat);
        let a = cat
            .recs
            .iter()
            .position(|r| r.key == "Twin_Gorget_A")
            .unwrap();
        let b = cat
            .recs
            .iter()
            .position(|r| r.key == "Twin_Gorget_B")
            .unwrap();
        let c = cat
            .recs
            .iter()
            .position(|r| r.key == "Twin_Gorget_C")
            .unwrap();
        assert!(owned.contains(&a));
        assert!(owned.contains(&b), "same stats, same item, another page");
        assert!(!owned.contains(&c), "different stats, a different item");
        let better = cat.find("Better Helm").unwrap();
        assert!(owned.contains(&better), "in a bag");
        let druid = cat.find("Druid Helm").unwrap();
        assert!(owned.contains(&druid), "owning is not wearing");
        /* a dump holding only the stone does not own the helm */
        let text = format!(
            "{}{}",
            header(),
            ["Augmentation\tBetter Helm (Exaltation)\t7\t1\t0"].join("\r\n")
        );
        let c2 = Corpus::read(&dump_of(&text), &cat);
        assert!(owned_keys(&c2, &cat).is_empty());
    }

    #[test]
    fn catalog_rank_takes_the_best_percentile_not_the_lowest_number() {
        let cat = cat_of(&[
            r#"{"n":"Ring A","t":"A","cls":{"c":["WAR","CLR"]},"sl":["Fingers"],"st":{"hp":50}}"#,
            r#"{"n":"Ring B","t":"B","cls":{"c":["WAR"]},"sl":["Fingers"],"st":{"hp":40}}"#,
            r#"{"n":"Ring C","t":"C","cls":{"c":["WAR"]},"sl":["Fingers"],"st":{"hp":30}}"#,
            r#"{"n":"Ring D","t":"D","cls":{"c":["WAR"]},"sl":["Fingers"],"st":{"hp":20}}"#,
            r#"{"n":"Ring E","t":"E","cls":{"c":["CLR"]},"sl":["Fingers"],"st":{"hp":60}}"#,
        ]);
        let mut rk = Ranker::new(50);
        let a = cat.find("Ring A").unwrap();
        /* WAR: 1 of 4 (0.25). CLR: 2 of 2 (1.0). Best percentile is WAR. */
        let r = catalog_rank(&mut rk, &cat, a, "Fingers", 0, &trio()).expect("ranked");
        assert_eq!((r.cls, r.rank, r.of), ("WAR", 1, 4));
        let c = cat.find("Ring C").unwrap();
        let r = catalog_rank(&mut rk, &cat, c, "Fingers", 0, &trio()).unwrap();
        assert_eq!((r.rank, r.of), (3, 4));
        /* Any Slot ranks in the item's own slots */
        let r = catalog_rank(&mut rk, &cat, a, ANY, 0, &trio()).unwrap();
        assert_eq!(r.slot, "Fingers");
        assert!(catalog_rank(&mut rk, &cat, a, "Head", 0, &trio()).is_none());
        let druids = vec!["DRU".to_owned()];
        assert!(catalog_rank(&mut rk, &cat, a, "Fingers", 0, &druids).is_none());
    }

    /* ---- the exalt audit ---- */

    #[test]
    fn exalt_audit_flags_empty_sockets_and_idle_stones_with_homes() {
        let items: Vec<crate::data::Item> = [
            r#"{"n":"Plain Helm","t":"Plain_Helm","cls":{"all":1},"sl":["Head"],"st":{"ac":10},"foc":"Helm Focus"}"#,
            r#"{"n":"Glow Ring","t":"Glow_Ring","cls":{"all":1},"sl":["Fingers"],"eff":[{"n":"Glow","m":"worn"}]}"#,
            r#"{"n":"Druid Ring","t":"Druid_Ring","cls":{"c":["DRU"]},"sl":["Fingers"],"eff":[{"n":"Bark","m":"worn"}]}"#,
            r#"{"n":"Boot Click","t":"Boot_Click","cls":{"all":1},"sl":["Feet"],"eff":[{"n":"Run","m":"clicky"}]}"#,
            r#"{"n":"Dual","t":"Dual","cls":{"all":1},"sl":["Head"],"foc":"Focus A","eff":[{"n":"Click B","m":"clicky"}]}"#,
        ]
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
        let xcat = ExaltCat::build(&items);
        let text = format!(
            "{}{}",
            header(),
            [
                "Head\tPlain Helm +3\t1\t1\t10",
                "Head-Slot7\tEmpty\t0\t0\t0",
                "Head-Slot8\tEmpty\t0\t0\t0",
                "Head-Slot9\tEmpty\t0\t0\t0",
                "Fingers\tGlow Ring +3\t2\t1\t10",
                "Fingers-Slot7\tEmpty\t0\t0\t0",
                "Fingers-Slot8\tEmpty\t0\t0\t0",
                "Fingers-Slot9\tGlow Ring (Exaltation)\t2\t1\t10",
                "Fingers\tGlow Ring +1\t2\t1\t10",
                "Fingers-Slot7\tEmpty\t0\t0\t0",
                "Augmentation\tGlow Ring (Exaltation)\t2\t1\t0",
                "Augmentation\tDruid Ring (Exaltation)\t3\t1\t0",
                "Augmentation\tBoot Click (Exaltation)\t4\t1\t0",
                "Augmentation\tDual (Exaltation)\t5\t1\t0",
                "Augmentation\tNo Page (Exaltation)\t6\t1\t0",
                /* a helm inside a bank bag, with its own focus socketed: "Bank1-Slot3" is bank
                 * bag 1's third position, and "-Slot7" hanging off THAT is the socket */
                "Bank1\tBig Bag\t99\t1\t10",
                "Bank1-Slot3\tPlain Helm +2\t1\t1\t0",
                "Bank1-Slot3-Slot7\tPlain Helm (Exaltation)\t1\t1\t0",
            ]
            .join("\r\n")
        );
        let dump = dump_of(&text);
        let xd = exalt::read_dump(&dump.all, &items, &xcat);
        let audit = exalt_audit(&xd, &dump.all, &xcat, &trio());
        /* empties: helm 7,8,9; ring1 7,8; ring2 7 */
        assert_eq!(audit.summary.empty, 6);
        assert_eq!(audit.summary.worn_with_empty, 3);
        /* idle: five loose plus the helm focus socketed in the banked helm */
        assert_eq!(audit.summary.idle, 6);
        let by_name = |n: &str| {
            audit
                .idle
                .iter()
                .find(|it| it.name.starts_with(n))
                .unwrap_or_else(|| panic!("{n}"))
        };
        /* the loose glow ring: its worn effect fits ring 1's occupied socket only as a same-effect
         * non-swap, and ring 2 at +1 later (worn opens at +3) */
        let glow = by_name("Glow Ring");
        assert!(glow.empty.is_empty());
        assert!(
            glow.occupied.is_empty(),
            "replacing Glow with Glow is not a swap"
        );
        assert_eq!(glow.later.len(), 1);
        assert_eq!(glow.later[0].needs, Some(3));
        /* the druid ring: the any-class worn rings take it (the result would be druid-only),
         * and none of WAR/CLR/WIZ could wear that result */
        let druid = by_name("Druid Ring");
        assert_eq!(
            druid.why,
            Some("none of your classes could wear the result")
        );
        assert!(druid.empty.is_empty() && druid.occupied.is_empty() && druid.later.is_empty());
        /* the boot click: nothing worn on the feet */
        let boot = by_name("Boot Click");
        assert_eq!(boot.why, Some("no worn item shares a slot with it"));
        /* the dual stone: two effects it could be, and EACH is tried: the focus fits the helm's
         * empty focus socket and the click its empty click socket, so it has two homes now */
        let dual = by_name("Dual");
        assert!(dual.maybe);
        assert_eq!(dual.empty.len(), 2);
        let kinds: Vec<Kind> = dual.empty.iter().map(|h| h.kind).collect();
        assert!(kinds.contains(&Kind::Focus) && kinds.contains(&Kind::Click));
        assert!(dual.empty.iter().all(|h| h.w == 0));
        /* no page */
        let np = by_name("No Page");
        assert!(np.unknown);
        assert_eq!(np.why, Some("no wiki page for its source"));
        /* the socketed one in the bank is idle, with its host named, and fits the helm's focus */
        let banked = audit
            .idle
            .iter()
            .find(|it| it.host.is_some())
            .expect("a socketed idle stone");
        assert_eq!(banked.kind, Some(Kind::Focus));
        assert_eq!(banked.empty.len(), 1);
        /* the helm's focus socket lists both candidates */
        let helm_focus = audit
            .empty
            .iter()
            .find(|e| e.w == 0 && e.kind == Kind::Focus)
            .unwrap();
        assert_eq!(helm_focus.fits.len(), 2);
        let helm_click = audit
            .empty
            .iter()
            .find(|e| e.w == 0 && e.kind == Kind::Click)
            .unwrap();
        assert_eq!(
            helm_click.fits.len(),
            1,
            "only the dual stone could be a click"
        );
        assert_eq!(
            audit.summary.fillable, 2,
            "the helm's focus and click sockets"
        );
        assert_eq!(
            audit.summary.usable, 2,
            "the dual stone and the banked focus"
        );
        assert_eq!(audit.summary.later, 1);
        /* swaps: idle stones with no empty home and at least one occupied one; every idle stone
         * in this fixture that is neither usable nor later is one, and the sum is the whole */
        let s = &audit.summary;
        assert_eq!(
            s.swaps,
            audit
                .idle
                .iter()
                .filter(|it| it.empty.is_empty() && !it.occupied.is_empty())
                .count()
        );
        assert!(
            s.usable + s.swaps + s.later <= s.idle,
            "the three classes are disjoint"
        );
        /* the ones you could be using come first */
        assert!(!audit.idle[0].empty.is_empty());
        assert!(audit.idle.last().unwrap().why.is_some());
    }

    /* ---- the memory store ---- */

    #[test]
    fn memory_round_trips_through_settings_per_trio() {
        let mut settings = crate::settings::Settings::default();
        let mut mem = Memory::default();
        mem.set(
            "Head#1",
            MemRec {
                pick: "a+0".into(),
                set: vec!["a+0".into(), "b+1".into()],
                floor: 12.5,
            },
        );
        let key = trio_key(&trio());
        assert_eq!(
            key, "CLR/WAR/WIZ",
            "sorted, so the order of the picker does not make a new store"
        );
        put_mem(&mut settings, &key, &mem).expect("the struct round trip");
        let back = load_mem(&settings, &key);
        assert_eq!(back.map.get("Head#1"), mem.map.get("Head#1"));
        assert!(!back.dirty);
        assert!(load_mem(&settings, "DRU/RNG/SHM").map.is_empty());

        /* The disk round trip goes through a SCRATCH path. This test once called save_mem, which
         * wrote this fixture over the operator's real settings.json; Settings::save now refuses
         * under test, and this asserts that it does, so the incident cannot come back. */
        let e = save_mem(&mut settings, &key, &mem).unwrap_err();
        assert!(e.contains("refused"), "{e}");
        let p = std::env::temp_dir()
            .join(format!("grimoire-valet-mem-{}", std::process::id()))
            .join("settings.json");
        settings.save_to(&p).expect("scratch write");
        let on_disk = crate::settings::Settings::load_from(&p).expect("scratch read");
        assert_eq!(
            load_mem(&on_disk, &key).map.get("Head#1"),
            mem.map.get("Head#1")
        );
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /* ---- the real file ---- */

    #[test]
    fn the_real_catalogue_has_a_gap_for_every_body_slot() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        let cat = Catalogue::from_snapshot(s).expect("catalogue from the real snapshot");
        let sc = Scorer::make(&trio(), 50, None, None, &HashMap::new());
        for slot in ["Head", "Chest", "Primary", "Secondary", "Fingers", ANY] {
            let gap = slot_gap(&cat, &sc, slot, &HashSet::new(), 0.0, 0);
            assert!(gap.items.len() > 20, "{slot}: {} items", gap.items.len());
            assert!(
                gap.items.iter().all(|g| !cat.recs[g.rec].oe),
                "{slot}: out of era never appears"
            );
            assert!(
                gap.items.windows(2).all(|w| w[0].s_par >= w[1].s_par),
                "{slot}: sorted"
            );
        }
        let sec = slot_gap(&cat, &sc, "Secondary", &HashSet::new(), 0.0, 0);
        assert!(
            sec.items.iter().all(|g| !g.two_h),
            "no two-hander in the off hand"
        );
        let walk_cat = &cat;
        let corpus = Corpus::default();
        let env = Env {
            corpus: &corpus,
            cat: walk_cat,
            sc: &sc,
        };
        let w = Walk::new(&env, Memory::default());
        assert!(w.done(), "an empty corpus has nothing to ask");
        assert!(w
            .steps
            .iter()
            .all(|st| w.auto.get(&st.key) == Some(&Auto::NoneFit)));
    }
}
