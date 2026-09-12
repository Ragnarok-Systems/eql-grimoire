//! FIND: Spells. spells.json, the wiki's `Category:Spells` at 2001 pages. Decisions D5 and D6.
//!
//! WHY THIS SCREEN EXISTS. Its rail row shipped for weeks with no screen behind it and a page that
//! said "The snapshot has no spell records ... A spell list needs a spell source first." That was
//! true and it is not any more, so the page had to go with the absence. Nothing else in this crate
//! draws a spell: the gear catalogue's `effects` map is the item-proc slice, twenty-seven percent
//! of the pages and none of the classes or vendors.
//!
//! WHAT THE LIST ANSWERS, in the order a caster asks. Which spell (the name), who casts it and at
//! what level (the class line and the lowest level on it), and what kind of thing it is. The
//! detail pane then answers where to get it, which is the question the old `effects` map could
//! never answer at all: the wiki's vendor table is (zone, vendor, area, /loc) and it is here.
//!
//! CROSS SCREEN. `items_with_effect` names items this build already has, so each one is a link
//! into FIND / Items, and the ones gear-data does not carry say so instead of being drawn as a
//! dead link. Vendor zones link into FIND / Zones the same way, and only when Zones can land on
//! them.
//!
//! No em dashes and no en dashes anywhere in this module, by house rule.

use crate::data::spells::Spell;
use crate::screens::items::{
    count_line, detail_rows, dim, head_row, kv_table, link, list_named, list_row, mono, no_data,
    pane_section, parse_query, provenance, search_box, snapshot_key, view, wrong_bar, Col,
    DetailRows, SnapKey, View, ROW_H,
};
use crate::screens::zones::{zone_key, zone_keys};
use crate::theme::*;
use egui::{FontId, Ui};
use std::collections::HashSet;
use std::sync::Mutex;

/* ------------------------------------------------------------- cross-screen jumps -- */

/// The find box and any screen that names a spell ask this one to select it. The selection waits
/// here until this screen draws; the nav switch travels on `Cx.ask`, exactly as Items, Zones,
/// Drops and Quests do it.
static JUMP: Mutex<Option<String>> = Mutex::new(None);

pub fn jump_to(spell: &str) {
    *JUMP.lock().unwrap_or_else(|p| p.into_inner()) = Some(spell.to_owned());
}

fn take_jump() -> Option<String> {
    JUMP.lock().unwrap_or_else(|p| p.into_inner()).take()
}

/* ------------------------------------------------------------------- pure rules -- */

/// The level cell: the lowest level any class gets the spell at, or the reason there is none.
/// A spell with no class at all is not a blank cell, because a blank cell reads as a scrape that
/// lost the number rather than a page that never had one.
pub(crate) fn level_cell(s: &Spell) -> String {
    match s.min_level() {
        Some(l) if l.fract() == 0.0 => format!("{}", l as i64),
        Some(l) => l.to_string(),
        None if s.npc_only() => "npc".to_string(),
        None => "-".to_string(),
    }
}

/// The casting block, label by label, in the order a caster reads one: what skill, at what, what
/// it costs, how long it takes, how far, how long it lasts, which gem.
///
/// WHY THIS IS HAND BUILT AND NOT THE SERDE TABLE. The pane also draws `detail_rows`, which walks
/// the record through serde and needs no code per field, and that is exactly the problem: a field
/// reached only that way is never NAMED in this crate, so `reach.rs`'s field floor cannot see a
/// reader for it and the compiler cannot either. It said so. `rng`, `ft`, `rt`, `tgt` and `icon`
/// were in the file, in the struct, in the preferred order, and read by nothing that a reader
/// could point at. They are read here, under the labels the wiki page uses, which is also the only
/// version of this block a person can scan.
pub(crate) fn casting_rows(s: &Spell) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut put = |label: &str, v: Option<&String>| {
        if let Some(v) = v {
            if !v.is_empty() {
                out.push((label.to_owned(), v.clone()));
            }
        }
    };
    put("skill", s.skill.as_ref());
    put("target", s.tgt.as_ref());
    put("resist", s.res.as_ref());
    put("mana", s.mana.as_ref());
    put("cast", s.ct.as_ref());
    put("recast", s.rt.as_ref());
    put("fizzle", s.ft.as_ref());
    put("range", s.rng.as_ref());
    put("duration", s.dur.as_ref());
    put("gem", s.icon.as_ref());
    out
}

/// The class cell, cut to fit: the first two classes, then "+N". The full line is in the pane.
pub(crate) fn class_cell(s: &Spell) -> String {
    let n = s.classes.len();
    if n == 0 {
        return String::new();
    }
    let shown: Vec<&str> = s.classes.iter().take(2).map(|(c, _)| c.as_str()).collect();
    if n > shown.len() {
        format!("{} +{}", shown.join(", "), n - shown.len())
    } else {
        shown.join(", ")
    }
}

/* -------------------------------------------------------------------- the screen -- */

/// What the list column needs per row, computed once per snapshot rather than per frame.
#[derive(Clone, Debug)]
struct SpellFacts {
    lname: String,
    classes: String,
    level: String,
    kind: String,
}

struct Cache {
    key: SnapKey,
    facts: Vec<SpellFacts>,
    /// Every zone_key a Zone answers to, so a vendor's zone is a link only when Zones can land on
    /// it, and plain text with a reason when it cannot.
    zone_keys: HashSet<String>,
}

struct Hits {
    needle: String,
    idx: Vec<usize>,
}

struct Detail {
    idx: usize,
    rows: DetailRows,
    /// For each `items_with_effect` name, the gear-data display spelling when gear-data has it.
    /// None means this build carries no item under that name and the row says so.
    items: Vec<(String, Option<String>)>,
    /// merchants.json's answer to "who sells the scroll", under the two prefixes the stock lists
    /// use. See [`scroll_names`].
    scroll_sellers: Vec<crate::screens::items::SellerRow>,
    err: Option<String>,
}

/// The names a merchant's stock list uses for this spell's scroll.
///
/// SPELL PAGES AND STOCK LINES DO NOT SPELL THE SAME THING THE SAME WAY. A spell page is titled
/// "Gate"; the line on a merchant reads "Spell: Gate", and a bard's reads "Song: Cassindra's
/// Chorus of Clarity". Both prefixes are tried, and the spell's own name is tried too because a
/// stock list is the wiki and the wiki is not consistent. Nothing is invented here: these are the
/// two prefixes measured in the file, and a name that matches nothing simply returns nothing.
fn scroll_names(name: &str) -> [String; 3] {
    [
        format!("Spell: {name}"),
        format!("Song: {name}"),
        name.to_owned(),
    ]
}

#[derive(Default)]
pub struct SpellsScreen {
    query: String,
    sel: Option<usize>,
    cache: Option<Cache>,
    hits: Option<Hits>,
    detail: Option<Detail>,
    notice: Option<String>,
}

/// `Spell` serialises `key` and then every field of the template. The heading takes `n`, the
/// pane draws the lists and the stat block itself, so the typed table is the leftovers in a
/// reading order rather than serde's.
const PREFERRED: &[&str] = &[
    "key", "t", "era", "type", "skill", "tgt", "res", "mana", "ct", "rt", "ft", "rng", "dur",
    "icon", "dup",
];
/// The arrays and long strings the pane draws properly below; the typed table only counts them.
const EXPANDED: &[&str] = &["cls", "s", "obtain", "vend", "items", "note", "d"];

impl SpellsScreen {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut crate::screens::Cx<'_>) {
        let Some(data) = cx.data else {
            no_data(ui, cx.data_err, cx.settings.data_root.as_deref(), "spell");
            return;
        };
        self.refresh(data);
        if let Some(name) = take_jump() {
            let want = name.to_lowercase();
            match self
                .cache
                .as_ref()
                .and_then(|c| c.facts.iter().position(|f| f.lname == want))
            {
                Some(i) => {
                    self.sel = Some(i);
                    self.query.clear();
                    self.notice = None;
                }
                None => {
                    self.notice = Some(format!(
                        "No spell named \"{name}\" in spells.json ({} pages). The name came from another file.",
                        data.spells.len()
                    ))
                }
            }
        }
        let ask = egui::Panel::right("spells_detail")
            .default_size(460.0)
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
        if let Some(a) = ask {
            cx.ask = a;
        }
    }

    fn refresh(&mut self, data: &crate::data::Snapshot) {
        let key = snapshot_key(data);
        if self.cache.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let facts: Vec<SpellFacts> = data
            .spells
            .iter()
            .map(|s| SpellFacts {
                lname: s.name.to_lowercase(),
                classes: class_cell(s),
                level: level_cell(s),
                kind: s.spell_type.clone().unwrap_or_default(),
            })
            .collect();
        let zone_keys: HashSet<String> = data.zones.iter().flat_map(zone_keys).collect();
        self.cache = Some(Cache {
            key,
            facts,
            zone_keys,
        });
        self.hits = None;
        self.detail = None;
        self.sel = None;
    }

    fn hits(&mut self, needle: &str, data: &crate::data::Snapshot) -> &[usize] {
        if self.hits.as_ref().map_or(true, |h| h.needle != needle) {
            let idx = data
                .spells
                .iter()
                .enumerate()
                .filter(|(_, s)| needle.is_empty() || s.matches(needle))
                .map(|(i, _)| i)
                .collect();
            self.hits = Some(Hits {
                needle: needle.to_owned(),
                idx,
            });
        }
        self.hits.as_ref().map(|h| h.idx.as_slice()).unwrap_or(&[])
    }

    fn list_pane(&mut self, ui: &mut Ui, data: &crate::data::Snapshot) {
        let total = data.spells.len();
        let q = parse_query(&self.query, 'p');
        let root = data.report().root;
        ui.horizontal(|ui| {
            search_box(ui, &mut self.query, "spell name or class, substring");
            let shown = self.hits(&q.needle, data).len();
            count_line(ui, shown, total, "spells");
        });
        if let Some(p) = q.foreign_prefix {
            dim(
                ui,
                &format!(
                    "The prefix {p}: names the {} list. Searching spells for the rest.",
                    list_named(p)
                ),
            );
        }
        provenance(
            ui,
            &root,
            crate::data::SPELLS_FILE,
            total,
            "wiki spell pages",
        );
        if let Some(n) = self.notice.clone() {
            wrong_bar(ui, &n);
        }
        ui.add_space(6.0);
        if total == 0 {
            dim(
                ui,
                "No spells.json in this snapshot root, so there are no spells to list. The file is \
                 the wiki's Category:Spells and it belongs beside gear-data.json in the directory \
                 named above.",
            );
            return;
        }
        let hits: Vec<usize> = self.hits(&q.needle, data).to_vec();
        if hits.is_empty() {
            dim(ui, "No spell name or class contains that.");
            return;
        }
        head_row(
            ui,
            &[
                ("spell", false, 0.0),
                ("classes", false, 150.0),
                ("lvl", true, 40.0),
                ("kind", false, 120.0),
            ],
        );
        let mut clicked: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("spells_list")
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, hits.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in range {
                    let i = hits[row];
                    let Some(c) = &self.cache else { break };
                    let f = &c.facts[i];
                    let name = &data.spells[i].name;
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
                            text: &f.classes,
                            mono: false,
                            color: TEXT_2,
                            right: false,
                            width: 150.0,
                        },
                        Col {
                            text: &f.level,
                            mono: true,
                            color: TEXT_2,
                            right: true,
                            width: 40.0,
                        },
                        Col {
                            text: &f.kind,
                            mono: false,
                            color: TEXT_3,
                            right: false,
                            width: 120.0,
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
                "Select a spell to see its effects, its classes and where to get it.",
            );
            return None;
        };
        if self.detail.as_ref().map_or(true, |d| d.idx != i) {
            self.detail = Some(self.build_detail(i, data));
        }
        let d = self.detail.as_ref()?;
        let sp = &data.spells[i];
        let known_zone = |z: &str| {
            self.cache
                .as_ref()
                .is_some_and(|c| c.zone_keys.contains(&zone_key(z)))
        };
        let mut ask: Option<crate::screens::Ask> = None;
        egui::ScrollArea::vertical()
            .id_salt("spells_detail_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(&sp.name)
                        .font(crate::fonts::display(18.0))
                        .color(GOLD_HI),
                );
                ui.horizontal(|ui| {
                    mono(ui, &sp.t, TEXT_3);
                    if let Some(e) = &sp.era {
                        mono(ui, e, TEXT_3);
                    }
                    if sp.npc_only() {
                        mono(ui, "NPC only", GOLD_DIM);
                    }
                });
                if let Some(desc) = &sp.desc {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(desc)
                            .font(FontId::proportional(12.5))
                            .color(TEXT),
                    );
                }
                if let Some(e) = &d.err {
                    wrong_bar(ui, e);
                }

                pane_section(ui, "CAST BY");
                let line = sp.class_line();
                if line.is_empty() {
                    dim(ui, "The wiki page lists no class for this spell.");
                } else {
                    mono(ui, &line, TEXT);
                }
                for n in &sp.note {
                    dim(ui, n);
                }

                pane_section(ui, "EFFECTS");
                if sp.slots.is_empty() {
                    dim(ui, "The page's slot table is empty.");
                }
                for s in &sp.slots {
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        mono(ui, s, TEXT);
                    });
                }
                if let Some(f) = &sp.focus {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        dim(ui, "focus");
                        mono(ui, f, TEXT_2);
                    });
                }

                pane_section(ui, "CASTING");
                let cast = casting_rows(sp);
                if cast.is_empty() {
                    dim(ui, "The page carries no casting numbers at all.");
                } else {
                    kv_table(ui, "spells_casting", &cast);
                }

                pane_section(ui, "WHERE TO GET IT");
                let vendors = sp.vendors();
                if vendors.is_empty() && sp.obtain.is_empty() {
                    dim(ui, "The page's where_to_obtain field is empty.");
                }
                for g in sp.obtain_groups() {
                    if !g.head.is_empty() {
                        ui.horizontal(|ui| {
                            if known_zone(g.head) {
                                if link(ui, g.head, FontId::proportional(12.5))
                                    .on_hover_text("open in Zones")
                                    .clicked()
                                {
                                    crate::screens::zones::jump_to(g.head);
                                    ask = Some(crate::screens::Ask::ShowZone(g.head.to_owned()));
                                }
                            } else {
                                mono(ui, g.head, TEXT_2);
                            }
                        });
                    }
                    for l in &g.lines {
                        ui.horizontal(|ui| {
                            ui.add_space(if g.head.is_empty() { 0.0 } else { 14.0 });
                            mono(ui, l, TEXT);
                        });
                    }
                }
                for v in &vendors {
                    ui.horizontal(|ui| {
                        if known_zone(v.zone) {
                            if link(ui, v.zone, FontId::proportional(12.5))
                                .on_hover_text("open in Zones")
                                .clicked()
                            {
                                crate::screens::zones::jump_to(v.zone);
                                ask = Some(crate::screens::Ask::ShowZone(v.zone.to_owned()));
                            }
                        } else {
                            mono(ui, v.zone, TEXT_2);
                        }
                        mono(ui, v.who, TEXT);
                        if !v.area.is_empty() {
                            mono(ui, v.area, TEXT_3);
                        }
                        if !v.loc.is_empty() {
                            mono(ui, v.loc, TEXT_3);
                        }
                    });
                }

                /* WHO SELLS THE SCROLL, from merchants.json. The section above is the spell page's
                 * OWN where_to_obtain table, which names a zone and a vendor and no price; this is
                 * the merchant's stock line, which names the price. Two sources, both drawn, each
                 * under its own heading, because a merged list would hide which file said what. */
                if let Some(a) = crate::screens::items::sold_by_section(
                    ui,
                    &d.scroll_sellers,
                    false,
                    "No merchant's stock list in merchants.json carries this spell's scroll, under \
                     \"Spell: \", \"Song: \" or the bare name.",
                ) {
                    ask = Some(a);
                }

                pane_section(ui, "ITEMS THAT CAST IT");
                if d.items.is_empty() {
                    dim(
                        ui,
                        "No item on the wiki carries this spell as a click or a proc.",
                    );
                }
                for (name, in_items) in &d.items {
                    ui.horizontal(|ui| match in_items {
                        Some(display) => {
                            if link(ui, display, FontId::proportional(12.5))
                                .on_hover_text("open in Items")
                                .clicked()
                            {
                                crate::screens::items::jump_to(display);
                                ask = Some(crate::screens::Ask::ShowItem(display.clone()));
                            }
                        }
                        None => {
                            mono(ui, name, TEXT_2);
                            dim(ui, "not in gear-data.json under this name");
                        }
                    });
                }

                if let Some(other) = &sp.dup {
                    pane_section(ui, "ALSO ON THE WIKI");
                    dim(
                        ui,
                        &format!(
                            "The wiki has a second live page for this spell, \"{other}\", with its \
                             own numbers. The longer of the two is the one shown here.",
                        ),
                    );
                }

                pane_section(ui, "THE RECORD");
                kv_table(ui, "spells_typed", &d.rows.typed);

                pane_section(ui, "MORE");
                if d.rows.more.is_empty() {
                    dim(
                        ui,
                        "Nothing beyond the typed fields: extra is empty for this record.",
                    );
                } else {
                    kv_table(ui, "spells_more", &d.rows.more);
                }
            });
        ask
    }

    fn build_detail(&self, i: usize, data: &crate::data::Snapshot) -> Detail {
        let sp = &data.spells[i];
        let items: Vec<(String, Option<String>)> = sp
            .items
            .iter()
            .map(|n| (n.clone(), data.item(n).map(|it| it.name.clone())))
            .collect();
        let mut scroll_sellers: Vec<crate::screens::items::SellerRow> = Vec::new();
        for n in scroll_names(&sp.name) {
            for row in crate::screens::items::seller_rows(data, &n) {
                /* the three names can find the same line twice ("Gate" and "Spell: Gate" are two
                 * keys and a merchant may carry both spellings); one row per source line */
                if !scroll_sellers
                    .iter()
                    .any(|r| r.raw == row.raw && r.who == row.who)
                {
                    scroll_sellers.push(row);
                }
            }
        }
        scroll_sellers.sort_by(|a, b| a.zone.cmp(&b.zone).then_with(|| a.who.cmp(&b.who)));
        match view(sp) {
            Ok(v) => Detail {
                idx: i,
                rows: detail_rows(&sp.name, &v, &sp.extra, PREFERRED, EXPANDED),
                items,
                scroll_sellers,
                err: None,
            },
            Err(e) => Detail {
                idx: i,
                rows: detail_rows(&sp.name, &View::new(), &sp.extra, PREFERRED, &[]),
                items,
                scroll_sellers,
                err: Some(e),
            },
        }
    }
}

/* ------------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    fn spell(json: &str) -> Spell {
        serde_json::from_str(json).expect("a Spell")
    }

    #[test]
    fn the_level_cell_says_which_kind_of_no_level_it_is() {
        assert_eq!(
            level_cell(&spell(r#"{"n":"X","cls":[["Druid",14]]}"#)),
            "14"
        );
        assert_eq!(
            level_cell(&spell(r#"{"n":"X","cls":[["Ranger",22],["Druid",14]]}"#)),
            "14",
            "the lowest, not the first"
        );
        assert_eq!(
            level_cell(&spell(r#"{"n":"X","npc":1}"#)),
            "npc",
            "an npc only spell has no player level and says so"
        );
        assert_eq!(
            level_cell(&spell(r#"{"n":"X"}"#)),
            "-",
            "a page with no class is a dash, never an empty cell"
        );
    }

    #[test]
    fn the_class_cell_shows_two_and_counts_the_rest() {
        assert_eq!(class_cell(&spell(r#"{"n":"X"}"#)), "");
        assert_eq!(
            class_cell(&spell(r#"{"n":"X","cls":[["Druid",14]]}"#)),
            "Druid"
        );
        assert_eq!(
            class_cell(&spell(r#"{"n":"X","cls":[["Druid",1],["Cleric",1]]}"#)),
            "Druid, Cleric"
        );
        assert_eq!(
            class_cell(&spell(
                r#"{"n":"X","cls":[["Druid",1],["Cleric",1],["Bard",2],["Wizard",3]]}"#
            )),
            "Druid, Cleric +2"
        );
    }

    #[test]
    fn the_typed_table_leaves_the_lists_to_the_pane_and_never_repeats_the_heading() {
        let sp = spell(
            r#"{"n":"Lava Storm","t":"Lava_Storm","era":"Classic Era","d":"Calls down lava.",
                "cls":[["Wizard",32]],"s":["Decrease Hitpoints by 401"],"mana":"234",
                "type":"Detrimental","odd":7}"#,
        );
        let v = view(&sp).expect("serialises");
        let r = detail_rows(&sp.name, &v, &sp.extra, PREFERRED, EXPANDED);
        let keys: Vec<&str> = r.typed.iter().map(|(k, _)| k.as_str()).collect();
        assert!(!keys.contains(&"n"), "the heading is never also a row");
        assert_eq!(
            &keys[..4],
            &["key", "t", "era", "type"],
            "the preferred order leads"
        );
        let d_row = r
            .typed
            .iter()
            .find(|(k, _)| k == "d")
            .map(|(_, v)| v.as_str());
        assert_eq!(d_row, Some("Calls down lava."));
        let cls = r
            .typed
            .iter()
            .find(|(k, _)| k == "cls")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            cls,
            Some("1 entries, listed below"),
            "the pane draws the class line, the table only counts it"
        );
        assert_eq!(r.more, vec![("odd".to_owned(), "7".to_owned())]);
    }

    #[test]
    fn the_casting_block_is_labelled_in_reading_order_and_skips_what_the_page_lacks() {
        let sp = spell(
            r#"{"n":"Lava Storm","skill":"Evocation","tgt":"Targeted AE","res":"Fire (0)",
                "mana":"234","ct":"5.00","rt":"12.00","ft":"2.50","rng":"150","dur":"Instant",
                "icon":"O"}"#,
        );
        let rows = casting_rows(&sp);
        let labels: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "skill", "target", "resist", "mana", "cast", "recast", "fizzle", "range",
                "duration", "gem"
            ]
        );
        assert_eq!(rows[3].1, "234");
        assert_eq!(rows[7].1, "150", "range is the wiki's text, not a number");
        /* A page with none of them draws no rows rather than ten empty ones. */
        assert!(casting_rows(&spell(r#"{"n":"X"}"#)).is_empty());
        /* And a field the wiki left as an empty string is absent, not a blank row. */
        let blank = spell(r#"{"n":"X","mana":"","skill":"Alteration"}"#);
        assert_eq!(
            casting_rows(&blank),
            vec![("skill".to_owned(), "Alteration".to_owned())]
        );
    }

    /* ---- the real file ---- */

    #[test]
    fn the_real_file_fills_every_list_column() {
        let Some(s) = crate::data::testdata::snapshot() else {
            return;
        };
        assert_eq!(s.spells.len(), 2001);
        /* No row is blank in the two columns a reader scans: the level cell always says
         * something, and the kind column is the wiki's own on all but eleven pages. */
        assert!(s.spells.iter().all(|p| !level_cell(p).is_empty()));
        let kinds = s.spells.iter().filter(|p| p.spell_type.is_some()).count();
        assert_eq!(kinds, 1990);
        /* A spell every caster knows, end to end through the screen's own cells. */
        let ig = s.spell("Ignite").expect("Ignite");
        assert_eq!(class_cell(ig), "Druid, Ranger");
        assert_eq!(level_cell(ig), "8");
        assert_eq!(ig.spell_type.as_deref(), Some("Detrimental"));
        assert!(ig.items.iter().any(|i| i == "Burning Rapier"));
        /* and the item it names is really in gear-data, which is what makes the link a link */
        assert!(s.item("Burning Rapier").is_some());
        /* the vendor table resolves to real zones the Zones screen can land on */
        let aq = s.spell("Aanya's Quickening").expect("Aanya's Quickening");
        let v = aq.vendors();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].zone, "Firiona Vie");
        assert!(!v[0].loc.is_empty());
        let keys: HashSet<String> = s.zones.iter().flat_map(zone_keys).collect();
        assert!(
            keys.contains(&zone_key(v[0].zone)),
            "a vendor zone the Zones screen knows is what makes the link honest"
        );
    }

    #[test]
    fn the_screen_draws_headless_and_the_jump_lands() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let mut settings = crate::settings::Settings::default();
        let snap = crate::data::testdata::snapshot();
        settings.data_root = snap.map(|s| s.report().root);
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked("Broken_Stoic"),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut screen = SpellsScreen::default();
        let mut passes = 0;
        for (data, err) in [(None, Some("no snapshot: smoke test")), (snap, None)] {
            let mut cx = crate::screens::Cx {
                data,
                railed: false,
                data_err: err,
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
            let out = ctx.run_ui(egui::RawInput::default(), |ui| screen.ui(ui, &mut cx));
            assert!(!out.shapes.is_empty(), "nothing was painted");
            assert_eq!(cx.ask, crate::screens::Ask::None, "nothing was clicked");
            out.drop_without_applying_deltas();
            passes += 1;
        }
        assert_eq!(passes, 2);
        let Some(data) = snap else { return };
        /* the jump another screen makes lands on the row it names, and only after a draw */
        jump_to("Ignite");
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
        ctx.run_ui(egui::RawInput::default(), |ui| screen.ui(ui, &mut cx))
            .drop_without_applying_deltas();
        assert_eq!(
            screen.sel.map(|i| data.spells[i].name.as_str()),
            Some("Ignite"),
            "the jump selected the spell it names"
        );
        assert!(screen.notice.is_none());
        /* and a jump to a name this file does not carry says so rather than selecting nothing */
        jump_to("Not A Spell On The Wiki");
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
        ctx.run_ui(egui::RawInput::default(), |ui| screen.ui(ui, &mut cx))
            .drop_without_applying_deltas();
        assert!(screen
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("Not A Spell On The Wiki")));
    }

    #[test]
    fn a_scroll_is_looked_for_under_both_wiki_prefixes_and_the_bare_name() {
        assert_eq!(
            scroll_names("Gate"),
            [
                "Spell: Gate".to_owned(),
                "Song: Gate".to_owned(),
                "Gate".to_owned()
            ]
        );
    }

    /// THE SCROLL SELLERS, ON THE REAL FILES. The point of the section is that spells.json own
    /// where_to_obtain table names a vendor and no price, and merchants.json names the price.
    #[test]
    fn merchants_carry_spell_scrolls_and_the_prefixes_are_what_finds_them() {
        let Some(s) = crate::data::testdata::snapshot() else {
            return;
        };
        let mut with_seller = 0usize;
        let mut priced = 0usize;
        for sp in &s.spells {
            let mut found = false;
            for n in scroll_names(&sp.name) {
                for r in crate::screens::items::seller_rows(s, &n) {
                    found = true;
                    if r.price.is_some() {
                        priced += 1;
                    }
                }
            }
            if found {
                with_seller += 1;
            }
        }
        eprintln!(
            "spells with a merchant selling the scroll: {with_seller}, priced lines {priced}"
        );
        assert!(
            with_seller > 800,
            "the prefixes should reach most of the vendor sold spell list, got {with_seller}"
        );
        assert!(priced > 0);
        /* the bare name alone would not do it: "Gate" is a spell page and the stock line reads
         * "Spell: Gate", which is the whole reason scroll_names exists */
        assert!(
            crate::screens::items::seller_rows(s, "Spell: Gate").len()
                > crate::screens::items::seller_rows(s, "Gate").len(),
            "the prefixed name is what the stock lists use"
        );
    }
}
