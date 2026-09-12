//! Screen: commission. Decision D9. The engine, in process: the catalogue, one recipe, a quote.
//!
//! THE CALL THAT USED TO LIVE IN main.rs WAS WRONG, AND THIS IS THE CORRECTION.
//! `App::ask` sent `{"kind":"catalogue","query":q}`. The engine's request enum (grimoire-wasm
//! `dispatch.rs`) is tagged by `op`, and `catalogue` takes one field, `corpus`, the bytes of the
//! `.grim` artifact. So every ask came back `{"error":"missing field `op`"}` and the screen showed
//! the refusal as if the user had typed something wrong. The request builders below are the shape
//! `dispatch.rs` deserialises, and the tests round-trip each one through the real `dispatch`.
//!
//! THE CORPUS IS READ ONCE, LAZILY, OFF THE UI THREAD. `web/corpus.grim` is 139 KB and travels
//! into every catalogue and recipe request as a JSON array of numbers, which is what the browser
//! does too. The array text is built once and reused, so a recipe click is a string concatenation
//! and not 139 thousand allocations.
//!
//! THE QUOTE IS RENDERED THE WAY `grimoire quote` PRINTS IT (grimoire-forge `quote.rs`), line for
//! line, in the monospace face, so a number here and a number in the terminal line up. Coin
//! columns are right aligned by padding in a face with tabular figures, which is the only way a
//! column of money can be subtracted by eye.

use crate::screens::Cx;
use crate::theme::*;
use egui::{Align2, FontId, Pos2, Rect, Sense, Ui, Vec2};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

/* ================================================================== request builders == */

/// The corpus bytes as the JSON array text `dispatch.rs` expects for `corpus: Vec<u8>`.
pub fn corpus_array(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 4 + 2);
    s.push('[');
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(itoa(*b));
    }
    s.push(']');
    s
}

fn itoa(b: u8) -> &'static str {
    /* 256 fixed strings, so the 139 KB corpus does not allocate a String per byte. */
    static TABLE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    &TABLE.get_or_init(|| (0..=255u32).map(|n| n.to_string()).collect())[b as usize]
}

/// `{"op":"catalogue","corpus":[..]}`
pub fn catalogue_request(corpus_json: &str) -> String {
    format!(r#"{{"op":"catalogue","corpus":{corpus_json}}}"#)
}

/// `{"op":"recipe","corpus":[..],"key":".."}`
pub fn recipe_request(corpus_json: &str, key: &str) -> String {
    let key = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".into());
    format!(r#"{{"op":"recipe","corpus":{corpus_json},"key":{key}}}"#)
}

/// The crafter, as `dispatch.rs` reads a `hand`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandSpec {
    pub skill: u16,
    pub mastery: u8,
    /// Fraction off for a guildmate, 0.0 to 1.0.
    pub courtesy: f64,
    pub owns_tools: bool,
}

/// `{"op":"quote","recipe":{..},"qty":N,"hand":{..},"buyer_supplies":bool}`
pub fn quote_request(recipe: &Value, qty: u32, hand: &HandSpec, buyer_supplies: bool) -> String {
    serde_json::json!({
        "op": "quote",
        "recipe": recipe,
        "qty": qty,
        "hand": {
            "skill": hand.skill,
            "mastery": hand.mastery,
            "courtesy": hand.courtesy,
            "owns_tools": hand.owns_tools,
        },
        "buyer_supplies": buyer_supplies,
    })
    .to_string()
}

/// `{"op":"chance","skill":N,"trivial":N}`, for the con colour the quote line names.
pub fn chance_request(skill: u16, trivial: u16) -> String {
    serde_json::json!({ "op": "chance", "skill": skill, "trivial": trivial }).to_string()
}

/// Call the engine and split its answer: a JSON object carrying `error` is a refusal.
pub fn engine(request: &str) -> Result<Value, String> {
    let out = grimoire_wasm::call(request);
    let v: Value = serde_json::from_str(&out)
        .map_err(|e| format!("the engine answered with something that is not JSON: {e}"))?;
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        return Err(err.to_string());
    }
    Ok(v)
}

/* ============================================================ what the engine answers == */

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CatalogueEntry {
    pub key: String,
    pub id: u32,
    pub name: String,
    pub skill: String,
    pub trivial: u16,
    pub parts: usize,
    /// Every component has a known vendor price, so the job can be fully quoted.
    pub priced: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MaterialLine {
    pub item: u32,
    pub name: String,
    pub units: u32,
    pub disposition: String,
    /// Copper. `Coin` serialises transparent.
    pub cost: i64,
    pub buyer_supplies: bool,
    pub crafter_owns: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToPost {
    pub item: u32,
    pub name: String,
    pub units: u32,
    pub source: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuoteOut {
    pub chance: f64,
    pub runs: u32,
    pub attempts: f64,
    pub materials: Vec<MaterialLine>,
    pub to_post: Vec<ToPost>,
    pub material_cost: i64,
    pub labour: i64,
    pub risk: i64,
    pub subtotal: i64,
    pub courtesy: i64,
    pub total: i64,
}

/* ================================================================================ coin == */

/// Norrathian money, rendered as the engine renders `Coin`: empty denominations dropped
/// (`4p 5c`, never `4p 0g 0s 5c`), `0c` for nothing, a plain minus for a negative amount.
pub fn coin(c: i64) -> String {
    if c == 0 {
        return "0c".into();
    }
    let n = c.abs();
    let parts = [
        (n / 1000, 'p'),
        ((n % 1000) / 100, 'g'),
        ((n % 100) / 10, 's'),
        (n % 10, 'c'),
    ];
    let mut s = String::new();
    if c < 0 {
        s.push('-');
    }
    let mut first = true;
    for (v, suffix) in parts {
        if v == 0 {
            continue;
        }
        if !first {
            s.push(' ');
        }
        s.push_str(&v.to_string());
        s.push(suffix);
        first = false;
    }
    s
}

/// `Coin::scale`: multiply and round once.
pub fn scale(c: i64, f: f64) -> i64 {
    let v = c as f64 * f;
    if v.is_finite() {
        v.round() as i64
    } else {
        0
    }
}

/// `Quote::with_gratuity`: a tip sits on top and never inside.
pub fn with_gratuity(total: i64, fraction: f64) -> i64 {
    total + scale(total, fraction.max(0.0))
}

/* ======================================================================= the lines == */

/// What the header lines of a quote name: the job and the hand, as the CLI prints them.
#[derive(Debug, Clone, PartialEq)]
pub struct QuoteHead {
    pub name: String,
    pub qty: u32,
    pub skill: String,
    pub trivial: u16,
    pub hand_skill: u16,
    pub con: String,
    /// Fraction, 0.0 to 1.0.
    pub courtesy: f64,
}

/// The priced job, line for line as `grimoire quote` prints it. Pure, so it is testable, and
/// the screen only has to paint strings.
pub fn quote_lines(h: &QuoteHead, q: &QuoteOut) -> Vec<String> {
    let courtesy = h.courtesy;
    let mut out = Vec::new();
    out.push(format!("{} x{}", h.name, h.qty));
    out.push(format!(
        "  {} · trivial {} · a hand of skill {} cons it {} and lands {:.0}% of the time",
        h.skill,
        h.trivial,
        h.hand_skill,
        h.con,
        q.chance * 100.0
    ));
    out.push(format!(
        "  {} combines wanted, {:.1} attempts expected",
        q.runs, q.attempts
    ));
    out.push(String::new());
    out.push("  materials".into());
    for m in &q.materials {
        let note = if m.buyer_supplies {
            "  (you post it)"
        } else if m.crafter_owns {
            "  (he has one)"
        } else {
            ""
        };
        out.push(format!(
            "    {:>4} x {:<28} {:>12}{note}",
            m.units,
            m.name,
            coin(m.cost)
        ));
    }
    if !q.to_post.is_empty() {
        out.push(String::new());
        out.push("  you post him".into());
        for p in &q.to_post {
            out.push(format!("    {:>4} x {:<28} {}", p.units, p.name, p.source));
        }
    }
    out.push(String::new());
    out.push(format!("  materials        {:>14}", coin(q.material_cost)));
    out.push(format!("  his work         {:>14}", coin(q.labour)));
    out.push(format!("  risk             {:>14}", coin(q.risk)));
    out.push("  ------------------------------".into());
    out.push(format!("  subtotal         {:>14}", coin(q.subtotal)));
    if q.courtesy != 0 {
        out.push(format!(
            "  guild courtesy   {:>14}   -{:.0}%",
            format!("-{}", coin(q.courtesy)),
            courtesy * 100.0
        ));
    }
    out.push(format!("  total            {:>14}", coin(q.total)));
    out.push(format!(
        "  with a 20% tip   {:>14}",
        coin(with_gratuity(q.total, 0.20))
    ));
    out
}

/* ============================================================================ corpus == */

/// Where the artifact is looked for, in order.
///
/// A shipped build asks only about the directory it is running from: `corpus.grim` beside the
/// executable, then `web/corpus.grim` under it, which is how a copy ships next to the binary.
///
/// The checkout's own `web/corpus.grim` is probed FIRST, and only by a development build. It is
/// reached from `CARGO_MANIFEST_DIR`, which is a directory on the machine that COMPILED the
/// binary, so a release build has no business probing it or printing it; it is compiled out for
/// the same reason the snapshot's source tree root is (`data::candidates_labelled`). A build run
/// out of a checkout keeps the behaviour it had: the artifact `grimoire corpus` just cut wins
/// over any stale copy beside the exe.
pub fn corpus_candidates() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    #[cfg(any(test, debug_assertions))]
    v.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("web")
            .join("corpus.grim"),
    );
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        v.push(dir.join("corpus.grim"));
        v.push(dir.join("web").join("corpus.grim"));
    }
    v
}

/// The loaded artifact and its catalogue, which is the same for the life of the process.
#[derive(Debug)]
pub struct Corpus {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    /// The bytes as JSON array text, built once.
    pub json: String,
    pub catalogue: Vec<CatalogueEntry>,
}

pub fn load_corpus(candidates: &[PathBuf]) -> Result<Corpus, String> {
    let Some(path) = candidates.iter().find(|p| p.is_file()) else {
        let tried: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
        return Err(format!(
            "corpus.grim is absent. Looked at: {}",
            tried.join(" ; ")
        ));
    };
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json = corpus_array(&bytes);
    let v = engine(&catalogue_request(&json)).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut catalogue: Vec<CatalogueEntry> = serde_json::from_value(v)
        .map_err(|e| format!("{}: catalogue shape: {e}", path.display()))?;
    catalogue.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.key.cmp(&b.key))
    });
    Ok(Corpus {
        path: path.clone(),
        bytes,
        json,
        catalogue,
    })
}

enum Load {
    Unloaded,
    Loading(Receiver<Result<Corpus, String>>),
    Ready(Arc<Corpus>),
    Failed(String),
}

/* ============================================================================ screen == */

pub struct CommissionScreen {
    load: Load,
    search: String,
    selected: Option<String>,
    recipe: Option<Value>,
    recipe_err: Option<String>,
    qty: u32,
    /// The crafter's skill. NOT a number this screen invents: it is 0 until a recipe is picked,
    /// then that recipe's trivial (the skill the wiki says the combine is trivial at, which is the
    /// one number the corpus knows about the hand), and a person's own entry once they touch it.
    skill: u16,
    skill_touched: bool,
    courtesy_pct: u8,
    owns_tools: bool,
    buyer_supplies: bool,
    quote: Option<Vec<String>>,
    quote_err: Option<String>,
}

impl Default for CommissionScreen {
    fn default() -> Self {
        Self {
            load: Load::Unloaded,
            search: String::new(),
            selected: None,
            recipe: None,
            recipe_err: None,
            qty: 1,
            skill: 0,
            skill_touched: false,
            courtesy_pct: 0,
            owns_tools: true,
            buyer_supplies: false,
            quote: None,
            quote_err: None,
        }
    }
}

impl CommissionScreen {
    /// Start reading the corpus on a worker, once. The App calls this at launch so the rail's
    /// The Commission square reports the read from the first frame rather than the first
    /// visit; the screen calls it too, which is a no-op after the first.
    pub fn start(&mut self) {
        if let Load::Unloaded = self.load {
            let (tx, rx) = channel();
            let candidates = corpus_candidates();
            std::thread::spawn(move || {
                let _ = tx.send(load_corpus(&candidates));
            });
            self.load = Load::Loading(rx);
        }
    }

    /// Collect a finished read. Never blocks. Returns true while the worker is still out, so a
    /// caller can keep the frame clock ticking.
    pub fn poll(&mut self) -> bool {
        if let Load::Loading(rx) = &self.load {
            match rx.try_recv() {
                Ok(Ok(c)) => self.load = Load::Ready(Arc::new(c)),
                Ok(Err(e)) => self.load = Load::Failed(e),
                Err(std::sync::mpsc::TryRecvError::Empty) => return true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.load =
                        Load::Failed("the corpus reader thread ended without an answer".into())
                }
            }
        }
        false
    }

    /// What the rail's square is decided from: the same three states this screen draws.
    pub fn corpus(&self) -> crate::nav::Corpus {
        match &self.load {
            Load::Unloaded | Load::Loading(_) => crate::nav::Corpus::Reading,
            Load::Ready(_) => crate::nav::Corpus::Ready,
            Load::Failed(_) => crate::nav::Corpus::Failed,
        }
    }

    fn ensure_loaded(&mut self, ctx: &egui::Context) {
        self.start();
        if self.poll() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn pick(&mut self, corpus: &Corpus, key: &str) {
        self.selected = Some(key.to_string());
        self.quote = None;
        self.quote_err = None;
        match engine(&recipe_request(&corpus.json, key)) {
            Ok(v) => {
                if !self.skill_touched {
                    self.skill = v.get("trivial").and_then(Value::as_u64).unwrap_or(0) as u16;
                }
                self.recipe = Some(v);
                self.recipe_err = None;
            }
            Err(e) => {
                self.recipe = None;
                self.recipe_err = Some(e);
            }
        }
    }

    fn quote(&mut self) {
        let Some(recipe) = &self.recipe else { return };
        let hand = HandSpec {
            skill: self.skill,
            mastery: 0,
            courtesy: self.courtesy_pct as f64 / 100.0,
            owns_tools: self.owns_tools,
        };
        let name = recipe
            .get("product_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let skill = recipe
            .get("skill")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let trivial = recipe.get("trivial").and_then(Value::as_u64).unwrap_or(0) as u16;
        /* The con word is the engine's answer to the same question the quote asks; a refusal on
         * either call is the quote's refusal, shown in words, never a "?" in the header. */
        let result = engine(&quote_request(recipe, self.qty, &hand, self.buyer_supplies))
            .and_then(|v| {
                serde_json::from_value::<QuoteOut>(v).map_err(|e| format!("quote shape: {e}"))
            })
            .and_then(|q| {
                let con = engine(&chance_request(self.skill, trivial)).and_then(|v| {
                    v.get("con")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .ok_or_else(|| "the chance answer carries no con".to_owned())
                })?;
                Ok((q, con))
            });
        match result {
            Ok((q, con)) => {
                let head = QuoteHead {
                    name,
                    qty: self.qty,
                    skill,
                    trivial,
                    hand_skill: self.skill,
                    con,
                    courtesy: hand.courtesy,
                };
                self.quote = Some(quote_lines(&head, &q));
                self.quote_err = None;
            }
            Err(e) => {
                self.quote = None;
                self.quote_err = Some(e);
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, _cx: &mut Cx) {
        self.ensure_loaded(ui.ctx());
        let corpus = match &self.load {
            Load::Ready(c) => c.clone(),
            Load::Loading(_) | Load::Unloaded => {
                state_line(
                    ui,
                    WORKING,
                    "reading corpus.grim and building the catalogue",
                );
                return;
            }
            Load::Failed(e) => {
                problem(ui, e);
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Cut one with  grimoire corpus web/corpus.grim --from <recipes dir>  or copy the repo's web/corpus.grim beside the executable.").color(TEXT_2));
                return;
            }
        };

        ui.label(
            egui::RichText::new("COMMISSION")
                .font(crate::fonts::display(16.0))
                .color(GOLD_HI),
        );
        ui.label(
            egui::RichText::new(
                "Name the thing you need, pick the recipe, and the engine prices the job.",
            )
            .color(TEXT_2),
        );
        ui.add_space(4.0);
        let priced = corpus.catalogue.iter().filter(|e| e.priced).count();
        ui.label(egui::RichText::new(format!(
            "{} · {} bytes · {} recipes, {priced} fully priced from vendor goods · engine in process",
            corpus.path.display(), corpus.bytes.len(), corpus.catalogue.len()
        )).font(FontId::monospace(10.5)).color(TEXT_3));
        ui.add_space(8.0);

        egui::Panel::left("commission_list")
            .default_size(380.0)
            .resizable(true)
            .frame(egui::Frame::NONE.fill(INK))
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("recipe, skill or key, substring")
                        .desired_width(f32::INFINITY)
                        .font(FontId::monospace(13.0)),
                );
                let needle = self.search.trim().to_lowercase();
                let hits: Vec<&CatalogueEntry> = corpus
                    .catalogue
                    .iter()
                    .filter(|e| {
                        needle.is_empty()
                            || e.name.to_lowercase().contains(&needle)
                            || e.skill.to_lowercase().contains(&needle)
                            || e.key.to_lowercase().contains(&needle)
                    })
                    .collect();
                ui.label(
                    egui::RichText::new(format!(
                        "{} of {} recipes",
                        hits.len(),
                        corpus.catalogue.len()
                    ))
                    .font(FontId::monospace(10.5))
                    .color(TEXT_3),
                );
                let mut pick: Option<String> = None;
                egui::ScrollArea::vertical()
                    .id_salt("commission_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if hits.is_empty() {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Nothing in the corpus is called that.")
                                    .color(TEXT_2),
                            );
                        }
                        for e in hits {
                            let sel = self.selected.as_deref() == Some(e.key.as_str());
                            let (rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 34.0),
                                Sense::click(),
                            );
                            let p = ui.painter();
                            if sel {
                                p.rect_filled(rect, 0.0, PANEL_2);
                                p.rect_filled(
                                    Rect::from_min_size(
                                        rect.left_top(),
                                        Vec2::new(2.0, rect.height()),
                                    ),
                                    0.0,
                                    GOLD,
                                );
                            } else if resp.hovered() {
                                p.rect_filled(rect, 0.0, PANEL);
                            }
                            let col = if sel {
                                FLARE
                            } else if resp.hovered() {
                                GOLD_HI
                            } else {
                                TEXT
                            };
                            p.text(
                                Pos2::new(rect.left() + 10.0, rect.top() + 11.0),
                                Align2::LEFT_CENTER,
                                &e.name,
                                FontId::proportional(12.5),
                                col,
                            );
                            let facts = format!(
                                "{} · trivial {} · {} parts{}",
                                e.skill,
                                e.trivial,
                                e.parts,
                                if e.priced { "" } else { " · not fully priced" }
                            );
                            p.text(
                                Pos2::new(rect.left() + 10.0, rect.top() + 25.0),
                                Align2::LEFT_CENTER,
                                facts,
                                FontId::proportional(10.5),
                                TEXT_3,
                            );
                            p.text(
                                Pos2::new(rect.right() - 8.0, rect.top() + 11.0),
                                Align2::RIGHT_CENTER,
                                format!("#{}", e.id),
                                FontId::monospace(10.5),
                                TEXT_3,
                            );
                            if resp.clicked() {
                                pick = Some(e.key.clone());
                            }
                        }
                    });
                if let Some(k) = pick {
                    self.pick(&corpus, &k);
                }
            });

        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(INK).inner_margin(egui::Margin::symmetric(12, 0))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("commission_detail").auto_shrink([false, false]).show(ui, |ui| {
                if let Some(e) = &self.recipe_err {
                    problem(ui, e);
                    return;
                }
                let Some(recipe) = self.recipe.clone() else {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Pick a recipe on the left.").color(TEXT_2));
                    return;
                };
                let name = recipe.get("product_name").and_then(Value::as_str).unwrap_or("");
                ui.label(egui::RichText::new(name).font(crate::fonts::display(16.0)).color(GOLD_HI));
                let skill = recipe.get("skill").and_then(Value::as_str).unwrap_or("");
                let trivial = recipe.get("trivial").and_then(Value::as_u64).unwrap_or(0);
                let yields = recipe.get("yields").and_then(Value::as_u64).unwrap_or(1);
                let no_fail = recipe.get("no_fail").and_then(Value::as_bool).unwrap_or(false);
                let id = recipe.get("product").and_then(Value::as_u64).unwrap_or(0);
                let mut facts = format!("{skill} · trivial {trivial} · yields {yields}");
                if no_fail {
                    facts.push_str(" · cannot fail");
                }
                if id > 0 {
                    facts.push_str(&format!(" · item #{id}"));
                }
                ui.label(egui::RichText::new(facts).color(TEXT_2));
                if let Some(effect) = recipe.get("effect").and_then(Value::as_str) {
                    ui.label(egui::RichText::new(effect).font(FontId::proportional(11.5)).color(TEXT_2));
                }

                caps(ui, "COMPONENTS");
                let comps = recipe.get("components").and_then(Value::as_array).cloned().unwrap_or_default();
                for c in &comps {
                    let qty = c.get("qty").and_then(Value::as_u64).unwrap_or(0);
                    let cname = c.get("name").and_then(Value::as_str).unwrap_or("");
                    let disp = c.get("disposition").and_then(Value::as_str).unwrap_or("");
                    let source = c.get("source").and_then(Value::as_str).unwrap_or("");
                    let price = c.get("unit_price").and_then(Value::as_i64).unwrap_or(0);
                    let disp_word = match disp {
                        "PerAttempt" => "per attempt",
                        "PerRun" => "per run",
                        "Once" => "once",
                        other => other,
                    };
                    let price_word = if source == "Vendor" { coin(price) } else { "no vendor".to_string() };
                    ui.monospace(format!("    {qty:>4} x {cname:<28} {price_word:>12}   {disp_word}, {}", source.to_lowercase()));
                }

                caps(ui, "QUOTE");
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("qty").color(TEXT_2));
                    ui.add(egui::DragValue::new(&mut self.qty).range(1..=999));
                    /* Until a person types a skill, the number in this box is the recipe's
                     * trivial, and the label says so ON the line: a box labelled "his skill"
                     * holding a number nobody entered is an invented fact about a crafter. */
                    ui.label(egui::RichText::new(if self.skill_touched { "his skill" } else { "his skill (the recipe's trivial until you set it)" }).color(TEXT_2));
                    let r = ui.add(egui::DragValue::new(&mut self.skill).range(0..=300));
                    if r.changed() {
                        self.skill_touched = true;
                    }
                    if !self.skill_touched {
                        r.on_hover_text("nobody has entered his skill; the quote is priced as if his skill were the recipe's trivial");
                    }
                    ui.label(egui::RichText::new("courtesy %").color(TEXT_2));
                    ui.add(egui::DragValue::new(&mut self.courtesy_pct).range(0..=100));
                    ui.checkbox(&mut self.owns_tools, "he owns the tools");
                    ui.checkbox(&mut self.buyer_supplies, "I post the un-buyable parts");
                    if ui.button(egui::RichText::new("Quote").color(FLARE)).clicked() {
                        self.quote();
                    }
                });
                if let Some(e) = &self.quote_err {
                    ui.add_space(6.0);
                    problem(ui, e);
                }
                if let Some(lines) = &self.quote {
                    ui.add_space(6.0);
                    for l in lines {
                        ui.label(egui::RichText::new(l).font(FontId::monospace(11.5)).color(TEXT));
                    }
                }
            });
        });
    }
}

fn caps(ui: &mut Ui, s: &str) {
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 14.0), Sense::hover());
    crate::chrome::tracked(ui, rect.left_top(), s, 9.5, 1.8, GOLD_DIM);
    ui.add_space(2.0);
}

/// A leading square in a state colour and a line of plain text. The colour is on the square,
/// where the vocabulary puts it, and never on the words.
fn state_line(ui: &mut Ui, col: egui::Color32, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        ui.painter().rect_filled(
            Rect::from_center_size(rect.center(), Vec2::splat(6.0)),
            0.0,
            col,
        );
        ui.label(
            egui::RichText::new(text)
                .font(FontId::monospace(11.5))
                .color(TEXT_2),
        );
    });
}

/// A refusal is a result. It gets the WRONG state colour and its own words, not an empty table
/// that would read as "nothing matched".
///
/// A WRAPPED LABEL, NOT A PAINTED LINE. The absent-corpus message lists every path that was
/// looked at, and a painted single line ran that off the right edge of the panel, which turned "it
/// is not here, put it there" into "it is not here" with the "there" clipped away. The frame grows
/// to the text and the WRONG bar is painted down its full height afterwards.
fn problem(ui: &mut Ui, msg: &str) {
    let full = ui.available_width();
    let out = egui::Frame::NONE
        .fill(SUNK)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(full - 24.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(msg)
                        .font(FontId::proportional(12.5))
                        .color(TEXT),
                )
                .wrap(),
            );
        });
    let r = out.response.rect;
    ui.painter().rect_filled(
        Rect::from_min_size(r.left_top(), Vec2::new(3.0, r.height())),
        0.0,
        WRONG,
    );
}

/* ============================================================================= tests == */

#[cfg(test)]
mod tests {
    use super::*;

    /// The recipe `dispatch.rs` tests with, so a quote here is a quote there.
    fn recipe() -> Value {
        serde_json::json!({
            "id": 1, "product": 9001, "product_name": "Gold Malachite Bracelet",
            "skill": "Jewelry Making", "trivial": 146, "yields": 1, "no_fail": false,
            "components": [
                {"item": 101, "name": "Gold Bar", "qty": 1, "disposition": "PerAttempt", "source": "Vendor", "unit_price": 800},
                {"item": 102, "name": "Malachite", "qty": 1, "disposition": "PerAttempt", "source": "Drop", "unit_price": 0}
            ]
        })
    }

    #[test]
    fn coin_renders_as_the_engine_does() {
        assert_eq!(coin(4005), "4p 5c");
        assert_eq!(coin(1234), "1p 2g 3s 4c");
        assert_eq!(coin(1000), "1p");
        assert_eq!(coin(7), "7c");
        assert_eq!(coin(0), "0c");
        assert_eq!(coin(-1234), "-1p 2g 3s 4c");
        assert_eq!(
            scale(216_000, 0.85),
            183_600,
            "15% courtesy off 216p, the mockup's worked example"
        );
        assert_eq!(with_gratuity(1000, 0.20), 1200);
        assert_eq!(
            with_gratuity(1000, -1.0),
            1000,
            "a negative tip is not a discount"
        );
    }

    #[test]
    fn corpus_array_is_the_json_array_dispatch_reads() {
        let s = corpus_array(&[0, 7, 255, 10]);
        assert_eq!(s, "[0,7,255,10]");
        let v: Vec<u8> = serde_json::from_str(&s).unwrap();
        assert_eq!(v, vec![0, 7, 255, 10]);
        assert_eq!(corpus_array(&[]), "[]");
    }

    #[test]
    fn the_catalogue_and_recipe_requests_carry_op_and_corpus() {
        let json = corpus_array(&[1, 2, 3]);
        let v: Value = serde_json::from_str(&catalogue_request(&json)).unwrap();
        assert_eq!(v["op"], "catalogue");
        assert_eq!(v["corpus"], serde_json::json!([1, 2, 3]));
        let r: Value =
            serde_json::from_str(&recipe_request(&json, "recipe/Jewelry Making/000001")).unwrap();
        assert_eq!(r["op"], "recipe");
        assert_eq!(r["key"], "recipe/Jewelry Making/000001");
        assert_eq!(r["corpus"], serde_json::json!([1, 2, 3]));
        // three bytes are not an artifact: the engine must refuse in words, not trap
        let e = engine(&catalogue_request(&json)).unwrap_err();
        assert!(!e.is_empty());
        // and the OLD shape main.rs sent is exactly what the engine refuses
        let old = engine(r#"{"kind":"catalogue","query":"ring"}"#).unwrap_err();
        assert!(old.contains("op"), "{old}");
    }

    #[test]
    fn a_quote_request_round_trips_through_the_real_dispatch() {
        let hand = HandSpec {
            skill: 146,
            mastery: 0,
            courtesy: 0.15,
            owns_tools: true,
        };
        let req = quote_request(&recipe(), 10, &hand, false);
        let v: Value = serde_json::from_str(&req).unwrap();
        assert_eq!(v["op"], "quote");
        assert_eq!(v["qty"], 10);
        assert_eq!(v["hand"]["skill"], 146);
        assert_eq!(v["hand"]["owns_tools"], true);
        assert_eq!(v["buyer_supplies"], false);
        let out = engine(&req).expect("dispatch accepts the request");
        let q: QuoteOut =
            serde_json::from_value(out).expect("the quote shape is what the engine sends");
        assert_eq!(q.runs, 10);
        assert!(q.total > 0);
        assert!(q.courtesy > 0);
        assert_eq!(q.total, q.subtotal - q.courtesy);
        assert_eq!(q.materials.len(), 2);
        // the buyer posting parts moves the drop to the to_post list
        let mine = engine(&quote_request(&recipe(), 10, &hand, true)).unwrap();
        let qm: QuoteOut = serde_json::from_value(mine).unwrap();
        assert_eq!(
            qm.to_post
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Malachite"]
        );
        assert_eq!(qm.to_post[0].source, "Drop");
    }

    #[test]
    fn chance_request_names_the_con() {
        let v = engine(&chance_request(83, 83)).unwrap();
        assert_eq!(v["con"], "grey");
        assert_eq!(v["trivial_to_him"], true);
    }

    #[test]
    fn quote_lines_match_the_cli_shape() {
        let hand = HandSpec {
            skill: 146,
            mastery: 0,
            courtesy: 0.15,
            owns_tools: true,
        };
        let q: QuoteOut =
            serde_json::from_value(engine(&quote_request(&recipe(), 10, &hand, false)).unwrap())
                .unwrap();
        let head = QuoteHead {
            name: "Gold Malachite Bracelet".into(),
            qty: 10,
            skill: "Jewelry Making".into(),
            trivial: 146,
            hand_skill: 146,
            con: "grey".into(),
            courtesy: 0.15,
        };
        let lines = quote_lines(&head, &q);
        assert_eq!(lines[0], "Gold Malachite Bracelet x10");
        assert!(
            lines[1].starts_with(
                "  Jewelry Making · trivial 146 · a hand of skill 146 cons it grey and lands "
            ),
            "{}",
            lines[1]
        );
        assert!(lines[2].ends_with(" attempts expected"));
        assert_eq!(lines[4], "  materials");
        let gold = lines.iter().find(|l| l.contains("Gold Bar")).unwrap();
        // "    {:>4} x {:<28} {:>12}": the coin column ends at a fixed column, right aligned
        assert_eq!(gold.len(), 4 + 4 + 3 + 28 + 1 + 12, "{gold:?}");
        assert!(gold.ends_with(&coin(q.materials[0].cost)));
        let sub = lines.iter().find(|l| l.starts_with("  subtotal")).unwrap();
        assert_eq!(sub.len(), "  subtotal         ".len() + 14, "{sub:?}");
        let c = lines
            .iter()
            .find(|l| l.starts_with("  guild courtesy"))
            .unwrap();
        assert!(c.ends_with("-15%"), "{c}");
        assert!(c.contains(&format!("-{}", coin(q.courtesy))));
        assert!(lines.last().unwrap().starts_with("  with a 20% tip"));
        assert!(lines
            .last()
            .unwrap()
            .ends_with(&coin(with_gratuity(q.total, 0.2))));
        assert!(
            lines
                .iter()
                .all(|l| !l.contains('\u{2014}') && !l.contains('\u{2013}')),
            "no dashes of either length"
        );
    }

    /* The real artifact. Fails loudly when absent, unless GRIMOIRE_NO_DATA=1 says so on purpose. */
    fn real_corpus() -> Option<Corpus> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("web")
            .join("corpus.grim");
        if !path.is_file() {
            if std::env::var("GRIMOIRE_NO_DATA").as_deref() == Ok("1") {
                crate::data::testdata::say_skipped(&format!(
                    "{} is absent and GRIMOIRE_NO_DATA=1",
                    path.display()
                ));
                return None;
            }
            panic!("{} is absent. Cut one with  grimoire corpus web/corpus.grim --from <recipes>  or set GRIMOIRE_NO_DATA=1 to skip on purpose.", path.display());
        }
        Some(load_corpus(&[path]).expect("the real corpus opens and lists"))
    }

    #[test]
    fn the_real_corpus_lists_and_prices_a_recipe() {
        let Some(c) = real_corpus() else { return };
        assert!(!c.catalogue.is_empty());
        assert!(
            c.catalogue
                .windows(2)
                .all(|w| w[0].name.to_lowercase() <= w[1].name.to_lowercase()),
            "sorted by name"
        );
        let first = &c.catalogue[0];
        let rec = engine(&recipe_request(&c.json, &first.key)).expect("recipe by key");
        assert_eq!(rec["product_name"], first.name);
        let hand = HandSpec {
            skill: first.trivial,
            mastery: 0,
            courtesy: 0.0,
            owns_tools: true,
        };
        let q: QuoteOut =
            serde_json::from_value(engine(&quote_request(&rec, 1, &hand, false)).unwrap()).unwrap();
        assert_eq!(q.runs, 1);
        assert!(q.total >= 0);
        let e = load_corpus(&[PathBuf::from("C:/nowhere/corpus.grim")]).unwrap_err();
        assert!(e.contains("C:/nowhere/corpus.grim"), "{e}");
    }
}
