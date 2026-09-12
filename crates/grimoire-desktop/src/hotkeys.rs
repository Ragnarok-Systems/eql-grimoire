//! Global Ctrl+Alt hotkeys with leader chords. Decision D4.
//!
//! HOW A CHORD COMPLETES WITH THE GAME IN FRONT, AND WHY IT COST A REGISTRATION.
//! A global hotkey is a `RegisterHotKey` call: the OS delivers it whether or not a Grimoire window
//! has focus, which is the whole point, because the game has focus. A leader chord (`Ctrl+Alt+S`
//! then `K`) is a global press followed by a PLAIN key, and a plain key is not something the OS
//! hands to an unfocused window on its own. The first version of this module read the second key
//! from egui's input, which meant a Grimoire window had to be focused for `K` to land, and that
//! is the opposite of what a global hotkey is for. So now the second keys are THEMSELVES registered
//! as bare globals for exactly as long as the leader is armed (1.5 seconds), and unregistered the
//! moment the chord completes, lapses or is cancelled. For that window the game does not see `K`,
//! which is what the person pressing `Ctrl+Alt+S` a moment earlier meant. egui's own input is
//! still read as a fallback for the case where the bare key could not be registered (another
//! program owns a bare `K`, which is rare and reported on Settings).
//!
//! The direct aliases (`Ctrl+Alt+K`, `R`, `M`) are kept: they are one press instead of two and
//! they cost nothing. They sit below the D4 table in `bindings()` and are labelled as aliases.
//!
//! EVERY BINDING CAN BE CHANGED, D4. The table below is the DEFAULT; `Settings::hotkeys` carries
//! overrides keyed by the row's stable id, as chord text (`Ctrl+Alt+S then K`), and `table()`
//! applies them. An override that does not parse keeps the default and carries the parse error as
//! that row's conflict text, so a typo on the Settings screen never silently rebinds to nothing.
//! The chord machine is generic over that table: a leader is any modifier+key that at least one
//! chord row starts with, its second keys are whatever those rows name, and lapsing opens the tool
//! of a row bound to the bare leader (the `L` rule, derived rather than special cased).
//!
//! REGISTRATION FAILURES ARE RECORDED, NOT DROPPED. Another program owning `Ctrl+Alt+W` is the
//! normal case on a machine with a few years of software on it. Each binding carries its own
//! `registered` flag and `conflict` text, and one failure never stops the rest from registering.
//! Windows does not report WHICH program owns a hotkey, only that one does, and the conflict text
//! says exactly that rather than pretending to know. The second keys are probed once at install
//! (registered and released) so Settings can say whether a chord will complete from the game.
//!
//! THE CHORD MACHINE IS PURE. `Chord` takes a clock value and a table and returns tools; it
//! touches no OS, so arm, complete, lapse and cancel are all unit tested below without a window
//! or a hotkey manager.

use crate::windows::{LfgMode, Tool};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

/// How long a leader stays armed. D4 says 1.5 seconds.
pub const LEADER_WINDOW: Duration = Duration::from_millis(1500);

/// How often the root pass is asked to run so the hotkey receiver actually gets drained. Without
/// this egui would sleep until the next input event and a global press would sit in the channel
/// until the user wiggled the mouse.
pub const POLL_EVERY: Duration = Duration::from_millis(100);

/* ------------------------------------------------------------------- the table -- */

/// One default row: a stable id (what a Settings override is keyed by), the D4 chord text, the
/// tool it opens, and whether it is one of the direct aliases rather than a D4 row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowDef {
    pub id: &'static str,
    pub chord: &'static str,
    pub tool: Tool,
    pub alias: bool,
}

/// The D4 table, in D4's order, then the three direct aliases.
pub const DEFAULTS: &[RowDef] = &[
    RowDef {
        id: "companion",
        chord: "Ctrl+Alt+G",
        tool: Tool::Companion,
        alias: false,
    },
    RowDef {
        id: "watch",
        chord: "Ctrl+Alt+W",
        tool: Tool::Watch,
        alias: false,
    },
    RowDef {
        id: "parser",
        chord: "Ctrl+Alt+P",
        tool: Tool::Parser,
        alias: false,
    },
    RowDef {
        id: "sky",
        chord: "Ctrl+Alt+S then K",
        tool: Tool::Sky,
        alias: false,
    },
    RowDef {
        id: "chat",
        chord: "Ctrl+Alt+C",
        tool: Tool::Chat,
        alias: false,
    },
    RowDef {
        id: "overlays",
        chord: "Ctrl+Alt+D",
        tool: Tool::Overlays,
        alias: false,
    },
    RowDef {
        id: "lfg",
        chord: "Ctrl+Alt+L",
        tool: Tool::Lfg(LfgMode::Generic),
        alias: false,
    },
    RowDef {
        id: "lfg-raid",
        chord: "Ctrl+Alt+L then R",
        tool: Tool::Lfg(LfgMode::Raid),
        alias: false,
    },
    RowDef {
        id: "lfg-motes",
        chord: "Ctrl+Alt+L then M",
        tool: Tool::Lfg(LfgMode::Motes),
        alias: false,
    },
    /* Direct aliases. Ctrl+Alt+K is not in the brief's alias list, which named only R and M, but
     * one press is better than two for every chord, so Sky gets one too. Resolved against D4's
     * intent: every tool summonable from the game with the least effort. */
    RowDef {
        id: "sky-direct",
        chord: "Ctrl+Alt+K",
        tool: Tool::Sky,
        alias: true,
    },
    RowDef {
        id: "raid-direct",
        chord: "Ctrl+Alt+R",
        tool: Tool::Lfg(LfgMode::Raid),
        alias: true,
    },
    RowDef {
        id: "motes-direct",
        chord: "Ctrl+Alt+M",
        tool: Tool::Lfg(LfgMode::Motes),
        alias: true,
    },
];

/// A parsed chord: the global part (modifiers and key) and the optional second key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Spec {
    pub mods: Modifiers,
    pub key: Code,
    pub second: Option<Code>,
}

impl Spec {
    /// The global the OS is asked for.
    pub fn leader(&self) -> HotKey {
        HotKey::new(Some(self.mods), self.key)
    }

    /// Canonical text: `Ctrl+Alt+S then K`.
    pub fn text(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.mods.contains(Modifiers::CONTROL) {
            parts.push("Ctrl");
        }
        if self.mods.contains(Modifiers::ALT) {
            parts.push("Alt");
        }
        if self.mods.contains(Modifiers::SHIFT) {
            parts.push("Shift");
        }
        if self.mods.contains(Modifiers::SUPER) {
            parts.push("Win");
        }
        let name = key_name(self.key);
        parts.push(&name);
        let mut s = parts.join("+");
        if let Some(k) = self.second {
            s.push_str(" then ");
            s.push_str(&key_name(k));
        }
        s
    }
}

/// A key as the Settings screen prints it: `K`, `7`, `F5`, `Space`.
pub fn key_name(code: Code) -> String {
    let raw = format!("{code:?}");
    raw.strip_prefix("Key")
        .or_else(|| raw.strip_prefix("Digit"))
        .unwrap_or(&raw)
        .to_owned()
}

/// A key as a person types it: one letter, one digit, `F1`..`F12`, or a keyboard-types code
/// name such as `Space`.
pub fn key_code(name: &str) -> Result<Code, String> {
    let t = name.trim();
    let upper = t.to_ascii_uppercase();
    let mut chars = upper.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => {
            return format!("Key{c}")
                .parse::<Code>()
                .map_err(|_| format!("{t} is not a key"));
        }
        (Some(c), None) if c.is_ascii_digit() => {
            return format!("Digit{c}")
                .parse::<Code>()
                .map_err(|_| format!("{t} is not a key"));
        }
        _ => {}
    }
    if let Some(n) = upper.strip_prefix('F') {
        if let Ok(n) = n.parse::<u8>() {
            if (1..=12).contains(&n) {
                return format!("F{n}")
                    .parse::<Code>()
                    .map_err(|_| format!("{t} is not a key"));
            }
        }
    }
    t.parse::<Code>().map_err(|_| format!("{t} is not a key this build knows; use a letter, a digit, F1 to F12, or a code name such as Space"))
}

/// Parse `Ctrl+Alt+S then K`. Modifiers are `Ctrl`, `Alt`, `Shift`, `Win` (also `Control`,
/// `Super`, `Cmd`), in any order, joined by `+`; the last token before `then` is the key. A global
/// with no modifier at all is refused: it would swallow a plain key from every program.
pub fn parse_chord(text: &str) -> Result<Spec, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("empty".to_owned());
    }
    let (first, second) = match t.split_once(" then ") {
        Some((a, b)) => (a.trim(), Some(b.trim())),
        None => (t, None),
    };
    let mut mods = Modifiers::empty();
    let mut key: Option<Code> = None;
    let tokens: Vec<&str> = first.split('+').map(str::trim).collect();
    for (i, tok) in tokens.iter().enumerate() {
        if tok.is_empty() {
            return Err(format!("{t}: an empty token between plus signs"));
        }
        let last = i + 1 == tokens.len();
        match tok.to_ascii_lowercase().as_str() {
            "ctrl" | "control" if !last => mods |= Modifiers::CONTROL,
            "alt" | "option" if !last => mods |= Modifiers::ALT,
            "shift" if !last => mods |= Modifiers::SHIFT,
            "win" | "super" | "cmd" | "meta" if !last => mods |= Modifiers::SUPER,
            _ if last => key = Some(key_code(tok)?),
            _ => return Err(format!("{t}: {tok} is not a modifier; the key goes last")),
        }
    }
    let key = key.ok_or_else(|| format!("{t}: no key"))?;
    if mods.is_empty() {
        return Err(format!("{t}: a global hotkey needs a modifier (Ctrl, Alt, Shift or Win), or every program would lose that key"));
    }
    let second = match second {
        Some("") => return Err(format!("{t}: nothing after then")),
        Some(s) => {
            if s.contains('+') {
                return Err(format!(
                    "{t}: the second key is one plain key, no modifiers"
                ));
            }
            Some(key_code(s)?)
        }
        None => None,
    };
    Ok(Spec { mods, key, second })
}

/// One row of the effective table: the default with any override applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: &'static str,
    pub tool: Tool,
    pub alias: bool,
    pub spec: Spec,
    /// `spec.text()`, or the default text when nothing overrides it.
    pub text: String,
    pub default: &'static str,
    /// An override was applied.
    pub overridden: bool,
    /// The override did not parse; the default is in force and this says why.
    pub override_problem: Option<String>,
}

/// The effective table: `DEFAULTS` with `overrides` (row id to chord text) applied.
pub fn table(overrides: &BTreeMap<String, String>) -> Vec<Row> {
    DEFAULTS
        .iter()
        .map(|d| {
            let base = parse_chord(d.chord)
                .expect("the default table parses; see the test that holds it to that");
            match overrides.get(d.id) {
                None => Row {
                    id: d.id,
                    tool: d.tool,
                    alias: d.alias,
                    spec: base,
                    text: d.chord.to_owned(),
                    default: d.chord,
                    overridden: false,
                    override_problem: None,
                },
                Some(o) => match parse_chord(o) {
                    Ok(spec) => Row {
                        id: d.id,
                        tool: d.tool,
                        alias: d.alias,
                        spec,
                        text: spec.text(),
                        default: d.chord,
                        overridden: true,
                        override_problem: None,
                    },
                    Err(e) => Row {
                        id: d.id,
                        tool: d.tool,
                        alias: d.alias,
                        spec: base,
                        text: d.chord.to_owned(),
                        default: d.chord,
                        overridden: false,
                        override_problem: Some(format!(
                            "the saved binding {o:?} does not parse ({e}); the default is in force"
                        )),
                    },
                },
            }
        })
        .collect()
}

/// The short name of a tool inside the corner hint.
fn hint_name(t: Tool) -> &'static str {
    match t {
        Tool::Companion => "Main window",
        Tool::Watch => "Watch",
        Tool::Parser => "Parser",
        Tool::Sky => "Sky",
        Tool::Chat => "Chat",
        Tool::Overlays => "Overlays",
        Tool::Lfg(LfgMode::Generic) => "LFG",
        Tool::Lfg(LfgMode::Raid) => "Raid",
        Tool::Lfg(LfgMode::Motes) => "Motes",
    }
}

/// What a registered global does when it fires. One (modifiers, key) is a leader when any chord
/// row starts with it, and is otherwise a direct open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Open(Tool),
    Arm(HotKey),
}

/// The distinct globals to register, first appearance wins, with what each does. A key that is
/// both a bare row and a chord leader is a leader (lapsing opens the bare row's tool, the D4 `L`
/// rule). A bare key claimed by two rows is reported on the second row.
fn globals(table: &[Row]) -> (Vec<(HotKey, Action)>, HashMap<&'static str, String>) {
    let mut out: Vec<(HotKey, Action)> = Vec::new();
    let mut clashes: HashMap<&'static str, String> = HashMap::new();
    for r in table {
        let hk = r.spec.leader();
        let is_leader = table
            .iter()
            .any(|o| o.spec.leader() == hk && o.spec.second.is_some());
        let want = if is_leader {
            Action::Arm(hk)
        } else {
            Action::Open(r.tool)
        };
        match out.iter().find(|(h, _)| *h == hk) {
            None => out.push((hk, want)),
            Some((_, have)) => {
                if r.spec.second.is_none() && *have != want {
                    if let Some(first) = table
                        .iter()
                        .find(|o| o.spec.leader() == hk && o.spec.second.is_none() && o.id != r.id)
                    {
                        clashes.insert(
                            r.id,
                            format!(
                                "{} is already bound to {} in this table",
                                first.text,
                                hint_name(first.tool)
                            ),
                        );
                    }
                }
            }
        }
    }
    (out, clashes)
}

/// The bare second keys a leader can complete with, in table order, deduplicated.
fn seconds_of(table: &[Row], leader: HotKey) -> Vec<Code> {
    let mut v: Vec<Code> = Vec::new();
    for r in table {
        if r.spec.leader() == leader {
            if let Some(k) = r.spec.second {
                if !v.contains(&k) {
                    v.push(k);
                }
            }
        }
    }
    v
}

/* --------------------------------------------------------------- the chord machine -- */

/// The leader state. Pure: it is told the time and the table, it never asks for either.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Chord {
    armed: Option<(HotKey, Instant)>,
}

impl Chord {
    pub const fn new() -> Chord {
        Chord { armed: None }
    }

    /// Arm a leader at `now`. Arming replaces whatever was armed: `Ctrl+Alt+L` then `Ctrl+Alt+S`
    /// inside the window means the user changed their mind, so the pending generic LFG is dropped
    /// rather than opened on top of Sky.
    pub fn arm(&mut self, leader: HotKey, now: Instant) {
        self.armed = Some((leader, now));
    }

    pub fn cancel(&mut self) {
        self.armed = None;
    }

    pub fn armed(&self) -> Option<HotKey> {
        self.armed.map(|(l, _)| l)
    }

    fn lapsed(&self, now: Instant) -> bool {
        match self.armed {
            Some((_, since)) => now.saturating_duration_since(since) >= LEADER_WINDOW,
            None => false,
        }
    }

    /// The clock moved. If the window has lapsed the leader is disarmed, and a leader that is
    /// also bound bare (D4's `Ctrl+Alt+L` on its own) opens that row's tool. A leader with no bare
    /// row (`Ctrl+Alt+S`) lapses to nothing.
    pub fn tick(&mut self, now: Instant, table: &[Row]) -> Option<Tool> {
        if !self.lapsed(now) {
            return None;
        }
        let (leader, _) = self.armed.take()?;
        table
            .iter()
            .find(|r| r.spec.leader() == leader && r.spec.second.is_none())
            .map(|r| r.tool)
    }

    /// A second key arrived at `now`. Inside the window the right key completes the chord. A wrong
    /// key cancels it: the user is doing something else, and firing generic LFG a second later on
    /// the lapse would be a surprise. A key after the window has lapsed is not a second key at
    /// all; the lapse is applied and the key is ignored.
    pub fn key(&mut self, key: Code, now: Instant, table: &[Row]) -> Option<Tool> {
        let (leader, _) = self.armed?;
        if self.lapsed(now) {
            return self.tick(now, table);
        }
        self.armed = None;
        table
            .iter()
            .find(|r| r.spec.leader() == leader && r.spec.second == Some(key))
            .map(|r| r.tool)
    }

    /// The corner hint while armed: `S: K = Sky`, `L: R = Raid, M = Motes, wait = LFG`. Built
    /// from the table, so a rebound chord shows its real keys.
    pub fn hint(&self, table: &[Row]) -> Option<String> {
        let (leader, _) = self.armed?;
        let mut parts: Vec<String> = Vec::new();
        for r in table {
            if r.spec.leader() != leader {
                continue;
            }
            if let Some(k) = r.spec.second {
                let p = format!("{} = {}", key_name(k), hint_name(r.tool));
                if !parts.contains(&p) {
                    parts.push(p);
                }
            }
        }
        if let Some(bare) = table
            .iter()
            .find(|r| r.spec.leader() == leader && r.spec.second.is_none())
        {
            parts.push(format!("wait = {}", hint_name(bare.tool)));
        }
        Some(format!("{}: {}", key_name(leader.key), parts.join(", ")))
    }
}

/// Same tool twice in one poll collapses to one. A direct `Ctrl+Alt+R` while `L` is armed can be
/// seen by the global receiver AND, if a Grimoire window is focused and the bare `R` did not
/// register, by egui as a plain `R`; the caller must not get `Lfg(Raid)` twice for one press.
fn dedupe(v: Vec<Tool>) -> Vec<Tool> {
    let mut out: Vec<Tool> = Vec::with_capacity(v.len());
    for t in v {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/* ------------------------------------------------------------------- the bindings -- */

/// One row of the table Settings shows, with its registration outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The stable id an override is keyed by.
    pub id: &'static str,
    /// As shown: "Ctrl+Alt+S then K".
    pub chord: String,
    /// The D4 text, for a Reset control.
    pub default: &'static str,
    pub tool: Tool,
    /// A direct alias below the D4 table rather than a D4 row.
    pub alias: bool,
    /// A Settings override is in force.
    pub overridden: bool,
    /// The global part of this chord is registered with the OS, and for a chord row the second
    /// key registered when it was probed.
    pub registered: bool,
    /// Why it is not, in words. `Some` exactly when `registered` is false.
    pub conflict: Option<String>,
}

/* ---------------------------------------------------------------------- the OS half -- */

pub struct Hotkeys {
    /// Kept alive: dropping the manager drops the registrations.
    manager: Option<GlobalHotKeyManager>,
    table: Vec<Row>,
    /// The globals that did register, so `reinstall` and `Drop` can give them back.
    registered: Vec<HotKey>,
    /// Registration outcome per global. `None` = registered, `Some(text)` = why not.
    outcome: HashMap<HotKey, Option<String>>,
    /// The probe of every second key at install: `None` = it registered and was released.
    second_outcome: HashMap<Code, Option<String>>,
    /// A row whose bare key another row already holds.
    clashes: HashMap<&'static str, String>,
    /// Hotkey id (as the event reports it) to what it does.
    by_id: HashMap<u32, Action>,
    /// The bare second keys registered while a leader is armed, by event id.
    armed_keys: HashMap<u32, (HotKey, Code)>,
    chord: Chord,
    /// egui time (seconds, `InputState::time`) of the root pass BEFORE the one that armed the
    /// leader. A tool window's input older than this is stale and cannot complete a chord. See
    /// `second_keys` for why the bound is the previous pass and not the arming pass.
    armed_at: f64,
    last_poll_time: f64,
    /// Why there is no manager, kept from `install` so `reinstall` reports the same reason
    /// instead of a shorter one with the error dropped.
    manager_problem: Option<String>,
    /// An outcome changed since `bindings()` was last handed out: a bare second key that would
    /// not register when a leader armed, or a release that failed. The App re-reads `bindings`
    /// when this is set, so Settings shows it now and not after the next rebind.
    dirty: bool,
}

impl Hotkeys {
    /// Register every global in the effective table. A failure is recorded on its bindings and
    /// the rest still register. Must be called on the thread that pumps window messages (eframe's
    /// main thread), because that is where `RegisterHotKey` delivers.
    pub fn install(overrides: &BTreeMap<String, String>) -> Hotkeys {
        let mut hk = Hotkeys::unregistered(overrides);
        match GlobalHotKeyManager::new() {
            Ok(manager) => {
                hk.manager = Some(manager);
                hk.register_table();
            }
            Err(e) => {
                /* No manager at all: every binding is unregistered for the same reason, and
                 * Settings shows that reason on every row rather than a silent blank column. */
                let text = format!("global hotkeys are unavailable on this system: {e}");
                log::warn!("{text}");
                hk.record_manager_absence(&text);
                hk.manager_problem = Some(text);
            }
        }
        hk
    }

    /// Whether an outcome changed since the last `bindings()`; cleared by the call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// A `Hotkeys` that has not touched the OS. Every binding reports itself unregistered with a
    /// reason. What a test builds, and the state `reinstall` passes through.
    pub fn unregistered(overrides: &BTreeMap<String, String>) -> Hotkeys {
        let table = table(overrides);
        let (_, clashes) = globals(&table);
        Hotkeys {
            manager: None,
            table,
            registered: Vec::new(),
            outcome: HashMap::new(),
            second_outcome: HashMap::new(),
            clashes,
            by_id: HashMap::new(),
            armed_keys: HashMap::new(),
            chord: Chord::new(),
            armed_at: 0.0,
            last_poll_time: 0.0,
            manager_problem: None,
            dirty: false,
        }
    }

    /// Apply a changed set of overrides: release every registration, rebuild the table, register
    /// again. The manager is kept; a chord armed at the moment is dropped.
    pub fn reinstall(&mut self, overrides: &BTreeMap<String, String>) {
        self.release_all();
        let table = table(overrides);
        let (_, clashes) = globals(&table);
        self.table = table;
        self.clashes = clashes;
        self.outcome.clear();
        self.second_outcome.clear();
        self.by_id.clear();
        self.chord.cancel();
        match &self.manager {
            Some(_) => self.register_table(),
            None => {
                /* the reason `install` recorded, error and all, not a shorter restatement */
                let text = self
                    .manager_problem
                    .clone()
                    .unwrap_or_else(|| "global hotkeys are unavailable on this system".to_owned());
                self.record_manager_absence(&text);
            }
        }
        self.dirty = false;
    }

    fn record_manager_absence(&mut self, text: &str) {
        let (g, _) = globals(&self.table);
        for (hk, _) in g {
            self.outcome.insert(hk, Some(text.to_owned()));
        }
        for r in &self.table {
            if let Some(k) = r.spec.second {
                self.second_outcome.insert(k, Some(text.to_owned()));
            }
        }
    }

    fn register_table(&mut self) {
        let Some(manager) = &self.manager else { return };
        let (g, _) = globals(&self.table);
        for (hk, action) in g {
            if self.registered.contains(&hk) {
                /* Still registered from before, because its release failed (see `release_all`).
                 * The OS would answer AlreadyRegistered and the conflict text would blame
                 * "another program", which would be a lie: the owner is this process, and the
                 * binding works. */
                self.outcome.insert(hk, None);
                self.by_id.insert(hk.id(), action);
                continue;
            }
            match manager.register(hk) {
                Ok(()) => {
                    self.outcome.insert(hk, None);
                    self.by_id.insert(hk.id(), action);
                    self.registered.push(hk);
                }
                Err(e) => {
                    let text = conflict_text(
                        &Spec {
                            mods: hk.mods,
                            key: hk.key,
                            second: None,
                        }
                        .text(),
                        &e,
                    );
                    log::warn!("hotkey: {text}");
                    self.outcome.insert(hk, Some(text));
                }
            }
        }
        /* The probe: every second key, registered bare and released at once, so Settings can say
         * now whether the chord will complete from the game rather than the first time someone
         * presses it. */
        let mut seconds: Vec<Code> = Vec::new();
        for r in &self.table {
            if let Some(k) = r.spec.second {
                if !seconds.contains(&k) {
                    seconds.push(k);
                }
            }
        }
        for k in seconds {
            let bare = HotKey::new(None, k);
            match manager.register(bare) {
                Ok(()) => match manager.unregister(bare) {
                    Ok(()) => {
                        self.second_outcome.insert(k, None);
                    }
                    Err(e) => {
                        /* A RELEASE THAT FAILS IS NOT NOTHING. The bare key is now held by this
                         * process, stolen from the game and every other program, until a later
                         * release succeeds or the process exits. It is kept in `registered` so
                         * `release_all` and `Drop` try again, and the row says so rather than
                         * "registered". */
                        let text = format!("the second key {} registered but could not be released ({e}); this program holds the bare key until it can, which takes it from the game", key_name(k));
                        log::warn!("hotkey: {text}");
                        self.registered.push(bare);
                        self.second_outcome.insert(k, Some(text));
                    }
                },
                Err(e) => {
                    let text = format!(
                        "the second key {} cannot be taken while the leader is armed: {}",
                        key_name(k),
                        conflict_text(&key_name(k), &e)
                    );
                    log::warn!("hotkey: {text}");
                    self.second_outcome.insert(k, Some(text));
                }
            }
        }
    }

    /// Give every global back. A release that fails keeps its key in `registered` so the next
    /// call (and `Drop`) tries again, and is logged; it is not silently forgotten, because a
    /// forgotten registration is a key stolen from every other program with nothing saying so.
    fn release_all(&mut self) {
        self.release_armed();
        let mut kept: Vec<HotKey> = Vec::new();
        if let Some(m) = &self.manager {
            for key in &self.registered {
                if let Err(e) = m.unregister(*key) {
                    log::warn!("hotkey: {} could not be released ({e}); it stays registered to this process", Spec { mods: key.mods, key: key.key, second: None }.text());
                    kept.push(*key);
                }
            }
        }
        self.registered = kept;
    }

    /// Register the bare second keys of `leader` for the life of the chord.
    fn take_armed(&mut self, leader: HotKey) {
        self.release_armed();
        let Some(m) = &self.manager else { return };
        for k in seconds_of(&self.table, leader) {
            let bare = HotKey::new(None, k);
            match m.register(bare) {
                Ok(()) => {
                    self.armed_keys.insert(bare.id(), (bare, k));
                    if self.second_outcome.insert(k, None).flatten().is_some() {
                        /* it failed before and works now: Settings should stop saying it fails */
                        self.dirty = true;
                    }
                }
                Err(e) => {
                    let text = format!(
                        "the second key {} cannot be taken while the leader is armed: {}",
                        key_name(k),
                        conflict_text(&key_name(k), &e)
                    );
                    log::warn!("hotkey: {text}");
                    if self
                        .second_outcome
                        .insert(k, Some(text))
                        .flatten()
                        .is_none()
                    {
                        /* a new failure, at arm time: hand it to Settings now (`take_dirty`) */
                        self.dirty = true;
                    }
                }
            }
        }
    }

    /// Release the bare second keys. One that will not release stays with this process; it is
    /// moved to `registered` so `release_all` and `Drop` retry, its row says so, and the App is
    /// told to re-read the bindings.
    fn release_armed(&mut self) {
        let mut stuck: Vec<(HotKey, Code, String)> = Vec::new();
        if let Some(m) = &self.manager {
            for (hk, code) in self.armed_keys.values() {
                if let Err(e) = m.unregister(*hk) {
                    stuck.push((*hk, *code, e.to_string()));
                }
            }
        }
        self.armed_keys.clear();
        for (hk, code, e) in stuck {
            let text = format!("the second key {} could not be released after the chord ({e}); this program holds the bare key until it can, which takes it from the game", key_name(code));
            log::warn!("hotkey: {text}");
            self.registered.push(hk);
            self.second_outcome.insert(code, Some(text));
            self.dirty = true;
        }
    }

    /// Drain the OS receiver, resolve chords, apply the lapse, and keep the root pass ticking.
    /// Returns every tool the user asked for since the last poll, deduplicated, in order.
    ///
    /// CALL THIS FROM `App::heartbeat`, WHICH BOTH EFRAME CALLBACKS REACH. It asks for a repaint
    /// every `POLL_EVERY` so the receiver keeps being drained while the app is idle.
    ///
    /// THIS PARAGRAPH USED TO SAY the receiver was drained when the window was hidden or minimized
    /// because `eframe keeps running the root logic in those states, which is what makes
    /// Ctrl+Alt+G able to bring the window back`. Half of that is true and the half that mattered
    /// was not. eframe does keep calling the app in those states, but it calls `App::logic`
    /// (glow_integration.rs:637-649), and this crate implemented only `ui`, so the call landed on
    /// eframe`s empty default (epi.rs:167-169). The drain lived inside `ui`. Since the main
    /// window's toggle minimizes the app ITSELF, `Ctrl+Alt+G` put the window away and then could
    /// not bring it back, which is precisely what the sentence promised it could.
    ///
    /// A REPAINT REQUEST CANNOT RESCUE THAT, and this is the part worth remembering: eframe
    /// computes `show_ui` from VISIBILITY (glow_integration.rs:622), never from the repaint flag.
    /// Asking harder for a frame does not get you one.
    pub fn poll(&mut self, ctx: &egui::Context) -> Vec<Tool> {
        let now = Instant::now();
        let egui_now = ctx.input(|i| i.time);
        let mut out: Vec<Tool> = Vec::new();
        let armed_before = self.chord.armed();

        let rx = GlobalHotKeyEvent::receiver();
        let mut arm: Option<HotKey> = None;
        while let Ok(ev) = rx.try_recv() {
            if ev.state() != HotKeyState::Pressed {
                continue;
            }
            if let Some((_, code)) = self.armed_keys.get(&ev.id()).copied() {
                if let Some(tool) = self.chord.key(code, now, &self.table) {
                    out.push(tool);
                }
                continue;
            }
            match self.by_id.get(&ev.id()).copied() {
                Some(Action::Open(tool)) => {
                    /* A direct global while a leader is armed IS the completion the user meant
                     * (Ctrl+Alt+L, then Ctrl+Alt+R with the modifiers still held). Cancel the
                     * leader so the lapse does not also open generic LFG a second later. */
                    self.chord.cancel();
                    arm = None;
                    out.push(tool);
                }
                Some(Action::Arm(leader)) => {
                    self.chord.arm(leader, now);
                    self.armed_at = self.last_poll_time;
                    arm = Some(leader);
                }
                None => {}
            }
        }

        if self.chord.armed().is_some() {
            for key in self.second_keys(ctx) {
                match key {
                    None => self.chord.cancel(),
                    Some(code) => {
                        if let Some(tool) = self.chord.key(code, now, &self.table) {
                            out.push(tool);
                        }
                    }
                }
            }
        }

        if let Some(tool) = self.chord.tick(now, &self.table) {
            out.push(tool);
        }

        /* The bare second keys follow the armed state: taken when a leader arms, released the
         * moment nothing is armed. */
        let armed_after = self.chord.armed();
        if armed_after != armed_before || arm.is_some() {
            match armed_after {
                Some(leader) => self.take_armed(leader),
                None => self.release_armed(),
            }
        }

        self.last_poll_time = egui_now;
        ctx.request_repaint_after(POLL_EVERY);
        dedupe(out)
    }

    /// The second keys pressed since the last poll, from egui's own input: the current viewport's
    /// and every other Grimoire viewport's last pass. `None` is Escape, which cancels.
    ///
    /// This is the fallback path. With the bare globals registered the OS delivers the key to the
    /// receiver and egui never sees it; when a bare key could not be registered, a focused Grimoire
    /// window still completes the chord this way.
    ///
    /// The current (root) viewport's input is fresh every pass. A tool window's `InputState` holds
    /// the events of THAT window's last pass, which may be older than this root pass, so two
    /// things are checked: the window's pass time is not before the leader was armed (a `K`
    /// typed in the Watch window an hour ago must not complete a chord armed now), and the chord
    /// is disarmed the moment it completes so the same pass is not read twice. The bound is the
    /// root pass BEFORE the arming one because the global press can be up to `POLL_EVERY` older
    /// than the pass that drained it, and a `K` typed in that gap is a real second key.
    fn second_keys(&self, ctx: &egui::Context) -> Vec<Option<Code>> {
        let Some(leader) = self.chord.armed() else {
            return Vec::new();
        };
        let wanted: Vec<(Code, egui::Key)> = seconds_of(&self.table, leader)
            .into_iter()
            .filter_map(|c| egui_key(c).map(|k| (c, k)))
            .collect();
        let read = |i: &egui::InputState| -> Vec<Option<Code>> {
            let mut v = Vec::new();
            if i.key_pressed(egui::Key::Escape) {
                v.push(None);
            }
            for (code, key) in &wanted {
                if i.key_pressed(*key) {
                    v.push(Some(*code));
                }
            }
            v
        };
        let here = ctx.viewport_id();
        let mut keys = ctx.input(read);
        let others: Vec<egui::ViewportId> = ctx.input(|i| {
            i.raw
                .viewports
                .keys()
                .copied()
                .filter(|id| *id != here)
                .collect()
        });
        for id in others {
            let armed_at = self.armed_at;
            keys.extend(ctx.input_for(id, |i| {
                if i.time >= armed_at {
                    read(i)
                } else {
                    Vec::new()
                }
            }));
        }
        keys
    }

    /// The table Settings lists: D4's seven rows in D4's order, then the three direct aliases,
    /// each with whether its global (and, for a chord, its second key) registered and why not.
    pub fn bindings(&self) -> Vec<Binding> {
        self.table
            .iter()
            .map(|r| {
                let hk = r.spec.leader();
                let leader = match self.outcome.get(&hk) {
                    Some(Some(text)) => Some(text.clone()),
                    Some(None) => None,
                    /* `install` fills every global, so this only happens for a `Hotkeys` built by
                     * `unregistered`. Say so rather than claim a registration. */
                    None => Some("not registered: install() was not run".to_owned()),
                };
                let second = r
                    .spec
                    .second
                    .and_then(|k| match self.second_outcome.get(&k) {
                        Some(Some(text)) => Some(text.clone()),
                        Some(None) => None,
                        None => Some(format!(
                            "the second key {} was not probed: install() was not run",
                            key_name(k)
                        )),
                    });
                let conflict = r
                    .override_problem
                    .clone()
                    .or_else(|| self.clashes.get(r.id).cloned())
                    .or(leader)
                    .or(second);
                Binding {
                    id: r.id,
                    chord: r.text.clone(),
                    default: r.default,
                    tool: r.tool,
                    alias: r.alias,
                    overridden: r.overridden,
                    registered: conflict.is_none(),
                    conflict,
                }
            })
            .collect()
    }

    /// "S: K = Sky" while a leader is armed, otherwise nothing.
    pub fn leader_hint(&self) -> Option<String> {
        self.chord.hint(&self.table)
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        self.release_all();
    }
}

/// egui's name for a keyboard-types code, for the fallback read. Letters, digits and F keys;
/// anything else has no fallback and completes only through the registered bare global.
fn egui_key(code: Code) -> Option<egui::Key> {
    egui::Key::from_name(&key_name(code))
}

/// Words for a registration failure. Windows reports that a hotkey is taken and never by whom;
/// the text says that, rather than inventing a name.
fn conflict_text(chord: &str, e: &global_hotkey::Error) -> String {
    match e {
        global_hotkey::Error::AlreadyRegistered(_) => {
            format!("{chord} is already owned by another program (the OS does not say which)")
        }
        global_hotkey::Error::OsError(io) if io.raw_os_error() == Some(1409) => {
            /* ERROR_HOTKEY_ALREADY_REGISTERED, in case the crate hands the raw error through. */
            format!("{chord} is already owned by another program (the OS does not say which)")
        }
        other => format!("{chord} could not be registered: {other}"),
    }
}

/* ------------------------------------------------------------------- the corner hint -- */

/// Paint the leader hint in the bottom right corner of the current viewport, above everything.
/// Small, monospace, on a panel. The main window and every tool window call this with
/// `leader_hint()` so the hint follows whatever is on screen, as D4 asks.
pub fn draw_hint(ctx: &egui::Context, hint: &str) {
    use crate::theme::{GOLD_DEEP, PANEL_2, TEXT};
    let font = egui::FontId::monospace(11.5);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("grimoire.leader.hint"),
    ));
    let galley = painter.layout_no_wrap(hint.to_owned(), font.clone(), TEXT);
    let pad = egui::Vec2::new(10.0, 6.0);
    let size = galley.rect.size() + pad * 2.0;
    let screen = ctx.content_rect();
    let rect =
        egui::Rect::from_min_size(screen.right_bottom() - size - egui::Vec2::splat(12.0), size);
    painter.rect_filled(rect, egui::CornerRadius::ZERO, PANEL_2);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::ZERO,
        egui::Stroke::new(1.0, GOLD_DEEP),
        egui::StrokeKind::Middle,
    );
    painter.text(rect.min + pad, egui::Align2::LEFT_TOP, hint, font, TEXT);
}

/* ---------------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }
    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }
    fn defaults() -> Vec<Row> {
        table(&BTreeMap::new())
    }
    fn ctrl_alt(code: Code) -> HotKey {
        HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), code)
    }

    /* The table, exactly as D4 wrote it, then the aliases. A change to either side of this
     * assertion is a change to the product's hotkeys and must be made in the goal doc first. */
    #[test]
    fn binding_table_is_d4_exactly() {
        let d4: Vec<(&str, Tool)> = vec![
            ("Ctrl+Alt+G", Tool::Companion),
            ("Ctrl+Alt+W", Tool::Watch),
            ("Ctrl+Alt+P", Tool::Parser),
            ("Ctrl+Alt+S then K", Tool::Sky),
            /* ADDED TO D4 ON 2026-09-04, in the doc first, as the note above this test requires.
             * Chat over the game is what the owner says the app is FOR, so it is a D4 row rather
             * than an alias appended to the end. */
            ("Ctrl+Alt+C", Tool::Chat),
            /* ADDED TO D4 ON 2026-09-05, in the doc first, as the note above this test requires.
             * See D11: the combat overlays are windows in this same registry and DPS is the
             * first. It sits beside Chat rather than in the aliases because an overlay read
             * mid-pull is summoned with one press or it is not summoned at all. */
            ("Ctrl+Alt+D", Tool::Overlays),
            ("Ctrl+Alt+L", Tool::Lfg(LfgMode::Generic)),
            ("Ctrl+Alt+L then R", Tool::Lfg(LfgMode::Raid)),
            ("Ctrl+Alt+L then M", Tool::Lfg(LfgMode::Motes)),
        ];
        let aliases: Vec<(&str, Tool)> = vec![
            ("Ctrl+Alt+K", Tool::Sky),
            ("Ctrl+Alt+R", Tool::Lfg(LfgMode::Raid)),
            ("Ctrl+Alt+M", Tool::Lfg(LfgMode::Motes)),
        ];
        let got: Vec<(&str, Tool)> = DEFAULTS.iter().map(|r| (r.chord, r.tool)).collect();
        assert_eq!(
            &got[..d4.len()],
            &d4[..],
            "the first rows must be D4 verbatim, in D4 order"
        );
        assert_eq!(
            &got[d4.len()..],
            &aliases[..],
            "the tail must be exactly the documented aliases"
        );
        for (i, r) in DEFAULTS.iter().enumerate() {
            assert_eq!(r.alias, i >= d4.len(), "{}", r.chord);
        }
        let ids: std::collections::HashSet<&str> = DEFAULTS.iter().map(|r| r.id).collect();
        assert_eq!(
            ids.len(),
            DEFAULTS.len(),
            "row ids are distinct: an override is keyed by them"
        );
    }

    /* Every default parses, and prints back as itself. */
    #[test]
    fn every_default_parses_and_round_trips_as_text() {
        for r in defaults() {
            assert!(!r.overridden);
            assert_eq!(r.override_problem, None);
            assert_eq!(r.spec.text(), r.text, "{}", r.id);
            assert_eq!(r.text, r.default);
            assert_eq!(
                r.spec.mods,
                Modifiers::CONTROL | Modifiers::ALT,
                "every D4 global is Ctrl+Alt"
            );
        }
    }

    #[test]
    fn chord_parsing_reads_people_not_code_names() {
        let s = parse_chord("Ctrl+Alt+S then K").unwrap();
        assert_eq!(
            s,
            Spec {
                mods: Modifiers::CONTROL | Modifiers::ALT,
                key: Code::KeyS,
                second: Some(Code::KeyK)
            }
        );
        assert_eq!(
            parse_chord(" ctrl + alt + k ").unwrap().key,
            Code::KeyK,
            "case and spaces do not matter"
        );
        assert_eq!(
            parse_chord("Shift+Win+F5").unwrap(),
            Spec {
                mods: Modifiers::SHIFT | Modifiers::SUPER,
                key: Code::F5,
                second: None
            }
        );
        assert_eq!(parse_chord("Ctrl+7").unwrap().key, Code::Digit7);
        assert_eq!(parse_chord("Alt+Space").unwrap().key, Code::Space);
        assert!(
            parse_chord("K").unwrap_err().contains("modifier"),
            "a bare global would steal K from every program"
        );
        assert!(parse_chord("Ctrl+").is_err());
        assert!(parse_chord("Ctrl+Alt+S then").is_err());
        assert!(parse_chord("Ctrl+Alt+S then Ctrl+K")
            .unwrap_err()
            .contains("plain key"));
        assert!(parse_chord("Ctrl+Alt+Bogus").is_err());
        assert!(parse_chord("").is_err());
        assert_eq!(
            parse_chord("Control+Option+Q").unwrap().text(),
            "Ctrl+Alt+Q"
        );
    }

    #[test]
    fn overrides_apply_by_id_and_a_bad_one_keeps_the_default_with_a_reason() {
        let mut o = BTreeMap::new();
        o.insert("watch".to_owned(), "Ctrl+Shift+W".to_owned());
        o.insert("sky".to_owned(), "not a chord".to_owned());
        let t = table(&o);
        let watch = t.iter().find(|r| r.id == "watch").unwrap();
        assert!(watch.overridden);
        assert_eq!(watch.text, "Ctrl+Shift+W");
        assert_eq!(watch.spec.mods, Modifiers::CONTROL | Modifiers::SHIFT);
        let sky = t.iter().find(|r| r.id == "sky").unwrap();
        assert!(!sky.overridden, "a broken override is not in force");
        assert_eq!(sky.text, "Ctrl+Alt+S then K");
        assert!(sky
            .override_problem
            .as_deref()
            .unwrap()
            .contains("does not parse"));
        let hk = Hotkeys::unregistered(&o);
        let b = hk.bindings();
        let row = b.iter().find(|x| x.id == "sky").unwrap();
        assert!(!row.registered);
        assert!(
            row.conflict.as_deref().unwrap().contains("does not parse"),
            "{:?}",
            row.conflict
        );
    }

    #[test]
    fn globals_are_ten_distinct_keys_with_the_right_actions() {
        let (g, clashes) = globals(&defaults());
        /* TEN SINCE Ctrl+Alt+D JOINED D4 (2026-09-05, D11: the DPS overlay). The count is asserted
         * rather than derived so that a row silently colliding with an existing key, which
         * `globals` would fold into one entry, fails here instead of quietly costing the app a
         * chord. */
        assert_eq!(g.len(), 10);
        assert!(clashes.is_empty());
        let action = |code: Code| {
            g.iter()
                .find(|(h, _)| *h == ctrl_alt(code))
                .map(|(_, a)| *a)
                .unwrap()
        };
        assert_eq!(action(Code::KeyG), Action::Open(Tool::Companion));
        assert_eq!(action(Code::KeyW), Action::Open(Tool::Watch));
        assert_eq!(action(Code::KeyP), Action::Open(Tool::Parser));
        assert_eq!(action(Code::KeyS), Action::Arm(ctrl_alt(Code::KeyS)));
        assert_eq!(
            action(Code::KeyL),
            Action::Arm(ctrl_alt(Code::KeyL)),
            "L is a leader even though it is also bound bare"
        );
        assert_eq!(action(Code::KeyC), Action::Open(Tool::Chat));
        assert_eq!(action(Code::KeyD), Action::Open(Tool::Overlays));
        assert_eq!(action(Code::KeyK), Action::Open(Tool::Sky));
        assert_eq!(action(Code::KeyR), Action::Open(Tool::Lfg(LfgMode::Raid)));
        assert_eq!(action(Code::KeyM), Action::Open(Tool::Lfg(LfgMode::Motes)));
        assert_eq!(
            seconds_of(&defaults(), ctrl_alt(Code::KeyL)),
            vec![Code::KeyR, Code::KeyM]
        );
        assert_eq!(
            seconds_of(&defaults(), ctrl_alt(Code::KeyS)),
            vec![Code::KeyK]
        );
        assert!(seconds_of(&defaults(), ctrl_alt(Code::KeyW)).is_empty());
    }

    #[test]
    fn two_bare_rows_on_one_key_report_the_second() {
        let mut o = BTreeMap::new();
        o.insert("parser".to_owned(), "Ctrl+Alt+W".to_owned());
        let hk = Hotkeys::unregistered(&o);
        let b = hk.bindings();
        let parser = b.iter().find(|x| x.id == "parser").unwrap();
        assert!(
            parser
                .conflict
                .as_deref()
                .unwrap()
                .contains("already bound to Watch"),
            "{:?}",
            parser.conflict
        );
        let watch = b.iter().find(|x| x.id == "watch").unwrap();
        assert!(
            !watch.conflict.as_deref().unwrap().contains("already bound"),
            "the first claim stands: {:?}",
            watch.conflict
        );
    }

    /* ---- the chord machine ---- */

    #[test]
    fn s_then_k_opens_sky() {
        let t = defaults();
        let b = t0();
        let mut c = Chord::new();
        c.arm(ctrl_alt(Code::KeyS), b);
        assert_eq!(c.hint(&t).as_deref(), Some("S: K = Sky"));
        assert_eq!(c.key(Code::KeyK, at(b, 400), &t), Some(Tool::Sky));
        assert_eq!(c.armed(), None, "completing disarms");
        assert_eq!(c.hint(&t), None);
    }

    #[test]
    fn s_lapsing_opens_nothing() {
        let t = defaults();
        let b = t0();
        let mut c = Chord::new();
        c.arm(ctrl_alt(Code::KeyS), b);
        assert_eq!(c.tick(at(b, 1499), &t), None);
        assert_eq!(
            c.armed(),
            Some(ctrl_alt(Code::KeyS)),
            "still inside the window"
        );
        assert_eq!(
            c.tick(at(b, 1500), &t),
            None,
            "S lapses to nothing: no row is bound to the bare leader"
        );
        assert_eq!(c.armed(), None);
    }

    #[test]
    fn l_then_r_is_raid_and_l_then_m_is_motes() {
        let t = defaults();
        let b = t0();
        let mut c = Chord::new();
        c.arm(ctrl_alt(Code::KeyL), b);
        assert_eq!(
            c.hint(&t).as_deref(),
            Some("L: R = Raid, M = Motes, wait = LFG")
        );
        assert_eq!(
            c.key(Code::KeyR, at(b, 10), &t),
            Some(Tool::Lfg(LfgMode::Raid))
        );
        c.arm(ctrl_alt(Code::KeyL), at(b, 2000));
        assert_eq!(
            c.key(Code::KeyM, at(b, 2010), &t),
            Some(Tool::Lfg(LfgMode::Motes))
        );
    }

    #[test]
    fn l_lapsing_opens_generic_lfg_exactly_once() {
        let t = defaults();
        let b = t0();
        let mut c = Chord::new();
        c.arm(ctrl_alt(Code::KeyL), b);
        assert_eq!(c.tick(at(b, 100), &t), None);
        assert_eq!(c.tick(at(b, 1500), &t), Some(Tool::Lfg(LfgMode::Generic)));
        assert_eq!(c.tick(at(b, 1600), &t), None, "a lapse fires once");
        assert_eq!(c.armed(), None);
    }

    #[test]
    fn a_key_after_the_window_is_a_lapse_not_a_completion() {
        let t = defaults();
        let b = t0();
        let mut c = Chord::new();
        c.arm(ctrl_alt(Code::KeyL), b);
        assert_eq!(
            c.key(Code::KeyR, at(b, 1500), &t),
            Some(Tool::Lfg(LfgMode::Generic)),
            "late R: the L lapse applies, R is ignored"
        );
        c.arm(ctrl_alt(Code::KeyS), at(b, 3000));
        assert_eq!(
            c.key(Code::KeyK, at(b, 4600), &t),
            None,
            "late K: S lapses to nothing"
        );
        assert_eq!(c.armed(), None);
    }

    #[test]
    fn wrong_keys_cancel() {
        let t = defaults();
        let b = t0();
        let mut c = Chord::new();
        c.arm(ctrl_alt(Code::KeyL), b);
        assert_eq!(
            c.key(Code::KeyK, at(b, 10), &t),
            None,
            "K is not a second key of L"
        );
        assert_eq!(c.armed(), None);
        assert_eq!(
            c.tick(at(b, 5000), &t),
            None,
            "a cancelled L must not fire generic LFG on the lapse"
        );

        c.arm(ctrl_alt(Code::KeyS), b);
        assert_eq!(
            c.key(Code::KeyR, at(b, 10), &t),
            None,
            "R is not a second key of S"
        );
        assert_eq!(c.armed(), None);
    }

    #[test]
    fn cancel_and_rearm() {
        let t = defaults();
        let b = t0();
        let mut c = Chord::new();
        c.arm(ctrl_alt(Code::KeyL), b);
        c.cancel();
        assert_eq!(c.armed(), None);
        assert_eq!(c.tick(at(b, 9000), &t), None);

        /* Re-arming restarts the window from the new press. */
        c.arm(ctrl_alt(Code::KeyL), b);
        c.arm(ctrl_alt(Code::KeyL), at(b, 1000));
        assert_eq!(
            c.tick(at(b, 2000), &t),
            None,
            "1000ms after the second press: still armed"
        );
        assert_eq!(c.tick(at(b, 2500), &t), Some(Tool::Lfg(LfgMode::Generic)));

        /* Arming a different leader replaces the pending one without firing it. */
        c.arm(ctrl_alt(Code::KeyL), b);
        c.arm(ctrl_alt(Code::KeyS), at(b, 500));
        assert_eq!(c.hint(&t).as_deref(), Some("S: K = Sky"));
        assert_eq!(
            c.tick(at(b, 3000), &t),
            None,
            "the replaced L must not fire generic LFG"
        );
    }

    #[test]
    fn keys_with_nothing_armed_do_nothing() {
        let t = defaults();
        let b = t0();
        let mut c = Chord::new();
        assert_eq!(c.key(Code::KeyK, b, &t), None);
        assert_eq!(c.key(Code::KeyR, b, &t), None);
        assert_eq!(c.tick(at(b, 9000), &t), None);
        assert_eq!(c.hint(&t), None);
    }

    #[test]
    fn a_rebound_chord_completes_on_its_own_keys_and_the_hint_says_so() {
        let mut o = BTreeMap::new();
        o.insert("sky".to_owned(), "Ctrl+Shift+X then Y".to_owned());
        let t = table(&o);
        let leader = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyX);
        let (g, _) = globals(&t);
        assert!(g
            .iter()
            .any(|(h, a)| *h == leader && *a == Action::Arm(leader)));
        assert!(
            !g.iter().any(|(h, _)| *h == ctrl_alt(Code::KeyS)),
            "the old leader is gone"
        );
        let b = t0();
        let mut c = Chord::new();
        c.arm(leader, b);
        assert_eq!(c.hint(&t).as_deref(), Some("X: Y = Sky"));
        assert_eq!(
            c.key(Code::KeyK, at(b, 10), &t),
            None,
            "K no longer completes it"
        );
        c.arm(leader, at(b, 100));
        assert_eq!(c.key(Code::KeyY, at(b, 110), &t), Some(Tool::Sky));
    }

    #[test]
    fn dedupe_keeps_first_occurrence_and_order() {
        let v = vec![
            Tool::Lfg(LfgMode::Raid),
            Tool::Sky,
            Tool::Lfg(LfgMode::Raid),
            Tool::Watch,
            Tool::Sky,
        ];
        assert_eq!(
            dedupe(v),
            vec![Tool::Lfg(LfgMode::Raid), Tool::Sky, Tool::Watch]
        );
    }

    #[test]
    fn conflict_text_names_the_chord_and_admits_what_the_os_does_not_say() {
        let e = global_hotkey::Error::AlreadyRegistered(ctrl_alt(Code::KeyW));
        let t = conflict_text("Ctrl+Alt+W", &e);
        assert!(t.starts_with("Ctrl+Alt+W is already owned"), "{t}");
        assert!(t.contains("does not say which"), "{t}");
        let e = global_hotkey::Error::FailedToRegister("boom".into());
        let t = conflict_text("Ctrl+Alt+P", &e);
        assert!(t.starts_with("Ctrl+Alt+P could not be registered"), "{t}");
        assert!(t.contains("boom"), "{t}");
    }

    /* A `Hotkeys` that never touched the OS reports every row as unregistered with a reason,
     * never as registered. This is the shape Settings must be able to render. */
    #[test]
    fn bindings_without_install_are_honest() {
        let hk = Hotkeys::unregistered(&BTreeMap::new());
        let b = hk.bindings();
        assert_eq!(b.len(), DEFAULTS.len());
        for row in &b {
            assert!(!row.registered);
            assert!(row.conflict.is_some(), "{}", row.chord);
            assert!(!row.overridden);
        }
        assert_eq!(hk.leader_hint(), None);
        assert_eq!(b.iter().filter(|r| r.alias).count(), 3);
    }

    #[test]
    fn a_registered_leader_marks_all_its_rows_registered_and_a_failed_second_key_marks_the_chord() {
        let mut hk = Hotkeys::unregistered(&BTreeMap::new());
        let (g, _) = globals(&hk.table);
        for (h, _) in g {
            hk.outcome.insert(h, None);
        }
        for k in [Code::KeyK, Code::KeyR, Code::KeyM] {
            hk.second_outcome.insert(k, None);
        }
        hk.outcome.insert(
            ctrl_alt(Code::KeyW),
            Some(
                "Ctrl+Alt+W is already owned by another program (the OS does not say which)".into(),
            ),
        );
        let b = hk.bindings();
        let row = |chord: &str| b.iter().find(|x| x.chord == chord).unwrap().clone();
        assert!(!row("Ctrl+Alt+W").registered);
        assert!(row("Ctrl+Alt+W")
            .conflict
            .as_deref()
            .unwrap()
            .contains("another program"));
        for chord in [
            "Ctrl+Alt+L",
            "Ctrl+Alt+L then R",
            "Ctrl+Alt+L then M",
            "Ctrl+Alt+S then K",
            "Ctrl+Alt+G",
        ] {
            assert!(row(chord).registered, "{chord}");
            assert_eq!(row(chord).conflict, None);
        }
        /* the probe failed for K: the S chord cannot complete from the game and says so; the
         * direct alias Ctrl+Alt+K is untouched by that */
        hk.second_outcome.insert(Code::KeyK, Some("the second key K cannot be taken while the leader is armed: K is already owned by another program (the OS does not say which)".into()));
        let b = hk.bindings();
        let row = |chord: &str| b.iter().find(|x| x.chord == chord).unwrap().clone();
        assert!(!row("Ctrl+Alt+S then K").registered);
        assert!(row("Ctrl+Alt+S then K")
            .conflict
            .as_deref()
            .unwrap()
            .contains("second key K"));
        assert!(row("Ctrl+Alt+K").registered);
    }

    #[test]
    fn egui_fallback_knows_letters_digits_and_f_keys() {
        assert_eq!(egui_key(Code::KeyK), Some(egui::Key::K));
        assert_eq!(egui_key(Code::Digit7), Some(egui::Key::Num7));
        assert_eq!(egui_key(Code::F5), Some(egui::Key::F5));
        assert_eq!(key_name(Code::KeyK), "K");
        assert_eq!(key_name(Code::Digit0), "0");
        assert_eq!(key_name(Code::Space), "Space");
    }

    #[test]
    fn no_dashes_in_user_facing_text() {
        for r in DEFAULTS {
            assert!(!r.chord.contains('\u{2014}') && !r.chord.contains('\u{2013}'));
        }
        let t = defaults();
        let mut c = Chord::new();
        c.arm(ctrl_alt(Code::KeyL), t0());
        let h = c.hint(&t).unwrap();
        assert!(!h.contains('\u{2014}') && !h.contains('\u{2013}'));
    }
}
