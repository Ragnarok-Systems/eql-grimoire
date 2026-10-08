//! Screen: the Compendium. Zones, NPCs, items, quests and spells from the Monsters & Memories
//! community wiki (CC BY-SA 4.0), browsed offline.
//!
//! ONE SCREEN, FIVE KINDS. Each rail row is a kind; the screen is the same shape for all of them: a
//! searchable list on the left, the record on the right. Every name a record mentions that is also a
//! record (a zone's neighbours, an item's drop sources, a quest's giver) is a link, and a link to
//! another kind goes there through `Ask::ShowRecord`, so the rail follows.
//!
//! THE CREDIT IS NOT OPTIONAL. The licence asks for attribution wherever the data appears, so every
//! record ends with the line naming the wiki and a link to the page it came from.

use crate::lore::{value_text, Kind, Lore, Record};
use crate::screens::{Ask, Cx};
use crate::theme::*;
use egui::{FontId, RichText, Ui, Vec2};
use serde_json::Value;
use std::collections::HashMap;

/// How a field is drawn: plain text, or a list of names that link to records of a kind.
#[derive(Clone, Copy)]
enum Show {
    Text,
    Links(Kind),
}

/// The fields drawn for each kind, in order: label, JSON pointer into the record, and how.
fn spec(kind: Kind) -> &'static [(&'static str, &'static str, Show)] {
    use Show::*;
    match kind {
        Kind::Zone => &[
            ("Levels", "/level_text", Text),
            ("City", "/city", Text),
            ("Borders", "/adjacent", Links(Kind::Zone)),
            ("Monsters", "/monster_types", Text),
            ("Notable NPCs", "/notable_npcs", Links(Kind::Npc)),
            ("Quests", "/quests", Links(Kind::Quest)),
            ("Items", "/items", Links(Kind::Item)),
            ("NPCs", "/npcs", Links(Kind::Npc)),
        ],
        Kind::Npc => &[
            ("Found in", "/zones", Links(Kind::Zone)),
            ("Location", "/location", Text),
            ("Race", "/race", Text),
            ("Class", "/class", Text),
            ("Level", "/level_text", Text),
            ("HP", "/hp", Text),
            ("Respawn", "/respawn", Text),
            ("Factions", "/factions", Text),
            ("Opposing factions", "/opposing_factions", Text),
            ("Quests", "/quests", Links(Kind::Quest)),
            ("Merchant", "/merchant", Text),
        ],
        Kind::Item => &[
            ("Slot", "/slot", Text),
            ("Stats", "/stats", Text),
            ("Flags", "/flags", Text),
            ("Classes", "/classes", Text),
            ("Races", "/races", Text),
            ("Skill", "/skill", Text),
            ("Handed", "/handed", Text),
            ("Effects", "/effects", Text),
            ("Weight", "/weight", Text),
            ("Size", "/size", Text),
            ("Dropped in", "/drops/zones", Links(Kind::Zone)),
            ("Dropped by", "/drops/mobs", Links(Kind::Npc)),
            ("Notes", "/notes", Text),
        ],
        Kind::Quest => &[
            ("Given by", "/giver", Links(Kind::Npc)),
            ("Starts in", "/zone", Links(Kind::Zone)),
            ("Minimum level", "/min_level", Text),
            ("Classes", "/classes", Text),
            ("Races", "/races", Text),
            ("NPCs involved", "/npcs", Links(Kind::Npc)),
            ("Items involved", "/items", Links(Kind::Item)),
            ("Rewards", "/rewards", Links(Kind::Item)),
        ],
        Kind::Spell => &[
            ("Classes", "/classes", Text),
            ("Skill", "/skill", Text),
            ("Mana", "/mana", Text),
            ("Casting time", "/casting_time", Text),
            ("Duration", "/fields/duration", Text),
            ("Effects", "/fields/slots", Text),
            ("Notes", "/notes", Text),
        ],
    }
}

/// The longer prose a record may carry, drawn under the fields.
const PROSE: &[&str] = &["/summary", "/description"];

/// Spell classes arrive as `[{class, level}]`; drawn as "Necromancer 1, Shadow Knight 8".
fn class_levels(v: &Value) -> Option<String> {
    let a = v.as_array()?;
    let parts: Vec<String> = a
        .iter()
        .filter_map(|c| {
            let class = c.get("class")?.as_str()?;
            Some(match c.get("level").and_then(value_text) {
                Some(l) => format!("{class} {l}"),
                None => class.to_owned(),
            })
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// The value at a JSON pointer (`/drops/zones`) into a record's fields.
fn at<'r>(r: &'r Record, ptr: &str) -> Option<&'r Value> {
    let mut parts = ptr.trim_start_matches('/').split('/');
    let mut v = r.fields.get(parts.next()?)?;
    for p in parts {
        v = v.get(p)?;
    }
    Some(v)
}

/// A field's names as a list, whether the bundle gave a list or one string.
fn names(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter().filter_map(value_text).collect(),
        other => value_text(other).into_iter().collect(),
    }
}

#[derive(Default)]
struct KindState {
    query: String,
    selected: Option<usize>,
}

/// The Compendium's state: a search and a selection per kind.
#[derive(Default)]
pub struct CompendiumScreen {
    state: HashMap<Kind, KindState>,
    /// A jump asked for before the data was loaded, applied once it is.
    pending: Option<(Kind, String)>,
}

impl CompendiumScreen {
    /// Select the record named `name` in `kind` (from a link or another screen).
    pub fn jump(&mut self, kind: Kind, name: &str, lore: Option<&Lore>) {
        match lore.and_then(|l| l.find_in(kind, name)) {
            Some(i) => {
                let st = self.state.entry(kind).or_default();
                st.selected = Some(i);
                st.query.clear();
            }
            None => self.pending = Some((kind, name.to_owned())),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx, kind: Kind) {
        let Some(lore) = cx.lore.lore else {
            not_here(ui, cx, kind);
            return;
        };
        if let Some((k, name)) = self.pending.take() {
            self.jump(k, &name, Some(lore));
        }
        let st = self.state.entry(kind).or_default();
        let records = lore.records(kind);

        let avail = ui.available_size();
        ui.horizontal_top(|ui| {
            /* The list. */
            ui.vertical(|ui| {
                ui.set_width(280.0_f32.min(avail.x * 0.4));
                ui.add(
                    egui::TextEdit::singleline(&mut st.query)
                        .hint_text(format!("search {}", kind.plural()))
                        .desired_width(f32::INFINITY),
                );
                let q = st.query.trim().to_lowercase();
                let hits: Vec<usize> = records
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| q.is_empty() || r.fold.contains(&q))
                    .map(|(i, _)| i)
                    .collect();
                ui.label(
                    RichText::new(format!(
                        "{} of {} {}",
                        hits.len(),
                        records.len(),
                        kind.plural()
                    ))
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
                );
                ui.add_space(4.0);
                let row_h = 20.0;
                egui::ScrollArea::vertical()
                    .id_salt(("compendium-list", kind))
                    .auto_shrink([false, false])
                    .max_height(avail.y - 60.0)
                    .show_rows(ui, row_h, hits.len(), |ui, range| {
                        for &i in &hits[range] {
                            let on = st.selected == Some(i);
                            let txt = RichText::new(&records[i].name).color(if on {
                                GOLD_HI
                            } else {
                                TEXT
                            });
                            if ui
                                .add(egui::Button::selectable(on, txt).frame(false))
                                .clicked()
                            {
                                st.selected = Some(i);
                            }
                        }
                    });
            });
            ui.separator();

            /* The record. */
            ui.vertical(|ui| match st.selected.and_then(|i| records.get(i)) {
                Some(r) => {
                    egui::ScrollArea::vertical()
                        .id_salt(("compendium-record", kind))
                        .auto_shrink([false, false])
                        .show(ui, |ui| record(ui, cx, lore, kind, r));
                }
                None => {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!(
                            "Pick a {} from the list, or search for one.",
                            kind.noun()
                        ))
                        .color(TEXT_2),
                    );
                    credit(ui, lore, None);
                }
            });
        });
    }
}

/// One record, its fields, its prose and its credit.
fn record(ui: &mut Ui, cx: &mut Cx, lore: &Lore, kind: Kind, r: &Record) {
    ui.label(
        RichText::new(&r.name)
            .font(crate::fonts::display(20.0))
            .color(GOLD_HI),
    );
    ui.add_space(8.0);
    egui::Grid::new(("compendium-fields", kind))
        .num_columns(2)
        .spacing(Vec2::new(14.0, 6.0))
        .striped(false)
        .show(ui, |ui| {
            for (label, ptr, show) in spec(kind) {
                let Some(v) = at(r, ptr) else { continue };
                let shown = match show {
                    Show::Text => {
                        let text = if *ptr == "/classes" && kind == Kind::Spell {
                            class_levels(v)
                        } else {
                            value_text(v)
                        };
                        let Some(text) = text else { continue };
                        field_label(ui, label);
                        ui.label(RichText::new(text).color(TEXT));
                        true
                    }
                    Show::Links(to) => {
                        let list = names(v);
                        if list.is_empty() {
                            continue;
                        }
                        field_label(ui, label);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let n = list.len();
                            for (i, name) in list.iter().enumerate() {
                                let sep = if i + 1 < n { "," } else { "" };
                                if lore.find_in(*to, name).is_some() {
                                    if ui
                                        .link(RichText::new(format!("{name}{sep}")).color(GOLD))
                                        .clicked()
                                    {
                                        cx.ask = Ask::ShowRecord(*to, name.clone());
                                    }
                                } else {
                                    ui.label(RichText::new(format!("{name}{sep}")).color(TEXT_2));
                                }
                            }
                        });
                        true
                    }
                };
                if shown {
                    ui.end_row();
                }
            }
        });
    for ptr in PROSE {
        if let Some(text) = at(r, ptr).and_then(value_text) {
            ui.add_space(10.0);
            ui.label(RichText::new(text).color(TEXT_2));
        }
    }
    credit(ui, lore, r.url.as_deref());
}

fn field_label(ui: &mut Ui, label: &str) {
    ui.label(
        RichText::new(label)
            .font(FontId::proportional(12.0))
            .color(TEXT_3),
    );
}

/// The attribution the licence asks for, and the way to the source page.
fn credit(ui: &mut Ui, lore: &Lore, url: Option<&str>) {
    ui.add_space(14.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!(
                "From the Monsters & Memories community wiki, {}. Data of {}.",
                if lore.licence.is_empty() {
                    "CC BY-SA 4.0"
                } else {
                    &lore.licence
                },
                lore.data_version
            ))
            .font(FontId::proportional(11.0))
            .color(TEXT_3),
        );
        if let Some(u) = url {
            if ui
                .link(
                    RichText::new("Open the wiki page")
                        .font(FontId::proportional(11.0))
                        .color(GOLD),
                )
                .clicked()
            {
                crate::shell::open_url_quietly(u);
            }
        }
    });
}

/// No data yet: why, and where it was looked for.
fn not_here(ui: &mut Ui, cx: &Cx, kind: Kind) {
    ui.label(
        RichText::new(kind.plural().to_uppercase())
            .font(crate::fonts::display(16.0))
            .color(GOLD_HI),
    );
    ui.add_space(8.0);
    if cx.lore.loading {
        ui.label(RichText::new("Loading the wiki data...").color(TEXT_2));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
        return;
    }
    let why = cx.lore.why.unwrap_or("The wiki data is not loaded.");
    ui.label(RichText::new(why).color(TEXT_2));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_link_field_points_at_a_kind_and_spell_classes_read_as_levels() {
        for kind in Kind::ALL {
            assert!(!spec(kind).is_empty(), "{kind:?} draws nothing");
            for (label, ptr, _) in spec(kind) {
                assert!(ptr.starts_with('/'), "{label}: a JSON pointer");
            }
        }
        let v = serde_json::json!([{"class": "Necromancer", "level": 1}, {"class": "Shadow Knight", "level": 8}]);
        assert_eq!(
            class_levels(&v).as_deref(),
            Some("Necromancer 1, Shadow Knight 8")
        );
        assert_eq!(
            names(&serde_json::json!("Guard Pippi")),
            vec!["Guard Pippi"]
        );
        assert_eq!(names(&serde_json::json!(["a", "b"])), vec!["a", "b"]);
    }
}
