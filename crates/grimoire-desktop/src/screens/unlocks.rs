//! Screen: unlocks. Decision D9: what a character did BEFORE anything was watching, read off the
//! `/outputfile achievements` dump.
//!
//! WHAT IS HERE. The dump's parser, the trust rule, the unlock rows, the key and task lists, the
//! Sky test backfill and the whose-file check: the RULES and the DATA MODEL, with the drawing
//! kept to the bottom of the file. Every rule carries a test below that fails without it.
//!
//! THE FILE. Tab indented, three levels, every row below a section flagged C (complete) or I
//! (incomplete):
//!
//! ```text
//!     Untapped Potential: Classes                <- section: no flag, no tab
//!     I<TAB>Primary Class Unlock - Bard          <- achievement
//!     C<TAB><TAB>Obtain Amulet of the Fae.       <- criterion
//! ```
//!
//! THE TRAP. A criterion's C flag DOES NOT mean the criterion happened. When an unlock completes
//! by creation or by spending a token, the client force-marks every one of its criteria complete.
//! Proven inside one dump: where a completed Race Unlock claims three maxed factions, the
//! Progression section says none of them are maxed. So criteria are read off an INCOMPLETE
//! achievement freely, and off a COMPLETE one only when neither escape hatch fired. `trust` is
//! that rule and every consumer here goes through it.
//!
//! WHOSE FILE. `/outputfile` names its exports `<Char>_<server>-<Thing>.txt`. Applying one
//! character's achievements to another's tracker would be silent fiction, so the screen names
//! whose dump it read and says when that is not the character whose log is being tailed.

use crate::chrome::State;
use crate::screens::gear::{CLASSES, CLASS_NAMES};
use crate::screens::Cx;
use crate::theme::*;
use egui::{FontId, RichText, Stroke, StrokeKind, Ui, Vec2};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

/* ------------------------------------------------------------------ the parse -- */

/// Name matching across three vocabularies (client achievement text, wiki page titles, log
/// lines): case, trailing period, and the backtick and curly apostrophe the wiki drops all have
/// to stop mattering. Deliberately NOT the kills norm_name: that one strips leading articles,
/// which would fuse "a crude stein" with "crude stein". Never an identity key.
pub fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let lowered: String = s
        .chars()
        .map(|c| {
            if c == '`' || c == '\u{2018}' || c == '\u{2019}' {
                '\''
            } else {
                c
            }
        })
        .collect::<String>()
        .to_lowercase();
    let trimmed = lowered.trim().trim_end_matches('.');
    let mut space = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(ch);
        } else {
            space = true;
        }
    }
    out
}

/// What a criterion is: a real step, or one of the two escape hatches the dump prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CritKind {
    /// "This achievement will autocomplete if..." (one escape hatch)
    Auto,
    /// "...can be bypassed using a ... Token." (the other)
    Token,
    /// "Future Placeholder for X Requirements.", the client's own words for not built yet.
    Placeholder,
    /// "Obtain X."
    Obtain,
    /// "Get maximum faction with X."
    Faction,
    /// "Complete the 'X' ..."
    Task,
    Other,
}

static RX_AUTO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bautocomplete\b").expect("constant"));
static RX_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)bypassed using .*\bToken\b").expect("constant"));
static RX_PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Future Placeholder for ").expect("constant"));
static RX_OBTAIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Obtain\s+(.+?)\.?$").expect("constant"));
static RX_FACTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Get maximum faction with\s+(.+?)\.?$").expect("constant"));
static RX_TASK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Complete the\s+'(.+?)'").expect("constant"));
static RX_PROGRESS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)\s*/\s*(\d+)$").expect("constant"));

pub fn crit_kind(t: &str) -> CritKind {
    if RX_AUTO.is_match(t) {
        CritKind::Auto
    } else if RX_TOKEN.is_match(t) {
        CritKind::Token
    } else if RX_PLACEHOLDER.is_match(t) {
        CritKind::Placeholder
    } else if RX_OBTAIN.is_match(t) {
        CritKind::Obtain
    } else if RX_FACTION.is_match(t) {
        CritKind::Faction
    } else if RX_TASK.is_match(t) {
        CritKind::Task
    } else {
        CritKind::Other
    }
}

/// The item an "Obtain X." criterion names.
pub fn obtain_of(t: &str) -> Option<&str> {
    RX_OBTAIN
        .captures(t)
        .and_then(|m| m.get(1))
        .map(|g| g.as_str())
}

/// The task a "Complete the 'X'" criterion names.
pub fn task_of(t: &str) -> Option<&str> {
    RX_TASK
        .captures(t)
        .and_then(|m| m.get(1))
        .map(|g| g.as_str())
}

/// One criterion row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crit {
    pub done: bool,
    pub t: String,
    pub kind: CritKind,
    /// The only per-criterion NUMBER the dump carries: how far a counting achievement has got.
    pub have: Option<u64>,
    pub goal: Option<u64>,
}

/// One achievement row with its criteria.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ach {
    pub sec: String,
    pub name: String,
    pub done: bool,
    pub crit: Vec<Crit>,
}

/// What parsing a dump hands back: its sections, and every achievement under them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub sections: Vec<String>,
    pub list: Vec<Ach>,
}

/// Row shape is `<flag><TAB>[<TAB> per level]<text>[<TAB><done>/<goal>]`. Depth comes from the
/// EMPTY leading fields, and the text is the first non-empty one after the flag, not the last: a
/// Slayer criterion carries its kill count in a further field ("Gnolls", "770/5000"). Two fields
/// = an achievement, three or more = a criterion.
pub fn parse(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut sec = String::new();
    let mut cur: Option<Ach> = None;
    let flush = |cur: &mut Option<Ach>, out: &mut Parsed| {
        if let Some(a) = cur.take() {
            out.list.push(a);
        }
    };
    for raw in text.split('\n') {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() == 1 {
            flush(&mut cur, &mut out);
            sec = f[0].trim().to_owned();
            if !sec.is_empty() && !out.sections.contains(&sec) {
                out.sections.push(sec.clone());
            }
        } else if f.len() == 2 {
            flush(&mut cur, &mut out);
            cur = Some(Ach {
                sec: sec.clone(),
                name: f[1].trim().to_owned(),
                done: f[0].trim() == "C",
                crit: Vec::new(),
            });
        } else if let Some(a) = &mut cur {
            let rest: Vec<&str> = f[1..].iter().copied().filter(|x| !x.is_empty()).collect();
            let t = rest.first().map(|s| s.trim()).unwrap_or("").to_owned();
            let (have, goal) = match rest.get(1).and_then(|s| RX_PROGRESS.captures(s.trim())) {
                Some(m) => (m[1].parse().ok(), m[2].parse().ok()),
                None => (None, None),
            };
            let kind = crit_kind(&t);
            a.crit.push(Crit {
                done: f[0].trim() == "C",
                t,
                kind,
                have,
                goal,
            });
        }
    }
    flush(&mut cur, &mut out);
    out
}

/* ------------------------------------------------------------------ the trust -- */

/// Can this achievement's criteria be believed?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Incomplete, so nothing was force-marked and every C criterion is a real, earned step.
    Open,
    /// Complete, and neither escape hatch fired: it completed by doing the work.
    Earned,
    /// Complete, and it has no escape hatches at all (keys, raid conquests, slayer).
    Plain,
    /// Complete because the character was created that way. Criteria say nothing.
    Granted,
    /// Complete because an unlock token was spent. Criteria say nothing.
    Token,
}

impl Why {
    pub fn ok(self) -> bool {
        !matches!(self, Why::Granted | Why::Token)
    }

    /// The label that sits beside the name.
    pub fn text(self) -> &'static str {
        match self {
            Why::Open => "",
            Why::Earned | Why::Plain => "earned",
            Why::Granted => "granted at character creation",
            Why::Token => "unlocked with a token",
        }
    }

    /// The sentence that explains why nothing under it can be read.
    pub fn note(self) -> &'static str {
        match self {
            Why::Granted => "Being created this way ticks every requirement under it at once, so what is listed below records the creation and not anything you went and did.",
            Why::Token => "Spending a token ticks every requirement under it at once, so what is listed below records the token and not anything you went and did.",
            _ => "",
        }
    }
}

pub fn trust(a: &Ach) -> Why {
    let auto = a.crit.iter().find(|c| c.kind == CritKind::Auto);
    let token = a.crit.iter().find(|c| c.kind == CritKind::Token);
    if !a.done {
        return Why::Open;
    }
    if auto.is_some_and(|c| c.done) {
        return Why::Granted;
    }
    if token.is_some_and(|c| c.done) {
        return Why::Token;
    }
    if auto.is_some() || token.is_some() {
        Why::Earned
    } else {
        Why::Plain
    }
}

/* ---------------------------------------------------------------- the unlocks -- */

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnlockKind {
    Race,
    Class,
    Deity,
}

impl UnlockKind {
    fn prefix(self) -> &'static str {
        match self {
            UnlockKind::Race => "Race Unlock - ",
            UnlockKind::Class => "Primary Class Unlock - ",
            UnlockKind::Deity => "Deity Unlock - ",
        }
    }

    /// The sentence under the heading, saying where this kind of unlock comes from.
    pub fn hint(self) -> &'static str {
        match self {
            UnlockKind::Class => "A primary class opens by finishing that class's Plane of Sky test. The Sky screen lists the pieces each test wants and where they drop.",
            UnlockKind::Race => "A race opens on faction alone: take every faction listed here to its maximum and the race becomes selectable.",
            UnlockKind::Deity => "Nothing is implemented behind these yet. The client ships the rows as placeholders, so an empty list here is the client's silence and not a missing read.",
        }
    }
}

/// One requirement under an unlock. The two escape-hatch rows are not steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepRow {
    pub done: bool,
    pub t: String,
    pub kind: CritKind,
    /// The counting criterion's progress as the dump printed it (`Crit::have` of `Crit::goal`,
    /// "770/5000"), drawn after the step text. None when the row carries no count.
    pub progress: Option<(u64, u64)>,
}

impl StepRow {
    /// "770/5000" for a counting step, nothing for the rest.
    pub fn progress_text(&self) -> String {
        match self.progress {
            Some((have, goal)) => format!("{have}/{goal}"),
            None => String::new(),
        }
    }
}

/// One unlock row: what it is, whether it is unlocked, how, and, when
/// the criteria can be believed, how far along the requirements are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unlock {
    pub kind: UnlockKind,
    pub name: String,
    pub unlocked: bool,
    pub why: Why,
    pub trusted: bool,
    /// The client says the requirements for this one do not exist yet. Never a checklist.
    pub placeholder: bool,
    pub steps: Vec<StepRow>,
    pub have: usize,
    pub need: usize,
}

fn unlock_row(kind: UnlockKind, name: &str, a: &Ach) -> Unlock {
    let tr = trust(a);
    let steps: Vec<StepRow> = a
        .crit
        .iter()
        .filter(|c| c.kind != CritKind::Auto && c.kind != CritKind::Token)
        .map(|c| StepRow {
            done: c.done && tr.ok(),
            t: c.t.clone(),
            kind: c.kind,
            progress: c.have.zip(c.goal),
        })
        .collect();
    let real: Vec<&StepRow> = steps
        .iter()
        .filter(|c| c.kind != CritKind::Placeholder)
        .collect();
    Unlock {
        kind,
        name: name.to_owned(),
        unlocked: a.done,
        why: tr,
        trusted: tr.ok(),
        placeholder: !steps.is_empty() && real.is_empty(),
        have: if tr.ok() {
            real.iter().filter(|c| c.done).count()
        } else {
            0
        },
        need: real.len(),
        steps,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Unlocks {
    pub races: Vec<Unlock>,
    pub classes: Vec<Unlock>,
    pub deities: Vec<Unlock>,
}

/// Every Untapped Potential row, grouped. Section scoped rather than name scoped: the dump's own
/// three sections are the authority on which unlocks exist.
pub fn unlocks(p: &Parsed) -> Unlocks {
    let mut out = Unlocks::default();
    for a in &p.list {
        if !a.sec.to_lowercase().starts_with("untapped potential") {
            continue;
        }
        for kind in [UnlockKind::Race, UnlockKind::Class, UnlockKind::Deity] {
            if let Some(name) = a.name.strip_prefix(kind.prefix()) {
                let row = unlock_row(kind, name, a);
                match kind {
                    UnlockKind::Race => out.races.push(row),
                    UnlockKind::Class => out.classes.push(row),
                    UnlockKind::Deity => out.deities.push(row),
                }
                break;
            }
        }
    }
    out
}

/// One key the dump can prove: the Islands of Sky keys and the standalone
/// keys, from any section whose name ends in "Keys".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub name: String,
    pub done: bool,
    pub from: String,
}

pub fn keys(p: &Parsed) -> Vec<Key> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for a in &p.list {
        if !a.sec.to_lowercase().ends_with("keys") {
            continue;
        }
        let tr = trust(a);
        for c in &a.crit {
            if c.kind == CritKind::Auto || c.kind == CritKind::Token {
                continue;
            }
            if !seen.insert(c.t.clone()) {
                continue;
            }
            out.push(Key {
                name: c.t.trim_end_matches('.').to_owned(),
                done: c.done && tr.ok(),
                from: a.name.clone(),
            });
        }
    }
    out
}

/// One named task the dump can prove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    pub done: bool,
    pub from: String,
    pub t: String,
}

pub fn tasks(p: &Parsed) -> Vec<Task> {
    let mut out = Vec::new();
    for a in &p.list {
        let tr = trust(a);
        for c in &a.crit {
            if c.kind != CritKind::Task {
                continue;
            }
            let Some(name) = task_of(&c.t) else { continue };
            out.push(Task {
                name: name.to_owned(),
                done: c.done && tr.ok(),
                from: a.name.clone(),
                t: c.t.clone(),
            });
        }
    }
    out
}

/// Whose dump this is, from `<Char>_<server>-<Thing>.txt`.
pub fn whose(file: &str) -> (String, String) {
    let Some(us) = file.find('_') else {
        return (String::new(), String::new());
    };
    let ch = &file[..us];
    let rest = &file[us + 1..];
    let Some(dash) = rest.find('-') else {
        return (String::new(), String::new());
    };
    if ch.is_empty() || dash == 0 {
        return (String::new(), String::new());
    }
    (ch.to_owned(), rest[..dash].to_owned())
}

/* -------------------------------------------------------------------- sky -- */

/// One criterion naming two rewards at once. The client hands over a matched pair of weapons on
/// a single line, while sky.json files that test under one of the two, so the pair's line has to
/// be folded to the name the data uses or the test never matches itself.
///
/// It is a table of one because one is all that has been found. It is a table rather than an
/// `if` so that the second one, when it turns up, is a row and not a rewrite.
pub const REWARD_ALIAS: [(&str, &str); 1] = [("windhowl and spirit render", "Windhowl")];

/// Which of a class's Sky tests the client says you finished.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkyTests {
    pub unlocked: bool,
    pub why: Option<Why>,
    pub trusted: bool,
    /// Test names finished.
    pub by_test: BTreeSet<String>,
    /// Rewards the dump names that sky.json has no test for.
    pub unmatched: Vec<String>,
    pub done: usize,
    pub total: usize,
}

/// A Primary Class Unlock's "Obtain X" criteria ARE that class's Plane of Sky test rewards, one
/// per test. Returns, per class code, which tests the client says you finished. Untrusted unlocks
/// contribute nothing: every flag under them is C.
pub fn sky_tests(sky: &crate::data::Sky, p: &Parsed) -> BTreeMap<String, SkyTests> {
    let mut code_of: HashMap<String, &'static str> = HashMap::new();
    for (i, code) in CLASSES.iter().enumerate() {
        let n = norm(CLASS_NAMES[i]);
        /* the dump writes "Shadowknight"; every other surface writes two words */
        code_of.insert(n.replace(' ', ""), code);
        code_of.insert(n, code);
    }
    let mut out = BTreeMap::new();
    for a in &p.list {
        let Some(name) = a.name.strip_prefix(UnlockKind::Class.prefix()) else {
            continue;
        };
        let n = norm(name);
        let Some(code) = code_of.get(&n).or_else(|| code_of.get(&n.replace(' ', ""))) else {
            continue;
        };
        let Some(cls) = sky.classes.get(*code) else {
            continue;
        };
        let tr = trust(a);
        let by_reward: HashMap<String, &str> = cls
            .tests
            .iter()
            .map(|t| (norm(&t.reward), t.name.as_str()))
            .collect();
        let mut st = SkyTests {
            unlocked: a.done,
            why: Some(tr),
            trusted: tr.ok(),
            total: cls.tests.len(),
            ..Default::default()
        };
        for c in &a.crit {
            if c.kind != CritKind::Obtain {
                continue;
            }
            let Some(item) = obtain_of(&c.t) else {
                continue;
            };
            let key = norm(item);
            let key = REWARD_ALIAS
                .iter()
                .find(|(from, _)| *from == key)
                .map(|(_, to)| norm(to))
                .unwrap_or(key);
            match by_reward.get(&key) {
                None => st.unmatched.push(item.to_owned()),
                Some(test) => {
                    if c.done && tr.ok() {
                        st.by_test.insert((*test).to_owned());
                    }
                }
            }
        }
        st.done = st.by_test.len();
        out.insert((*code).to_owned(), st);
    }
    out
}

/* -------------------------------------------------------------- the file -- */

/// The parsed achievements dump, remembered against the ingest's read of it so the text is
/// parsed once per dump and not once per frame. D7: the ingest finds and reads the file
/// (`Ingest::achievements`, the same two folders the inventory dump is looked for in); this
/// screen owns only the grammar.
struct Cached {
    path: PathBuf,
    read_at: chrono::DateTime<chrono::Utc>,
    parsed: Parsed,
    character: String,
    server: String,
}

/* ------------------------------------------------------------------ screen -- */

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Classes,
    Races,
    Deities,
    Keys,
    Sky,
    Tasks,
}

pub struct UnlocksScreen {
    cache: Option<Cached>,
    view: View,
    q: String,
    hide_done: bool,
}

impl Default for UnlocksScreen {
    fn default() -> Self {
        UnlocksScreen {
            cache: None,
            view: View::Classes,
            q: String::new(),
            hide_done: false,
        }
    }
}

impl UnlocksScreen {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        /* The ingest polls the dump's mtime on its own clock; this screen only has to notice a
         * newer read and parse it once. */
        if let Some(d) = cx.ingest.achievements() {
            let stale = !self
                .cache
                .as_ref()
                .is_some_and(|c| c.path == d.path && c.read_at == d.read_at);
            if stale {
                let (character, server) =
                    whose(d.path.file_name().and_then(|n| n.to_str()).unwrap_or(""));
                self.cache = Some(Cached {
                    path: d.path.clone(),
                    read_at: d.read_at,
                    parsed: parse(&d.text),
                    character,
                    server,
                });
            }
        } else {
            self.cache = None;
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));

        heading(ui, "UNLOCKS");
        ui.label(RichText::new("What this character did before anything was watching, from the /outputfile achievements dump: class, race and deity unlocks, keys, Sky tests and named tasks.").color(TEXT_2));
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if ui.button("Re-read").clicked() {
                cx.ingest.rescan();
            }
            if cx.ingest.scanning() {
                mark(ui, State::Working, "the ingest is scanning");
            }
        });
        match (cx.ingest.achievements(), cx.ingest.achievements_problem()) {
            (Some(d), problem) => {
                let when: Option<chrono::DateTime<chrono::Local>> = d.modified.map(|m| m.into());
                let who = d
                    .character
                    .as_deref()
                    .map(|c| format!(" · {c}'s dump"))
                    .unwrap_or_default();
                mark(
                    ui,
                    State::Settled,
                    &format!(
                        "{}{who}{} · {} rows",
                        d.path.display(),
                        when.map(|w| format!(" · dumped {}", w.format("%Y-%m-%d %H:%M")))
                            .unwrap_or_default(),
                        d.lines
                    ),
                );
                if let Some(e) = problem {
                    mark(ui, State::Wrong, e);
                }
            }
            (None, Some(e)) => {
                /* No dump: the ingest's own words name the fix (Settings when no Logs folder is
                 * set, the slash command when one is), and the folders it looked in. */
                mark(ui, State::You, e);
                let tried: Vec<String> = cx
                    .ingest
                    .log_dir()
                    .tried
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                if !tried.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.label(RichText::new("looked in").color(TEXT_3));
                        ui.label(
                            RichText::new(tried.join("; "))
                                .font(FontId::monospace(11.0))
                                .color(TEXT_3),
                        );
                    });
                }
            }
            (None, None) => {
                mark(
                    ui,
                    State::Idle,
                    "the ingest has not looked for the achievements dump yet",
                );
            }
        }
        let Some(cached) = &self.cache else { return };
        let parsed = &cached.parsed;

        /* whose file: shown either way, and said when it is not the tailed character's */
        if let Some(me) = cx.ingest.active_character() {
            if !cached.character.is_empty() && !cached.character.eq_ignore_ascii_case(me) {
                mark(ui, State::You, &format!("This dump is {}'s on {} and the log being tailed is {me}'s. Shown, but it is not that character's record.", cached.character, cached.server));
            }
        }
        ui.add_space(6.0);

        let u = unlocks(parsed);
        ui.horizontal(|ui| {
            for (v, label, n) in [
                (View::Classes, "Classes", u.classes.len()),
                (View::Races, "Races", u.races.len()),
                (View::Deities, "Deities", u.deities.len()),
                (View::Keys, "Keys", keys(parsed).len()),
                (
                    View::Sky,
                    "Sky tests",
                    cx.data
                        .map(|s| sky_tests(&s.sky, parsed).len())
                        .unwrap_or(0),
                ),
                (View::Tasks, "Tasks", tasks(parsed).len()),
            ] {
                if ui
                    .selectable_label(self.view == v, format!("{label} {n}"))
                    .clicked()
                {
                    self.view = v;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("filter").color(TEXT_3));
            ui.add(egui::TextEdit::singleline(&mut self.q).desired_width(220.0));
            ui.checkbox(&mut self.hide_done, "hide done");
        });
        ui.add_space(6.0);
        let q = self.q.trim().to_lowercase();

        egui::ScrollArea::vertical()
            .id_salt("unlocks-body")
            .show(ui, |ui| match self.view {
                View::Classes => unlock_list(ui, &u.classes, UnlockKind::Class, &q, self.hide_done),
                View::Races => unlock_list(ui, &u.races, UnlockKind::Race, &q, self.hide_done),
                View::Deities => unlock_list(ui, &u.deities, UnlockKind::Deity, &q, self.hide_done),
                View::Keys => key_list(ui, &keys(parsed), &q, self.hide_done),
                View::Sky => sky_list(
                    ui,
                    cx.data.map(|s| &s.sky),
                    cx.data_err,
                    parsed,
                    &q,
                    self.hide_done,
                ),
                View::Tasks => task_list(ui, &tasks(parsed), &q, self.hide_done),
            });
    }
}

fn unlock_list(ui: &mut Ui, list: &[Unlock], kind: UnlockKind, q: &str, hide_done: bool) {
    ui.label(RichText::new(kind.hint()).color(TEXT_3));
    let rows: Vec<&Unlock> = list
        .iter()
        .filter(|r| !(hide_done && r.unlocked))
        .filter(|r| {
            q.is_empty()
                || r.name.to_lowercase().contains(q)
                || r.steps.iter().any(|s| s.t.to_lowercase().contains(q))
        })
        .collect();
    let on = list.iter().filter(|r| r.unlocked).count();
    ui.label(
        RichText::new(format!(
            "{on}/{} unlocked · {} shown",
            list.len(),
            rows.len()
        ))
        .color(TEXT_2),
    );
    ui.add_space(4.0);
    if list.is_empty() {
        mark(
            ui,
            State::Idle,
            "the dump has no rows of this kind under Untapped Potential",
        );
        return;
    }
    if rows.is_empty() {
        ui.label(RichText::new("Nothing matches.").color(TEXT_3));
        return;
    }
    for u in rows {
        let state = if u.unlocked {
            (
                State::Settled,
                if u.why.text().is_empty() {
                    "unlocked".to_owned()
                } else {
                    format!("unlocked · {}", u.why.text())
                },
            )
        } else if u.placeholder {
            (State::Idle, "not implemented".to_owned())
        } else {
            /* Part way is not WORKING: that colour is for a thread of ours doing something, and
             * nothing here runs. The square is idle (nothing is happening) and the count and the
             * progress track beside it carry the quantity. */
            (State::Idle, format!("{}/{}", u.have, u.need))
        };
        let open = !u.unlocked && u.have > 0;
        let head = egui::CollapsingHeader::new(RichText::new(&u.name).color(if u.unlocked {
            GOLD_HI
        } else {
            TEXT
        }))
        .id_salt(("unlock", kind as u8, &u.name))
        .icon(crate::chrome::fold_icon)
        .default_open(open);
        head.show(ui, |ui| {
            ui.horizontal(|ui| {
                mark(ui, state.0, &state.1);
                if !u.unlocked && u.need > 0 && u.have > 0 {
                    progress(ui, u.have as f32 / u.need as f32);
                }
            });
            if !u.trusted {
                /* An unlocked-by-token or by-creation row has nothing to show under it: every
                 * criterion reads complete whether or not it happened. */
                ui.label(RichText::new(u.why.note()).color(TEXT_3));
            } else if u.placeholder {
                ui.label(
                    RichText::new(
                        u.steps
                            .iter()
                            .map(|s| s.t.as_str())
                            .collect::<Vec<_>>()
                            .join(" "),
                    )
                    .color(TEXT_3),
                );
            } else {
                for s in &u.steps {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        if s.done {
                            mark(ui, State::Settled, &s.t);
                        } else {
                            mark(ui, State::Idle, &s.t);
                        }
                        /* a counting step's "770/5000", from the dump's own field */
                        let p = s.progress_text();
                        if !p.is_empty() {
                            ui.label(RichText::new(p).font(FontId::monospace(11.0)).color(TEXT_2));
                        }
                    });
                }
            }
        });
    }
}

fn key_list(ui: &mut Ui, list: &[Key], q: &str, hide_done: bool) {
    ui.label(RichText::new("The keys the dump can prove: the Islands of Sky keys and the standalone ones, from every section named Keys.").color(TEXT_3));
    let rows: Vec<&Key> = list
        .iter()
        .filter(|k| !(hide_done && k.done))
        .filter(|k| {
            q.is_empty() || k.name.to_lowercase().contains(q) || k.from.to_lowercase().contains(q)
        })
        .collect();
    ui.label(
        RichText::new(format!(
            "{}/{} done · {} shown",
            list.iter().filter(|k| k.done).count(),
            list.len(),
            rows.len()
        ))
        .color(TEXT_2),
    );
    ui.add_space(4.0);
    if list.is_empty() {
        mark(ui, State::Idle, "the dump has no section named Keys");
        return;
    }
    egui::Grid::new("unlocks-keys")
        .num_columns(2)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for k in rows {
                mark(
                    ui,
                    if k.done { State::Settled } else { State::Idle },
                    &k.name,
                );
                ui.label(RichText::new(&k.from).color(TEXT_3));
                ui.end_row();
            }
        });
}

fn task_list(ui: &mut Ui, list: &[Task], q: &str, hide_done: bool) {
    ui.label(RichText::new("Named tasks the dump records. There are few: the file has no general 'you finished quest X' row.").color(TEXT_3));
    let rows: Vec<&Task> = list
        .iter()
        .filter(|t| !(hide_done && t.done))
        .filter(|t| {
            q.is_empty() || t.name.to_lowercase().contains(q) || t.from.to_lowercase().contains(q)
        })
        .collect();
    ui.label(
        RichText::new(format!(
            "{}/{} done · {} shown",
            list.iter().filter(|t| t.done).count(),
            list.len(),
            rows.len()
        ))
        .color(TEXT_2),
    );
    ui.add_space(4.0);
    if list.is_empty() {
        mark(
            ui,
            State::Idle,
            "no 'Complete the ...' criterion anywhere in the dump",
        );
        return;
    }
    egui::Grid::new("unlocks-tasks")
        .num_columns(2)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for t in rows {
                mark(
                    ui,
                    if t.done { State::Settled } else { State::Idle },
                    &t.name,
                );
                ui.label(RichText::new(&t.from).color(TEXT_3));
                ui.end_row();
            }
        });
}

fn sky_list(
    ui: &mut Ui,
    sky: Option<&crate::data::Sky>,
    data_err: Option<&str>,
    parsed: &Parsed,
    q: &str,
    hide_done: bool,
) {
    ui.label(RichText::new("A class unlock's Obtain criteria are that class's Plane of Sky test rewards, one per test. Only an open or earned unlock can say which you finished.").color(TEXT_3));
    let Some(sky) = sky else {
        mark(
            ui,
            State::Wrong,
            data_err
                .unwrap_or("no sky.json loaded; put the snapshot in data/ beside the executable"),
        );
        return;
    };
    let tests = sky_tests(sky, parsed);
    if tests.is_empty() {
        mark(
            ui,
            State::Idle,
            "no Primary Class Unlock rows in the dump match a class in sky.json",
        );
        return;
    }
    egui::Grid::new("unlocks-sky")
        .num_columns(4)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for h in ["class", "tests done", "unlock", "not matched to a test"] {
                ui.label(RichText::new(h).color(TEXT_3));
            }
            ui.end_row();
            for (code, t) in &tests {
                let name = crate::screens::gear::class_name(code);
                if hide_done && t.done == t.total && t.total > 0 {
                    continue;
                }
                if !q.is_empty()
                    && !name.to_lowercase().contains(q)
                    && !code.to_lowercase().contains(q)
                {
                    continue;
                }
                ui.label(RichText::new(format!("{name} ({code})")).color(TEXT));
                /* done is settled; anything short of it is idle, with the count saying how short.
                 * WORKING is not "part way": nothing of ours runs here. */
                let st = if t.trusted && t.done == t.total && t.total > 0 {
                    State::Settled
                } else {
                    State::Idle
                };
                let text = if t.trusted {
                    format!("{}/{}", t.done, t.total)
                } else {
                    format!("unknown of {}", t.total)
                };
                let r = mark(ui, st, &text);
                if !t.by_test.is_empty() {
                    r.on_hover_text(t.by_test.iter().cloned().collect::<Vec<_>>().join("\n"));
                }
                ui.label(
                    RichText::new(if t.unlocked {
                        format!("unlocked · {}", t.why.map(Why::text).unwrap_or(""))
                    } else {
                        "locked".to_owned()
                    })
                    .color(if t.unlocked { GOLD_HI } else { TEXT_2 }),
                );
                ui.label(RichText::new(t.unmatched.join(", ")).color(TEXT_3));
                ui.end_row();
            }
        });
}

/// A thin bar for "have of need". Gold, not a state colour: it is a quantity, not a status.
fn progress(ui: &mut Ui, frac: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(90.0, 6.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, SUNK);
    let w = rect.width() * frac.clamp(0.0, 1.0);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, Vec2::new(w, rect.height())),
        0.0,
        GOLD_DIM,
    );
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

/* ------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::testdata;

    /// The shape of a real dump, condensed: three sections, escape hatches, a placeholder,
    /// a slayer count, a keys section that repeats, and a task row.
    const DUMP: &str = "Untapped Potential: Classes\r\n\
I\tPrimary Class Unlock - Ranger\r\n\
C\t\tObtain Windhowl and Spirit Render.\r\n\
C\t\tObtain Chest of Thunder.\r\n\
I\t\tObtain Bow of the Woodlands.\r\n\
I\t\tThis achievement will autocomplete if you chose to confirm your Primary Class as a Ranger.\r\n\
I\t\tThis achievement can be bypassed using a Primary Class Unlock Token.\r\n\
C\tPrimary Class Unlock - Enchanter\r\n\
C\t\tObtain Rod of Insight.\r\n\
C\t\tObtain Circlet of Falinkan.\r\n\
I\t\tThis achievement will autocomplete if you chose to confirm your Primary Class as a Enchanter.\r\n\
C\t\tThis achievement can be bypassed using a Primary Class Unlock Token.\r\n\
C\tPrimary Class Unlock - Monk\r\n\
C\t\tObtain Sash of Spirit.\r\n\
C\t\tThis achievement will autocomplete if you chose to confirm your Primary Class as a Monk.\r\n\
I\t\tThis achievement can be bypassed using a Primary Class Unlock Token.\r\n\
C\tPrimary Class Unlock - Shadowknight\r\n\
C\t\tObtain Blade of Fear.\r\n\
I\t\tThis achievement will autocomplete if you chose to confirm your Primary Class as a Shadowknight.\r\n\
I\t\tThis achievement can be bypassed using a Primary Class Unlock Token.\r\n\
Untapped Potential: Races\r\n\
I\tRace Unlock - Dark Elf\r\n\
C\t\tGet maximum faction with Dark Bargainers.\r\n\
I\t\tGet maximum faction with Dreadguard Outer.\r\n\
I\t\tThis achievement will autocomplete if your character was created as a Dark Elf.\r\n\
I\t\tThis achievement can be bypassed using a Race Unlock Token.\r\n\
Untapped Potential: Deity\r\n\
I\tDeity Unlock - Veeshan\r\n\
I\t\tFuture Placeholder for Veeshan Requirements.\r\n\
I\t\tThis achievement will autocomplete if you chose to confirm your Deity as Veeshan.\r\n\
I\t\tThis achievement can be bypassed using a Deity Unlock Token.\r\n\
General: Slayer\r\n\
I\tGnoll Slayer\r\n\
I\t\tGnolls\t770/5000\r\n\
General: Keys\r\n\
C\tKey to the Sky\r\n\
C\t\tObtain the Key of Veeshan.\r\n\
I\t\tObtain the Key of Swords.\r\n\
EverQuest: Keys\r\n\
C\tKey to the Sky\r\n\
C\t\tObtain the Key of Veeshan.\r\n\
I\t\tObtain the Key of Swords.\r\n\
EverQuest: Raids\r\n\
I\tRaid Conquests\r\n\
C\t\tComplete the 'Hatching' task in Skyshrine.\r\n\
I\t\tComplete the 'Sleeper' task.\r\n";

    /* Names repeat across sections (General: Keys and EverQuest: Keys ship the same four); the
     * production rules walk `list` with a section in hand, so a by-name lookup exists only here. */
    trait ByName {
        fn by_name(&self, name: &str) -> Option<&Ach>;
    }
    impl ByName for Parsed {
        fn by_name(&self, name: &str) -> Option<&Ach> {
            self.list.iter().find(|a| a.name == name)
        }
    }

    #[test]
    fn parse_reads_sections_achievements_and_criteria_by_shape() {
        let p = parse(DUMP);
        assert_eq!(
            p.sections,
            vec![
                "Untapped Potential: Classes",
                "Untapped Potential: Races",
                "Untapped Potential: Deity",
                "General: Slayer",
                "General: Keys",
                "EverQuest: Keys",
                "EverQuest: Raids"
            ]
        );
        assert_eq!(p.list.len(), 10);
        let rng = p
            .by_name("Primary Class Unlock - Ranger")
            .expect("ranger row");
        assert_eq!(rng.sec, "Untapped Potential: Classes");
        assert!(!rng.done);
        assert_eq!(rng.crit.len(), 5);
        assert_eq!(
            rng.crit[0],
            Crit {
                done: true,
                t: "Obtain Windhowl and Spirit Render.".into(),
                kind: CritKind::Obtain,
                have: None,
                goal: None
            }
        );
        assert_eq!(rng.crit[3].kind, CritKind::Auto);
        assert_eq!(rng.crit[4].kind, CritKind::Token);
        /* the slayer count: the name is the first non-empty field, the count the second */
        let gn = p.by_name("Gnoll Slayer").unwrap();
        assert_eq!(gn.crit[0].t, "Gnolls");
        assert_eq!((gn.crit[0].have, gn.crit[0].goal), (Some(770), Some(5000)));
        /* names repeat across sections: first wins by name, both are in the list */
        assert_eq!(p.by_name("Key to the Sky").unwrap().sec, "General: Keys");
        assert_eq!(
            p.list.iter().filter(|a| a.name == "Key to the Sky").count(),
            2
        );
        assert_eq!(parse(""), Parsed::default());
        /* a criterion before any achievement is dropped, not crashed on */
        let orphan = parse("Sec\r\nC\t\tOrphan.\r\nI\tThing\r\n");
        assert_eq!(orphan.list.len(), 1);
        assert!(orphan.list[0].crit.is_empty());
    }

    #[test]
    fn crit_kind_table() {
        let cases = [
            ("This achievement will autocomplete if you chose to confirm your Primary Class as a Bard.", CritKind::Auto),
            ("This achievement can be bypassed using a Race Unlock Token.", CritKind::Token),
            ("Future Placeholder for Veeshan Requirements.", CritKind::Placeholder),
            ("Obtain Amulet of the Fae.", CritKind::Obtain),
            ("obtain the Key of Swords", CritKind::Obtain),
            ("Get maximum faction with Dark Bargainers.", CritKind::Faction),
            ("Complete the 'Hatching' task in Skyshrine.", CritKind::Task),
            ("Gnolls", CritKind::Other),
            ("Reach level 50.", CritKind::Other),
        ];
        for (t, want) in cases {
            assert_eq!(crit_kind(t), want, "{t}");
        }
        assert_eq!(
            obtain_of("Obtain Amulet of the Fae."),
            Some("Amulet of the Fae")
        );
        assert_eq!(
            obtain_of("Obtain Amulet of the Fae"),
            Some("Amulet of the Fae")
        );
        assert_eq!(
            task_of("Complete the 'Hatching' task in Skyshrine."),
            Some("Hatching")
        );
        assert_eq!(task_of("Complete something else"), None);
    }

    #[test]
    fn trust_reads_the_escape_hatches() {
        let p = parse(DUMP);
        let by = |n: &str| p.by_name(n).unwrap();
        assert_eq!(trust(by("Primary Class Unlock - Ranger")), Why::Open);
        assert_eq!(trust(by("Primary Class Unlock - Enchanter")), Why::Token);
        assert_eq!(trust(by("Primary Class Unlock - Monk")), Why::Granted);
        assert_eq!(
            trust(by("Primary Class Unlock - Shadowknight")),
            Why::Earned
        );
        assert_eq!(
            trust(by("Key to the Sky")),
            Why::Plain,
            "complete with no hatches at all"
        );
        assert!(Why::Open.ok() && Why::Earned.ok() && Why::Plain.ok());
        assert!(!Why::Granted.ok() && !Why::Token.ok());
        assert_eq!(Why::Plain.text(), "earned");
        assert!(!Why::Token.note().is_empty());
        assert!(Why::Open.note().is_empty());
    }

    #[test]
    fn unlock_rows_exclude_the_hatches_and_never_trust_a_forced_one() {
        let u = unlocks(&parse(DUMP));
        assert_eq!(u.classes.len(), 4);
        assert_eq!(u.races.len(), 1);
        assert_eq!(u.deities.len(), 1);
        let rng = &u.classes[0];
        assert_eq!(rng.name, "Ranger");
        assert!(!rng.unlocked);
        assert_eq!(rng.steps.len(), 3, "the two hatch rows are not steps");
        assert_eq!((rng.have, rng.need), (2, 3));
        assert!(rng.trusted && !rng.placeholder);
        assert!(
            rng.steps
                .iter()
                .all(|s| s.progress.is_none() && s.progress_text().is_empty()),
            "obtain steps carry no count"
        );
        /* a counting criterion's have/goal reaches the step row as "770/5000" */
        let counted = Ach {
            sec: "Untapped Potential: Classes".into(),
            name: "Primary Class Unlock - Counted".into(),
            done: false,
            crit: vec![Crit {
                done: false,
                t: "Gnolls".into(),
                kind: CritKind::Other,
                have: Some(770),
                goal: Some(5000),
            }],
        };
        let row = unlock_row(UnlockKind::Class, "Counted", &counted);
        assert_eq!(row.steps[0].progress, Some((770, 5000)));
        assert_eq!(row.steps[0].progress_text(), "770/5000");
        let enc = &u.classes[1];
        assert!(enc.unlocked && !enc.trusted);
        assert_eq!(enc.why, Why::Token);
        assert_eq!(enc.have, 0, "a token unlock's ticks mean nothing");
        assert!(enc.steps.iter().all(|s| !s.done));
        let mnk = &u.classes[2];
        assert_eq!(mnk.why, Why::Granted);
        let shd = &u.classes[3];
        assert_eq!(shd.why, Why::Earned);
        assert_eq!((shd.have, shd.need), (1, 1));
        let de = &u.races[0];
        assert_eq!(de.kind, UnlockKind::Race);
        assert_eq!((de.have, de.need), (1, 2));
        let vee = &u.deities[0];
        assert!(vee.placeholder, "the client's own words for not built yet");
        assert_eq!((vee.have, vee.need), (0, 0));
        assert_eq!(vee.steps.len(), 1);
        /* rows outside Untapped Potential never become unlocks */
        assert!(!u
            .classes
            .iter()
            .chain(&u.races)
            .chain(&u.deities)
            .any(|r| r.name.contains("Slayer")));
    }

    #[test]
    fn keys_dedupe_across_sections_and_go_through_trust() {
        let p = parse(DUMP);
        let k = keys(&p);
        assert_eq!(
            k.len(),
            2,
            "the same four ship in two Keys sections; each once"
        );
        assert_eq!(
            k[0],
            Key {
                name: "Obtain the Key of Veeshan".into(),
                done: true,
                from: "Key to the Sky".into()
            }
        );
        assert_eq!(k[1].name, "Obtain the Key of Swords");
        assert!(!k[1].done);
        /* a granted key achievement proves nothing */
        let forced = parse("General: Keys\r\nC\tKey Ring\r\nC\t\tObtain the Key of Nothing.\r\nC\t\tThis achievement will autocomplete if you are lucky.\r\n");
        let k = keys(&forced);
        assert_eq!(k.len(), 1);
        assert!(!k[0].done);
    }

    #[test]
    fn tasks_are_the_complete_the_rows() {
        let t = tasks(&parse(DUMP));
        assert_eq!(t.len(), 2);
        assert_eq!(
            t[0],
            Task {
                name: "Hatching".into(),
                done: true,
                from: "Raid Conquests".into(),
                t: "Complete the 'Hatching' task in Skyshrine.".into()
            }
        );
        assert!(!t[1].done);
    }

    #[test]
    fn whose_reads_the_export_name() {
        assert_eq!(
            whose("Reviir_qeynos-Achievements.txt"),
            ("Reviir".into(), "qeynos".into())
        );
        assert_eq!(
            whose("Testchar_oggok-Inventory.txt"),
            ("Testchar".into(), "oggok".into())
        );
        assert_eq!(
            whose("eqlog_Reviir_qeynos.txt"),
            (String::new(), String::new())
        );
        assert_eq!(whose(""), (String::new(), String::new()));
    }

    #[test]
    fn norm_makes_three_spellings_one() {
        assert_eq!(
            norm("Selo`s Drums of the March."),
            "selo s drums of the march"
        );
        assert_eq!(
            norm("Selo\u{2019}s Drums of the March"),
            "selo s drums of the march"
        );
        assert_eq!(norm("  Shadow Knight  "), "shadow knight");
        assert_eq!(
            norm("A crude stein"),
            "a crude stein",
            "articles stay: not the kills norm"
        );
        assert_eq!(norm(""), "");
    }

    fn sky_with(code: &str, tests: &[(&str, &str)]) -> crate::data::Sky {
        let mut s = crate::data::Sky::default();
        let mut cls = crate::data::sky::SkyClass::default();
        for (n, reward) in tests {
            cls.tests.push(crate::data::sky::SkyTest {
                name: (*n).to_owned(),
                reward: (*reward).to_owned(),
                ..Default::default()
            });
        }
        s.classes.insert(code.to_owned(), cls);
        s
    }

    #[test]
    fn sky_tests_backfill_only_from_a_trusted_unlock_and_resolve_the_alias() {
        let mut sky = sky_with(
            "RNG",
            &[
                ("Ranger Test of Aim", "Windhowl"),
                ("Ranger Test of Storms", "Chest of Thunder"),
                ("Ranger Test of Wood", "Bow of the Woodlands"),
            ],
        );
        let enc = sky_with(
            "ENC",
            &[
                ("Enchanter Test of Mind", "Rod of Insight"),
                ("Enchanter Test of Sight", "Circlet of Falinkan"),
            ],
        );
        sky.classes.extend(enc.classes);
        let shd = sky_with("SHD", &[("Shadow Knight Test of Fear", "Blade of Fear")]);
        sky.classes.extend(shd.classes);
        let out = sky_tests(&sky, &parse(DUMP));
        let rng = out.get("RNG").expect("ranger");
        assert_eq!(rng.total, 3);
        assert_eq!(rng.done, 2);
        assert!(
            rng.by_test.contains("Ranger Test of Aim"),
            "the pair line resolves to the sword's test: {:?}",
            rng.by_test
        );
        assert!(rng.by_test.contains("Ranger Test of Storms"));
        assert!(rng.unmatched.is_empty());
        let enc = out.get("ENC").expect("enchanter");
        assert!(enc.unlocked && !enc.trusted);
        assert_eq!(enc.done, 0, "a token unlock contributes nothing");
        /* the dump's one-word Shadowknight finds the two-word class */
        let shd = out.get("SHD").expect("shadowknight");
        assert_eq!(shd.done, 1);
        assert_eq!(shd.why, Some(Why::Earned));
        /* a reward sky.json does not know is reported, not dropped */
        let sky2 = sky_with("RNG", &[("Ranger Test of Aim", "Windhowl")]);
        let out = sky_tests(&sky2, &parse(DUMP));
        assert_eq!(
            out["RNG"].unmatched,
            vec!["Chest of Thunder", "Bow of the Woodlands"]
        );
        /* no monk in sky: no row */
        assert!(!out.contains_key("MNK"));
    }

    /* ---- the real file ---- */

    #[test]
    fn the_real_sky_rewards_match_the_client_s_obtain_lines() {
        let Some(s) = testdata::snapshot() else {
            return;
        };
        /* build the dump the client would print for an open Berserker unlock with its first two
         * rewards obtained, from sky.json's own reward names */
        let ber = s.sky.classes.get("BER").expect("BER in sky.json");
        assert!(ber.tests.len() >= 3, "{} tests", ber.tests.len());
        let mut text =
            String::from("Untapped Potential: Classes\r\nI\tPrimary Class Unlock - Berserker\r\n");
        for (i, t) in ber.tests.iter().enumerate() {
            text.push_str(&format!(
                "{}\t\tObtain {}.\r\n",
                if i < 2 { "C" } else { "I" },
                t.reward
            ));
        }
        text.push_str("I\t\tThis achievement will autocomplete if you chose to confirm your Primary Class as a Berserker.\r\n");
        text.push_str(
            "I\t\tThis achievement can be bypassed using a Primary Class Unlock Token.\r\n",
        );
        let out = sky_tests(&s.sky, &parse(&text));
        let b = out.get("BER").expect("berserker row");
        assert_eq!((b.done, b.total), (2, ber.tests.len()));
        assert!(b.unmatched.is_empty(), "{:?}", b.unmatched);
        /* every class in sky.json has a name this file can map from the dump's spelling */
        for code in s.sky.classes.keys() {
            assert!(
                CLASSES.contains(&code.as_str()),
                "{code} is not one of the sixteen"
            );
        }
        /* the alias exists because one class's test is filed under the sword */
        let has_windhowl = s
            .sky
            .classes
            .values()
            .any(|c| c.tests.iter().any(|t| t.reward == "Windhowl"));
        assert!(
            has_windhowl,
            "sky.json keeps the pair test under Windhowl; if that moved, REWARD_ALIAS is stale"
        );
    }
}
