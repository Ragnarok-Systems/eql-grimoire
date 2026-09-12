//! Screen: LFG. Decisions D4 (chords land here in one of three modes), D5 (GROUP: LFG, Raid,
//! Motes), D9.
//!
//! WHAT THIS IS, AND WHAT IT IS NOT.
//! A local board. Entries are written on this machine, kept in this machine's settings, and read
//! by nobody else, because no transport exists between installs tonight and the goal doc says
//! nothing on screen may be invented. So there are no "other players" here. What the board DOES
//! do is hold the lines you mean to say in game and hand each one to the clipboard already shaped
//! as an `/ooc` or `/shout` line, so the chord to open it, the click to copy it, and the paste in
//! game are the whole loop. The screen says exactly that, in one dim line.
//!
//! THREE MODES, ONE SCREEN. `Ctrl+Alt+L` opens Generic, `Ctrl+Alt+L` then `R` opens Raid,
//! `Ctrl+Alt+L` then `M` opens Motes. The Windows registry sets `mode` on open; the three chips at
//! the top switch it by hand. The mode changes the prefix of the copied line and nothing else, so
//! one board holds all three kinds of entry and each remembers which kind it was.
//!
//! WHO YOU ARE. Nothing asks: the busiest `eqlog_<Character>_<server>.txt` in the log folder IS
//! the character being played, and the character's name is in that file name.
//! Both rules already live in `crate::ingest`, which lists the folder,
//! picks the newest file and tails it, and this screen does NOT carry a second copy of them: two
//! implementations of "which log is active" is how the Parser and the LFG board end up naming two
//! different characters on the same afternoon. The board reads the ingest's `sources()` report,
//! takes the log it is actually reading, and applies the ingest's own `character_of_log` to its
//! name. What this screen owns is the mapping from that report to a line a person can act on,
//! which is `who_of` below, and that carries the tests.
//!
//! CLASS AND LEVEL are typed, not derived: sixteen class codes, a level clamped to 1..50. The log
//! does not state either reliably, so the compose row asks and never guesses. An empty field is
//! omitted from the copied line.

use crate::ingest::{character_of_log, Source, SourceKind};
use crate::screens::Cx;
use crate::settings::Settings;
use crate::theme::*;
use crate::windows::LfgMode;
use chrono::{DateTime, Local, Utc};
use egui::{FontId, RichText, Stroke, Ui};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/* ---------------------------------------------------------------- persistence -- */

/// The key under which the board lives in the settings file's extra map.
///
/// Named here and nowhere else. The settings lane owns the file; this screen owns the shape of
/// what sits under this one key. (Reported to the integrator as the coordination point.)
pub const BOARD_KEY: &str = "lfg_board";

/// The persisted shape under `BOARD_KEY`. Versioned so a later transport can migrate it rather
/// than guess at it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Board {
    /// Bumped when the shape changes. 1 = this file.
    #[serde(default = "board_version")]
    pub version: u32,
    /// Oldest first, as written. The screen shows newest first.
    #[serde(default)]
    pub entries: Vec<Entry>,
    /// What the compose row last carried for class and level, so they survive a restart. Both
    /// optional: empty means "not stated" and stays out of the copied line.
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub level: Option<u32>,
}

fn board_version() -> u32 {
    1
}

/// One line on the board.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub when: DateTime<Utc>,
    /// Character name from the log the ingest was reading when the entry was posted. Empty when
    /// no log was being read; the row then shows "(unknown)" rather than a made-up name.
    pub who: String,
    pub what: String,
    pub mode: Mode,
    /// Class code and level as they stood when the entry was posted, kept with the entry so the
    /// copied line is reproducible after the compose row changes.
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub level: Option<u32>,
}

/// The board's own copy of the three modes, so the persisted JSON does not depend on whatever
/// derives `crate::windows::LfgMode` happens to carry. Converted at the one boundary below.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Generic,
    Raid,
    Motes,
}

impl Mode {
    pub fn of(m: &LfgMode) -> Mode {
        match m {
            LfgMode::Generic => Mode::Generic,
            LfgMode::Raid => Mode::Raid,
            LfgMode::Motes => Mode::Motes,
        }
    }
    pub fn to_lfg(self) -> LfgMode {
        match self {
            Mode::Generic => LfgMode::Generic,
            Mode::Raid => LfgMode::Raid,
            Mode::Motes => LfgMode::Motes,
        }
    }
    /// The heading, a caps run, so it is drawn in Cinzel.
    pub fn heading(self) -> &'static str {
        match self {
            Mode::Generic => "LOOKING FOR GROUP",
            Mode::Raid => "LOOKING FOR RAID",
            Mode::Motes => "LOOKING FOR MOTES",
        }
    }
    /// The chip label.
    pub fn chip(self) -> &'static str {
        match self {
            Mode::Generic => "Group",
            Mode::Raid => "Raid",
            Mode::Motes => "Motes",
        }
    }
    /// The prefix of the in-game line. Exactly the three strings the lane brief specifies.
    pub fn prefix(self) -> &'static str {
        match self {
            Mode::Generic => "LFG:",
            Mode::Raid => "LFR:",
            Mode::Motes => "LF Motes (D4):",
        }
    }
    pub const ALL: [Mode; 3] = [Mode::Generic, Mode::Raid, Mode::Motes];
}

/// Read the board out of settings. Absent gives an empty board; unreadable says so through `Err`
/// so the screen can show it rather than silently starting over.
pub fn board_from(settings: &Settings) -> Result<Board, String> {
    /* THE ONE PLACE THIS SCREEN TOUCHES THE SETTINGS LANE'S EXTRA MAP (read). Shape as the
     * settings lane wrote it: `pub extra: serde_json::Map<String, serde_json::Value>`, flattened
     * into the file, so this key survives every save the settings screen makes. */
    match settings.extra.get(BOARD_KEY) {
        None => Ok(Board {
            version: board_version(),
            ..Board::default()
        }),
        Some(v) => {
            let b = serde_json::from_value::<Board>(v.clone())
                .map_err(|e| format!("settings key {BOARD_KEY} is not a board: {e}"))?;
            /* THE VERSION IS CHECKED, NOT JUST STAMPED. A board written by a later build with a
             * shape this one does not know is refused with the two numbers, so the screen says
             * "saved by a newer build" instead of drawing whatever fields happened to match and
             * then saving over the rest. Version 1 is the first shape, so there is nothing older
             * to migrate; an absent field defaulted to 1 above. */
            if b.version > board_version() {
                return Err(format!(
                    "settings key {BOARD_KEY} was saved by a newer build (board version {}, this build reads {}); it is left untouched",
                    b.version,
                    board_version()
                ));
            }
            Ok(b)
        }
    }
}

/// Write the board into settings. Does not save; the caller saves and reports.
pub fn board_into(settings: &mut Settings, board: &Board) -> Result<(), String> {
    let v = serde_json::to_value(board).map_err(|e| format!("board would not serialise: {e}"))?;
    /* THE ONE PLACE THIS SCREEN TOUCHES THE SETTINGS LANE'S EXTRA MAP (write). */
    settings.extra.insert(BOARD_KEY.to_owned(), v);
    Ok(())
}

/* --------------------------------------------------------------- class and level -- */

/// The sixteen class codes, in the game's class order. A typed class that is not one of these is
/// refused, not shouted into `/ooc` as typed.
pub const CLASSES: [&str; 16] = [
    "WAR", "CLR", "PAL", "RNG", "SHD", "DRU", "MNK", "BRD", "ROG", "SHM", "NEC", "WIZ", "MAG",
    "ENC", "BST", "BER",
];

/// The game's level cap.
pub const MAX_LEVEL: u32 = 50;

/// Normalise a typed class code: trimmed, upper cased, and one of the sixteen. `Ok("")` for an
/// empty field (omitted from the line), `Err` naming the rejected text otherwise.
pub fn class_code(typed: &str) -> Result<String, String> {
    let t = typed.trim().to_ascii_uppercase();
    if t.is_empty() {
        return Ok(String::new());
    }
    if CLASSES.contains(&t.as_str()) {
        Ok(t)
    } else {
        Err(format!("{} is not a class code (WAR, CLR, PAL, RNG, SHD, DRU, MNK, BRD, ROG, SHM, NEC, WIZ, MAG, ENC, BST, BER)", typed.trim()))
    }
}

/// A parsed level, clamped into 1..=50. Only a parsed level: a blank field must stay blank rather
/// than default to a level nobody typed, so a parse failure is reported instead.
pub fn clamp_level(v: u32) -> u32 {
    v.clamp(1, MAX_LEVEL)
}

/// Parse the typed level. `Ok(None)` for an empty field, `Err` for text that is not a number.
pub fn level_of(typed: &str) -> Result<Option<u32>, String> {
    let t = typed.trim();
    if t.is_empty() {
        return Ok(None);
    }
    match t.parse::<u32>() {
        Ok(0) => Err("level 0 is not a level".to_owned()),
        Ok(n) => Ok(Some(clamp_level(n))),
        Err(_) => Err(format!("{t} is not a level")),
    }
}

/// The in-game line for an entry, exactly as it goes onto the clipboard.
///
/// `LFG: <class> <lvl> <text>` for Generic, `LFR: ...` for Raid, `LF Motes (D4): ...` for Motes.
/// A part that is not known is left out rather than filled in, and the result is single spaced so
/// an omitted part never leaves a double space behind.
pub fn copy_line(mode: Mode, class: &str, level: Option<u32>, text: &str) -> String {
    let mut parts: Vec<String> = vec![mode.prefix().to_owned()];
    let c = class.trim();
    if !c.is_empty() {
        parts.push(c.to_owned());
    }
    if let Some(l) = level {
        parts.push(l.to_string());
    }
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !t.is_empty() {
        parts.push(t);
    }
    parts.join(" ")
}

/// Where the character name came from, or why there is none. Shown next to the compose row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Who {
    /// A character name, and the file it was read from.
    Named { who: String, file: String },
    /// A log is being read, but its name does not carry a character (the client names logs
    /// `eqlog_<Character>_<server>.txt`; a renamed file may not).
    NoCharacter(String),
    /// The ingest is still reading the log folder for the first time. Not a failure.
    Scanning,
    /// A log folder exists but nothing in it is being read; the ingest's own words say why.
    NotTailed(String),
    /// The ingest found no log folder at all.
    NoLogDir,
}

impl Who {
    pub fn name(&self) -> &str {
        match self {
            Who::Named { who, .. } => who.as_str(),
            _ => "",
        }
    }
    /// The line the compose row shows.
    pub fn line(&self) -> String {
        match self {
            Who::Named { who, file } => format!("{who} (from {file})"),
            Who::NoCharacter(f) => format!("character unknown: {f} does not name one (expected eqlog_<Character>_<server>.txt)"),
            Who::Scanning => "character: reading the log folder".to_owned(),
            Who::NotTailed(why) => format!("character unknown: {why}"),
            Who::NoLogDir => "character unknown: set the log folder in Settings".to_owned(),
        }
    }
}

/// The character, from the ingest's report of what it is reading.
///
/// The tailed log is the one `Source` of kind `Log` with a `last_read`; the ingest marks every
/// other log "not tailed". A `Log` source with no `last_read` and no active one carries the
/// reason in `problem` (no files, unreadable folder). No `Log` source at all means the ingest had
/// no folder to list, unless it is still on its first scan.
pub fn who_of(sources: &[Source], scanning: bool) -> Who {
    let logs: Vec<&Source> = sources
        .iter()
        .filter(|s| matches!(s.kind, SourceKind::Log))
        .collect();
    if let Some(active) = logs.iter().find(|s| s.last_read.is_some()) {
        let file = active
            .path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        return match character_of_log(&file) {
            Some(who) => Who::Named { who, file },
            None => Who::NoCharacter(file),
        };
    }
    if let Some(first) = logs.first() {
        return Who::NotTailed(
            first
                .problem
                .clone()
                .unwrap_or_else(|| format!("{} is not being read", first.path.display())),
        );
    }
    if scanning {
        return Who::Scanning;
    }
    Who::NoLogDir
}

/* -------------------------------------------------------------------- the screen -- */

pub struct LfgScreen {
    /// The mode the window was opened in (D4), or the chip the user last pressed.
    pub mode: LfgMode,
    board: Board,
    /// Loaded from settings on the first frame, because `Default` runs before settings exist.
    loaded: bool,
    /// The compose row.
    text: String,
    class_typed: String,
    level_typed: String,
    /// What the compose row last refused, if anything. Cleared on the next successful post.
    refused: Option<String>,
    /// The last save or load failure. Shown until a later save succeeds.
    problem: Option<String>,
    /// Set for a moment after a copy so the button can say it happened.
    copied_at: Option<Instant>,
}

impl Default for LfgScreen {
    fn default() -> Self {
        Self {
            mode: LfgMode::Generic,
            board: Board::default(),
            loaded: false,
            text: String::new(),
            class_typed: String::new(),
            level_typed: String::new(),
            refused: None,
            problem: None,
            copied_at: None,
        }
    }
}

impl LfgScreen {
    fn mode(&self) -> Mode {
        Mode::of(&self.mode)
    }

    fn load_once(&mut self, cx: &mut Cx) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        match board_from(cx.settings) {
            Ok(b) => {
                self.class_typed = b.class.clone();
                self.level_typed = b.level.map(|l| l.to_string()).unwrap_or_default();
                self.board = b;
            }
            Err(e) => {
                /* Keep the bad value in place rather than overwriting it on the next save: the
                 * next save happens only when the user posts, and the message says what is wrong
                 * until then. */
                self.problem = Some(e);
            }
        }
    }

    fn persist(&mut self, cx: &mut Cx) {
        self.board.class = self.class_typed.trim().to_ascii_uppercase();
        self.board.level = level_of(&self.level_typed).ok().flatten();
        let r = board_into(cx.settings, &self.board).and_then(|_| cx.settings.save());
        self.problem = r.err();
    }

    /// The compose row's current line, or why it cannot be made.
    fn compose_line(&self) -> Result<String, String> {
        let class = class_code(&self.class_typed)?;
        let level = level_of(&self.level_typed)?;
        Ok(copy_line(self.mode(), &class, level, &self.text))
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        self.load_once(cx);
        /* Read each frame: `sources()` is a handful of clones, and the ingest re-lists the folder
         * on its own clock, so there is nothing here worth a second timer. While the first scan
         * runs, the line says so and repaints are kept coming so it clears without a click. */
        let who = who_of(&cx.ingest.sources(), cx.ingest.scanning());
        if who == Who::Scanning {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        let mode = self.mode();

        /* The heading, a caps run, in Cinzel. */
        ui.label(
            RichText::new(mode.heading())
                .font(crate::fonts::display(16.0))
                .color(GOLD_HI),
        );
        ui.add_space(6.0);

        /* Three chips. The active one is filled; the others sit on the ground with a hairline. */
        ui.horizontal(|ui| {
            for m in Mode::ALL {
                let on = m == mode;
                let txt = RichText::new(m.chip())
                    .font(FontId::proportional(12.5))
                    .color(if on { FLARE } else { TEXT_2 });
                let b = egui::Button::new(txt)
                    .fill(if on {
                        PANEL_2
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .stroke(Stroke::new(1.0, if on { GOLD_DIM } else { RULE }))
                    .corner_radius(egui::CornerRadius::ZERO);
                if ui.add(b).clicked() {
                    self.mode = m.to_lfg();
                }
            }
        });
        ui.add_space(4.0);

        /* The one honest line about what this board is. */
        ui.label(
            RichText::new("This board is local to this machine. No transport exists yet, so nobody else's entries appear here.")
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
        ui.add_space(10.0);

        /* The compose row. */
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("you")
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
            );
            let who_col = if matches!(who, Who::Named { .. }) {
                TEXT
            } else {
                TEXT_3
            };
            /* a file name and a character name are compared character by character: monospace */
            ui.label(
                RichText::new(who.line())
                    .font(FontId::monospace(11.5))
                    .color(who_col),
            );
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("class")
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
            );
            /* Hints name the field, never a sample value: a sample class in a class box reads as
             * a class somebody chose. */
            ui.add(
                egui::TextEdit::singleline(&mut self.class_typed)
                    .hint_text("code")
                    .desired_width(46.0)
                    .font(FontId::monospace(12.0)),
            );
            ui.label(
                RichText::new("level")
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.level_typed)
                    .hint_text("lvl")
                    .desired_width(34.0)
                    .font(FontId::monospace(12.0)),
            );
        });
        ui.add_space(4.0);
        let mut post = false;
        let mut copy_now = false;
        ui.horizontal(|ui| {
            let hint = match mode {
                Mode::Generic => "what you are looking for",
                Mode::Raid => "which raid, when",
                Mode::Motes => "where you are farming, how many",
            };
            let te = egui::TextEdit::singleline(&mut self.text)
                .hint_text(hint)
                .desired_width((ui.available_width() - 190.0).max(160.0))
                .font(FontId::proportional(12.5));
            let r = ui.add(te);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                post = true;
            }
            if ui.button(RichText::new("Post").color(FLARE)).clicked() {
                post = true;
            }
            let recently = self
                .copied_at
                .is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
            let label = if recently { "Copied" } else { "Copy line" };
            /* "Copied" is a settled state for a moment, then the button goes back to being a
             * button. Keep the frames coming so it does. */
            if recently {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
            /* The word changes; the colour does not. SETTLED is a state square's colour, and a
             * button that just did its job is not a row that has its data. */
            if ui.button(RichText::new(label).color(TEXT)).clicked() {
                copy_now = true;
            }
        });

        /* The line as it will be copied, so the prefix is visible before the paste. Monospace: a
         * chat line is compared character by character against what lands in game. */
        match self.compose_line() {
            Ok(line) => {
                ui.label(
                    RichText::new(line)
                        .font(FontId::monospace(11.5))
                        .color(TEXT_2),
                );
            }
            Err(e) => {
                /* A refusal is a state, so it takes the state colour. */
                ui.label(
                    RichText::new(e)
                        .font(FontId::proportional(11.5))
                        .color(WRONG),
                );
            }
        }

        if copy_now {
            match self.compose_line() {
                Ok(line) => {
                    ui.ctx().copy_text(line);
                    self.copied_at = Some(Instant::now());
                    self.refused = None;
                }
                Err(e) => self.refused = Some(e),
            }
        }
        if post {
            match (class_code(&self.class_typed), level_of(&self.level_typed)) {
                (Ok(class), Ok(level)) if !self.text.trim().is_empty() => {
                    self.board.entries.push(Entry {
                        when: Utc::now(),
                        who: who.name().to_owned(),
                        what: self.text.split_whitespace().collect::<Vec<_>>().join(" "),
                        mode,
                        class,
                        level,
                    });
                    self.text.clear();
                    self.refused = None;
                    self.persist(cx);
                }
                (Err(e), _) | (_, Err(e)) => self.refused = Some(e),
                _ => self.refused = Some("nothing to post: the line is empty".to_owned()),
            }
        }
        if let Some(r) = &self.refused {
            ui.label(
                RichText::new(r)
                    .font(FontId::proportional(11.5))
                    .color(WRONG),
            );
        }
        if let Some(p) = &self.problem {
            ui.label(
                RichText::new(format!("not saved: {p}"))
                    .font(FontId::proportional(11.5))
                    .color(WRONG),
            );
        }

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(6.0);

        /* The board, newest first. */
        if self.board.entries.is_empty() {
            ui.label(
                RichText::new(
                    "Nothing posted yet. Post a line above and it stays here across restarts.",
                )
                .font(FontId::proportional(12.0))
                .color(TEXT_3),
            );
            return;
        }
        let mut remove: Option<usize> = None;
        let mut copy: Option<usize> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, e) in self.board.entries.iter().enumerate().rev() {
                ui.horizontal(|ui| {
                    let when: DateTime<Local> = e.when.with_timezone(&Local);
                    ui.label(
                        RichText::new(when.format("%Y-%m-%d %H:%M").to_string())
                            .font(FontId::monospace(11.0))
                            .color(TEXT_3),
                    );
                    let who = if e.who.is_empty() {
                        "(unknown)"
                    } else {
                        e.who.as_str()
                    };
                    ui.label(
                        RichText::new(who)
                            .font(FontId::monospace(11.5))
                            .color(TEXT_2),
                    );
                    ui.label(
                        RichText::new(e.mode.prefix())
                            .font(FontId::monospace(11.0))
                            .color(GOLD_DIM),
                    );
                    ui.label(
                        RichText::new(&e.what)
                            .font(FontId::proportional(12.5))
                            .color(TEXT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("Remove").color(TEXT_2)).clicked() {
                            remove = Some(i);
                        }
                        if ui.button(RichText::new("Copy").color(TEXT)).clicked() {
                            copy = Some(i);
                        }
                    });
                });
            }
        });
        if let Some(i) = copy {
            let e = &self.board.entries[i];
            ui.ctx()
                .copy_text(copy_line(e.mode, &e.class, e.level, &e.what));
            self.copied_at = Some(Instant::now());
        }
        if let Some(i) = remove {
            self.board.entries.remove(i);
            self.persist(cx);
        }
    }
}

/* ------------------------------------------------------------------------ tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn copy_line_generic_is_lfg_class_level_text() {
        assert_eq!(
            copy_line(Mode::Generic, "WAR", Some(50), "tank for Sky"),
            "LFG: WAR 50 tank for Sky"
        );
    }

    #[test]
    fn copy_line_raid_is_lfr() {
        assert_eq!(
            copy_line(Mode::Raid, "CLR", Some(48), "PoSky 8pm"),
            "LFR: CLR 48 PoSky 8pm"
        );
    }

    #[test]
    fn copy_line_motes_is_lf_motes_d4() {
        assert_eq!(
            copy_line(Mode::Motes, "NEC", Some(50), "Dreadlands, 2 spots"),
            "LF Motes (D4): NEC 50 Dreadlands, 2 spots"
        );
    }

    #[test]
    fn copy_line_omits_what_is_not_known_without_double_spaces() {
        assert_eq!(
            copy_line(Mode::Generic, "", None, "  any   role "),
            "LFG: any role"
        );
        assert_eq!(copy_line(Mode::Generic, "WAR", None, "x"), "LFG: WAR x");
        assert_eq!(copy_line(Mode::Generic, "", Some(12), "x"), "LFG: 12 x");
        assert_eq!(copy_line(Mode::Raid, "", None, ""), "LFR:");
    }

    #[test]
    fn copy_line_has_no_dashes() {
        for m in Mode::ALL {
            let l = copy_line(m, "WAR", Some(50), "x");
            assert!(!l.contains('\u{2014}') && !l.contains('\u{2013}'), "{l}");
        }
    }

    #[test]
    fn class_code_accepts_the_sixteen_case_folded_and_refuses_the_rest() {
        for c in CLASSES {
            assert_eq!(class_code(&c.to_ascii_lowercase()).unwrap(), c);
        }
        assert_eq!(class_code("  ").unwrap(), "");
        assert!(class_code("Warrior").is_err(), "full names are not codes");
        assert!(class_code("XYZ").is_err());
    }

    #[test]
    fn level_is_clamped_to_the_old_app_range() {
        assert_eq!(clamp_level(0), 1);
        assert_eq!(clamp_level(1), 1);
        assert_eq!(clamp_level(50), 50);
        assert_eq!(clamp_level(99), 50);
        assert_eq!(level_of(" 47 ").unwrap(), Some(47));
        assert_eq!(level_of("120").unwrap(), Some(50));
        assert_eq!(level_of("").unwrap(), None);
        assert!(level_of("0").is_err());
        assert!(level_of("fifty").is_err());
    }

    fn log(path: &str, tailed: bool, problem: Option<&str>) -> Source {
        Source {
            kind: SourceKind::Log,
            path: PathBuf::from(path),
            last_read: tailed.then(Utc::now),
            records: 0,
            problem: problem.map(str::to_owned),
        }
    }

    #[test]
    fn who_is_the_character_of_the_log_being_tailed() {
        /* two logs, only one tailed: the tailed one wins regardless of order */
        let srcs = vec![
            log("C:/EQ/Logs/eqlog_Alt_legends.txt", false, Some("not tailed: only the most recently written log is read, and that is eqlog_Stoic_legends.txt")),
            log("C:/EQ/Logs/eqlog_Stoic_legends.txt", true, None),
        ];
        assert_eq!(
            who_of(&srcs, false),
            Who::Named {
                who: "Stoic".into(),
                file: "eqlog_Stoic_legends.txt".into()
            }
        );
        assert_eq!(who_of(&srcs, false).name(), "Stoic");
        assert_eq!(
            who_of(&srcs, false).line(),
            "Stoic (from eqlog_Stoic_legends.txt)"
        );
        /* a tailed log whose name carries no character is said so, not guessed */
        let renamed = vec![log("C:/EQ/Logs/eqlog_backup.txt", true, None)];
        assert_eq!(
            who_of(&renamed, false),
            Who::NoCharacter("eqlog_backup.txt".into())
        );
        assert_eq!(who_of(&renamed, false).name(), "");
    }

    #[test]
    fn who_names_every_failure_in_the_ingests_words() {
        assert_eq!(who_of(&[], false), Who::NoLogDir);
        assert!(who_of(&[], false).line().contains("Settings"));
        assert_eq!(
            who_of(&[], true),
            Who::Scanning,
            "a first scan in flight is not a missing folder"
        );
        /* a folder with no logs: the ingest reports the folder as a Log source with the reason */
        let empty = vec![log(
            "C:/EQ/Logs",
            false,
            Some("No eqlog_*.txt files in C:/EQ/Logs. Is logging on? (/log)"),
        )];
        assert_eq!(
            who_of(&empty, false),
            Who::NotTailed("No eqlog_*.txt files in C:/EQ/Logs. Is logging on? (/log)".into())
        );
        assert!(who_of(&empty, false)
            .line()
            .starts_with("character unknown: "));
        /* a log source with no reason still names the path rather than saying nothing */
        let mute = vec![log("C:/EQ/Logs/eqlog_Stoic_legends.txt", false, None)];
        assert!(who_of(&mute, false)
            .line()
            .contains("eqlog_Stoic_legends.txt"));
        /* an inventory source is not a log and never names a character */
        let inv = vec![Source {
            kind: SourceKind::Inventory,
            path: PathBuf::from("C:/EQ/Stoic-Inventory.txt"),
            last_read: Some(Utc::now()),
            records: 3,
            problem: None,
        }];
        assert_eq!(who_of(&inv, false), Who::NoLogDir);
    }

    #[test]
    fn who_lines_carry_no_dashes() {
        for w in [
            Who::Named {
                who: "Stoic".into(),
                file: "eqlog_Stoic_legends.txt".into(),
            },
            Who::NoCharacter("x.txt".into()),
            Who::Scanning,
            Who::NotTailed("why".into()),
            Who::NoLogDir,
        ] {
            let l = w.line();
            assert!(!l.contains('\u{2014}') && !l.contains('\u{2013}'), "{l}");
        }
    }

    #[test]
    fn board_round_trips_through_json_with_lowercase_modes() {
        let b = Board {
            version: 1,
            entries: vec![Entry {
                when: DateTime::parse_from_rfc3339("2026-09-02T03:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                who: "Stoic".into(),
                what: "tank".into(),
                mode: Mode::Motes,
                class: "WAR".into(),
                level: Some(50),
            }],
            class: "WAR".into(),
            level: Some(50),
        };
        let v = serde_json::to_value(&b).unwrap();
        assert_eq!(v["entries"][0]["mode"], "motes");
        assert_eq!(v["version"], 1);
        let back: Board = serde_json::from_value(v).unwrap();
        assert_eq!(back, b);
        /* an old board with no version field still loads */
        let bare: Board = serde_json::from_value(serde_json::json!({ "entries": [] })).unwrap();
        assert_eq!(bare.version, 1);
    }

    /// The version stamp is read on load, not only written: a board from a newer build is
    /// refused with both numbers and left in the file untouched.
    #[test]
    fn a_board_from_a_newer_build_is_refused_not_guessed_at() {
        let mut s = Settings::default();
        s.extra.insert(
            BOARD_KEY.to_owned(),
            serde_json::json!({ "version": 2, "entries": [], "shape_this_build_never_saw": true }),
        );
        let e = board_from(&s).unwrap_err();
        assert!(e.contains("newer build"), "{e}");
        assert!(e.contains("version 2") && e.contains("reads 1"), "{e}");
        assert!(s.extra.contains_key(BOARD_KEY), "refusing never deletes");
        /* the current version and an unstamped (old) board both load */
        s.extra.insert(
            BOARD_KEY.to_owned(),
            serde_json::json!({ "version": 1, "entries": [] }),
        );
        assert!(board_from(&s).is_ok());
        s.extra
            .insert(BOARD_KEY.to_owned(), serde_json::json!({ "entries": [] }));
        assert_eq!(board_from(&s).unwrap().version, 1);
    }

    #[test]
    fn board_lives_under_lfg_board_in_settings_and_survives_other_keys() {
        let mut s = Settings::default();
        s.extra.insert(
            "someone_elses_key".to_owned(),
            serde_json::json!({ "kept": true }),
        );
        assert_eq!(
            board_from(&s).unwrap().entries.len(),
            0,
            "absent is an empty board, not an error"
        );
        let b = Board {
            version: 1,
            entries: vec![],
            class: "CLR".into(),
            level: Some(12),
        };
        board_into(&mut s, &b).unwrap();
        assert!(s.extra.contains_key(BOARD_KEY));
        assert_eq!(
            s.extra["someone_elses_key"]["kept"], true,
            "writing the board touches one key"
        );
        assert_eq!(board_from(&s).unwrap(), b);
        /* a corrupt value is an error that names the key, never a silent reset */
        s.extra
            .insert(BOARD_KEY.to_owned(), serde_json::json!("not a board"));
        let e = board_from(&s).unwrap_err();
        assert!(e.contains(BOARD_KEY), "{e}");
    }

    #[test]
    fn mode_mirrors_lfg_mode_both_ways() {
        for m in Mode::ALL {
            assert_eq!(Mode::of(&m.to_lfg()), m);
        }
    }
}
