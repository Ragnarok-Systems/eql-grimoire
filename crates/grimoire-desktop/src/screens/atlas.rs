//! Screen: the Atlas. Each zone's illustrated map with its mob camps, named mobs, trainers,
//! merchants and places pinned where they are, after the Eth-ur Atlas (mnmatlas.com), used with its
//! maker's permission.
//!
//! WHAT IT SHOWS. The bundle's zone records carry `atlas`: the stitched map image and every spot as
//! a fraction of it (so a pin lands the same at any zoom), with its kind, levels, mobs and notes.
//! Drag to pan, scroll to zoom, hover a pin for its card. Place names are drawn by the app (the
//! map image has none) once zoomed in far enough to read them.
//!
//! THE CREDIT IS NOT OPTIONAL. Every view names the atlas and links the zone on it, and each card
//! links its own spot.

use crate::lore::{Kind, Lore, Record};
use crate::screens::Cx;
use crate::theme::*;
use egui::{Color32, FontId, Pos2, Rect, RichText, Sense, Ui, Vec2};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

/// One spot on a zone's atlas map.
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// Position as a fraction of the map, 0 to 1 from the top left.
    pub x: f32,
    pub y: f32,
    pub levels: Option<(u32, Option<u32>)>,
    pub mobs: Vec<String>,
    pub note: Option<String>,
    pub approximate: bool,
}

/// A zone's atlas: its map file, its page on the atlas, and its spots.
#[derive(Clone, Debug)]
pub struct ZoneAtlas {
    pub zone: String,
    pub path: String,
    pub url: String,
    pub spots: Vec<Spot>,
}

/// The atlas of a zone record, or None when the bundle has none for it.
pub fn zone_atlas(r: &Record) -> Option<ZoneAtlas> {
    let a = r.fields.get("atlas")?;
    let path = a.get("path")?.as_str()?.to_owned();
    let url = a
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("https://www.mnmatlas.com/")
        .to_owned();
    let spots = a
        .get("places")
        .and_then(Value::as_array)
        .map(|ps| ps.iter().filter_map(spot).collect())
        .unwrap_or_default();
    Some(ZoneAtlas {
        zone: r.name.clone(),
        path,
        url,
        spots,
    })
}

fn spot(v: &Value) -> Option<Spot> {
    let n = |k: &str| v.get(k).and_then(Value::as_f64).map(|f| f as f32);
    let levels = v.get("levels").and_then(Value::as_array).and_then(|l| {
        let lo = l.first()?.as_u64()? as u32;
        Some((lo, l.get(1).and_then(Value::as_u64).map(|h| h as u32)))
    });
    Some(Spot {
        id: v.get("id")?.as_str()?.to_owned(),
        name: v.get("name")?.as_str()?.to_owned(),
        kind: v
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("Marker")
            .to_owned(),
        x: n("x")?.clamp(0.0, 1.0),
        y: n("y")?.clamp(0.0, 1.0),
        levels,
        mobs: v
            .get("mobs")
            .and_then(Value::as_array)
            .map(|m| {
                m.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        note: v
            .get("note")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned),
        approximate: v
            .get("approximate")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// "levels 10 to 15", "levels 20+", or nothing.
pub fn levels_text(l: Option<(u32, Option<u32>)>) -> Option<String> {
    let (lo, hi) = l?;
    Some(match hi {
        Some(h) if h != lo => format!("levels {lo} to {h}"),
        Some(_) => format!("level {lo}"),
        None => format!("levels {lo}+"),
    })
}

/// Kinds of spot, in the order the filters list them, with the colour each pin is drawn in. A kind
/// not listed (stairs, ladders, lifts) is drawn small and grey: it is for finding the way.
const KINDS: &[(&str, Color32)] = &[
    ("Mob camp", WRONG),
    ("Named mob", ORANGE),
    ("Notable NPC", CYAN),
    ("Quest", GOLD_HI),
    ("Class trainer", VIOLET),
    ("Vendor", SETTLED),
    ("Bank", GOLD),
    ("Inn", GOLD),
    ("Stable", GOLD),
    ("Tradeskill", SETTLED),
    ("Forge", SETTLED),
    ("Teleporter", WORKING),
    ("Exit", TEXT),
    ("Place name", TEXT_2),
];

pub fn kind_colour(kind: &str) -> Color32 {
    KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, c)| *c)
        .unwrap_or(TEXT_3)
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

/// Does a spot answer a search? Its name, its mobs, its kind or its note.
pub fn matches(s: &Spot, query: &str) -> bool {
    let q = fold(query.trim());
    q.is_empty()
        || fold(&s.name).contains(&q)
        || s.mobs.iter().any(|m| fold(m).contains(&q))
        || fold(&s.kind).contains(&q)
        || s.note.as_deref().is_some_and(|n| fold(n).contains(&q))
}

/// Pan and zoom of the map view. `zoom` 1.0 fits the whole map in the pane; `centre` is the map
/// point (as fractions) at the middle of the pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub zoom: f32,
    pub centre: Pos2,
}

impl Default for View {
    fn default() -> Self {
        View {
            zoom: 1.0,
            centre: Pos2::new(0.5, 0.5),
        }
    }
}

pub const MIN_ZOOM: f32 = 1.0;
pub const MAX_ZOOM: f32 = 8.0;

impl View {
    /// The map's rectangle on screen for a pane and a map aspect (width / height).
    pub fn map_rect(&self, pane: Rect, aspect: f32) -> Rect {
        let fit = if pane.width() / pane.height() > aspect {
            Vec2::new(pane.height() * aspect, pane.height())
        } else {
            Vec2::new(pane.width(), pane.width() / aspect)
        };
        let size = fit * self.zoom;
        let min = pane.center() - Vec2::new(self.centre.x * size.x, self.centre.y * size.y);
        Rect::from_min_size(min, size)
    }

    /// Zoom by `factor` keeping the map point under `at` (a screen point) where it is.
    pub fn zoom_at(&mut self, factor: f32, at: Pos2, pane: Rect, aspect: f32) {
        let before = self.map_rect(pane, aspect);
        let p = Pos2::new(
            (at.x - before.min.x) / before.width(),
            (at.y - before.min.y) / before.height(),
        );
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.map_rect(pane, aspect);
        // Move the centre so the point p sits under `at` again.
        let drift = Vec2::new(
            (after.min.x + p.x * after.width()) - at.x,
            (after.min.y + p.y * after.height()) - at.y,
        );
        self.centre += Vec2::new(drift.x / after.width(), drift.y / after.height());
        self.clamp();
    }

    /// Pan by a screen distance.
    pub fn pan(&mut self, delta: Vec2, pane: Rect, aspect: f32) {
        let r = self.map_rect(pane, aspect);
        self.centre -= Vec2::new(delta.x / r.width(), delta.y / r.height());
        self.clamp();
    }

    /// Centre on a spot, zoomed in enough to see around it.
    pub fn focus(&mut self, x: f32, y: f32) {
        self.zoom = self.zoom.max(3.0);
        self.centre = Pos2::new(x, y);
        self.clamp();
    }

    fn clamp(&mut self) {
        self.centre.x = self.centre.x.clamp(0.0, 1.0);
        self.centre.y = self.centre.y.clamp(0.0, 1.0);
    }
}

/// The Atlas screen's state.
pub struct AtlasScreen {
    zone: Option<String>,
    query: String,
    /// Kinds hidden by the filters; everything shows by default.
    hidden: BTreeSet<String>,
    view: View,
    /// A spot picked from the search, drawn highlighted.
    picked: Option<(String, String)>,
    textures: HashMap<String, Result<egui::TextureHandle, String>>,
}

impl Default for AtlasScreen {
    fn default() -> Self {
        AtlasScreen {
            zone: None,
            query: String::new(),
            // Navigation markers start hidden: they crowd the map and nobody asks for them.
            hidden: [
                "Stairs",
                "Ladder",
                "Lift",
                "Level connection",
                "Passage",
                "Pit trap",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            view: View::default(),
            picked: None,
            textures: HashMap::new(),
        }
    }
}

/// The widest an atlas map is decoded at; the bundle's copies are at most 2,600.
const ATLAS_MAX_W: u32 = 4096;

fn decode(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let img =
        image::load_from_memory(bytes).map_err(|e| format!("the map could not be read: {e}"))?;
    let img = if img.width() > ATLAS_MAX_W {
        img.resize(ATLAS_MAX_W, u32::MAX, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

impl AtlasScreen {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let Some(lore) = cx.lore.lore else {
            ui.add_space(8.0);
            ui.label(
                RichText::new("The Atlas needs the wiki bundle, which isn't loaded yet.")
                    .color(TEXT_2),
            );
            return;
        };
        let atlases: Vec<ZoneAtlas> = lore
            .records(Kind::Zone)
            .iter()
            .filter_map(zone_atlas)
            .collect();
        if atlases.is_empty() {
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "This wiki bundle carries no atlas maps yet. The next wiki sync adds them.",
                )
                .color(TEXT_2),
            );
            return;
        }
        if !self
            .zone
            .as_ref()
            .is_some_and(|z| atlases.iter().any(|a| &a.zone == z))
        {
            self.zone = Some(atlases[0].zone.clone());
            self.view = View::default();
        }

        let avail = ui.available_size();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(260.0_f32.min(avail.x * 0.35));
                self.side(ui, &atlases);
            });
            ui.separator();
            ui.vertical(|ui| {
                let Some(a) = atlases.iter().find(|a| Some(&a.zone) == self.zone.as_ref()) else {
                    return;
                };
                self.map(ui, lore, a);
            });
        });
    }

    /// The zone picker, the filters and the search.
    fn side(&mut self, ui: &mut Ui, atlases: &[ZoneAtlas]) {
        let current = self.zone.clone().unwrap_or_default();
        egui::ComboBox::from_id_salt("atlas-zone")
            .width(ui.available_width())
            .selected_text(&current)
            .show_ui(ui, |ui| {
                for a in atlases {
                    if ui.selectable_label(a.zone == current, &a.zone).clicked() {
                        self.zone = Some(a.zone.clone());
                        self.view = View::default();
                        self.picked = None;
                    }
                }
            });
        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("search camps, mobs, trainers")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);

        let q = self.query.trim().to_owned();
        if !q.is_empty() {
            // Search every zone: the asker may not know where it is.
            let hits: Vec<(&ZoneAtlas, &Spot)> = atlases
                .iter()
                .flat_map(|a| {
                    a.spots
                        .iter()
                        .filter(|s| matches(s, &q))
                        .map(move |s| (a, s))
                })
                .take(200)
                .collect();
            ui.label(
                RichText::new(format!("{} found", hits.len()))
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
            );
            egui::ScrollArea::vertical()
                .id_salt("atlas-hits")
                .max_height(ui.available_height() - 8.0)
                .show(ui, |ui| {
                    for (a, s) in hits {
                        let line = format!("{}  ·  {}", s.name, a.zone);
                        let r = ui.add(
                            egui::Button::selectable(
                                false,
                                RichText::new(line).color(kind_colour(&s.kind)),
                            )
                            .frame(false),
                        );
                        if r.clicked() {
                            self.zone = Some(a.zone.clone());
                            self.picked = Some((a.zone.clone(), s.id.clone()));
                            self.hidden.remove(&s.kind);
                            self.view.focus(s.x, s.y);
                        }
                    }
                });
            return;
        }

        ui.label(
            RichText::new("SHOW")
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
        let zone_kinds: BTreeSet<&str> = atlases
            .iter()
            .find(|a| Some(&a.zone) == self.zone.as_ref())
            .map(|a| a.spots.iter().map(|s| s.kind.as_str()).collect())
            .unwrap_or_default();
        for (kind, colour) in KINDS.iter().filter(|(k, _)| zone_kinds.contains(k)) {
            let mut on = !self.hidden.contains(*kind);
            if ui
                .checkbox(&mut on, RichText::new(*kind).color(*colour))
                .changed()
            {
                if on {
                    self.hidden.remove(*kind);
                } else {
                    self.hidden.insert(kind.to_string());
                }
            }
        }
    }

    /// The map, its pins, its hover card and its credit.
    fn map(&mut self, ui: &mut Ui, lore: &Lore, a: &ZoneAtlas) {
        let tex = self.textures.entry(a.zone.clone()).or_insert_with(|| {
            std::fs::read(lore.root.join(&a.path))
                .map_err(|e| format!("the map file is missing: {e}"))
                .and_then(|b| decode(&b))
                .map(|img| {
                    ui.ctx().load_texture(
                        format!("atlas:{}", a.zone),
                        img,
                        egui::TextureOptions::LINEAR,
                    )
                })
        });
        let tex = match tex {
            Ok(t) => t.clone(),
            Err(why) => {
                ui.label(RichText::new(why.as_str()).color(TEXT_2));
                return;
            }
        };
        let aspect = tex.size_vec2().x / tex.size_vec2().y;
        let credit_h = 22.0;
        let size = Vec2::new(
            ui.available_width(),
            (ui.available_height() - credit_h).max(200.0),
        );
        let (pane, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());

        if resp.dragged() {
            self.view.pan(resp.drag_delta(), pane, aspect);
        }
        if let Some(p) = resp.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.view.zoom_at((scroll / 400.0).exp(), p, pane, aspect);
            }
        }
        if resp.double_clicked() {
            self.view = View::default();
        }

        let painter = ui.painter_at(pane);
        painter.rect_filled(pane, 4.0, SUNK);
        let mr = self.view.map_rect(pane, aspect);
        painter.image(
            tex.id(),
            mr,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        let at = |s: &Spot| Pos2::new(mr.min.x + s.x * mr.width(), mr.min.y + s.y * mr.height());
        // Place names once there is room to read them.
        if self.view.zoom >= 1.6 && !self.hidden.contains("Place name") {
            for s in a.spots.iter().filter(|s| s.kind == "Place name") {
                painter.text(
                    at(s),
                    egui::Align2::CENTER_CENTER,
                    &s.name,
                    crate::fonts::display(13.0),
                    Color32::from_rgb(0x3B, 0x2A, 0x16),
                );
            }
        }
        let pointer = resp.hover_pos();
        let mut hovered: Option<&Spot> = None;
        let r = (4.0 + self.view.zoom).min(9.0);
        for s in a
            .spots
            .iter()
            .filter(|s| s.kind != "Place name" && !self.hidden.contains(&s.kind))
        {
            let p = at(s);
            if !pane.contains(p) {
                continue;
            }
            let picked = self
                .picked
                .as_ref()
                .is_some_and(|(z, id)| z == &a.zone && id == &s.id);
            let known = KINDS.iter().any(|(k, _)| *k == s.kind);
            let radius = if known { r } else { r * 0.6 };
            if picked {
                painter.circle_filled(p, radius * 2.4, GOLD_HI.gamma_multiply(0.35));
            }
            painter.circle(p, radius, kind_colour(&s.kind), egui::Stroke::new(2.0, INK));
            if pointer.is_some_and(|q| q.distance(p) <= radius + 3.0) {
                hovered = Some(s);
            }
        }
        if let Some(s) = hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            let resp = resp.on_hover_ui_at_pointer(|ui| card(ui, s));
            if resp.clicked() {
                crate::shell::open_url_quietly(&format!("{}?place={}", a.url, s.id));
            }
        }

        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("Map and spots: the Eth-ur Atlas (mnmatlas.com), used with its maker's permission. Drag to move, scroll to zoom, click a pin to open it.")
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
            );
            if ui.link(RichText::new(format!("{} on the atlas", a.zone)).font(FontId::proportional(11.0)).color(GOLD)).clicked() {
                crate::shell::open_url_quietly(&a.url);
            }
        });
    }
}

/// A spot's hover card: name, kind, levels, mobs, note.
fn card(ui: &mut Ui, s: &Spot) {
    ui.label(
        RichText::new(&s.name)
            .font(crate::fonts::display(15.0))
            .color(GOLD_HI),
    );
    let mut line = s.kind.clone();
    if let Some(l) = levels_text(s.levels) {
        line = format!("{line}, {l}");
    }
    ui.label(RichText::new(line).color(kind_colour(&s.kind)));
    if !s.mobs.is_empty() {
        ui.label(RichText::new(format!("Mobs: {}", s.mobs.join(", "))).color(TEXT));
    }
    if let Some(n) = &s.note {
        ui.label(RichText::new(n).color(TEXT_2));
    }
    if s.approximate {
        ui.label(
            RichText::new("Spot approximate.")
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rec(v: Value) -> Record {
        Record::from_value(v).expect("a record")
    }

    #[test]
    fn a_zone_record_carries_its_atlas_spots_as_fractions() {
        let r = rec(
            json!({"name": "Shaded Dunes", "atlas": {"path": "atlas/shaded-dunes.jpg", "url": "https://www.mnmatlas.com/shaded-dunes/", "places": [
                {"id": "b1", "name": "Bandits", "kind": "Mob camp", "x": 0.866, "y": 0.54, "levels": [10, 15], "mobs": ["A bandit"], "approximate": true},
                {"id": "g", "name": "Goblins", "kind": "Mob camp", "x": 0.2, "y": 0.3, "levels": [20]},
                {"id": "bad", "name": "no position", "kind": "Bank"}
            ]}}),
        );
        let a = zone_atlas(&r).expect("has an atlas");
        assert_eq!(a.spots.len(), 2, "a spot without a position is skipped");
        assert_eq!(a.spots[0].mobs, vec!["A bandit".to_owned()]);
        assert_eq!(
            levels_text(a.spots[0].levels).as_deref(),
            Some("levels 10 to 15")
        );
        assert_eq!(
            levels_text(a.spots[1].levels).as_deref(),
            Some("levels 20+")
        );
        assert!(zone_atlas(&rec(json!({"name": "Night Harbor"}))).is_none());
    }

    #[test]
    fn search_finds_a_spot_by_its_mobs_not_only_its_name() {
        let s = Spot {
            id: "b1".into(),
            name: "Bandits".into(),
            kind: "Mob camp".into(),
            x: 0.5,
            y: 0.5,
            levels: None,
            mobs: vec!["A bandit".into()],
            note: Some("By the oasis".into()),
            approximate: false,
        };
        assert!(matches(&s, "bandit"));
        assert!(matches(&s, "OASIS"));
        assert!(matches(&s, "mob camp"));
        assert!(!matches(&s, "goblin"));
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_still() {
        let pane = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let mut v = View::default();
        let at = Pos2::new(600.0, 200.0);
        let before = v.map_rect(pane, 1.5);
        let p = (
            (at.x - before.min.x) / before.width(),
            (at.y - before.min.y) / before.height(),
        );
        v.zoom_at(2.0, at, pane, 1.5);
        let after = v.map_rect(pane, 1.5);
        let q = (
            (at.x - after.min.x) / after.width(),
            (at.y - after.min.y) / after.height(),
        );
        assert!(
            (p.0 - q.0).abs() < 1e-4 && (p.1 - q.1).abs() < 1e-4,
            "{p:?} vs {q:?}"
        );
        assert_eq!(v.zoom, 2.0);
        v.zoom_at(100.0, at, pane, 1.5);
        assert_eq!(v.zoom, MAX_ZOOM, "zoom is capped");
    }
}
