//! FIND: Items. The gear-data.json list, searchable, filtered by slot and era, with a detail pane
//! that shows every field the file carries. Decisions D5 (the FIND group) and D6 (data at runtime).
//!
//! THIS FILE ALSO HOLDS THE HELPERS THE OTHER TWO FIND SCREENS SHARE (zones.rs, drops.rs): the JSON
//! view of a record, the detail table, the search box, the list row, the chips, the empty-data
//! notice. They live here rather than in a fourth file because the FIND lane owns exactly three
//! files and a helper module would have been a fourth, written into a directory other lanes edit.
//!
//! WHAT IS READ TYPED AND WHAT IS READ THROUGH A JSON VIEW.
//! The list, the chips and the sources read the typed fields the data lane fixed (`Item::sl`,
//! `Item::era`, `Item::src`), so a rename there is a compile error here rather than a list that
//! quietly goes empty. The detail table is the one place a record is serialised back to JSON
//! (`view`): the contract's promise is that NOTHING the file carries is hidden, and the only way to
//! keep that promise without restating the record's field list in a second place is to walk the
//! serialised object, typed keys and flattened `extra` alike. One `serde_json::to_value` per
//! selection, cached until the selection changes.
//!
//! MEASURED SHAPE OF gear-data.json items (6891 records, keyed by lowercased name):
//!   cls {all:1} or {c:[..]} or {all:1,x:[..]}   era "Velious Era" (absent on 1876)   fl [flags]
//!   n display name   oe 1   rc {..}   size "SMALL"   sl ["Neck"] (18 distinct)   st {ac:2,..}
//!   src {d:[[zone,[[mob,lvl,con],..]],..], q:1, c:1, s:1, v:1, f:1}   t wiki slug   wt weight
//!   plus dly dmg skill sv charges eff foc oer rep ex inst instfix rcp deity haste req x stray
//!   dmg_bonus rec on subsets. The src letters are decoded by `Item::source_kinds` (data lane,
//!   one word per letter); anything else in `src` is shown raw and unlabelled.

use crate::data::Item;
use crate::theme::*;
use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    Vec2,
};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::Mutex;

/* ======================================================================================
 *  Shared FIND helpers. pub(crate) so zones.rs and drops.rs use the same ones.
 * ====================================================================================== */

/// A record seen as the file saw it: the JSON object with typed and extra keys alike.
pub(crate) type View = Map<String, Value>;

/// The identity of a snapshot, so per-screen caches rebuild exactly when the data changes and
/// never on a frame where it did not. Address plus the three counts: an address alone can be
/// reused by a reload that lands on the same allocation.
pub(crate) type SnapKey = (usize, usize, usize, usize);

pub(crate) fn snapshot_key(s: &crate::data::Snapshot) -> SnapKey {
    let r = s.report();
    (std::ptr::from_ref(s) as usize, r.items, r.zones, r.drops)
}

/// The JSON view of one record. See the module comment for why this exists.
pub(crate) fn view<T: serde::Serialize>(rec: &T) -> Result<View, String> {
    match serde_json::to_value(rec) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(other) => Err(format!(
            "record serialised to a JSON {} rather than an object",
            kind_of(&other)
        )),
        Err(e) => Err(format!("record could not be serialised: {e}")),
    }
}

pub(crate) fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/* ------------------------------------------------------------------ the detail table -- */

pub(crate) struct DetailRows {
    /// The typed fields, in the order `detail_rows` defines.
    pub typed: Vec<(String, String)>,
    /// Every key in `extra`, alphabetical, so nothing the file carries is hidden.
    pub more: Vec<(String, String)>,
}

/// The rows of a record's detail table. PURE, and the ordering is the contract. First, the
/// heading's own key is never a row: `name`, or `n` (the file's key for it), whichever holds
/// exactly the heading text. Second, typed rows (keys not in `extra`) come first, in `preferred`
/// order, then the rest of the typed keys alphabetically. Third, `more` rows are every key of
/// `extra`, alphabetically. A key called `extra` holding an object is the more section itself (a
/// record that stores extra unflattened, as `Drop` does, serialises it as one nested object) and
/// is not a typed row either.
///
/// Keys named in `summarised` are rendered as a count with a note that they are listed below, for
/// arrays the screen expands into a proper table (a roster of 64 mobs as one JSON string is not a
/// table). The array is not hidden, it is drawn elsewhere in the same pane.
pub(crate) fn detail_rows(
    name: &str,
    view: &View,
    extra: &View,
    preferred: &[&str],
    summarised: &[&str],
) -> DetailRows {
    let is_heading = |k: &str, v: &Value| matches!(k, "name" | "n") && v.as_str() == Some(name);
    let is_extra_bag = |k: &str, v: &Value| k == "extra" && v.is_object();
    let mut typed: Vec<(&String, &Value)> = view
        .iter()
        .filter(|(k, v)| !is_heading(k, v) && !is_extra_bag(k, v) && !extra.contains_key(*k))
        .collect();
    typed.sort_by(|(a, _), (b, _)| {
        let pa = preferred.iter().position(|p| p == a).unwrap_or(usize::MAX);
        let pb = preferred.iter().position(|p| p == b).unwrap_or(usize::MAX);
        pa.cmp(&pb).then_with(|| a.cmp(b))
    });
    let render = |k: &String, v: &Value| -> String {
        if summarised.contains(&k.as_str()) {
            match v {
                Value::Array(a) => format!("{} entries, listed below", a.len()),
                Value::Object(o) => format!("{} keys, listed below", o.len()),
                other => render_value(other),
            }
        } else {
            render_value(v)
        }
    };
    let typed = typed
        .into_iter()
        .map(|(k, v)| (k.clone(), render(k, v)))
        .collect();
    let mut more: Vec<(String, String)> = extra
        .iter()
        .filter(|(k, v)| !is_heading(k, v))
        .map(|(k, v)| (k.clone(), render(k, v)))
        .collect();
    more.sort_by(|(a, _), (b, _)| a.cmp(b));
    DetailRows { typed, more }
}

/// A number as the file printed it: the data lane types the wiki's integers as f64, so 2.0 is
/// the file's 2 and prints as 2; 12.8 stays 12.8.
pub(crate) fn num_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

/// One value as text for a monospace cell. Scalars bare, arrays of scalars comma joined, an
/// object whose values are all numbers as a label column and a RIGHT ALIGNED number column (this
/// is what `st` stats and coin look like), anything nested as compact JSON so it is exact.
pub(crate) fn render_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) => num_text(f),
            None => n.to_string(),
        },
        Value::String(s) => s.clone(),
        Value::Array(a)
            if a.iter()
                .all(|x| !matches!(x, Value::Array(_) | Value::Object(_))) =>
        {
            a.iter().map(render_value).collect::<Vec<_>>().join(", ")
        }
        Value::Object(o) if !o.is_empty() && o.values().all(Value::is_number) => {
            let w = o.keys().map(String::len).max().unwrap_or(0);
            let cells: Vec<(&String, String)> =
                o.iter().map(|(k, x)| (k, render_value(x))).collect();
            let n = cells.iter().map(|(_, s)| s.len()).max().unwrap_or(0);
            cells
                .iter()
                .map(|(k, s)| format!("{k:<w$}  {s:>n$}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        other => other.to_string(),
    }
}

/* -------------------------------------------------------------------- the search box -- */

/// What a search box holds once the D6 type prefix has been read off it.
pub(crate) struct Query {
    /// Case folded, trimmed, prefix removed.
    pub needle: String,
    /// A prefix that names a DIFFERENT list than the one this box searches. The screen says so
    /// rather than silently searching for the literal text "z:foo" and finding nothing.
    pub foreign_prefix: Option<char>,
}

/// D6: `i:` `z:` `d:` select a list, and `p:` joined them when Spells got a screen. In a per-list
/// box the matching prefix is simply stripped, a foreign one is stripped and reported. Plain
/// substring, case folded, no fuzzy matching.
///
/// SPELLS ARE `p:` AND NOT `s:` because `s:` is already the sky list's letter in
/// [`crate::data::HitKind`], and one letter meaning two lists means neither.
pub(crate) fn parse_query(raw: &str, own: char) -> Query {
    let t = raw.trim();
    let lower = t.to_lowercase();
    let mut chars = lower.chars();
    let (needle, foreign_prefix) = match (chars.next(), chars.next()) {
        (Some(p @ ('i' | 'z' | 'd' | 'p')), Some(':')) => {
            let rest = lower[2..].trim().to_owned();
            (rest, if p == own { None } else { Some(p) })
        }
        _ => (lower, None),
    };
    Query {
        needle,
        foreign_prefix,
    }
}

pub(crate) fn list_named(prefix: char) -> &'static str {
    match prefix {
        'i' => "Items",
        'z' => "Zones",
        'p' => "Spells",
        _ => "Drops",
    }
}

/// The monospace search box. Returns true when the text changed this frame.
pub(crate) fn search_box(ui: &mut Ui, text: &mut String, hint: &str) -> bool {
    let te = egui::TextEdit::singleline(text)
        .hint_text(hint)
        .desired_width(300.0)
        .font(FontId::monospace(13.0));
    ui.add(te).changed()
}

/// "214 of 6891 items", monospace, right of the search box.
pub(crate) fn count_line(ui: &mut Ui, shown: usize, total: usize, what: &str) {
    ui.label(
        egui::RichText::new(format!("{shown} of {total} {what}"))
            .font(FontId::monospace(11.5))
            .color(TEXT_2),
    );
}

/// Which file, where, how many. D6: the FIND screens say which path they read.
pub(crate) fn provenance(ui: &mut Ui, root: &Path, file: &str, n: usize, what: &str) {
    ui.label(
        egui::RichText::new(format!("{file} in {} · {n} {what}", root.display()))
            .font(FontId::monospace(10.5))
            .color(TEXT_3),
    );
}

/* ----------------------------------------------------------------------- list rows -- */

pub(crate) const ROW_H: f32 = 22.0;

pub(crate) struct Col<'a> {
    pub text: &'a str,
    pub mono: bool,
    pub color: Color32,
    /// Right anchored columns are laid from the right edge; the LAST right column in the slice
    /// sits rightmost. This is how a number column ends up right aligned.
    pub right: bool,
    /// 0.0 means "the rest of the row", for exactly one left column.
    pub width: f32,
}

/// One row of a result list, painted rather than laid out: 6891 rows through `show_rows` need
/// a row that costs one allocation and a few painter calls. Selection is the nav vocabulary:
/// PANEL_2 fill and a 2px gold LEFT edge.
pub(crate) fn list_row(ui: &mut Ui, selected: bool, cols: &[Col<'_>]) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::ZERO, PANEL_2);
        p.rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(2.0, ROW_H)),
            CornerRadius::ZERO,
            GOLD,
        );
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::ZERO, PANEL);
    }
    let pad = 10.0;
    let gap = 8.0;
    let mut left = rect.left() + pad;
    let mut right = rect.right() - pad;
    for c in cols.iter().rev().filter(|c| c.right) {
        let r = Rect::from_min_max(
            Pos2::new(right - c.width, rect.top()),
            Pos2::new(right, rect.bottom()),
        );
        p.with_clip_rect(r).text(
            Pos2::new(r.right(), r.center().y),
            Align2::RIGHT_CENTER,
            c.text,
            col_font(c),
            c.color,
        );
        right -= c.width + gap;
    }
    for c in cols.iter().filter(|c| !c.right) {
        let w = if c.width > 0.0 {
            c.width
        } else {
            (right - left).max(0.0)
        };
        let r = Rect::from_min_max(
            Pos2::new(left, rect.top()),
            Pos2::new(left + w, rect.bottom()),
        );
        p.with_clip_rect(r).text(
            Pos2::new(r.left(), r.center().y),
            Align2::LEFT_CENTER,
            c.text,
            col_font(c),
            c.color,
        );
        left += w + gap;
    }
    resp
}

fn col_font(c: &Col<'_>) -> FontId {
    if c.mono {
        FontId::monospace(11.5)
    } else {
        FontId::proportional(12.5)
    }
}

/// A header row for a painted table: every label dim and monospace, same column geometry as the
/// rows under it so the numbers sit under their heading.
pub(crate) fn head_row(ui: &mut Ui, cols: &[(&str, bool, f32)]) {
    let cols: Vec<Col<'_>> = cols
        .iter()
        .map(|(t, right, w)| Col {
            text: t,
            mono: true,
            color: TEXT_3,
            right: *right,
            width: *w,
        })
        .collect();
    list_row(ui, false, &cols);
}

/// A filter chip: label and count, monospace, gold when on. Gold here is selection, which is the
/// theme's own use of it (`selection.bg_fill = GOLD_DEEP`), not a state colour.
pub(crate) fn chip(ui: &mut Ui, label: &str, count: usize, on: bool) -> Response {
    let text = format!("{label} {count}");
    let font = FontId::monospace(10.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), TEXT_2);
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(galley.rect.width() + 14.0, 18.0), Sense::click());
    let (fill, stroke, col) = if on {
        (GOLD_DEEP, GOLD_DIM, GOLD_HI)
    } else if resp.hovered() {
        (PANEL_2, GOLD_DEEP, GOLD_HI)
    } else {
        (PANEL, RULE, TEXT_2)
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, fill);
    p.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, stroke),
        StrokeKind::Middle,
    );
    p.text(rect.center(), Align2::CENTER_CENTER, text, font, col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Clickable text that reads as a link: gold, brighter and underlined on hover.
pub(crate) fn link(ui: &mut Ui, text: &str, font: FontId) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), TEXT);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(galley.rect.width(), galley.rect.height().max(16.0)),
        Sense::click(),
    );
    let col = if resp.hovered() { GOLD_HI } else { GOLD };
    let p = ui.painter();
    p.text(
        Pos2::new(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        col,
    );
    if resp.hovered() {
        p.line_segment(
            [
                Pos2::new(rect.left(), rect.bottom() - 1.0),
                Pos2::new(rect.right(), rect.bottom() - 1.0),
            ],
            Stroke::new(1.0, GOLD_HI),
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A caps section label inside a pane: tracked Cinzel, GOLD_DIM, the rail's own vocabulary.
pub(crate) fn pane_section(ui: &mut Ui, label: &str) {
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 14.0), Sense::hover());
    crate::chrome::tracked(ui, rect.left_top(), label, 9.5, 1.8, GOLD_DIM);
    ui.add_space(4.0);
}

/// A dim body line: the honest "there is nothing here because ..." sentence.
pub(crate) fn dim(ui: &mut Ui, s: &str) {
    ui.label(
        egui::RichText::new(s)
            .font(FontId::proportional(12.0))
            .color(TEXT_3),
    );
}

pub(crate) fn mono(ui: &mut Ui, s: &str, col: Color32) {
    ui.label(
        egui::RichText::new(s)
            .font(FontId::monospace(11.5))
            .color(col),
    );
}

/// [`mono`] with a hover. Separate rather than a return value on `mono`, because `mono` is called
/// in over a hundred places that want nothing back and an ignored `Response` reads like a bug.
///
/// The hover is where a split line's SOURCE goes: the panes print the tidy halves (item and price,
/// faction and delta) and this puts the wiki's own line one hover away, so nothing the file
/// carries is more than a mouse away from the thing derived from it.
pub(crate) fn mono_hover(ui: &mut Ui, s: &str, col: Color32, hover: &str) {
    ui.label(
        egui::RichText::new(s)
            .font(FontId::monospace(11.5))
            .color(col),
    )
    .on_hover_text(hover);
}

/// The WRONG state bar: something is wrong and these are its words. Wraps, so a long loader error
/// is read in full rather than clipped.
pub(crate) fn wrong_bar(ui: &mut Ui, msg: &str) {
    let w = ui.available_width();
    let galley = ui.painter().layout(
        msg.to_owned(),
        FontId::proportional(12.5),
        TEXT,
        (w - 24.0).max(40.0),
    );
    let h = galley.rect.height() + 16.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, SUNK);
    p.rect_filled(
        Rect::from_min_size(rect.left_top(), Vec2::new(3.0, h)),
        CornerRadius::ZERO,
        WRONG,
    );
    p.galley(
        Pos2::new(rect.left() + 12.0, rect.top() + 8.0),
        galley,
        TEXT,
    );
}

/* --------------------------------------------------------------- who sells it -- */

/// One row of the SOLD BY section: a merchant, where they stand, and what they ask.
///
/// OWNED, NOT BORROWED, because it is built once per selection and cached on the pane's `Detail`
/// beside the drop table, exactly as `SourceZone` is. The snapshot outlives the frame, but the
/// cache outlives the frame's borrow of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SellerRow {
    /// The merchant's display name as the wiki spells it ("a brownie merchant").
    pub who: String,
    /// The merchant's `zone` string, verbatim. Not the zone record's spelling: this is what the
    /// merchant page says, and the link resolves it rather than rewriting it.
    pub zone: String,
    /// The zone as this snapshot spells it, when it has a page for it. None means the link is not
    /// offered, which is the difference between "no page for this" and "not a zone".
    pub zone_page: Option<String>,
    /// `loc`, verbatim: one coordinate pair, or three, or "Need Info".
    pub loc: String,
    /// The price as the wiki wrote it, or None when the line carries none this build reads.
    pub price: Option<String>,
    /// The whole source line, for the hover. Nothing the file carries is more than one hover away.
    pub raw: String,
}

/// Who sells an item, by name, from the snapshot's inverse index. Sorted by zone then merchant, so
/// the pane reads as places rather than as a jumble of shop names.
pub(crate) fn seller_rows(data: &crate::data::Snapshot, item: &str) -> Vec<SellerRow> {
    let mut rows: Vec<SellerRow> = data
        .sellers_of(item)
        .iter()
        .filter_map(|p| {
            let (m, sale) = data.sale(*p)?;
            Some(SellerRow {
                who: m.name.clone(),
                zone: m.zone.clone(),
                zone_page: data.zone(&m.zone).map(|z| z.name.clone()).or_else(|| {
                    /* a two zone string ("Grobb, Neriak Foreign Quarter") links to the first piece
                     * this snapshot has a page for, and prints the whole string either way */
                    m.zone_pieces()
                        .iter()
                        .skip(1)
                        .find_map(|p| data.zone(p).map(|z| z.name.clone()))
                }),
                loc: m.loc.clone(),
                price: sale.price.map(str::to_owned),
                raw: sale.raw.to_owned(),
            })
        })
        .collect();
    rows.sort_by(|a, b| a.zone.cmp(&b.zone).then_with(|| a.who.cmp(&b.who)));
    rows
}

/// Draw the SOLD BY section. Returns the navigation the reader asked for, if any.
///
/// `flagged_sold` is whether the record itself already claimed a vendor (gear-data's `src.s`).
/// When it did and this names nobody, the pane says exactly that rather than drawing the same
/// blank as an item nothing ever claimed to sell: those are two different facts about the wiki.
pub(crate) fn sold_by_section(
    ui: &mut Ui,
    rows: &[SellerRow],
    flagged_sold: bool,
    empty_note: &str,
) -> Option<crate::screens::Ask> {
    let mut ask = None;
    pane_section(ui, "SOLD BY");
    if rows.is_empty() {
        if flagged_sold {
            dim(
                ui,
                &format!(
                    "gear-data.json flags this item as sold by a vendor and {} names none of them.",
                    crate::data::MERCHANTS_FILE
                ),
            );
        } else {
            dim(ui, empty_note);
        }
        return ask;
    }
    mono(
        ui,
        &format!(
            "{} merchant{}",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        ),
        TEXT_3,
    );
    head_row(
        ui,
        &[
            ("merchant", false, 0.0),
            ("zone", false, 150.0),
            ("price", true, 120.0),
        ],
    );
    for r in rows {
        ui.horizontal(|ui| {
            let name = ui.label(
                egui::RichText::new(&r.who)
                    .font(FontId::proportional(12.5))
                    .color(TEXT),
            );
            if !r.loc.is_empty() {
                name.on_hover_text(format!("{}\n{}", r.raw, r.loc));
            } else {
                name.on_hover_text(&r.raw);
            }
            ui.add_space(6.0);
            match &r.zone_page {
                Some(page) => {
                    if link(ui, &r.zone, FontId::proportional(12.0))
                        .on_hover_text(format!("open {page} in Zones"))
                        .clicked()
                    {
                        crate::screens::zones::jump_to(page);
                        ask = Some(crate::screens::Ask::ShowZone(page.clone()));
                    }
                }
                /* No page for this spelling, so no link. The string is still printed: the reader
                 * can search the wiki for it, and a dead link would be worse than plain text. */
                None => mono(ui, &r.zone, TEXT_3),
            }
            ui.add_space(6.0);
            match &r.price {
                Some(p) => mono(ui, p, TEXT_2),
                None => dim(ui, "no price on the line"),
            }
        });
    }
    ask
}

/// The two column table: label proportional dim, value monospace.
pub(crate) fn kv_table(ui: &mut Ui, id: &str, rows: &[(String, String)]) {
    let value_w = (ui.available_width() - 110.0).max(120.0);
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([14.0, 4.0])
        .min_col_width(40.0)
        .max_col_width(value_w)
        .show(ui, |ui| {
            for (k, v) in rows {
                ui.label(
                    egui::RichText::new(k)
                        .font(FontId::proportional(12.0))
                        .color(TEXT_3),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(v)
                            .font(FontId::monospace(11.5))
                            .color(TEXT),
                    )
                    .wrap(),
                );
                ui.end_row();
            }
        });
}

/* `open_url` LIVES IN `crate::shell` NOW. It stood here, in the Items screen, because Items was
 * the first thing that needed it; `titlebar` then grew its own copy that logged instead of
 * returning, and `settings` a third that inlined the call on a folder with its own wording. One
 * mechanism with two NAMED policies is what replaced them. This screen's callers take the
 * `Result` form, which is the same behaviour they had. */
pub(crate) use crate::shell::open_url;

/// The honest empty state. D6: say the data is absent and where to put it, and the loader's own
/// words verbatim. The locations are `crate::data::candidates()`, IN THE ORDER THE LOADER PROBES
/// THEM, so this list can never disagree with what `Snapshot::locate` actually did. The probe
/// order is the loader's, not this screen's, and the loader's order is the truth on screen.
pub(crate) fn no_data(
    ui: &mut Ui,
    data_err: Option<&str>,
    override_root: Option<&Path>,
    list: &str,
) {
    /* A caps run, so Cinzel is allowed on it: the display face stops at headings and cap runs
     * (fonts.rs), and "No item data loaded" in sentence case was neither. */
    ui.label(
        egui::RichText::new(format!("NO {} DATA LOADED", list.to_ascii_uppercase()))
            .font(crate::fonts::display(15.0))
            .color(GOLD),
    );
    ui.add_space(8.0);
    match data_err {
        Some(e) => wrong_bar(ui, e),
        None => wrong_bar(
            ui,
            "No snapshot is loaded and the loader reported no error. Nothing has been read yet.",
        ),
    }
    ui.add_space(10.0);
    dim(ui, "The snapshot is looked for here, in this order:");
    ui.add_space(4.0);
    let mut n = 0;
    let mut row = |ui: &mut Ui, label: &str, path: String| {
        n += 1;
        ui.horizontal(|ui| {
            mono(ui, &format!("{n}."), TEXT_3);
            mono(ui, &path, TEXT);
            dim(ui, label);
        });
    };
    if let Some(p) = override_root {
        /* The settings field exists by contract; whether the App consults it before the probe
         * list is the integrator's wiring. It is listed because it is set, and a set path the
         * user cannot see is a path they cannot correct. */
        row(ui, "data_root in settings", p.display().to_string());
    }
    /* THE WORDS ARRIVE WITH THE PATH. This used to guess a label back out of the path's shape,
     * comparing it against the executable's folder and against a literal root, which is a second
     * opinion about a list the loader already knows the answer for. `candidates_labelled` carries
     * both, so a directory that is added, dropped or renamed changes this screen in the same
     * edit. */
    for (c, label) in crate::data::candidates_labelled() {
        row(ui, label, c.display().to_string());
    }
    ui.add_space(8.0);
    dim(
        ui,
        &format!(
            "Each location needs {} and {}/.",
            crate::data::FILES.join(", "),
            crate::data::ATLAS_DIR
        ),
    );
}

/* ------------------------------------------------------------- cross-screen jumps -- */

/// Another screen asking this one to select an item by name. Consumed on the next frame this
/// screen draws. The nav switch itself travels on `Cx.ask` (`Ask::ShowItem`), which the caller
/// sets in the same click; this static carries the selection the App has no field for. Without
/// the App acting on `ask`, the request still lands the next time Items is opened.
static JUMP: Mutex<Option<String>> = Mutex::new(None);

pub fn jump_to(item: &str) {
    *JUMP.lock().unwrap_or_else(|p| p.into_inner()) = Some(item.to_owned());
}

fn take_jump() -> Option<String> {
    JUMP.lock().unwrap_or_else(|p| p.into_inner()).take()
}

/* ======================================================================================
 *  Items: facts, chips, filters. Pure.
 * ====================================================================================== */

/// One mob under a zone in `src.d`. POSITIONAL in the file: [0] mob name, [1] level text ("34",
/// "60-64") or null, [2] rarity text ("Common" .. "Ultra Rare") or null.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceMob {
    pub name: String,
    pub level: Option<String>,
    pub rarity: Option<String>,
}

/// One zone in `src.d`. POSITIONAL: [0] zone name, [1] the mob rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceZone {
    pub zone: String,
    pub mobs: Vec<SourceMob>,
}

/// `Item::src.d`, grouped as the file groups it: zone, then its mobs. The positional read lives
/// in ONE place, the data module's `Item::drop_rows` (one row per zone and mob, page order); this
/// puts the zone grouping back because the pane draws zone headings with mobs under them. An
/// entry that is not the measured shape is skipped there, never guessed here.
pub(crate) fn sources_of(it: &Item) -> Vec<SourceZone> {
    let mut out: Vec<SourceZone> = Vec::new();
    for d in it.drop_rows() {
        let mob = SourceMob {
            name: d.mob.to_owned(),
            level: d.level.map(str::to_owned),
            rarity: d.rarity.map(str::to_owned),
        };
        match out.last_mut() {
            Some(z) if z.zone == d.zone => z.mobs.push(mob),
            _ => out.push(SourceZone {
                zone: d.zone.to_owned(),
                mobs: vec![mob],
            }),
        }
    }
    out
}

/// The keys of `src` that neither `d` (the drop table) nor `Item::source_kinds` (q c s f v)
/// decode, as the file has them. None were measured; if the scrape grows one it shows up here
/// rather than vanishing.
pub(crate) fn src_unknown(src: Option<&Value>) -> Vec<(String, String)> {
    const KNOWN: [&str; 6] = ["d", "q", "c", "s", "f", "v"];
    src.and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter(|(k, _)| !KNOWN.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), render_value(v)))
                .collect()
        })
        .unwrap_or_default()
}

/// What the list and the filters need from an item, extracted once per snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ItemFacts {
    pub lname: String,
    pub slots: Vec<String>,
    pub era: Option<String>,
}

/// An empty era string is no era: the chip for "no era" has to catch it or the item is
/// unreachable through the era chips.
pub(crate) fn item_facts(it: &Item) -> ItemFacts {
    ItemFacts {
        lname: it.name.to_lowercase(),
        slots: it.sl.clone(),
        era: it
            .era
            .as_deref()
            .filter(|e| !e.is_empty())
            .map(str::to_owned),
    }
}

/// The filter chips, DERIVED from the data: every distinct slot and era with its count, sorted by
/// count descending then name, plus how many items carry no era at all so they stay reachable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Chips {
    pub slots: Vec<(String, usize)>,
    pub eras: Vec<(String, usize)>,
    pub no_era: usize,
}

pub(crate) fn chips<'a>(rows: impl Iterator<Item = (&'a [String], Option<&'a str>)>) -> Chips {
    use std::collections::BTreeMap;
    let mut slots: BTreeMap<String, usize> = BTreeMap::new();
    let mut eras: BTreeMap<String, usize> = BTreeMap::new();
    let mut no_era = 0;
    for (sl, era) in rows {
        for s in sl {
            *slots.entry(s.clone()).or_default() += 1;
        }
        match era {
            Some(e) => *eras.entry(e.to_owned()).or_default() += 1,
            None => no_era += 1,
        }
    }
    let order = |m: BTreeMap<String, usize>| {
        let mut v: Vec<(String, usize)> = m.into_iter().collect();
        v.sort_by(|(a, na), (b, nb)| nb.cmp(na).then_with(|| a.cmp(b)));
        v
    };
    Chips {
        slots: order(slots),
        eras: order(eras),
        no_era,
    }
}

/// The chips for a list of items: the one entry point the screen uses and the tests exercise.
pub(crate) fn chips_of(items: &[Item]) -> Chips {
    chips(items.iter().map(|it| {
        (
            it.sl.as_slice(),
            it.era.as_deref().filter(|e| !e.is_empty()),
        )
    }))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum EraFilter {
    #[default]
    Any,
    Named(String),
    /// Only the items that carry no era.
    None,
}

pub(crate) fn passes(f: &ItemFacts, needle: &str, slot: Option<&str>, era: &EraFilter) -> bool {
    if !needle.is_empty() && !f.lname.contains(needle) {
        return false;
    }
    if let Some(s) = slot {
        if !f.slots.iter().any(|x| x == s) {
            return false;
        }
    }
    match era {
        EraFilter::Any => true,
        EraFilter::Named(e) => f.era.as_deref() == Some(e.as_str()),
        EraFilter::None => f.era.is_none(),
    }
}

/* ======================================================================================
 *  The screen.
 * ====================================================================================== */

struct Cache {
    key: SnapKey,
    facts: Vec<ItemFacts>,
    chips: Chips,
}

struct Hits {
    needle: String,
    slot: Option<String>,
    era: EraFilter,
    idx: Vec<usize>,
}

struct Detail {
    idx: usize,
    rows: DetailRows,
    sources: Vec<SourceZone>,
    /// The words `Item::source_kinds` decodes from src's q c s f v flags.
    kinds: Vec<&'static str>,
    /// Any other src key, raw. See `src_unknown`.
    src_unknown: Vec<(String, String)>,
    /// item-tooltips.json's stat block for this name, line by line, when the wiki has one.
    tooltip: Option<Vec<String>>,
    /// quest-items.json's droppers for this name: (mob, zone), zone empty when the scrape found
    /// the dropper but not where it lives.
    quest_drops: Vec<(String, String)>,
    /// How many distinct zones the gear-data drop table names (`Item::drop_zones`).
    zone_count: usize,
    /// merchants.json's answer to "who sells this", the sibling of the drop table above it.
    sold_by: Vec<SellerRow>,
    err: Option<String>,
}

#[derive(Default)]
pub struct ItemsScreen {
    query: String,
    slot: Option<String>,
    era: EraFilter,
    sel: Option<usize>,
    cache: Option<Cache>,
    hits: Option<Hits>,
    detail: Option<Detail>,
    notice: Option<String>,
}

/// Typed-field order for the detail table: what a player reads first. These are the file's own
/// keys because `Item` serialises under them (`n` for name, the rest unrenamed).
const PREFERRED: &[&str] = &[
    "n", "sl", "era", "cls", "rc", "st", "sv", "dmg", "dly", "skill", "eff", "foc", "haste",
    "size", "wt", "fl", "oe", "src", "rcp", "t",
];

impl ItemsScreen {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut crate::screens::Cx<'_>) {
        let Some(data) = cx.data else {
            no_data(ui, cx.data_err, cx.settings.data_root.as_deref(), "item");
            return;
        };
        self.refresh(data);
        if let Some(name) = take_jump() {
            let want = name.to_lowercase();
            match self.cache.as_ref().and_then(|c| c.facts.iter().position(|f| f.lname == want)) {
                Some(i) => {
                    self.sel = Some(i);
                    self.query.clear();
                    self.slot = None;
                    self.era = EraFilter::Any;
                    self.notice = None;
                }
                None => self.notice = Some(format!("No item named \"{name}\" in gear-data.json ({} items). The name came from another file.", data.items.len())),
            }
        }
        let ask = egui::Panel::right("items_detail")
            .default_size(440.0)
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
        let facts: Vec<ItemFacts> = data.items.iter().map(item_facts).collect();
        let chips = chips_of(&data.items);
        self.cache = Some(Cache { key, facts, chips });
        self.hits = None;
        self.detail = None;
        self.sel = None;
    }

    fn hits(&mut self, needle: &str) -> &[usize] {
        let stale = match &self.hits {
            Some(h) => h.needle != needle || h.slot != self.slot || h.era != self.era,
            None => true,
        };
        if stale {
            let idx = match &self.cache {
                Some(c) => c
                    .facts
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| passes(f, needle, self.slot.as_deref(), &self.era))
                    .map(|(i, _)| i)
                    .collect(),
                None => Vec::new(),
            };
            self.hits = Some(Hits {
                needle: needle.to_owned(),
                slot: self.slot.clone(),
                era: self.era.clone(),
                idx,
            });
        }
        self.hits.as_ref().map(|h| h.idx.as_slice()).unwrap_or(&[])
    }

    fn list_pane(&mut self, ui: &mut Ui, data: &crate::data::Snapshot) {
        let total = data.items.len();
        let q = parse_query(&self.query, 'i');
        let report = data.report();

        ui.horizontal(|ui| {
            search_box(ui, &mut self.query, "item name, substring");
            let shown = self.hits(&q.needle).len();
            count_line(ui, shown, total, "items");
        });
        if let Some(p) = q.foreign_prefix {
            dim(
                ui,
                &format!(
                    "The prefix {p}: names the {} list. Searching items for the rest.",
                    list_named(p)
                ),
            );
        }
        provenance(
            ui,
            &report.root,
            crate::data::GEAR_FILE,
            report.items,
            "items",
        );
        if let Some(n) = self.notice.clone() {
            wrong_bar(ui, &n);
        }

        /* The chips. Two rows, derived from the data, each a toggle; the count on each chip is the
         * whole list, not the current result, so a chip never reads 0 while it is the reason. */
        let chips = self
            .cache
            .as_ref()
            .map(|c| c.chips.clone())
            .unwrap_or_default();
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            mono(ui, "slot", TEXT_3);
            for (s, n) in &chips.slots {
                let on = self.slot.as_deref() == Some(s.as_str());
                if chip(ui, s, *n, on).clicked() {
                    self.slot = if on { None } else { Some(s.clone()) };
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            mono(ui, "era ", TEXT_3);
            for (e, n) in &chips.eras {
                let on = self.era == EraFilter::Named(e.clone());
                if chip(ui, e, *n, on).clicked() {
                    self.era = if on {
                        EraFilter::Any
                    } else {
                        EraFilter::Named(e.clone())
                    };
                }
            }
            if chips.no_era > 0 {
                let on = self.era == EraFilter::None;
                if chip(ui, "no era", chips.no_era, on).clicked() {
                    self.era = if on { EraFilter::Any } else { EraFilter::None };
                }
            }
        });
        ui.add_space(6.0);

        let hits: Vec<usize> = self.hits(&q.needle).to_vec();
        if hits.is_empty() {
            if total == 0 {
                dim(ui, "gear-data.json loaded with zero items. The file is present but its items map is empty.");
            } else {
                dim(ui, "Nothing matches that name with these chips.");
            }
            return;
        }
        let mut clicked: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("items_list")
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, hits.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in range {
                    let i = hits[row];
                    let Some(c) = &self.cache else { break };
                    let f = &c.facts[i];
                    let name = &data.items[i].name;
                    let slots = f.slots.join(", ");
                    let era = f.era.as_deref().unwrap_or("");
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
                            text: &slots,
                            mono: true,
                            color: TEXT_3,
                            right: true,
                            width: 130.0,
                        },
                        Col {
                            text: era,
                            mono: true,
                            color: TEXT_3,
                            right: true,
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
                "Select an item to see everything gear-data.json carries for it.",
            );
            return None;
        };
        if self.detail.as_ref().map_or(true, |d| d.idx != i) {
            self.detail = Some(build_detail(i, data));
        }
        let d = self.detail.as_ref()?;
        let it = &data.items[i];
        let mut ask = None;
        egui::ScrollArea::vertical().id_salt("items_detail_scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.label(egui::RichText::new(&it.name).font(crate::fonts::display(18.0)).color(GOLD_HI));
            ui.add_space(6.0);
            if let Some(e) = &d.err {
                wrong_bar(ui, e);
            }
            kv_table(ui, "items_typed", &d.rows.typed);

            pane_section(ui, "WIKI STAT BLOCK");
            match &d.tooltip {
                Some(lines) if !lines.is_empty() => {
                    for l in lines {
                        /* the wiki's lines, compared line by line against another item's: mono */
                        mono(ui, l, TEXT);
                    }
                }
                Some(_) => dim(ui, &format!("{} has a record for this name with an empty stat block.", crate::data::TOOLTIPS_FILE)),
                None => dim(ui, &format!("No record for this name in {}.", crate::data::TOOLTIPS_FILE)),
            }

            pane_section(ui, "SOURCES");
            if d.sources.is_empty() && d.kinds.is_empty() && d.src_unknown.is_empty() {
                dim(ui, "No src on this record: gear-data.json lists no source for this item.");
            }
            if d.sources.is_empty() && !(d.kinds.is_empty() && d.src_unknown.is_empty()) {
                dim(ui, "No drop table (src.d) for this item.");
            }
            if d.zone_count > 0 {
                mono(ui, &format!("gear-data drop table: {} zone{}", d.zone_count, if d.zone_count == 1 { "" } else { "s" }), TEXT_3);
            }
            for sz in &d.sources {
                if link(ui, &sz.zone, FontId::proportional(12.5)).on_hover_text("open in Zones").clicked() {
                    crate::screens::zones::jump_to(&sz.zone);
                    ask = Some(crate::screens::Ask::ShowZone(sz.zone.clone()));
                }
                for m in &sz.mobs {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        mono(ui, &m.name, TEXT);
                        if let Some(l) = &m.level {
                            mono(ui, l, TEXT_2);
                        }
                        if let Some(r) = &m.rarity {
                            mono(ui, r, TEXT_3);
                        }
                    });
                }
            }
            if !d.kinds.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    dim(ui, "also");
                    mono(ui, &d.kinds.join(", "), TEXT);
                });
            }
            if !d.src_unknown.is_empty() {
                ui.add_space(4.0);
                dim(ui, "Other src keys, as the file has them (nothing measured says what they mean):");
                kv_table(ui, "items_src_unknown", &d.src_unknown);
            }
            ui.add_space(4.0);
            if d.quest_drops.is_empty() {
                dim(ui, &format!("{} drops: no row for this name.", crate::data::QUEST_ITEMS_FILE));
            } else {
                /* PLAIN TEXT, NOT A LINK. This was "N droppers" opening the Drops screen on
                 * this item; that screen is gone (see `nav::NAV`) and its whole DROPPED BY pane
                 * is the list printed immediately under this line, mob and zone, every row. A
                 * link that lands on what the reader is already looking at is a door to the room
                 * they are standing in, so the count stays and the door goes. */
                ui.horizontal(|ui| {
                    dim(ui, &format!("{} drops:", crate::data::QUEST_ITEMS_FILE));
                    mono(ui, &format!("{} dropper{}", d.quest_drops.len(), if d.quest_drops.len() == 1 { "" } else { "s" }), TEXT_2);
                });
                for (mob, zone) in &d.quest_drops {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        mono(ui, mob, TEXT);
                        if zone.is_empty() {
                            mono(ui, "zone not on the wiki", TEXT_3);
                        } else {
                            mono(ui, zone, TEXT_2);
                        }
                    });
                }
            }

            /* WHO SELLS IT, directly under who drops it. The drop table above answers half of
             * "where do I get this" and the wiki's `src.s` flag answered the other half with a
             * word; merchants.json answers it with a name, a zone and a price. */
            if let Some(a) = sold_by_section(
                ui,
                &d.sold_by,
                d.kinds.contains(&"sold"),
                &format!("{} lists no vendor for this name.", crate::data::MERCHANTS_FILE),
            ) {
                ask = Some(a);
            }

            pane_section(ui, "MORE");
            if d.rows.more.is_empty() {
                dim(ui, "Nothing beyond the typed fields: extra is empty for this record.");
            } else {
                kv_table(ui, "items_more", &d.rows.more);
            }
        });
        ask
    }
}

fn build_detail(i: usize, data: &crate::data::Snapshot) -> Detail {
    let it = &data.items[i];
    let sources = sources_of(it);
    let kinds = it.source_kinds();
    let src_unknown = src_unknown(it.src.as_ref());
    /* The two other files that know this name, through the snapshot's own indexes: the wiki's
     * stat block (item-tooltips.json) and the quest-items drop table. Absence is a fact printed
     * as one, never a blank. */
    let tooltip = data.tooltip(&it.name).map(|t| t.sb.clone());
    let quest_drops: Vec<(String, String)> = data
        .drop_sources(&it.name)
        .map(|d| {
            d.sources
                .iter()
                .map(|s| (s.mob.clone(), s.zone.clone()))
                .collect()
        })
        .unwrap_or_default();
    let zone_count = it.drop_zones().len();
    /* WHO SELLS IT, the sibling of the drop table. Built here with the rest of the detail, once
     * per selection, because the pane redraws sixty times a second and this walks the postings. */
    let sold_by = seller_rows(data, &it.name);
    let (rows, err) = match view(it) {
        Ok(v) => (
            detail_rows(&it.name, &v, &it.extra, PREFERRED, &["src"]),
            None,
        ),
        Err(e) => (
            detail_rows(&it.name, &View::new(), &it.extra, PREFERRED, &[]),
            Some(e),
        ),
    };
    Detail {
        idx: i,
        rows,
        sources,
        kinds,
        src_unknown,
        tooltip,
        quest_drops,
        zone_count,
        sold_by,
        err,
    }
}

/* ======================================================================================
 *  Tests.
 * ====================================================================================== */

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v(j: Value) -> View {
        match j {
            Value::Object(m) => m,
            _ => panic!("test view must be an object"),
        }
    }

    fn item(j: Value) -> Item {
        serde_json::from_value(j).expect("test item parses")
    }

    /* ---- the chip derivation, from a small Vec<Item> ---- */

    #[test]
    fn chips_are_distinct_counted_and_ordered_by_count_then_name() {
        let items = vec![
            item(json!({"n": "A", "sl": ["Neck"], "era": "Kunark Era"})),
            item(json!({"n": "B", "sl": ["Primary", "Secondary"], "era": "Velious Era"})),
            item(json!({"n": "C", "sl": ["Primary"]})),
            item(json!({"n": "D", "sl": ["Neck", "Primary"], "era": "Velious Era"})),
        ];
        let c = chips_of(&items);
        assert_eq!(
            c.slots,
            vec![
                ("Primary".to_owned(), 3),
                ("Neck".to_owned(), 2),
                ("Secondary".to_owned(), 1)
            ]
        );
        assert_eq!(
            c.eras,
            vec![("Velious Era".to_owned(), 2), ("Kunark Era".to_owned(), 1)]
        );
        assert_eq!(c.no_era, 1);
    }

    #[test]
    fn chips_break_count_ties_by_name_and_treat_an_empty_era_as_none() {
        let items = vec![item(json!({"n": "A", "sl": ["Wrist", "Arms"], "era": ""}))];
        let c = chips_of(&items);
        assert_eq!(
            c.slots,
            vec![("Arms".to_owned(), 1), ("Wrist".to_owned(), 1)]
        );
        assert!(c.eras.is_empty());
        assert_eq!(c.no_era, 1);
        assert!(chips_of(&[]).slots.is_empty());
    }

    /* ---- the detail table ordering ---- */

    #[test]
    fn detail_rows_order_preferred_then_alpha_then_more_alpha_and_never_name() {
        let view = v(
            json!({"name": "x", "wt": 1, "n": "X", "era": "Sky Era", "zeta": 1, "alpha": 2, "ex1": true, "ex0": null}),
        );
        let extra = v(json!({"ex1": true, "ex0": null}));
        let r = detail_rows("x", &view, &extra, &["n", "era"], &[]);
        let keys: Vec<&str> = r.typed.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["n", "era", "alpha", "wt", "zeta"]);
        let more: Vec<&str> = r.more.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(more, vec!["ex0", "ex1"]);
        assert!(!r.typed.iter().any(|(k, _)| k == "name"));
        assert_eq!(r.more[0].1, "null");
    }

    #[test]
    fn detail_rows_drop_the_heading_key_whatever_it_is_called_and_the_extra_bag() {
        /* `Item` serialises its name under the file's `n`; `Drop` serialises `extra` unflattened
         * as one nested object. Neither is a row. */
        let view = v(json!({"n": "A Bone Necklace", "extra": {"deity": "Karana"}, "wt": 1.0}));
        let r = detail_rows("A Bone Necklace", &view, &View::new(), &[], &[]);
        assert_eq!(r.typed, vec![("wt".to_owned(), "1".to_owned())]);
        /* A different `n` is information, not the heading, and stays. */
        let r = detail_rows("a bone necklace", &view, &View::new(), &[], &[]);
        assert_eq!(r.typed[0].0, "n");
    }

    #[test]
    fn detail_rows_summarise_expanded_arrays_with_a_count() {
        let view = v(json!({"mobs": [{"n": "a"}, {"n": "b"}], "city": false}));
        let r = detail_rows("", &view, &View::new(), &[], &["mobs"]);
        assert_eq!(
            r.typed,
            vec![
                ("city".to_owned(), "false".to_owned()),
                ("mobs".to_owned(), "2 entries, listed below".to_owned())
            ]
        );
    }

    #[test]
    fn a_real_item_type_yields_typed_rows_under_the_files_keys_and_extra_under_more() {
        let it = item(
            json!({"n": "T", "sl": ["Neck"], "st": {"ac": 2, "sta": 10}, "cls": {"all": 1}, "mystery": [1, 2]}),
        );
        let r = detail_rows(
            &it.name,
            &view(&it).unwrap(),
            &it.extra,
            PREFERRED,
            &["src"],
        );
        let typed: Vec<&str> = r.typed.iter().map(|(k, _)| k.as_str()).collect();
        assert!(typed.contains(&"sl") && typed.contains(&"st"), "{typed:?}");
        assert!(!typed.contains(&"n"), "the heading is not a row: {typed:?}");
        assert!(
            !typed.contains(&"cls") && !typed.contains(&"mystery"),
            "extra keys are not typed rows: {typed:?}"
        );
        let more: Vec<&str> = r.more.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(more, vec!["cls", "mystery"]);
        let st = r.typed.iter().find(|(k, _)| k == "st").unwrap();
        assert_eq!(
            st.1, "ac    2\nsta  10",
            "stats are a right aligned number column"
        );
    }

    #[test]
    fn render_value_right_aligns_numeric_objects_and_joins_scalar_arrays() {
        assert_eq!(render_value(&json!(["lore", "no_drop"])), "lore, no_drop");
        assert_eq!(
            render_value(&json!({"ac": 2, "dex": 10})),
            "ac    2\ndex  10"
        );
        /* The data lane types stats as f64: 2.0 prints as the file's 2, 1.5 stays 1.5. */
        assert_eq!(
            render_value(&json!({"ac": 2.0, "dex": 10.0})),
            "ac    2\ndex  10"
        );
        assert_eq!(render_value(&json!(2.0)), "2");
        assert_eq!(render_value(&json!(-3.0)), "-3");
        assert_eq!(render_value(&json!("SMALL")), "SMALL");
        assert_eq!(render_value(&json!(1.5)), "1.5");
        assert_eq!(num_text(12.8), "12.8");
        assert_eq!(
            render_value(&json!({"all": 1, "x": ["NEC"]})),
            r#"{"all":1,"x":["NEC"]}"#
        );
    }

    /* ---- the search box ---- */

    #[test]
    fn query_prefix_is_stripped_when_own_and_reported_when_foreign() {
        let q = parse_query("  i:Bone NECK ", 'i');
        assert_eq!(q.needle, "bone neck");
        assert_eq!(q.foreign_prefix, None);
        let q = parse_query("Z:sky", 'i');
        assert_eq!(q.needle, "sky");
        assert_eq!(q.foreign_prefix, Some('z'));
        let q = parse_query("plain", 'i');
        assert_eq!(q.needle, "plain");
        assert_eq!(q.foreign_prefix, None);
        let q = parse_query("i:", 'i');
        assert_eq!(q.needle, "");
    }

    /* ---- facts, sources, filters ---- */

    #[test]
    fn item_facts_fold_the_name_and_treat_an_empty_era_as_none() {
        let f = item_facts(&item(
            json!({"n": "A Bone Necklace", "sl": ["Neck"], "era": "Kunark Era"}),
        ));
        assert_eq!(
            f,
            ItemFacts {
                lname: "a bone necklace".into(),
                slots: vec!["Neck".into()],
                era: Some("Kunark Era".into())
            }
        );
        let f = item_facts(&item(json!({"n": "x", "sl": ["Ear"], "era": ""})));
        assert_eq!(f.slots, vec!["Ear".to_owned()]);
        assert_eq!(f.era, None);
        assert_eq!(
            item_facts(&item(json!({"n": "x"}))).slots,
            Vec::<String>::new()
        );
    }

    #[test]
    fn sources_are_read_positionally_with_nulls_kept_as_none() {
        let it = item(
            json!({"n": "T", "src": {"d": [["Droga", [["a goblin penmaster", null, null]]], ["Temple of Droga", [["a goblin penmaster", "34", "Common"]]]], "q": 1, "zz": [1]}}),
        );
        let s = sources_of(&it);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].zone, "Droga");
        assert_eq!(
            s[0].mobs,
            vec![SourceMob {
                name: "a goblin penmaster".into(),
                level: None,
                rarity: None
            }]
        );
        assert_eq!(
            s[1].mobs[0],
            SourceMob {
                name: "a goblin penmaster".into(),
                level: Some("34".into()),
                rarity: Some("Common".into())
            }
        );
        assert_eq!(it.source_kinds(), vec!["quest"]);
        assert_eq!(
            src_unknown(it.src.as_ref()),
            vec![("zz".to_owned(), "1".to_owned())]
        );
        assert!(sources_of(&item(json!({"n": "T", "src": {"c": 1}}))).is_empty());
        assert!(src_unknown(None).is_empty());
    }

    #[test]
    fn passes_combines_needle_slot_and_era() {
        let f = ItemFacts {
            lname: "ancient silk pantaloons".into(),
            slots: vec!["Legs".into()],
            era: Some("Velious Era".into()),
        };
        assert!(passes(
            &f,
            "silk",
            Some("Legs"),
            &EraFilter::Named("Velious Era".into())
        ));
        assert!(!passes(&f, "silk", Some("Neck"), &EraFilter::Any));
        assert!(!passes(&f, "silk", None, &EraFilter::None));
        assert!(!passes(&f, "wool", None, &EraFilter::Any));
        let bare = ItemFacts {
            lname: "x".into(),
            slots: vec![],
            era: None,
        };
        assert!(passes(&bare, "", None, &EraFilter::None));
    }

    /* ---- the real file. Fails loudly when absent; GRIMOIRE_NO_DATA=1 skips on purpose. ---- */

    #[test]
    fn real_gear_data_yields_the_measured_slots_eras_and_sources() {
        let Some(s) = crate::data::testdata::snapshot() else {
            return;
        };
        assert_eq!(
            s.items.len(),
            6891,
            "items count changed; re-measure the module comment"
        );
        let c = chips_of(&s.items);
        assert_eq!(c.slots.len(), 18, "18 distinct slots were measured");
        assert_eq!(c.slots[0], ("Primary".to_owned(), 1731));
        assert!(
            c.eras.iter().any(|(e, n)| e == "Velious Era" && *n == 2067),
            "{:?}",
            c.eras
        );
        assert_eq!(c.no_era, 1876);
        let with_sources = s
            .items
            .iter()
            .filter(|it| !sources_of(it).is_empty())
            .count();
        assert_eq!(with_sources, 3781, "3781 items carry src.d");
        /* The grouped reader and the data lane's flat reader see the same rows. */
        let grouped: usize = s
            .items
            .iter()
            .map(|it| sources_of(it).iter().map(|z| z.mobs.len()).sum::<usize>())
            .sum();
        let flat: usize = s.items.iter().map(|it| it.drop_rows().len()).sum();
        assert_eq!(grouped, flat);
        assert!(
            s.items
                .iter()
                .all(|it| src_unknown(it.src.as_ref()).is_empty()),
            "an src key nothing decodes appeared"
        );
        let bone = s.item("A Bone Necklace").expect("A Bone Necklace");
        let d = build_detail(
            s.items
                .iter()
                .position(|it| std::ptr::eq(it, bone))
                .unwrap(),
            s,
        );
        assert_eq!(
            d.sources
                .iter()
                .map(|z| z.zone.as_str())
                .collect::<Vec<_>>(),
            vec!["Droga", "Temple of Droga"]
        );
        assert_eq!(d.kinds, vec!["quest"]);
        assert!(
            d.rows.more.iter().any(|(k, _)| k == "cls"),
            "cls lives in extra and shows under MORE"
        );
        assert!(d
            .rows
            .typed
            .iter()
            .any(|(k, v)| k == "src" && v.ends_with("listed below")));
    }

    /// WHO SELLS IT, end to end through the screen helper the pane calls. The data lane proves the
    /// index; this proves the pane gets rows out of it, with a zone the reader can click and a
    /// price that came off the line rather than out of a formatter.
    #[test]
    fn seller_rows_name_a_merchant_a_place_and_a_price() {
        let Some(s) = crate::data::testdata::snapshot() else {
            return;
        };
        let rows = seller_rows(s, "Belt Pouch");
        assert!(!rows.is_empty(), "the wiki sells belt pouches");
        assert!(
            rows.iter().all(|r| !r.who.is_empty() && !r.zone.is_empty()),
            "every row names a shop and a place"
        );
        assert!(
            rows.iter().any(|r| r.price.is_some()),
            "at least one belt pouch has a price on the line"
        );
        /* the price is the wiki text, not a rendered number: it still carries a denomination */
        for r in rows
            .iter()
            .filter_map(|r| r.price.as_deref().map(str::to_owned))
        {
            assert!(
                r.chars().any(|c| c.is_ascii_alphabetic()) || r.contains('?'),
                "a price should read as the wiki wrote it: {r:?}"
            );
        }
        /* sorted by zone then merchant, which is what makes the section readable */
        let mut sorted = rows.clone();
        sorted.sort_by(|a, b| a.zone.cmp(&b.zone).then_with(|| a.who.cmp(&b.who)));
        assert_eq!(rows, sorted);
        /* and the raw source line is on every row, so the hover can show it */
        assert!(rows
            .iter()
            .all(|r| r.raw.to_lowercase().contains("belt pouch")));
        /* a name nobody sells comes back empty rather than panicking or inventing */
        assert!(seller_rows(s, "no such item, ever").is_empty());
    }

    /// THE ZONE LINK IS OFFERED ONLY WHERE THERE IS A PAGE TO LAND ON. A dead link is the defect
    /// this lane is about, wearing a different hat.
    #[test]
    fn a_zone_link_is_offered_only_when_the_snapshot_has_that_zone() {
        let Some(s) = crate::data::testdata::snapshot() else {
            return;
        };
        let mut offered = 0usize;
        let mut plain = 0usize;
        for it in &s.items {
            for r in seller_rows(s, &it.name) {
                match &r.zone_page {
                    Some(page) => {
                        offered += 1;
                        assert!(
                            s.zone(page).is_some(),
                            "{page:?} is offered as a link and this snapshot has no such zone"
                        );
                    }
                    None => {
                        plain += 1;
                        assert!(
                            s.zone(&r.zone).is_none(),
                            "{:?} has a page and was drawn as plain text",
                            r.zone
                        );
                    }
                }
            }
        }
        assert!(offered > 0, "no seller row offered a zone link at all");
        eprintln!("seller rows: {offered} with a zone page, {plain} without");
    }
}
