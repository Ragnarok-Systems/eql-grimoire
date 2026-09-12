//! FIND: Zones. The kills-data.json zone list merged with the atlas-wiki page for the same zone,
//! the items dropped there from three files, and the wiki link. Decisions D5 and D6.
//!
//! WHAT THE RECORD HOLDS (the data lane's `Zone`, joined on the short key at load):
//!   mobs        kills-data's roster: {n, lv, lvl, named, t}         3843 across 76 zones
//!   atlas_mobs  the wiki page's roster: {n, lvl, named, u, loc, drops, dr}
//!   atlas_items the wiki page's item list: {n, u, g}
//!   wiki_title, wiki_url, updated, atlas_file, city
//! `drops[j]` indexes `atlas_items` and `dr[j]` is that drop's rarity: PARALLEL ARRAYS. That is
//! the rule the atlas page's parallel arrays follow, and it carries a test here.
//! `g` on an item is present on 7389 of 12871 entries and nothing measured says what it means, so
//! it is shown raw and unlabelled. `loc` is positional ([0] and [1] the two /loc coordinates in
//! the wiki's order, [2] an optional third, axes unverified) and is shown as numbers, unlabelled.
//!
//! ZONE NAMES DISAGREE ACROSS FILES ("The Plane of Sky" in kills-data, "Plane of Sky" on the
//! atlas page, "The Plane of Mischief" in quest-items drops). `zone_key` folds case, drops a
//! leading "the " and keeps alphanumerics, and two names merge ONLY when their keys are equal.
//! Nothing fuzzier: D6 forbids it for items and the same wrong-hit argument applies here.

use crate::data::zones::{AtlasItem, AtlasMob, ZoneMob};
use crate::data::Zone;
use crate::screens::items::{
    count_line, detail_rows, dim, head_row, kv_table, link, list_named, list_row, mono, mono_hover,
    no_data, num_text, open_url, pane_section, parse_query, provenance, render_value, search_box,
    snapshot_key, sources_of, view, wrong_bar, Col, DetailRows, SnapKey, SourceMob, View, ROW_H,
};
use crate::theme::*;
use egui::{FontId, Ui};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/* ------------------------------------------------------------- cross-screen jumps -- */

/// Drops and Items ask this screen to select a zone by name (any of the three files' spellings).
/// The nav switch itself travels on `Cx.ask` (`Ask::ShowZone`), which the caller sets in the same
/// click; this static carries the selection the App has no field for. Without the App acting on
/// `ask`, the request still lands the next time Zones is opened.
static JUMP: Mutex<Option<String>> = Mutex::new(None);

pub fn jump_to(zone: &str) {
    *JUMP.lock().unwrap_or_else(|p| p.into_inner()) = Some(zone.to_owned());
}

fn take_jump() -> Option<String> {
    JUMP.lock().unwrap_or_else(|p| p.into_inner()).take()
}

/* ------------------------------------------------------------------- pure rules -- */

/// The cross-file zone identity. See the module comment.
pub(crate) fn zone_key(s: &str) -> String {
    let lower = s.trim().to_lowercase();
    let body = lower.strip_prefix("the ").unwrap_or(&lower);
    body.chars().filter(char::is_ascii_alphanumeric).collect()
}

/// Every key a zone answers to: its tracker name, its short key and its wiki title.
pub(crate) fn zone_keys(z: &Zone) -> Vec<String> {
    /* First occurrence wins and order is kept. Not Vec::dedup: that removes CONSECUTIVE repeats
     * only, and name and wiki title (which usually agree) sit either side of the key. */
    let mut out: Vec<String> = Vec::new();
    let all = [
        Some(z.name.as_str()),
        Some(z.key.as_str()),
        z.wiki_title.as_deref(),
    ];
    for k in all.into_iter().flatten().map(zone_key) {
        if !k.is_empty() && !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

/// A tracker mob's level as text: the wiki's range string when there is one, else the single
/// number kills-data derived from it (12.8 for "7-20 / 11-13"), else nothing.
pub(crate) fn level_of(m: &ZoneMob) -> String {
    match (&m.lvl, m.lv) {
        (Some(l), _) if !l.is_empty() => l.clone(),
        (_, Some(n)) => num_text(n),
        _ => String::new(),
    }
}

/// `loc` as text: the non-null entries as the file has them, comma joined. None when the wiki
/// gave no location or gave only nulls.
pub(crate) fn loc_text(loc: Option<&[Value]>) -> Option<String> {
    let parts: Vec<String> = loc?
        .iter()
        .filter(|v| !v.is_null())
        .map(render_value)
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// One mob in the merged roster.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MergedMob {
    pub name: String,
    /// kills-data's level first, the atlas page's when kills-data has none, both when they
    /// disagree: "59-61 (atlas 58-62)".
    pub level: String,
    pub named: bool,
    pub in_kills: bool,
    pub in_atlas: bool,
    /// (item name, rarity) pairs, `drops[j]` with `dr[j]`. An index past the item list is skipped.
    pub drops: Vec<(String, Option<String>)>,
    pub url: Option<String>,
    pub loc: Option<String>,
    /// kills-data's wiki slug (`t`), the only handle a mob has when no atlas page carries it.
    pub slug: Option<String>,
}

impl MergedMob {
    fn new(name: &str) -> Self {
        MergedMob {
            name: name.to_owned(),
            level: String::new(),
            named: false,
            in_kills: false,
            in_atlas: false,
            drops: Vec::new(),
            url: None,
            loc: None,
            slug: None,
        }
    }
}

/// Union of the two rosters by case folded name, sorted by name. PURE. A mob listed twice on the
/// atlas page contributes both drop lists rather than the last one only.
pub(crate) fn merge_roster(
    kills: &[ZoneMob],
    atlas: &[AtlasMob],
    items: &[AtlasItem],
) -> Vec<MergedMob> {
    let mut by: HashMap<String, MergedMob> = HashMap::new();
    for k in kills {
        let m = by
            .entry(k.name.to_lowercase())
            .or_insert_with(|| MergedMob::new(&k.name));
        m.in_kills = true;
        let level = level_of(k);
        if !level.is_empty() {
            m.level = level;
        }
        m.named = m.named || k.named;
        if !k.t.is_empty() {
            m.slug = Some(k.t.clone());
        }
    }
    for a in atlas {
        let m = by
            .entry(a.name.to_lowercase())
            .or_insert_with(|| MergedMob::new(&a.name));
        m.in_atlas = true;
        if let Some(l) = a.lvl.as_deref().filter(|l| !l.is_empty()) {
            if m.level.is_empty() {
                m.level = l.to_owned();
            } else if l != m.level {
                m.level = format!("{} (atlas {l})", m.level);
            }
        }
        m.named = m.named || a.named;
        if !a.u.is_empty() {
            m.url = Some(a.u.clone());
        }
        if let Some(l) = loc_text(a.loc.as_deref()) {
            m.loc = Some(l);
        }
        /* The data module resolves the parallel arrays (`drops[j]` indexes items, `dr[j]` is that
         * drop's rarity) in one place, `AtlasMob::drop_list`; an index past the end is skipped
         * there rather than here. */
        m.drops.extend(
            a.drop_list(items)
                .into_iter()
                .map(|(it, rarity)| (it.name.clone(), rarity.map(str::to_owned))),
        );
    }
    let mut out: Vec<MergedMob> = by.into_values().collect();
    out.sort_by_key(|m| m.name.to_lowercase());
    out
}

/// An atlas page item with the mobs that drop it: the roster's `drops` indexes, inverted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PageItem {
    pub name: String,
    pub url: Option<String>,
    pub g: Option<String>,
    pub droppers: Vec<String>,
}

pub(crate) fn page_items(atlas: &[AtlasMob], items: &[AtlasItem]) -> Vec<PageItem> {
    let mut out: Vec<PageItem> = items
        .iter()
        .map(|it| PageItem {
            name: it.name.clone(),
            url: Some(it.u.clone()).filter(|u| !u.is_empty()),
            g: it.g.map(num_text),
            droppers: Vec::new(),
        })
        .collect();
    for m in atlas {
        for &idx in &m.drops {
            if let Some(it) = out.get_mut(idx) {
                it.droppers.push(m.name.clone());
            }
        }
    }
    out.sort_by_key(|it| it.name.to_lowercase());
    out
}

/* -------------------------------------------------------------------- the screen -- */

#[derive(Clone, Debug)]
struct ZoneFacts {
    lname: String,
    keys: Vec<String>,
    tracked: usize,
    wiki_mobs: usize,
    has_page: bool,
    city: Option<bool>,
}

/// A gear-data item that lists this zone in `src.d`.
#[derive(Clone, Debug)]
struct GearSource {
    item: String,
    mobs: Vec<SourceMob>,
}

struct Cache {
    key: SnapKey,
    facts: Vec<ZoneFacts>,
    /// zone_key -> gear-data items dropped there.
    gear: HashMap<String, Vec<GearSource>>,
    /// zone_key -> (item, mob) from quest-items drops.
    quest: HashMap<String, Vec<(String, String)>>,
}

struct Hits {
    needle: String,
    idx: Vec<usize>,
}

/// One merchant standing in this zone, as merchants.json has them. Every line is kept as the file
/// wrote it AND as the split reads it, so the pane can print a tidy two column stock list and the
/// hover can still show the source line.
struct ZoneMerchant {
    /// The merchants.json map key, which is the identity: 32 merchants share the name "Clockwork
    /// Merchant" and only the key tells them apart, so the expand set is keyed on this.
    key: String,
    who: String,
    /// The wiki page title with underscores ("A_Brownie_Merchant"), so a reader who wants the page
    /// has the exact title to ask the wiki for. There is no Merchants screen to link to.
    t: String,
    loc: String,
    lvl: String,
    race: String,
    cls: String,
    /// Every key `Merchant::extra` picked up: fields the file gained after this build measured it.
    /// Empty on the shipped file, and drawn when it is not, because the crate's promise is that
    /// nothing the file carries is hidden and merchants have no detail pane of their own to hold
    /// the usual MORE table.
    unknown: Vec<String>,
    /// (item, price, raw). Price is None where the wiki lists none this build reads.
    sells: Vec<(String, Option<String>, String)>,
    /// (faction, delta, the page name when this snapshot has one, raw).
    fac: Vec<(String, Option<String>, Option<String>, String)>,
    ofac: Vec<(String, Option<String>, Option<String>, String)>,
    quests: Vec<String>,
    loot: Vec<String>,
}

/// One faction this zone moves, and which way.
struct ZoneFaction {
    key: String,
    name: String,
    /// The wiki page title, as on [`ZoneMerchant`] and for the same reason.
    t: String,
    /// "raises" or "lowers", the word `factions::Way` prints.
    way: &'static str,
    desc: Option<String>,
    /// Keys of `Faction::extra`, as on [`ZoneMerchant::unknown`].
    unknown: Vec<String>,
    /// The mob list for THIS direction, verbatim. Shown on expand; see the pane for why the
    /// entries are not filtered down to this zone.
    mobs: Vec<String>,
    quests: Vec<String>,
}

struct Detail {
    idx: usize,
    rows: DetailRows,
    roster: Vec<MergedMob>,
    items: Vec<PageItem>,
    gear: Vec<GearSource>,
    quest: Vec<(String, String)>,
    merchants: Vec<ZoneMerchant>,
    factions: Vec<ZoneFaction>,
    err: Option<String>,
}

#[derive(Default)]
pub struct ZonesScreen {
    query: String,
    sel: Option<usize>,
    cache: Option<Cache>,
    hits: Option<Hits>,
    detail: Option<Detail>,
    /// Roster rows whose atlas drops are unfolded, by case folded mob name.
    expanded: HashSet<String>,
    notice: Option<String>,
}

/// The `Zone` type's own field names, as it serialises them.
const PREFERRED: &[&str] = &[
    "key",
    "tracked",
    "city",
    "wiki_title",
    "wiki_url",
    "updated",
    "atlas_file",
    "mobs",
    "atlas_mobs",
    "atlas_items",
];
/// The arrays drawn as tables in the pane; the typed table shows their count and says so.
const EXPANDED: &[&str] = &["mobs", "atlas_mobs", "atlas_items"];

impl ZonesScreen {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut crate::screens::Cx<'_>) {
        let Some(data) = cx.data else {
            no_data(ui, cx.data_err, cx.settings.data_root.as_deref(), "zone");
            return;
        };
        self.refresh(data);
        if let Some(name) = take_jump() {
            /* The snapshot's own index first (key, tracker name or wiki title, case folded), then
             * this screen's looser key (which drops a leading "The" and punctuation) for a name
             * another file spelled its own way. */
            let want = zone_key(&name);
            let by_index = data
                .zone(&name)
                .and_then(|z| data.zones.iter().position(|x| std::ptr::eq(x, z)));
            match by_index.or_else(|| self.cache.as_ref().and_then(|c| c.facts.iter().position(|f| f.keys.contains(&want)))) {
                Some(i) => {
                    self.sel = Some(i);
                    self.query.clear();
                    self.notice = None;
                }
                None => {
                    self.notice = Some(format!(
                        "No zone matching \"{name}\" in the zone list ({} zones from kills-data.json and atlas-wiki/). That name came from another file; the lists are merged by name and this one has no match.",
                        data.zones.len()
                    ))
                }
            }
        }
        let ask = egui::Panel::right("zones_detail")
            .default_size(480.0)
            .resizable(true)
            .frame(
                egui::Frame::NONE
                    .fill(PANEL)
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(ui, |ui| self.detail_pane(ui, data))
            .inner;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(INK))
            .show(ui, |ui| self.list_pane(ui, data));
        /* Routing goes through Cx.ask, the integrator's channel. The target screen's jump_to
         * already holds the selection, so the App only has to switch the nav. */
        if let Some(a) = ask {
            cx.ask = a;
        }
    }

    fn refresh(&mut self, data: &crate::data::Snapshot) {
        let key = snapshot_key(data);
        if self.cache.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let facts: Vec<ZoneFacts> = data
            .zones
            .iter()
            .map(|z| ZoneFacts {
                lname: z.name.to_lowercase(),
                keys: zone_keys(z),
                tracked: z.mobs.len(),
                wiki_mobs: z.atlas_mobs.len(),
                has_page: z.has_wiki_page(),
                city: z.city,
            })
            .collect();
        /* The gear index: every gear-data item's src.d, keyed by zone. One pass, once. */
        let mut gear: HashMap<String, Vec<GearSource>> = HashMap::new();
        for it in &data.items {
            for sz in sources_of(it) {
                gear.entry(zone_key(&sz.zone))
                    .or_default()
                    .push(GearSource {
                        item: it.name.clone(),
                        mobs: sz.mobs,
                    });
            }
        }
        let mut quest: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for d in &data.drops {
            for s in &d.sources {
                if !s.zone.is_empty() {
                    quest
                        .entry(zone_key(&s.zone))
                        .or_default()
                        .push((d.name.clone(), s.mob.clone()));
                }
            }
        }
        self.cache = Some(Cache {
            key,
            facts,
            gear,
            quest,
        });
        self.hits = None;
        self.detail = None;
        self.sel = None;
        self.expanded.clear();
    }

    fn hits(&mut self, needle: &str) -> &[usize] {
        if self.hits.as_ref().map_or(true, |h| h.needle != needle) {
            let idx = match &self.cache {
                Some(c) => c
                    .facts
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| needle.is_empty() || f.lname.contains(needle))
                    .map(|(i, _)| i)
                    .collect(),
                None => Vec::new(),
            };
            self.hits = Some(Hits {
                needle: needle.to_owned(),
                idx,
            });
        }
        self.hits.as_ref().map(|h| h.idx.as_slice()).unwrap_or(&[])
    }

    fn list_pane(&mut self, ui: &mut Ui, data: &crate::data::Snapshot) {
        let total = data.zones.len();
        let q = parse_query(&self.query, 'z');
        let report = data.report();
        ui.horizontal(|ui| {
            search_box(ui, &mut self.query, "zone name, substring");
            let shown = self.hits(&q.needle).len();
            count_line(ui, shown, total, "zones");
        });
        if let Some(p) = q.foreign_prefix {
            dim(
                ui,
                &format!(
                    "The prefix {p}: names the {} list. Searching zones for the rest.",
                    list_named(p)
                ),
            );
        }
        provenance(
            ui,
            &report.root,
            &format!(
                "{} with {}/",
                crate::data::KILLS_FILE,
                crate::data::ATLAS_DIR
            ),
            report.zones,
            "zones",
        );
        if let Some(n) = self.notice.clone() {
            wrong_bar(ui, &n);
        }
        ui.add_space(6.0);
        let hits: Vec<usize> = self.hits(&q.needle).to_vec();
        if hits.is_empty() {
            if total == 0 {
                dim(ui, "The zone list loaded with zero zones. kills-data.json is present but its zones map is empty and atlas-wiki/ holds no pages.");
            } else {
                dim(ui, "No zone name contains that.");
            }
            return;
        }
        head_row(
            ui,
            &[
                ("zone", false, 0.0),
                ("city", true, 40.0),
                ("tracked", true, 60.0),
                ("wiki mobs", true, 70.0),
            ],
        );
        let mut clicked: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("zones_list")
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, hits.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in range {
                    let i = hits[row];
                    let Some(c) = &self.cache else { break };
                    let f = &c.facts[i];
                    let name = &data.zones[i].name;
                    let tracked = if f.city.is_some() {
                        f.tracked.to_string()
                    } else {
                        String::new()
                    };
                    let wiki = if f.has_page {
                        f.wiki_mobs.to_string()
                    } else {
                        String::new()
                    };
                    let sel = self.sel == Some(i);
                    let cols = [
                        Col {
                            text: name,
                            mono: false,
                            color: if sel { FLARE } else { TEXT },
                            right: false,
                            width: 0.0,
                        },
                        Col {
                            text: if f.city == Some(true) { "city" } else { "" },
                            mono: true,
                            color: TEXT_3,
                            right: true,
                            width: 40.0,
                        },
                        Col {
                            text: &tracked,
                            mono: true,
                            color: TEXT_2,
                            right: true,
                            width: 60.0,
                        },
                        Col {
                            text: &wiki,
                            mono: true,
                            color: TEXT_2,
                            right: true,
                            width: 70.0,
                        },
                    ];
                    if list_row(ui, sel, &cols).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        if let Some(i) = clicked {
            self.sel = Some(i);
            self.notice = None;
            self.expanded.clear();
        }
    }

    fn detail_pane(
        &mut self,
        ui: &mut Ui,
        data: &crate::data::Snapshot,
    ) -> Option<crate::screens::Ask> {
        let Some(i) = self.sel else {
            dim(
                ui,
                "Select a zone for its mob roster, what drops there, and the wiki page.",
            );
            return None;
        };
        if self.detail.as_ref().map_or(true, |d| d.idx != i) {
            self.detail = Some(self.build_detail(i, data));
        }
        let d = self.detail.as_ref()?;
        let z = &data.zones[i];
        let mut open_err: Option<String> = None;
        let mut toggle: Option<String> = None;
        let mut ask: Option<crate::screens::Ask> = None;
        egui::ScrollArea::vertical().id_salt("zones_detail_scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.label(egui::RichText::new(&z.name).font(crate::fonts::display(18.0)).color(GOLD_HI));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                match (&z.wiki_url, z.has_wiki_page()) {
                    (Some(u), _) => {
                        if link(ui, "wiki", FontId::proportional(12.5)).on_hover_text(u.as_str()).clicked() {
                            if let Err(e) = open_url(u) {
                                open_err = Some(e);
                            }
                        }
                    }
                    (None, true) => dim(ui, &format!("atlas page {} has no wikiUrl", z.atlas_file.as_deref().unwrap_or("?"))),
                    (None, false) => dim(ui, "no atlas page for this zone in the snapshot, so no wiki link"),
                }
                if let Some(t) = &z.wiki_title {
                    if t != &z.name {
                        mono(ui, &format!("wiki title: {t}"), TEXT_3);
                    }
                }
                if let Some(u) = &z.updated {
                    mono(ui, &format!("atlas updated {u}"), TEXT_3);
                }
            });
            ui.add_space(6.0);
            if let Some(e) = &d.err {
                wrong_bar(ui, e);
            }
            kv_table(ui, "zones_typed", &d.rows.typed);

            /* The roster. Number columns right aligned, mono. Click a row to unfold its atlas
             * drops; the count column says whether there is anything to unfold. */
            pane_section(ui, "MOBS");
            if d.roster.is_empty() {
                dim(ui, "No mob roster: kills-data.json lists no mobs for this zone and the atlas page has no roster.");
            } else {
                mono(ui, &format!("{} mobs: {} from kills-data.json, {} from the atlas page", d.roster.len(), z.mobs.len(), z.atlas_mobs.len()), TEXT_3);
                head_row(ui, &[("mob", false, 0.0), ("level", true, 110.0), ("named", true, 46.0), ("drops", true, 40.0), ("in", true, 24.0)]);
                for m in &d.roster {
                    let key = m.name.to_lowercase();
                    let open = self.expanded.contains(&key);
                    let drops = if m.drops.is_empty() { String::new() } else { m.drops.len().to_string() };
                    let src = match (m.in_kills, m.in_atlas) {
                        (true, true) => "KA",
                        (true, false) => "K",
                        (false, true) => "A",
                        (false, false) => "",
                    };
                    let cols = [
                        Col { text: &m.name, mono: false, color: if open { FLARE } else { TEXT }, right: false, width: 0.0 },
                        Col { text: &m.level, mono: true, color: TEXT_2, right: true, width: 110.0 },
                        Col { text: if m.named { "named" } else { "" }, mono: true, color: TEXT_2, right: true, width: 46.0 },
                        Col { text: &drops, mono: true, color: TEXT_2, right: true, width: 40.0 },
                        Col { text: src, mono: true, color: TEXT_3, right: true, width: 24.0 },
                    ];
                    let r = list_row(ui, open, &cols);
                    let r = match (&m.url, &m.slug) {
                        (Some(u), _) => r.on_hover_text(u.as_str()),
                        (None, Some(t)) => r.on_hover_text(format!("kills-data slug {t}")),
                        (None, None) => r,
                    };
                    if r.clicked() {
                        toggle = Some(key.clone());
                    }
                    if open {
                        if let Some(l) = &m.loc {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                mono(ui, &format!("loc {l}"), TEXT_3);
                            });
                        }
                        if m.drops.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                dim(ui, if m.in_atlas { "the atlas page lists no drops for this mob" } else { "no atlas entry for this mob, so no drop list" });
                            });
                        }
                        for (item, rarity) in &m.drops {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                if link(ui, item, FontId::monospace(11.5)).on_hover_text("open in Items").clicked() {
                                    crate::screens::items::jump_to(item);
                                    ask = Some(crate::screens::Ask::ShowItem(item.clone()));
                                }
                                if let Some(r) = rarity {
                                    mono(ui, r, TEXT_3);
                                }
                            });
                        }
                    }
                }
                dim(ui, "in: K = kills-data.json roster, A = atlas page roster. Level is kills-data's, with the atlas value when they differ.");
            }

            pane_section(ui, "ITEMS DROPPED HERE");
            mono(ui, &format!("from the atlas page: {}", d.items.len()), TEXT_3);
            if d.items.is_empty() {
                dim(ui, if z.has_wiki_page() { "the atlas page has an empty item list" } else { "no atlas page for this zone in the snapshot" });
            }
            for it in &d.items {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    let r = link(ui, &it.name, FontId::proportional(12.5)).on_hover_text(match &it.url {
                        Some(u) => format!("open in Items. Wiki: {u}"),
                        None => "open in Items".to_owned(),
                    });
                    if r.clicked() {
                        crate::screens::items::jump_to(&it.name);
                        ask = Some(crate::screens::Ask::ShowItem(it.name.clone()));
                    }
                    if let Some(g) = &it.g {
                        mono(ui, &format!("g {g}"), TEXT_3);
                    }
                    if !it.droppers.is_empty() {
                        let shown: Vec<&str> = it.droppers.iter().take(4).map(String::as_str).collect();
                        let more = it.droppers.len().saturating_sub(4);
                        let s = if more > 0 { format!("{} +{more}", shown.join(", ")) } else { shown.join(", ") };
                        mono(ui, &s, TEXT_3);
                    }
                });
            }
            ui.add_space(6.0);
            mono(ui, &format!("from gear-data.json src.d: {}", d.gear.len()), TEXT_3);
            if d.gear.is_empty() {
                dim(ui, &format!("no gear-data item names this zone in src.d (matched on \"{}\")", zone_key(&z.name)));
            }
            for g in &d.gear {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    if link(ui, &g.item, FontId::proportional(12.5)).on_hover_text("open in Items").clicked() {
                        crate::screens::items::jump_to(&g.item);
                        ask = Some(crate::screens::Ask::ShowItem(g.item.clone()));
                    }
                    let mobs: Vec<String> = g
                        .mobs
                        .iter()
                        .map(|m| {
                            let mut s = m.name.clone();
                            if let Some(l) = &m.level {
                                s.push(' ');
                                s.push_str(l);
                            }
                            if let Some(r) = &m.rarity {
                                s.push(' ');
                                s.push_str(r);
                            }
                            s
                        })
                        .collect();
                    mono(ui, &mobs.join("; "), TEXT_3);
                });
            }
            ui.add_space(6.0);
            mono(ui, &format!("from quest-items.json drops: {}", d.quest.len()), TEXT_3);
            if d.quest.is_empty() {
                dim(ui, &format!("no quest-items drop row names this zone (matched on \"{}\")", zone_key(&z.name)));
            }
            /* THESE NAMES USED TO OPEN THE DROPS SCREEN, which is gone (see `nav::NAV`). They
             * go to Items instead, which is the jump the Drops screen's own heading offered, and
             * only when gear-data really has the name: the quest-items drop table keys 1637 items
             * and 682 of them are gear-data items, so a link on all of them would dead end on
             * "no item named" more often than it landed. The other 955 are printed as text, with
             * the same sentence the Drops screen used to print in their place. */
            for (item, mob) in &d.quest {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    match data.item(item) {
                        Some(rec) => {
                            if link(ui, item, FontId::proportional(12.5)).on_hover_text("open in Items").clicked() {
                                crate::screens::items::jump_to(&rec.name);
                                ask = Some(crate::screens::Ask::ShowItem(rec.name.clone()));
                            }
                        }
                        None => {
                            mono(ui, item, TEXT);
                            dim(ui, "not in gear-data.json items under this name");
                        }
                    }
                    mono(ui, mob, TEXT_3);
                });
            }

            /* MERCHANTS. Zone detail is the only home merchants.json got, deliberately: the owner
             * has been cutting rows off the rail, and "who sells here" is a question about a place,
             * which is a page that already exists. A row is one shop; clicking it unfolds the
             * whole stock list, the faction hits for killing them, and anything else the page
             * carries, so nothing in the record is out of reach from here. */
            pane_section(ui, "MERCHANTS");
            if d.merchants.is_empty() {
                dim(ui, &format!("{} names no merchant in this zone (matched on the zone key, the tracker name and the wiki title).", crate::data::MERCHANTS_FILE));
            } else {
                let stock: usize = d.merchants.iter().map(|m| m.sells.len()).sum();
                mono(ui, &format!("{} merchant{}, {stock} stock line{}", d.merchants.len(), if d.merchants.len() == 1 { "" } else { "s" }, if stock == 1 { "" } else { "s" }), TEXT_3);
                head_row(ui, &[("merchant", false, 0.0), ("loc", false, 190.0), ("race", false, 90.0), ("lvl", true, 40.0), ("sells", true, 44.0)]);
                for m in &d.merchants {
                    let key = format!("merchant:{}", m.key);
                    let open = self.expanded.contains(&key);
                    let n = if m.sells.is_empty() { String::new() } else { m.sells.len().to_string() };
                    let cols = [
                        Col { text: &m.who, mono: false, color: if open { FLARE } else { TEXT }, right: false, width: 0.0 },
                        Col { text: &m.loc, mono: true, color: TEXT_3, right: false, width: 190.0 },
                        Col { text: &m.race, mono: true, color: TEXT_2, right: false, width: 90.0 },
                        Col { text: &m.lvl, mono: true, color: TEXT_2, right: true, width: 40.0 },
                        Col { text: &n, mono: true, color: TEXT_2, right: true, width: 44.0 },
                    ];
                    let r = list_row(ui, open, &cols);
                    let r = r.on_hover_text(match m.cls.is_empty() {
                        true => format!("wiki page {}", m.t),
                        false => format!("{}\nwiki page {}", m.cls, m.t),
                    });
                    if r.clicked() {
                        toggle = Some(key.clone());
                    }
                    if !open {
                        continue;
                    }
                    /* Nothing the file carries is hidden, and a merchant has no detail pane of its
                     * own to hold the MORE table every other record gets. Empty on the shipped
                     * file; the day the puller adds a field, this is where it shows up. */
                    if !m.unknown.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            dim(ui, "fields this build did not type");
                            mono(ui, &m.unknown.join(", "), TEXT_2);
                        });
                    }
                    if m.sells.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            dim(ui, "the merchant page carries no stock list");
                        });
                    }
                    for (item, price, raw) in &m.sells {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            /* The item is a link only when this snapshot really holds it. 361 of
                             * the 2720 names in the stock lists are gear-data items; the rest are
                             * tradeskill supplies, food and spell scrolls with no item page, and
                             * a link on those would dead end. */
                            match data.item(item) {
                                Some(rec) => {
                                    if link(ui, item, FontId::monospace(11.5)).on_hover_text(format!("open in Items. Source line: {raw}")).clicked() {
                                        crate::screens::items::jump_to(&rec.name);
                                        ask = Some(crate::screens::Ask::ShowItem(rec.name.clone()));
                                    }
                                }
                                None => {
                                    mono_hover(ui, item, TEXT, raw);
                                }
                            }
                            match price {
                                Some(p) => mono(ui, p, TEXT_2),
                                None => dim(ui, "no price"),
                            }
                        });
                    }
                    for (label, hits) in [("kill faction", &m.fac), ("second faction list (ofac)", &m.ofac)] {
                        if hits.is_empty() {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            dim(ui, label);
                        });
                        for (name, delta, page, raw) in hits {
                            ui.horizontal(|ui| {
                                ui.add_space(34.0);
                                match page {
                                    /* A faction with a page is not a link: there is no Factions
                                     * screen to land on, and a door to a room that does not exist
                                     * is the defect this whole lane is about. It reads in the
                                     * brighter colour, and its own page's words are the hover. */
                                    Some(_) => mono_hover(ui, name, TEXT, &format!("{raw}\nfactions.json has a page for this faction")),
                                    None => mono_hover(ui, name, TEXT_3, raw),
                                };
                                if let Some(dl) = delta {
                                    mono(ui, dl, TEXT_2);
                                }
                            });
                        }
                    }
                    for (label, list) in [("quests", &m.quests), ("drops when killed", &m.loot)] {
                        if list.is_empty() {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            dim(ui, label);
                            mono(ui, &list.join("; "), TEXT_2);
                        });
                    }
                }
                dim(ui, "Prices, /loc and faction deltas are the wiki's own text, never recomputed. Hover a line for the source.");
            }

            /* FACTIONS. The other half of the same idea: factions.json says which zones raise and
             * lower it, so a zone page can say which factions it moves. */
            pane_section(ui, "FACTIONS");
            if d.factions.is_empty() {
                dim(ui, &format!("no faction in {} names this zone.", crate::data::FACTIONS_FILE));
            } else {
                mono(ui, &format!("{} faction line{} from {}", d.factions.len(), if d.factions.len() == 1 { "" } else { "s" }, crate::data::FACTIONS_FILE), TEXT_3);
                head_row(ui, &[("faction", false, 0.0), ("here", false, 70.0), ("mobs", true, 46.0), ("quests", true, 50.0)]);
                for f in &d.factions {
                    let key = format!("faction:{}", f.key);
                    let open = self.expanded.contains(&key);
                    let mobs = if f.mobs.is_empty() { String::new() } else { f.mobs.len().to_string() };
                    let quests = if f.quests.is_empty() { String::new() } else { f.quests.len().to_string() };
                    let cols = [
                        Col { text: &f.name, mono: false, color: if open { FLARE } else { TEXT }, right: false, width: 0.0 },
                        Col { text: f.way, mono: true, color: TEXT_2, right: false, width: 70.0 },
                        Col { text: &mobs, mono: true, color: TEXT_2, right: true, width: 46.0 },
                        Col { text: &quests, mono: true, color: TEXT_2, right: true, width: 50.0 },
                    ];
                    let r = list_row(ui, open, &cols);
                    let r = match &f.desc {
                        Some(t) if !t.is_empty() => r.on_hover_text(format!("{t}\n\nwiki page {}", f.t)),
                        _ => r.on_hover_text(format!("factions.json carries no description for this faction\n\nwiki page {}", f.t)),
                    };
                    if r.clicked() {
                        toggle = Some(key.clone());
                    }
                    if !open {
                        continue;
                    }
                    if !f.unknown.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            dim(ui, "fields this build did not type");
                            mono(ui, &f.unknown.join(", "), TEXT_2);
                        });
                    }
                    if let Some(t) = &f.desc {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.add(egui::Label::new(egui::RichText::new(t).font(FontId::proportional(12.0)).color(TEXT_2)).wrap());
                        });
                    }
                    /* THE MOB LIST IS THE WHOLE LIST, NOT THIS ZONE'S SLICE, and that is a
                     * decision rather than an oversight. Every entry reads "a dark ritualist
                     * (Castle Mistmoore)", so filtering to this zone means parsing the bracket,
                     * and 306 of the 9866 entries nest their brackets, carry a body type or have
                     * no zone at all. A filter would be right most of the time and would silently
                     * hide the rest. The heading says which list this is; the reader reads. */
                    for m in &f.mobs {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            mono(ui, m, TEXT_3);
                        });
                    }
                    for q in &f.quests {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            match data.quests.iter().find(|rec| rec.name.eq_ignore_ascii_case(q)) {
                                Some(rec) => {
                                    if link(ui, q, FontId::monospace(11.5)).on_hover_text("open in Quests").clicked() {
                                        crate::screens::quests::jump_to(&rec.name);
                                        ask = Some(crate::screens::Ask::ShowQuest(rec.name.clone()));
                                    }
                                }
                                None => {
                                    mono(ui, q, TEXT_3);
                                    dim(ui, "no quest under this title in quest-items.json");
                                }
                            }
                        });
                    }
                }
                dim(ui, "raises and lowers are factions.json's zr and zl. The mob list is the faction's whole list for that direction, not this zone's slice: the entries name their own zone in brackets and 306 of them do not, so filtering would hide those.");
            }

            pane_section(ui, "MORE");
            if d.rows.more.is_empty() {
                dim(ui, "Nothing beyond the typed fields: extra is empty for this record.");
            } else {
                kv_table(ui, "zones_more", &d.rows.more);
            }
        });
        if let Some(k) = toggle {
            if !self.expanded.remove(&k) {
                self.expanded.insert(k);
            }
        }
        if let Some(e) = open_err {
            self.notice = Some(e);
        }
        ask
    }

    fn build_detail(&self, i: usize, data: &crate::data::Snapshot) -> Detail {
        let z = &data.zones[i];
        let (gear, quest) = match &self.cache {
            Some(c) => {
                let keys = zone_keys(z);
                let mut gear: Vec<GearSource> = Vec::new();
                let mut quest: Vec<(String, String)> = Vec::new();
                for k in &keys {
                    if let Some(g) = c.gear.get(k) {
                        gear.extend(g.iter().cloned());
                    }
                    if let Some(q) = c.quest.get(k) {
                        quest.extend(q.iter().cloned());
                    }
                }
                gear.sort_by_key(|g| g.item.to_lowercase());
                gear.dedup_by(|a, b| a.item == b.item && a.mobs == b.mobs);
                quest.sort();
                quest.dedup();
                (gear, quest)
            }
            None => (Vec::new(), Vec::new()),
        };
        let roster = merge_roster(&z.mobs, &z.atlas_mobs, &z.atlas_items);
        let items = page_items(&z.atlas_mobs, &z.atlas_items);
        let merchants = zone_merchants(z, data);
        let factions = zone_factions(z, data);
        match view(z) {
            Ok(v) => Detail {
                idx: i,
                rows: detail_rows(&z.name, &v, &z.extra, PREFERRED, EXPANDED),
                roster,
                items,
                gear,
                quest,
                merchants,
                factions,
                err: None,
            },
            Err(e) => Detail {
                idx: i,
                rows: detail_rows(&z.name, &View::new(), &z.extra, PREFERRED, &[]),
                roster,
                items,
                gear,
                quest,
                merchants,
                factions,
                err: Some(e),
            },
        }
    }
}

/* ------------------------------------------------------- merchants and factions -- */

/// The merchants standing in this zone, flattened for drawing. PURE over the snapshot: it reads
/// `Snapshot::merchants_in`, which matches the zone's key, tracker name and wiki title against the
/// merchant's own `zone` string, folded, and never rewrites either side.
fn zone_merchants(z: &crate::data::Zone, data: &crate::data::Snapshot) -> Vec<ZoneMerchant> {
    let page = |name: &str| data.faction(name).map(|f| f.name.clone());
    let mut out: Vec<ZoneMerchant> = data
        .merchants_in(z)
        .into_iter()
        .map(|m| ZoneMerchant {
            key: m.key.clone(),
            who: m.name.clone(),
            t: m.t.clone(),
            loc: m.loc.clone(),
            lvl: m.lvl.clone().unwrap_or_default(),
            race: m.race.clone(),
            cls: m.cls.clone().unwrap_or_default(),
            unknown: m.extra.keys().cloned().collect(),
            sells: m
                .sales()
                .into_iter()
                .map(|s| {
                    (
                        s.item.to_owned(),
                        s.price.map(str::to_owned),
                        s.raw.to_owned(),
                    )
                })
                .collect(),
            fac: m
                .faction_hits()
                .into_iter()
                .map(|h| {
                    (
                        h.faction.to_owned(),
                        h.delta.map(str::to_owned),
                        page(h.faction),
                        h.raw.to_owned(),
                    )
                })
                .collect(),
            ofac: m
                .other_faction_hits()
                .into_iter()
                .map(|h| {
                    (
                        h.faction.to_owned(),
                        h.delta.map(str::to_owned),
                        page(h.faction),
                        h.raw.to_owned(),
                    )
                })
                .collect(),
            quests: m.quests.clone(),
            loot: m.loot.clone(),
        })
        .collect();
    out.sort_by_key(|m| m.who.to_lowercase());
    out
}

/// The factions this zone moves, flattened for drawing.
fn zone_factions(z: &crate::data::Zone, data: &crate::data::Snapshot) -> Vec<ZoneFaction> {
    data.factions_of_zone(z)
        .into_iter()
        .map(|m| {
            let raise = m.way == crate::data::factions::Way::Raise;
            ZoneFaction {
                key: format!("{}|{}", m.faction.key, m.way.label()),
                name: m.faction.name.clone(),
                t: m.faction.t.clone(),
                way: m.way.label(),
                desc: m.faction.desc.clone(),
                unknown: m.faction.extra.keys().cloned().collect(),
                mobs: if raise {
                    m.faction.mobs_raise.clone()
                } else {
                    m.faction.mobs_lower.clone()
                },
                quests: if raise {
                    m.faction.quests_raise.clone()
                } else {
                    m.faction.quests_lower.clone()
                },
            }
        })
        .collect()
}

/* ------------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// ONE FRAME AT A TIME, AND THIS IS NOT TIDINESS.
    ///
    /// `JUMP` is a process wide static and `ZonesScreen::ui` CONSUMES it (`take_jump`) on every
    /// frame it draws. The test harness runs tests on parallel threads in one process, so two
    /// tests that each set a jump and then draw will steal each other's: one screen opens on the
    /// other's zone, or on none, and the failure lands on whichever test lost the race rather than
    /// on the code. It is a real race and it was found by adding the second frame driving test to
    /// this file: both went red together in the full suite and both passed alone.
    ///
    /// The fix belongs here rather than in `jump_to`, because the static is right for the product:
    /// one screen, one reader, one pending jump. It is only many-at-once under a test harness. So
    /// every test in this module that drives a Zones frame takes this lock first.
    static ONE_FRAME_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Take [`ONE_FRAME_AT_A_TIME`], surviving a poisoned lock: an earlier test that panicked
    /// while holding it must not turn one failure into every frame test failing after it.
    fn frame_lock() -> std::sync::MutexGuard<'static, ()> {
        ONE_FRAME_AT_A_TIME
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn zone_key_folds_the_three_files_spellings_together_and_nothing_else() {
        assert_eq!(zone_key("The Plane of Sky"), zone_key("Plane of Sky"));
        assert_eq!(zone_key("Ak'Anon"), zone_key("Ak'anon"));
        assert_eq!(zone_key("Dagnor's Cauldron"), "dagnorscauldron");
        assert_eq!(zone_key("The Temple of Droga"), zone_key("Temple of Droga"));
        assert_ne!(zone_key("Droga"), zone_key("Temple of Droga"));
        assert_ne!(zone_key("Cabilis East"), zone_key("Cabilis"));
    }

    #[test]
    fn zone_keys_cover_name_key_and_wiki_title_without_repeats() {
        let z = Zone {
            key: "airplane".into(),
            name: "The Plane of Sky".into(),
            wiki_title: Some("Plane of Sky".into()),
            ..Default::default()
        };
        assert_eq!(zone_keys(&z), vec!["planeofsky", "airplane"]);
        let z = Zone {
            key: "befallen".into(),
            name: "Befallen".into(),
            wiki_title: Some("Befallen".into()),
            ..Default::default()
        };
        assert_eq!(zone_keys(&z), vec!["befallen"]);
    }

    #[test]
    fn level_text_prefers_the_range_string_then_the_number_then_nothing() {
        let m: ZoneMob = serde_json::from_value(json!({"n": "a", "lvl": "8-10", "lv": 9})).unwrap();
        assert_eq!(level_of(&m), "8-10");
        let m: ZoneMob = serde_json::from_value(json!({"n": "a", "lv": 12.8})).unwrap();
        assert_eq!(level_of(&m), "12.8");
        let m: ZoneMob = serde_json::from_value(json!({"n": "a", "lv": 60, "lvl": ""})).unwrap();
        assert_eq!(level_of(&m), "60");
        let m: ZoneMob = serde_json::from_value(json!({"n": "a"})).unwrap();
        assert_eq!(level_of(&m), "");
    }

    #[test]
    fn loc_text_drops_nulls_and_is_none_when_nothing_is_left() {
        assert_eq!(
            loc_text(Some(&[json!(-130), json!(1200.5), Value::Null])),
            Some("-130, 1200.5".to_owned())
        );
        assert_eq!(loc_text(Some(&[Value::Null])), None);
        assert_eq!(loc_text(Some(&[])), None);
        assert_eq!(loc_text(None), None);
    }

    fn km(n: &str, lvl: Option<&str>, named: bool) -> ZoneMob {
        ZoneMob {
            name: n.into(),
            lvl: lvl.map(str::to_owned),
            named,
            ..Default::default()
        }
    }
    fn am(n: &str, lvl: Option<&str>, drops: Vec<usize>, dr: Vec<Option<&str>>) -> AtlasMob {
        AtlasMob {
            name: n.into(),
            lvl: lvl.map(str::to_owned),
            u: format!("https://{n}"),
            drops,
            dr: dr.into_iter().map(|x| x.map(str::to_owned)).collect(),
            ..Default::default()
        }
    }
    fn ai(n: &str) -> AtlasItem {
        AtlasItem {
            name: n.into(),
            ..Default::default()
        }
    }

    #[test]
    fn merge_roster_unions_by_name_and_pairs_drops_with_rarities() {
        let kills = vec![
            km("A blade storm", Some("59-61"), true),
            km("only in kills", None, false),
            ZoneMob {
                name: "lv only".into(),
                lv: Some(12.0),
                t: "Lv_only".into(),
                ..Default::default()
            },
        ];
        let atlas = vec![
            am(
                "a blade storm",
                Some("58-62"),
                vec![1, 0, 9],
                vec![Some("Rare"), None],
            ),
            AtlasMob {
                loc: Some(vec![json!(1), json!(2), Value::Null]),
                ..am("only in atlas", Some("5"), vec![], vec![])
            },
        ];
        let items = vec![ai("Amber"), ai("Bandages")];
        let m = merge_roster(&kills, &atlas, &items);
        let names: Vec<&str> = m.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["A blade storm", "lv only", "only in atlas", "only in kills"]
        );
        /* No `lvl` string: the numeric `lv` stands in. The kills slug rides along. */
        assert_eq!(m[1].level, "12");
        assert_eq!(m[1].slug.as_deref(), Some("Lv_only"));
        assert!(!m[1].named);
        let bs = &m[0];
        assert!(bs.in_kills && bs.in_atlas);
        assert_eq!(bs.level, "59-61 (atlas 58-62)");
        assert!(bs.named);
        /* drops[0]=1 -> Bandages with dr[0]=Rare; drops[1]=0 -> Amber with dr[1]=null;
         * drops[2]=9 is past the item list and is skipped, not invented. */
        assert_eq!(
            bs.drops,
            vec![
                ("Bandages".to_owned(), Some("Rare".to_owned())),
                ("Amber".to_owned(), None)
            ]
        );
        assert_eq!(bs.url.as_deref(), Some("https://a blade storm"));
        assert_eq!(bs.loc, None);
        let oa = &m[2];
        assert!(!oa.in_kills && oa.in_atlas);
        assert_eq!(oa.level, "5");
        assert_eq!(oa.loc.as_deref(), Some("1, 2"));
        let ok = &m[3];
        assert!(ok.in_kills && !ok.in_atlas);
        assert_eq!(ok.level, "");
        assert!(!ok.named);
    }

    #[test]
    fn a_mob_listed_twice_on_the_page_keeps_both_drop_lists() {
        let atlas = vec![
            am("x", None, vec![0], vec![Some("Common")]),
            am("X", None, vec![1], vec![None]),
        ];
        let items = vec![ai("Amber"), ai("Bandages")];
        let m = merge_roster(&[], &atlas, &items);
        assert_eq!(m.len(), 1);
        assert_eq!(
            m[0].drops,
            vec![
                ("Amber".to_owned(), Some("Common".to_owned())),
                ("Bandages".to_owned(), None)
            ]
        );
    }

    #[test]
    fn page_items_invert_the_drop_indexes_and_keep_undropped_items() {
        let atlas = vec![
            am("a", None, vec![1], vec![]),
            am("b", None, vec![1, 0], vec![]),
        ];
        let items = vec![
            ai("Zed"),
            ai("Alpha"),
            AtlasItem {
                name: "Never".into(),
                u: "https://n".into(),
                g: Some(3.0),
                ..Default::default()
            },
        ];
        let p = page_items(&atlas, &items);
        assert_eq!(p[0].name, "Alpha");
        assert_eq!(p[0].droppers, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(p[0].url, None, "an empty u is no url");
        assert_eq!(p[1].name, "Never");
        assert!(p[1].droppers.is_empty());
        assert_eq!(p[1].url.as_deref(), Some("https://n"));
        assert_eq!(p[1].g.as_deref(), Some("3"));
        assert_eq!(p[2].name, "Zed");
        assert_eq!(p[2].droppers, vec!["b".to_owned()]);
    }

    #[test]
    fn the_typed_table_summarises_the_three_rosters_and_never_the_name() {
        let z = Zone {
            key: "befallen".into(),
            name: "Befallen".into(),
            city: Some(false),
            mobs: vec![km("a", None, false)],
            ..Default::default()
        };
        let r = detail_rows(&z.name, &view(&z).unwrap(), &z.extra, PREFERRED, EXPANDED);
        let keys: Vec<&str> = r.typed.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "key",
                "tracked",
                "city",
                "wiki_title",
                "wiki_url",
                "updated",
                "atlas_file",
                "mobs",
                "atlas_mobs",
                "atlas_items"
            ]
        );
        assert_eq!(r.typed[7].1, "1 entries, listed below", "the mobs row");
        assert!(r.more.is_empty());
    }

    /* ---- the real files. Fail loudly when absent; GRIMOIRE_NO_DATA=1 skips on purpose. ---- */

    #[test]
    fn real_kills_data_and_atlas_pages_have_the_measured_rosters() {
        let Some(s) = crate::data::testdata::snapshot() else {
            return;
        };
        let tracked: usize = s.zones.iter().map(|z| z.mobs.len()).sum();
        /* 3843 until 2026-09-03, when the 46 Kunark and Velious atlas pages that had never been
         * run through the roster generator were added to kills-data. 3843 + 2990 = 6833, which
         * is also the number of mobs the 122 atlas pages hold, because the roster is derived
         * from them. The two counts agreeing is the check, not the constant. */
        let on_pages: usize = s.zones.iter().map(|z| z.atlas_mobs.len()).sum();
        assert_eq!(tracked, 6833, "every kills-data mob is on a Zone");
        assert_eq!(
            tracked, on_pages,
            "the roster is the atlas pages, mob for mob"
        );
        assert_eq!(s.zones.iter().filter(|z| z.in_tracker()).count(), 122);
        assert_eq!(
            s.zones.iter().filter(|z| z.city.is_some()).count(),
            76,
            "city is the kill tracker zone table's field and it still covers only the first 76"
        );

        let air = s.zone("airplane").expect("airplane");
        assert_eq!(air.name, "The Plane of Sky");
        assert_eq!(air.mobs.len(), 64);
        assert!(
            zone_keys(air).contains(&zone_key("Plane of Sky")),
            "the wiki title is a key too: {:?}",
            zone_keys(air)
        );
        let merged = merge_roster(&air.mobs, &air.atlas_mobs, &air.atlas_items);
        assert!(merged.len() >= 64);
        assert!(
            merged.iter().any(|m| m.in_kills && m.in_atlas),
            "at least one Sky mob is on both rosters"
        );

        let bef = s.zone("befallen").expect("befallen");
        assert_eq!(bef.atlas_mobs.len(), 40);
        assert_eq!(bef.atlas_items.len(), 119);
        assert_eq!(
            bef.wiki_url.as_deref(),
            Some("https://eqlwiki.com/index.php/Befallen")
        );
        /* The parallel-array rule: every mob's dr is as long as its drops. */
        for m in &bef.atlas_mobs {
            assert_eq!(
                m.dr.len(),
                m.drops.len(),
                "dr and drops are parallel on {}",
                m.name
            );
        }
        let dread = bef
            .atlas_mobs
            .iter()
            .find(|m| m.name == "a dread bone")
            .expect("a dread bone is on the Befallen page");
        assert_eq!(dread.drops[0], 2);
        assert_eq!(bef.atlas_items[2].name, "Bandages");
        let merged = merge_roster(&bef.mobs, &bef.atlas_mobs, &bef.atlas_items);
        let dm = merged.iter().find(|m| m.name == "a dread bone").unwrap();
        assert!(dm.in_kills && dm.in_atlas);
        assert_eq!(dm.level, "8-10");
        assert_eq!(
            dm.drops[0],
            ("Bandages".to_owned(), Some("Always".to_owned()))
        );
        let items = page_items(&bef.atlas_mobs, &bef.atlas_items);
        assert_eq!(items.len(), 119);
        assert!(items
            .iter()
            .find(|it| it.name == "Bandages")
            .unwrap()
            .droppers
            .contains(&"a dread bone".to_owned()));

        /* The one page with null wiki fields still draws: the link line says why there is none. */
        let lake = s.zone("lakenerius").expect("lakenerius");
        assert!(lake.has_wiki_page() && lake.wiki_url.is_none());
        assert!(merge_roster(&lake.mobs, &lake.atlas_mobs, &lake.atlas_items).is_empty());

        /* The cross-file indexes land on real zones: a gear-data source and a quest drop zone
         * both resolve to a Zone through zone_key. */
        let keys: HashSet<String> = s.zones.iter().flat_map(zone_keys).collect();
        assert!(
            keys.contains(&zone_key("Temple of Droga")),
            "gear-data's Temple of Droga has a zone"
        );
        assert!(
            keys.contains(&zone_key("The Plane of Mischief")),
            "quest-items' The Plane of Mischief has a zone"
        );
    }
    /* ------------------------------- the quest drop rows, which used to open the Drops screen -- */

    /// Draw the real detail pane for one zone and return every text run with the rectangle and the
    /// COLOUR it was painted in.
    ///
    /// THE COLOUR IS THE POINT. What changed in these rows is a branch, and the two arms of it
    /// differ on screen by exactly one thing a test can read: `screens::items::link` paints its
    /// word in GOLD and `mono` paints its word in TEXT. Asserting the words alone would pass on an
    /// implementation that linked everything or linked nothing, because both arms print the name.
    ///
    /// `Shape::Vec` is flattened for the reason `screens::watch` flattens it: a test that read only
    /// the top level would find nothing and pass.
    fn draw_zone_detail(
        ctx: &egui::Context,
        screen: &mut ZonesScreen,
        data: &crate::data::Snapshot,
    ) -> Vec<(String, egui::Rect, egui::Color32)> {
        let mut settings = crate::settings::Settings::default();
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked("Broken_Stoic"),
            youtube: crate::watcher::Channel::unchecked("BrokenStoic"),
        };
        let mut cx = crate::screens::Cx {
            data: Some(data),
            railed: false,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: &mut ingest,
            player: Default::default(),
            stage: None,
            demand: None,
            ask: Default::default(),
            chat: crate::chat::ChatHandle::idle(),
            chat_wanted: false,
            auth: Default::default(),
            auth_begin: false,
            auth_cancel: false,
            yt: Default::default(),
            yt_wanted: false,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 20000.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        assert!(!out.shapes.is_empty(), "the zone screen painted nothing");
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        fn flatten(sh: egui::Shape, out: &mut Vec<egui::Shape>) {
            match sh {
                egui::Shape::Vec(v) => {
                    for x in v {
                        flatten(x, out);
                    }
                }
                other => out.push(other),
            }
        }
        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.shape, &mut flat);
        }
        flat.iter()
            .filter_map(|sh| match sh {
                egui::Shape::Text(t) => {
                    /* THE COLOUR IS READ OFF THE LAYOUT JOB, NOT OFF `fallback_color`. A galley
                     * built by `ui.label(RichText::new(..).color(..))` carries the colour in its
                     * section format and leaves the shape's fallback at the style's default, so
                     * reading the fallback returns the same grey for every label on the screen
                     * and the two arms below become indistinguishable. Measured: the first cut of
                     * this read the fallback and both arms came back #8C8C8C. */
                    let col = t
                        .galley
                        .job
                        .sections
                        .first()
                        .map(|sec| sec.format.color)
                        .unwrap_or(t.fallback_color);
                    Some((
                        t.galley.text().to_owned(),
                        egui::Rect::from_min_size(t.pos, t.galley.size()),
                        col,
                    ))
                }
                _ => None,
            })
            .collect()
    }

    /// A QUEST DROP ROW IS A LINK WHEN GEAR-DATA HAS THE NAME AND PLAIN TEXT WITH A REASON WHEN IT
    /// DOES NOT.
    ///
    /// These rows used to open the Drops screen, which is gone (`nav::NAV`). Items is where an item
    /// record lives and it is the jump the Drops screen's own heading offered, but only 682 of the
    /// drop table's 1637 names are gear-data items, so an unconditional link would have dead ended
    /// on "no item named" more often than it landed. Hence two arms, and hence this test: it drives
    /// BOTH of them in one frame, off one zone that really has both kinds, so an implementation
    /// that collapsed to either arm alone fails whichever arm it collapsed to.
    ///
    /// WHAT THIS DOES NOT COVER, said here rather than left to be assumed: the CLICK. The detail
    /// pane's `Panel::right` does not take its own side of a bare root `Ui`, so in a headless frame
    /// it and the central list pane land on one another and a synthetic press never reaches the
    /// link, whatever rectangle it is aimed at (measured: the pointer's `interact_pos` is inside
    /// the word's rect and no `Response` reports a click). The line that raises
    /// `Ask::ShowItem` is therefore held by nothing here. It is character for character the gear
    /// list's line twenty rows above it in the same function, which has been in this screen since
    /// it was written, and it was driven by hand in the running app.
    #[test]
    fn a_quest_drop_row_links_to_items_when_gear_data_has_the_name_and_says_so_when_it_does_not() {
        let Some(s) = crate::data::testdata::snapshot() else {
            return;
        };
        /* The screen's own index, rebuilt: zone_key -> the item names quest-items drops there. */
        let mut by_zone: HashMap<String, Vec<&str>> = HashMap::new();
        for d in &s.drops {
            for src in &d.sources {
                if !src.zone.is_empty() {
                    by_zone
                        .entry(zone_key(&src.zone))
                        .or_default()
                        .push(&d.name);
                }
            }
        }
        /* A zone that really has both kinds. Searched rather than named, so a data refresh moves
         * the fixture instead of breaking it, and asserted to exist so it cannot pass on nothing.
         *
         * THE SMALLEST SUCH ZONE, and that is not tidiness. The detail pane draws the whole mob
         * roster and the whole atlas item list ABOVE these rows, inside a scroll area that paints
         * only what fits the viewport. On the Plane of Sky, 64 mobs and 273 atlas items push them
         * past any believable frame height and the test fails on a run that is merely off screen,
         * which reads as a defect and is not one. */
        let mut fixture: Option<(&Zone, String, String)> = None;
        let mut best = usize::MAX;
        for z in &s.zones {
            let mut here: Vec<&str> = Vec::new();
            for k in zone_keys(z) {
                if let Some(v) = by_zone.get(&k) {
                    here.extend(v);
                }
            }
            let known = here.iter().find(|n| s.item(n).is_some()).copied();
            let unknown = here.iter().find(|n| s.item(n).is_none()).copied();
            if let (Some(k), Some(u)) = (known, unknown) {
                let page = z.mobs.len() + z.atlas_mobs.len() + z.atlas_items.len() + here.len();
                if page < best {
                    best = page;
                    fixture = Some((z, k.to_owned(), u.to_owned()));
                }
            }
        }
        let (zone, known, unknown) = fixture.expect(
            "no zone in the snapshot lists both a quest drop that is a gear-data item and one that \
             is not, so this test could not drive both arms and proves nothing",
        );

        let _one_at_a_time = frame_lock();
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        /* One screen, and the first frame thrown away: the pane lays out shorter on its opening
         * frame than on every frame after it, and a rectangle read from the opening frame belongs
         * to a layout the reader never sees. */
        let mut screen = ZonesScreen::default();
        jump_to(&zone.name);
        let _warm = draw_zone_detail(&ctx, &mut screen, s);
        let runs = draw_zone_detail(&ctx, &mut screen, s);

        /* A NAME CAN BE PAINTED MORE THAN ONCE, because the drop table lists it once per mob that
         * drops it in the zone, and the screen prints a row for each. So every occurrence is read
         * and every one has to agree: a rule applied to the first row and not the rest is exactly
         * the kind of half fix this asserts against. */
        let inks = |want: &str| -> Vec<egui::Color32> {
            let hits: Vec<egui::Color32> = runs
                .iter()
                .filter(|(t, _, _)| t == want)
                .map(|(_, _, c)| *c)
                .collect();
            assert!(
                !hits.is_empty(),
                "\"{want}\" was not painted on {}: {:?}",
                zone.name,
                runs.iter().map(|(t, _, _)| t).take(60).collect::<Vec<_>>()
            );
            hits
        };
        let all = |want: &str, col: egui::Color32, why: &str| {
            let got = inks(want);
            assert!(
                got.iter().all(|c| *c == col),
                "{why}: \"{want}\" was painted {got:?} and every one of them should be {col:?}"
            );
        };

        /* THE ARM THAT CAN LINK PAINTS THE LINK'S OWN INK. */
        all(
            &known,
            GOLD,
            "a gear-data item is not drawn as a link to its record",
        );
        /* THE ARM THAT CANNOT PAINTS PLAIN TEXT AND SAYS WHY, in the sentence the Drops screen
         * used to print in its heading. */
        all(
            &unknown,
            TEXT,
            "a name gear-data does not have is offered as a link to it anyway",
        );
        all(
            "not in gear-data.json items under this name",
            TEXT_3,
            "the unlinked name is left without its reason",
        );
        assert_ne!(GOLD, TEXT, "the fixture cannot tell the two arms apart");
    }

    /// THE MERCHANT SECTION, THROUGH THE FUNCTION THE PANE CALLS. Zone detail is the only home
    /// merchants.json got, so the flattening has to carry every line of the record: an empty stock
    /// list here would draw a shop with nothing in it and look like the wiki said so.
    #[test]
    fn a_zone_carries_its_merchants_with_their_whole_stock_list() {
        let Some(s) = crate::data::testdata::snapshot() else {
            return;
        };
        let z = s.zone("Lesser Faydark").expect("Lesser Faydark");
        let ms = zone_merchants(z, s);
        assert!(!ms.is_empty(), "Lesser Faydark has merchants");
        let brownie = ms
            .iter()
            .find(|m| m.who == "a brownie merchant")
            .expect("a brownie merchant stands in Lesser Faydark");
        assert_eq!(brownie.race, "Brownie");
        assert_eq!(brownie.lvl, "30");
        assert!(!brownie.loc.is_empty());
        assert!(
            !brownie.t.is_empty(),
            "the wiki page title is drawn on the hover"
        );
        /* every stock line survives the flattening, and every one keeps its source */
        let raw = s
            .merchants
            .iter()
            .find(|m| m.key == "a brownie merchant")
            .expect("the record");
        assert_eq!(brownie.sells.len(), raw.sells.len());
        for ((item, _price, line), src) in brownie.sells.iter().zip(raw.sells.iter()) {
            assert_eq!(line, src, "the source line is kept beside the split");
            assert!(
                src.starts_with(item.as_str()),
                "{src:?} should start with {item:?}"
            );
        }
        /* the faction hit is split and pointed at its page when the wiki has one */
        assert!(brownie
            .fac
            .iter()
            .any(|(name, delta, _, _)| name == "Brownie" && delta.as_deref() == Some("-30")));
        /* rows are in name order, which is what makes a 90 shop zone readable */
        let mut sorted: Vec<String> = ms.iter().map(|m| m.who.to_lowercase()).collect();
        let seen = sorted.clone();
        sorted.sort();
        assert_eq!(seen, sorted);
    }

    /// THE FACTION SECTION. One row per direction, never two of one, and the mob list it unfolds
    /// is the one for that direction.
    #[test]
    fn a_zone_carries_the_factions_it_moves_one_row_per_direction() {
        let Some(s) = crate::data::testdata::snapshot() else {
            return;
        };
        let z = s.zone("Cazic Thule").expect("Cazic Thule");
        let fs_ = zone_factions(z, s);
        assert!(!fs_.is_empty(), "Cazic Thule moves factions");
        for f in &fs_ {
            assert!(f.way == "raises" || f.way == "lowers", "{}", f.way);
            let rec = s.faction(&f.name).expect("every row names a real faction");
            let want = if f.way == "raises" {
                &rec.mobs_raise
            } else {
                &rec.mobs_lower
            };
            assert_eq!(
                &f.mobs, want,
                "the unfolded list is the one for this direction"
            );
            let wantq = if f.way == "raises" {
                &rec.quests_raise
            } else {
                &rec.quests_lower
            };
            assert_eq!(&f.quests, wantq);
            assert_eq!(f.t, rec.t);
        }
        let mut keys: Vec<&str> = fs_.iter().map(|f| f.key.as_str()).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "a faction and a direction appear once");
        /* the expand key is the faction AND the direction, so unfolding raises does not unfold
         * lowers for the same faction */
        assert!(fs_.iter().all(|f| f.key.ends_with(f.way)));
    }

    /// THE TWO NEW SECTIONS ARE PAINTED BY A REAL FRAME, not merely computed by a function a test
    /// calls. Everything above proves the flattening; this proves the pane draws it. A screen that
    /// built the rows and never reached the drawing code would satisfy every other test in this
    /// file and put nothing on screen, which is the whole defect this lane is about, wearing the
    /// last hat it has left.
    ///
    /// THE FENCE IS THE MOBS SECTION. Its sentence is asserted first, so a frame that painted
    /// nothing, or stopped before the detail pane, fails for the right reason instead of letting an
    /// absence pass for free.
    #[test]
    fn a_real_frame_paints_the_merchants_and_the_factions_of_a_zone() {
        let Some(data) = crate::data::testdata::snapshot() else {
            return;
        };
        let _one_at_a_time = frame_lock();
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let mut screen = ZonesScreen::default();
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut settings = crate::settings::Settings::default();
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let mut pass = || -> Vec<String> {
            let mut cx = crate::screens::Cx {
                data: Some(data),
                railed: false,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                player: Default::default(),
                stage: None,
                demand: None,
                ask: Default::default(),
                chat: crate::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(1600.0, 20000.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();
            fn flat(sh: egui::Shape, out: &mut Vec<egui::Shape>) {
                match sh {
                    egui::Shape::Vec(v) => {
                        for s in v {
                            flat(s, out);
                        }
                    }
                    other => out.push(other),
                }
            }
            let mut all = Vec::new();
            for cs in shapes {
                flat(cs.shape, &mut all);
            }
            all.iter()
                .filter_map(|sh| match sh {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect()
        };
        /* one frame to take the jump and select the zone, a second to draw the detail pane */
        jump_to("Lesser Faydark");
        let _ = pass();
        let words = pass();
        let has = |want: &str| words.iter().any(|w| w == want);

        assert!(
            words.iter().any(|w| w.contains("from kills-data.json")),
            "the frame did not reach the MOBS section, so nothing below it proves anything: {:?}",
            words.iter().take(30).collect::<Vec<_>>()
        );
        assert!(
            has("a brownie merchant"),
            "the MERCHANTS section never painted the shop the goal names"
        );
        assert!(
            words
                .iter()
                .any(|w| w.starts_with("Prices, /loc and faction deltas")),
            "the MERCHANTS section did not paint its provenance line: {:?}",
            words.iter().filter(|w| w.len() > 40).collect::<Vec<_>>()
        );
        assert!(
            words
                .iter()
                .any(|w| w.starts_with("raises and lowers are factions.json")),
            "the FACTIONS section did not paint"
        );
        assert!(
            words
                .iter()
                .any(|w| w.contains(" merchants, ") && w.contains(" stock lines")),
            "the merchant count line did not paint: {:?}",
            words
                .iter()
                .filter(|w| w.contains("merchant"))
                .collect::<Vec<_>>()
        );
    }
}
