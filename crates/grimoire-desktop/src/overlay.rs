//! What a combat overlay IS, as data. Decision D11, stage one.
//!
//! # Eleven mockups, four widgets
//!
//! The owner's mockup sheet has eleven overlay windows on it. Six of them are the SAME TABLE with
//! different parameters, and seeing that is the whole reason this file exists:
//!
//! | mockup | metric | rate | columns |
//! |---|---|---|---|
//! | Default DPS (compact) | `dealt` | no | rank, name, bar, value, share |
//! | Detailed DPS | `dealt` | yes | rank, name, value, share |
//! | Group Healing | `healed` | yes | rank, name, value, share |
//! | Tank / Damage Taken | `taken` | yes | rank, name, value, share |
//! | Minimal (tiny) | `dealt` | no | name, bar, value |
//! | Vertical dock | `dealt` | no | rank, name, bar, value |
//!
//! [`Fighter`](crate::fights::Fighter) already carries `dealt`, `taken`, `healed`, `received`,
//! `swings`, `landed`, `avoided`, `kills` and `deaths`, so a [`Metric`] is a FIELD SELECTOR and the
//! difference between the owner's damage meter and his healing meter is one enum arm. Mockup 9, the
//! full combat HUD, is not a twelfth thing either: it is an overlay holding several widgets.
//!
//! So an overlay is AN ORDERED LIST OF WIDGETS PLUS A NAME, and that one sentence is this file.
//!
//! # Why the config is a type and not a pile of settings keys
//!
//! THE PREVIEW IS THE REASON. The owner's requirement for the builder was that the `+` button shows
//! a mockup while you build the overlay. That is free, and only free, if drawing is a pure function
//! of (config, fight): the builder draws the same call into a framed rectangle and what it shows IS
//! the overlay rather than a picture of one. A renderer that reached for window state, or read a
//! settings key by name, could not be drawn twice on one frame.
//!
//! # What is NOT here yet, and why the enum has one arm
//!
//! [`Widget`] carries only [`Widget::Ranked`]. The encounter header, the timers list and the loot
//! feed are the other three of the four, and they are deliberately absent rather than stubbed: an
//! arm that exists and draws nothing is a thing a builder would offer a person and then disappoint
//! them with. They arrive with their renderers.
use crate::fights::Fighter;
use serde::{Deserialize, Serialize};

/// WHICH NUMBER OFF A [`Fighter`] A TABLE RANKS BY.
///
/// A FIELD SELECTOR AND NOT A CALCULATION. Every arm here names a field the engine already
/// aggregated; nothing is derived, so a new metric cannot quietly invent a number. A rate is a
/// separate flag ([`Ranked::rate`]) rather than four more arms, because "per second" is a way of
/// PRINTING any of these and not a different measurement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Damage dealt. The owner's DPS meter.
    #[default]
    Dealt,
    /// Damage taken. The tank view.
    Taken,
    /// Healing put out. The healing view.
    Healed,
    /// Healing received.
    Received,
}

impl Metric {
    /// The value this metric reads off one fighter.
    pub fn of(self, f: &Fighter) -> u64 {
        match self {
            Metric::Dealt => f.dealt,
            Metric::Taken => f.taken,
            Metric::Healed => f.healed,
            Metric::Received => f.received,
        }
    }

    /// What the header calls it. Short, because it sits beside a number on a window that has to be
    /// read at a glance over a game.
    pub fn unit(self, rate: bool) -> &'static str {
        match (self, rate) {
            (Metric::Dealt, true) => "dps",
            (Metric::Dealt, false) => "dmg",
            (Metric::Taken, true) => "dtps",
            (Metric::Taken, false) => "taken",
            (Metric::Healed, true) => "hps",
            (Metric::Healed, false) => "healed",
            (Metric::Received, true) => "hps in",
            (Metric::Received, false) => "healed by",
        }
    }

    /// WHAT AN EMPTY TABLE CALLS THE THING NOBODY HAS. A NOUN PHRASE AND NOT A COLUMN LABEL.
    ///
    /// # `Metric::unit` WAS USED FOR THIS AND IT IS NOT A NOUN
    ///
    /// It is an abbreviation meant to sit beside a number: `dmg`, `taken`, `healed`, `healed
    /// by`. Dropped into "Nobody named has any {}." that gives, for three of the four metrics:
    ///
    /// ```text
    /// Nobody named has any dmg.
    /// Nobody named has any taken.
    /// Nobody named has any healed by.
    /// ```
    ///
    /// Only `healed` reads as English, and it reads as the wrong sense of it. A panel whose
    /// entire job is to say honestly that there is nothing to show should not be the one line
    /// on the page a reader has to parse twice.
    ///
    /// A SEPARATE METHOD RATHER THAN A CLEVERER FORMAT, because the two jobs genuinely differ:
    /// a column header wants the shortest thing that fits over a number, and a sentence wants a
    /// noun. One function cannot be good at both without a rule per metric anyway, and a rule
    /// per metric is what this is.
    pub fn nothing(self) -> &'static str {
        match self {
            Metric::Dealt => "dealt any damage",
            Metric::Taken => "taken any damage",
            Metric::Healed => "healed anybody",
            Metric::Received => "been healed",
        }
    }

    /// What the builder offers, in the order it offers them.
    ///
    /// A LIST AND NOT A DERIVE, so the ORDER on screen is a decision on the record. Dealt is first
    /// because a damage meter is what the owner opens this app for.
    pub const ALL: [Metric; 4] = [
        Metric::Dealt,
        Metric::Taken,
        Metric::Healed,
        Metric::Received,
    ];

    /// The name a person picks in the builder.
    pub fn label(self) -> &'static str {
        match self {
            Metric::Dealt => "Damage dealt",
            Metric::Taken => "Damage taken",
            Metric::Healed => "Healing done",
            Metric::Received => "Healing received",
        }
    }
}

/// WHICH COLUMNS A RANKED TABLE DRAWS.
///
/// FOUR BOOLS AND NOT A `Vec<Column>`, because the ORDER of these is not a choice: a rank sits left
/// of a name, a bar is the widest thing and takes what is left, and a share reads after the value
/// it is a share of. A reorderable list would offer a person a hundred arrangements of which two
/// are legible. What varies between the owner's six table mockups is which are PRESENT.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cols {
    /// The `1` `2` `3` position. Off by default: the row's position already states it, which is why
    /// the owner cut the rank chip from the first overlay.
    pub rank: bool,
    /// The figure itself.
    pub value: bool,
    /// The percentage of what the group did.
    pub share: bool,
    /// The proportional bar, which is what makes two rows comparable without reading.
    pub bar: bool,
    /// THE COLUMN HEADER ROW: `# NAME DPS %`, in the owner's own mockup's own words.
    ///
    /// # WITHOUT IT THE VALUE COLUMN HAS NO UNIT ANYWHERE ON SCREEN
    ///
    /// `Ranked::rate` turns the figure into a per-second one, and the only place in the whole
    /// renderer that ever painted the word `dps` beside a number was `dps::header`, which
    /// `headline: false` skips. Every table on the Live page, on all eight Dashboards tabs, on
    /// Analysis and on the Reports Session tab is built with `headline: false`, so all of them
    /// printed a RATE under a heading naming a TOTAL: `DAMAGE DEALT` over a column of dps.
    ///
    /// MEASURED, ON THE CAPTURE'S LAST FIGHT: `Losumyda` dealt 20 points over 36 seconds and the
    /// row read `Losumyda 0 100%`. A reader is shown somebody who dealt nothing and who dealt
    /// all of it, in one line, and neither figure is wrong: the first is a rate that rounds to
    /// zero and the second is a share of a total. Nothing on the page said they were different
    /// kinds of number.
    ///
    /// AND THE UNIT MOVES UNDER THE READER. `dps::dps` withholds a rate below `MIN_RATE_SECS`,
    /// so `row` falls back to the raw total for the first two seconds of a pull: the column
    /// silently reads `damage` and then `dps` with nothing marking the change.
    ///
    /// A HEADER AND NOT A SUFFIX ON EVERY ROW, because the owner's detailed mockup is a header
    /// (`#  Name  DPS  Damage  %`) and because a unit repeated down twelve rows is twelve
    /// copies of a fact that does not vary. OFF BY DEFAULT: the compact overlay in the mockup
    /// sheet has no header row and is read from across a room, where one line of chrome is a
    /// row of data not shown.
    pub head: bool,
}

impl Default for Cols {
    fn default() -> Self {
        Cols {
            rank: false,
            value: true,
            share: true,
            bar: true,
            head: false,
        }
    }
}

/// A RANKED TABLE OF PEOPLE, BIGGEST FIRST.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ranked {
    pub metric: Metric,
    /// Print per-second rather than a total. Held behind a floor at draw time: the log stamps to
    /// the second, so a fight younger than a few seconds has no rate the file can express.
    pub rate: bool,
    pub cols: Cols,
    /// How many rows at most. A group is five and a raid is a few dozen; a cap keeps an overlay
    /// from growing taller than the screen when a raid zone folds into one fight.
    pub cap: usize,
    /// Draw the reader's own figure large above the table.
    pub headline: bool,
    /// DOES THE TABLE STOP WHEN IT RUNS OUT OF ROOM? The same flag, for the same two surfaces,
    /// as [`Detail::fit`], whose doc gives the reasons.
    #[serde(default)]
    pub fit: bool,
    /// DRAW THE ROSTER'S TOTAL UNDER THE LAST ROW. The meter only: a table has its own head.
    ///
    /// ON BY DEFAULT because the owner picked the meter mockup that has it, and a stored overlay
    /// with no such key loads it on, so every meter he already has gets the line he asked for.
    pub foot: bool,
}

impl Default for Ranked {
    fn default() -> Self {
        Ranked {
            metric: Metric::Dealt,
            rate: true,
            cols: Cols::default(),
            cap: 12,
            /* AN OVERLAY IS SIZED BY THE HAND THAT DRAGGED IT. See `Detail::fit`. */
            fit: false,
            headline: true,
            foot: true,
        }
    }
}

/// WHOSE DETAIL A PANEL SHOWS.
///
/// AN ABILITY BREAKDOWN IS ABOUT ONE PERSON, which is what makes it different from a ranked table.
/// The mockup's is headed `Ability Breakdown (Reviir)`, the reader's own; a raid leader reading
/// somebody else's parse wants the top dealer instead. Two answers, both derivable, neither a name
/// this app had to guess.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subject {
    /// The log's owner. `Who::You`, which the aggregator folded his character name into.
    #[default]
    You,
    /// Whoever dealt the most in this fight.
    TopDealer,
}

impl Subject {
    pub fn label(self) -> &'static str {
        match self {
            Subject::You => "You",
            Subject::TopDealer => "Top dealer",
        }
    }

    pub const ALL: [Subject; 2] = [Subject::You, Subject::TopDealer];
}

/// A panel about ONE fighter: their abilities, their targets, their elements, their swings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Detail {
    pub who: Subject,
    pub cap: usize,
    /// DOES THE PANEL WRITE ITS OWN HEADING?
    ///
    /// TRUE FOR AN OVERLAY, FALSE ON THE DASHBOARD, and it is the same flag `Ranked::headline`
    /// is: a presentation choice the CONFIG makes, not the renderer. An overlay window has no
    /// chrome of its own, so the panel must say what it is; a dashboard card already has a head
    /// with the name in it, and a panel that headed itself there would be one panel under two
    /// headings, which reads as a rendering fault.
    pub head: bool,
    /// DOES THE PANEL STOP WHEN IT RUNS OUT OF ROOM?
    ///
    /// # THE TWO SURFACES WANT OPPOSITE THINGS AND BOTH ARE RIGHT
    ///
    /// AN OVERLAY CLIPS, DELIBERATELY. `screens::dps` says so in as many words: a meter over a
    /// game has no scrollbar, and if a fold makes more rows than a person wants, he drags the
    /// window shorter and the rest are cut. The window is his ruler.
    ///
    /// A DASHBOARD CARD MAY NOT. Its height is set by the grid and not by the reader's hand on
    /// this card, so a row cut in half by a card's bottom edge is not a choice anybody made. It
    /// is what the owner was looking at when he said the bottom of all the cards was cut off.
    /// Every other body on that page already obeys one rule (`room_for` and a `+N more` line:
    /// draw what fits, say what you left out) and these four panels never got it, because they
    /// were written for the overlay and borrowed by the page afterwards.
    ///
    /// SET BY THE SURFACE AND NOT BY THE RENDERER, exactly as [`Detail::head`] is.
    pub fit: bool,
}

impl Default for Detail {
    fn default() -> Self {
        Detail {
            who: Subject::You,
            cap: 8,
            head: true,
            /* AN OVERLAY IS SIZED BY THE HAND THAT DRAGGED IT. See the field. */
            fit: false,
        }
    }
}

/// The damage-over-time chart.
/* NO `Eq`: `height` is an `f32`. `PartialEq` is what the config comparisons need. */
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Timeline {
    /// How many fighters get a line. More than about five is unreadable at overlay width.
    pub cap: usize,
    /// Draw the event marks (deaths, crits, abilities) as ticks along the axis.
    pub marks: bool,
    /// Height in points. A chart has no natural height the way a row of text does.
    pub height: f32,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline {
            cap: 4,
            marks: true,
            height: 120.0,
        }
    }
}

/// THE READER'S OWN FIGHT, AT A GLANCE: the personal coach the owner picked.
///
/// His rate large, a line of his last `window` seconds under it, how often his swings land, his
/// melee crits and parries, his biggest abilities, and his rate against his own usual on the same
/// mob. Every figure is one the log states about him; the usual is his own stored fights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Coach {
    /// How many seconds of the fight the line under the figure covers.
    pub window: u32,
    /// How many abilities are listed. Zero lists none.
    pub abilities: usize,
    /// Compare the rate with the reader's stored fights against the same mob.
    pub usual: bool,
}

impl Default for Coach {
    fn default() -> Self {
        Coach {
            window: 30,
            abilities: 3,
            usual: true,
        }
    }
}

/// ONE LINE: THE MINIMAL PILL the owner picked. The reader's rate, and whichever of the fight clock,
/// his rank and the thing he is fighting he wants beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pill {
    pub clock: bool,
    pub rank: bool,
    pub target: bool,
}

impl Default for Pill {
    fn default() -> Self {
        Pill {
            clock: true,
            rank: true,
            target: true,
        }
    }
}

/// One thing an overlay or the Analysis page can draw.
///
/// SIX ARMS, AND EVERY ONE OF THEM READS A FIELD THE ENGINE MEASURED. The three that are still
/// absent are absent because the log cannot support them: there is no threat arm, no encounter
/// health arm and no pet arm, and the goal doc carries the evidence for each.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Widget {
    /// Everyone, ranked by one metric.
    Ranked(Ranked),
    /// EVERYONE, RANKED, IN THE DAMAGE METER IDIOM: THE ROW IS THE BAR.
    ///
    /// # WHY THIS IS A SECOND ARM AND NOT A FLAG ON `Ranked`
    ///
    /// The owner asked for the overlay to look like Details!, Recount or Skada. Those three
    /// share one shape, and it is not the shape of a table: a tight stack of rows where each
    /// row IS its own bar, filled to that person's share, the name written ON the fill and the
    /// figure on the right of it. No column head, no rules, no separate bar column.
    ///
    /// `Ranked` IS A TABLE: rank chip, name, a bar in a column of its own, a figure, a share,
    /// under a head. Nothing in a set of column flags can express `the bar is the background`,
    /// so these are two renderers rather than one renderer with a switch. The CONFIG they take
    /// is the same (which metric, rate or total, how many rows), which is why this carries a
    /// `Ranked` rather than a struct of its own: a person who has set up one has set up both.
    ///
    /// `Ranked::cols` IS IGNORED HERE, and it has to be: every one of those five flags names a
    /// column this shape does not have.
    Meter(Ranked),
    /// One fighter's damage by what they used.
    Abilities(Detail),
    /// One fighter's damage by what they hit.
    Targets(Detail),
    /// One fighter's spell damage by element.
    Elements(Detail),
    /// One fighter's swings, by how they were stopped.
    Outcomes(Detail),
    /// Damage over the fight, a line per fighter.
    Timeline(Timeline),
    /// The reader's own fight at a glance. See [`Coach`].
    Coach(Coach),
    /// The reader's rate on one line. See [`Pill`].
    Pill(Pill),
}

impl Widget {
    /// What the builder calls this in a list.
    pub fn label(&self) -> String {
        match self {
            Widget::Ranked(r) => format!("{} ({})", r.metric.label(), r.metric.unit(r.rate)),
            Widget::Meter(r) => {
                format!("{} meter ({})", r.metric.label(), r.metric.unit(r.rate))
            }
            Widget::Abilities(d) => format!("Abilities ({})", d.who.label()),
            Widget::Targets(d) => format!("Targets ({})", d.who.label()),
            Widget::Elements(d) => format!("Elements ({})", d.who.label()),
            Widget::Outcomes(d) => format!("Hit results ({})", d.who.label()),
            Widget::Timeline(_) => "Damage over time".to_owned(),
            Widget::Coach(_) => "Your fight (coach)".to_owned(),
            Widget::Pill(_) => "One line (pill)".to_owned(),
        }
    }

    /// EVERY WIDGET THE BUILDER CAN ADD, in the order it offers them.
    ///
    /// A LIST AND NOT A DERIVE, so a new arm has to be offered deliberately. An arm that exists and
    /// is not here is a panel nobody can ever add, which is this tree's signature defect.
    pub fn every() -> Vec<Widget> {
        vec![
            Widget::Meter(Ranked::default()),
            Widget::Coach(Coach::default()),
            Widget::Pill(Pill::default()),
            Widget::Ranked(Ranked::default()),
            Widget::Timeline(Timeline::default()),
            Widget::Abilities(Detail::default()),
            Widget::Targets(Detail::default()),
            Widget::Outcomes(Detail::default()),
            Widget::Elements(Detail::default()),
        ]
    }
}

/// ONE OVERLAY WINDOW, AS THE OWNER CONFIGURED IT.
/* NO `Eq`, BECAUSE THIS CARRIES TWO `f32` SIZES. `PartialEq` is what the tests compare with and
 * is all this needs; `Eq` would be a promise about float equality that nothing here wants to make. */
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overlay {
    /// THE NAME IN THE SETTINGS FILE, AND IT MUST NEVER CHANGE ONCE WRITTEN.
    ///
    /// SEPARATE FROM `name` BECAUSE A NAME IS THE OWNER'S AND HE MAY RETYPE IT. Keying the window's
    /// remembered size, its pin and its open flag off a string he can edit would reset all three
    /// the first time he fixed a typo. `id` is minted once and never shown.
    pub id: String,
    /// What he calls it. Shown in the Parser list, the taskbar and alt-tab.
    pub name: String,
    /// Whether the window is on screen. Persisted, so the overlays he uses come back with the app.
    pub open: bool,
    /// Keep it above the game. On by default for an overlay: one that the game covers on the frame
    /// it opens has done nothing.
    pub pinned: bool,
    /// Show the pin and close chips while the pointer is inside. Off by default; the drag chord and
    /// the hotkey reach everything without them.
    pub chips: bool,
    /// Remembered window size in points. Width is the owner's; height follows the content.
    pub w: f32,
    pub h: f32,
    /// WHAT THIS OVERLAY SHOWS, OR `None` FOR A PERSON WHO HAS NEVER SAID.
    ///
    /// # WHY THIS IS AN `Option` AND NOT A `Vec`, WHICH IS A DEFECT AND NOT A PREFERENCE
    ///
    /// `None` MEANS NOBODY CHOSE, so this overlay shows whatever the build ships today.
    /// `Some(v)` means a person opened the builder and settled on `v`, INCLUDING `Some(vec![])`
    /// for one he deliberately emptied. A `Vec` cannot tell those apart, and the cost of that
    /// was measured on the owner's own machine on 2026-09-08.
    ///
    /// WHAT HAPPENED. `main.rs` collects window edits (a drag, a resize, a pin, an OS close)
    /// and writes them to settings. To find the overlay an edit belongs to it called
    /// `or_default`, which MATERIALISES the shipped overlay when the file has none, and then
    /// wrote the whole resolved list back. So the first time anybody moved an overlay window,
    /// the shipped widget list was frozen into his settings file as though he had chosen it.
    /// From that moment `or_default` never fired again for him and NO future change to the
    /// shipped default could ever reach his screen. The owner asked for the overlay to become
    /// a damage meter; the code shipped, every test was green, and his window kept drawing the
    /// old table off a line in his settings file that he never typed.
    ///
    /// THIS TREE'S SIGNATURE DEFECT IS CODE THAT IS CORRECT AND UNREACHABLE, and a default
    /// silently promoted to a saved choice is one of its shapes. See [`Overlay::panels`] for
    /// the read side and [`forget_unchosen`] for what is done about the files already written.
    ///
    /// KEPT UNDER ITS OLD NAME so an existing `"widgets": [..]` still loads: a list somebody
    /// really did build has to survive, and only [`forget_unchosen`] may drop one.
    #[serde(default)]
    pub widgets: Option<Vec<Widget>>,
    /// WHERE THE OWNER LEFT THIS WINDOW: the top left of its outer rect, in points.
    ///
    /// `None` until the window has been on screen and stayed put; the registry then places it
    /// beside the main window. A drag moves it, and the next launch opens it where it was left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f32; 2]>,
}

impl Default for Overlay {
    fn default() -> Self {
        Overlay {
            id: String::new(),
            name: "Overlay".to_owned(),
            open: false,
            pinned: true,
            chips: false,
            w: 460.0,
            h: 200.0,
            /* THE SHIPPED OVERLAY IS A METER, which is what an overlay over a running game IS:
             * a tight stack of bars read at a glance from across a room. The table shape is
             * still in the vocabulary for a person who wants columns. */
            /* NOBODY HAS CHOSEN, so this resolves to whatever ships. See the field's doc. */
            widgets: None,
            at: None,
        }
    }
}

impl Overlay {
    /// THE ONE THE APP SHIPS WITH, so a fresh install has a damage meter rather than an empty list
    /// and a `+` button.
    ///
    /// ITS ID IS FIXED AND IS NOT MINTED. `dps` is the id the D4 hotkey row and the old
    /// `Slot::Dps` settings key already used, so an owner who has already set a pin or a chips
    /// preference keeps it across this change instead of silently getting the defaults back.
    pub fn default_dps() -> Overlay {
        Overlay {
            id: "dps".to_owned(),
            name: "DPS".to_owned(),
            ..Overlay::default()
        }
    }

    /// WHAT AN OVERLAY SHOWS WHEN NOBODY HAS CHOSEN: THE SHIPPED PANELS.
    ///
    /// A METER, because that is what an overlay over a running game is: a tight stack of bars
    /// read at a glance from across a room. See `screens::dps::meter`.
    pub fn shipped_panels() -> Vec<Widget> {
        vec![Widget::Meter(Ranked::default())]
    }

    /// EVERY PANEL LIST THIS APP HAS EVER SHIPPED AS ITS DEFAULT, oldest first.
    ///
    /// # WHY A HISTORY AND NOT JUST TODAY'S
    ///
    /// A stored list that is EXACTLY one of these was not a choice: it is a default that an
    /// older build wrote into somebody's settings file on his behalf. [`forget_unchosen`] uses
    /// that to hand those files back to whatever ships now.
    ///
    /// AN ENTRY IS ADDED HERE AND NEVER REMOVED, on the day the shipped default changes. The
    /// list is short and it is the price of never repeating 2026-09-08: see the doc on
    /// [`Overlay::widgets`].
    fn shipped_panels_ever() -> Vec<Vec<Widget>> {
        vec![
            /* Up to 2026-09-08: the ranked TABLE, before the owner asked for a meter. */
            vec![Widget::Ranked(Ranked::default())],
            /* 2026-09-08 onwards. */
            Overlay::shipped_panels(),
        ]
    }

    /// WHAT THIS OVERLAY DRAWS: what was chosen, or what ships if nothing was.
    ///
    /// THE ONE READ PATH, so the overlay window, the builder's preview and the dashboard's
    /// read-only card cannot disagree about what an overlay contains. See [`Overlay::widgets`].
    pub fn panels(&self) -> Vec<Widget> {
        self.widgets.clone().unwrap_or_else(Overlay::shipped_panels)
    }

    /// A NEW OVERLAY WITH AN ID NOTHING ELSE IS USING.
    ///
    /// COUNTED, NOT RANDOM AND NOT A CLOCK. This crate has no rng and a timestamp would make two
    /// machines disagree about a file they might one day share; a small integer that skips what is
    /// taken is enough for a list a person maintains by hand, and it makes the settings file
    /// readable. `taken` is every id already in use.
    pub fn fresh(taken: &[String], name: &str) -> Overlay {
        let mut n = 1;
        let id = loop {
            let c = format!("ov{n}");
            if !taken.contains(&c) {
                break c;
            }
            n += 1;
        };
        Overlay {
            id,
            name: name.to_owned(),
            open: true,
            ..Overlay::default()
        }
    }
}

impl Widget {
    /// A SHORT NAME FOR THIS KIND OF WIDGET THAT NEVER CHANGES, for the id of its pop-out window.
    ///
    /// NOT [`Widget::label`], which carries the config (`Damage meter (dps)`) and would give a meter
    /// switched to healing a second window instead of the one it already has.
    pub fn slug(&self) -> &'static str {
        match self {
            Widget::Ranked(_) => "ranked",
            Widget::Meter(_) => "meter",
            Widget::Abilities(_) => "abilities",
            Widget::Targets(_) => "targets",
            Widget::Elements(_) => "elements",
            Widget::Outcomes(_) => "outcomes",
            Widget::Timeline(_) => "timeline",
            Widget::Coach(_) => "coach",
            Widget::Pill(_) => "pill",
        }
    }

    /// THE NAME THE OWNER AND HIS AGENT CALL THIS WIDGET BY, shown on its window when the pointer is
    /// in it, so a window on screen can be named out loud and both mean the same one.
    ///
    /// SHORT AND DIFFERENT FROM EVERY DASHBOARD CARD'S NAME. `Graph` and not `Timeline`, because the
    /// dashboard already has a Timeline card and one word for two things is the confusion this list
    /// exists to prevent.
    pub fn codename(&self) -> &'static str {
        match self {
            Widget::Meter(_) => "Meter",
            Widget::Coach(_) => "Coach",
            Widget::Pill(_) => "Pill",
            Widget::Ranked(_) => "Table",
            Widget::Timeline(_) => "Graph",
            Widget::Abilities(_) => "Abilities",
            Widget::Targets(_) => "Targets",
            Widget::Outcomes(_) => "Hits",
            Widget::Elements(_) => "Elements",
        }
    }

    /// THE SIZE A WINDOW OF JUST THIS WIDGET OPENS AT, width then height, in points. A starting
    /// size and no more: the window is dragged to whatever the owner wants and keeps it.
    pub fn pop_size(&self) -> (f32, f32) {
        match self {
            Widget::Pill(_) => (420.0, 48.0),
            Widget::Coach(_) => (340.0, 250.0),
            Widget::Meter(_) => (360.0, 220.0),
            Widget::Ranked(_) | Widget::Timeline(_) => (460.0, 220.0),
            Widget::Abilities(_)
            | Widget::Targets(_)
            | Widget::Elements(_)
            | Widget::Outcomes(_) => (320.0, 220.0),
        }
    }
}

/// WINDOWS POPPED OUT BEFORE THE NAMES EXISTED, CALLED BY THEIR NAMES.
///
/// The owner's first three popped windows were saved as "One line (pill)", "Your fight (coach)" and
/// "Damage dealt meter (dps)", and he could not tell what anything was called. A popped window whose
/// name is still exactly its widget's long label takes the widget's codename. A name he typed is
/// his and is not touched, and nor is any overlay that is not a pop-out.
pub fn rename_popped(list: &mut [Overlay]) {
    for w in Widget::every() {
        let id = pop_id(&w);
        for o in list
            .iter_mut()
            .filter(|o| o.id == id && o.name == w.label())
        {
            o.name = w.codename().to_owned();
        }
    }
}

/// THE ID OF THE WINDOW THAT SHOWS JUST ONE KIND OF WIDGET.
pub fn pop_id(w: &Widget) -> String {
    format!("pop-{}", w.slug())
}

/// OPEN THE WINDOW THAT SHOWS JUST THIS KIND OF WIDGET, OR CLOSE IT IF IT IS OPEN. Returns whether it
/// is open afterwards.
///
/// # WHAT THIS IS FOR
///
/// The owner's call: every widget KNOWN working in a real always-on-top window, before its look is
/// worked on. A builder session per widget is a long way round to that; one button per widget on
/// the Overlays page is not.
///
/// # ONE WINDOW PER KIND, AND A SECOND PRESS IS THE SAME WINDOW
///
/// The id is the kind's [`Widget::slug`], so pressing the button again closes that window and a
/// third press opens it where it was, at the size it was dragged to, with whatever was set on it in
/// the builder. It is an ordinary overlay in every other way: it is listed, editable and removable.
pub fn pop(list: &mut Vec<Overlay>, w: &Widget) -> bool {
    let id = pop_id(w);
    if let Some(o) = list.iter_mut().find(|o| o.id == id) {
        /* A WINDOW POPPED BEFORE THE NAMES EXISTED STILL CARRIES THE LONG ONE. Renamed only when it
         * is exactly that, so a name the owner typed is his. */
        if o.name == w.label() {
            o.name = w.codename().to_owned();
        }
        o.open = !o.open;
        return o.open;
    }
    let (width, height) = w.pop_size();
    list.push(Overlay {
        id,
        /* BY THE NAME THE OWNER AND HIS AGENT CALL IT. See [`Widget::codename`]. */
        name: w.codename().to_owned(),
        open: true,
        pinned: true,
        chips: false,
        w: width,
        h: height,
        widgets: Some(vec![w.clone()]),
        at: None,
    });
    true
}

/// EVERY OVERLAY, WITH THE SHIPPED ONE PUT BACK IF THE FILE HAS NONE.
///
/// A FRESH INSTALL AND A DELIBERATELY EMPTIED LIST LOOK THE SAME IN JSON, and this treats them the
/// same on purpose: an owner who deletes every overlay gets the DPS one back rather than a Parser
/// page with nothing on it and no way to tell whether the feature is broken. Deleting the last one
/// is not a state worth honouring.
/// HAND BACK EVERY PANEL LIST THAT WAS NEVER ACTUALLY CHOSEN.
///
/// # WHAT THIS UNDOES
///
/// Builds before 2026-09-08 wrote the shipped panel list into a person's settings file as a
/// side effect of him DRAGGING AN OVERLAY WINDOW: see the defect written out on
/// [`Overlay::widgets`]. Those records are indistinguishable from a choice by their shape and
/// distinguishable by their CONTENT: they are byte for byte a list this app shipped.
///
/// # THE RULE, AND WHAT IT COSTS
///
/// A stored list EQUAL to one of [`Overlay::shipped_panels_ever`] is forgotten; anything else
/// is left exactly alone. What that costs is one case: a person who opened the builder and
/// deliberately assembled a list identical to a shipped default gets today's default instead.
/// He is indistinguishable from the many who never opened the builder at all, and handing him
/// the current default is the answer that is right for everybody else.
///
/// A LIST HE REALLY BUILT IS NEVER TOUCHED, which is why this compares the whole list rather
/// than, say, forgetting anything holding a `Ranked`. An overlay he emptied on purpose is
/// `Some(vec![])`, which matches nothing here and stays empty.
///
/// RUN ONCE, AT LOAD. `Settings::load` calls it, so nothing downstream ever sees a fossil.
pub fn forget_unchosen(list: &mut [Overlay]) {
    let shipped = Overlay::shipped_panels_ever();
    for o in list.iter_mut() {
        if o.widgets.as_ref().is_some_and(|w| shipped.contains(w)) {
            o.widgets = None;
        }
    }
}

/// FOLD WINDOW EDITS INTO THE STORED LIST, WITHOUT INVENTING A CHOICE.
///
/// # WHAT AN EDIT IS
///
/// A drag, a resize, a pin, a chip toggle or an OS close. `windows::Windows::overlay_edits`
/// hands over the whole [`Overlay`] whose window changed, matched here by `id` because the list
/// may have been reordered or shortened by the Parser page on the same frame.
///
/// # WHY THIS IS A FUNCTION AND NOT SIX LINES IN `main.rs`
///
/// It used to be six lines in `main.rs`, and those six lines held the defect written out on
/// [`Overlay::widgets`]: they resolved the list through [`or_default`] before merging, so the
/// shipped panel list was written to disk as a side effect of moving a window. Nothing could
/// reach that code to test it, because it lived inside the app's frame loop between a viewport
/// callback and a registry lock. Code no test can drive is where this tree's defects live.
///
/// THE STORED LIST IS THE BASE. An edit for an id the file has never heard of is APPENDED
/// rather than dropped, because the person really did move that window and it must come back
/// where he put it; it arrives carrying `widgets: None` and it is stored that way.
pub fn apply_edits(stored: &[Overlay], edits: Vec<Overlay>) -> Vec<Overlay> {
    let mut list = stored.to_vec();
    for e in edits {
        match list.iter_mut().find(|o| o.id == e.id) {
            Some(slot) => *slot = e,
            None => list.push(e),
        }
    }
    list
}

pub fn or_default(list: &[Overlay]) -> Vec<Overlay> {
    if list.is_empty() {
        vec![Overlay::default_dps()]
    } else {
        list.to_vec()
    }
}

#[cfg(test)]
mod tests {
    /// EVERY WIDGET HAS A BUTTON'S WORTH OF WINDOW, AND THE BUTTON TOGGLES IT.
    ///
    /// WHAT MUTATION MAKES THIS RED: two kinds sharing a slug; `pop` making a second window on a
    /// second press; a popped window that is not on top or shows something besides its widget.
    #[test]
    fn a_pop_button_opens_a_window_of_just_that_widget_and_the_next_press_closes_it() {
        let mut list = vec![Overlay::default_dps()];
        for w in Widget::every() {
            assert!(
                pop(&mut list, &w),
                "pressing {} did not open a window",
                w.label()
            );
            let o = list
                .iter()
                .find(|o| o.id == pop_id(&w))
                .unwrap_or_else(|| panic!("no window for {}", w.label()));
            assert!(
                o.open && o.pinned,
                "{} popped out closed or not on top",
                w.label()
            );
            assert_eq!(
                o.name,
                w.codename(),
                "the {} window is not called by its name",
                w.label()
            );
            assert_eq!(
                o.widgets.as_deref(),
                Some(std::slice::from_ref(&w)),
                "the {} window shows something besides that widget",
                w.label()
            );
        }
        assert_eq!(
            list.len(),
            Widget::every().len() + 1,
            "two kinds of widget share a window, so one button opens the other's"
        );

        let pill = Widget::Pill(Pill::default());
        assert!(
            !pop(&mut list, &pill),
            "a second press did not close the pill"
        );
        assert!(pop(&mut list, &pill), "a third press did not open it again");
        assert_eq!(
            list.len(),
            Widget::every().len() + 1,
            "pressing a button again made a second window"
        );
    }

    use super::*;
    /// A POPPED WINDOW STILL CARRYING ITS LONG LABEL IS CALLED BY ITS NAME; NOTHING ELSE IS RENAMED.
    ///
    /// WHAT MUTATION MAKES THIS RED: renaming a name the owner typed, renaming an overlay that is not
    /// a pop-out, or renaming nothing.
    #[test]
    fn an_old_popped_window_takes_its_name_and_a_typed_one_is_kept() {
        let pill = Widget::Pill(Pill::default());
        let coach = Widget::Coach(Coach::default());
        let meter = Widget::Meter(Ranked::default());
        let mut list = vec![
            Overlay {
                id: pop_id(&pill),
                name: pill.label(),
                ..Overlay::default()
            },
            Overlay {
                id: pop_id(&coach),
                name: "my own coach".to_owned(),
                ..Overlay::default()
            },
            Overlay {
                id: "ov1".to_owned(),
                name: meter.label(),
                ..Overlay::default()
            },
        ];
        rename_popped(&mut list);
        assert_eq!(list[0].name, "Pill", "a popped window kept its long label");
        assert_eq!(
            list[1].name, "my own coach",
            "a name the owner typed was replaced"
        );
        assert_eq!(
            list[2].name,
            meter.label(),
            "an overlay that is not a pop-out was renamed"
        );

        let settings = include_str!("settings.rs");
        let settings = &settings[..settings.find("mod tests {").expect("the test module")];
        assert!(
            settings.contains("crate::overlay::rename_popped(&mut s.overlays);"),
            "settings load without renaming the owner's old popped windows"
        );
    }
    /// EVERY WIDGET HAS ITS OWN NAME, AND NONE IS A DASHBOARD CARD'S.
    ///
    /// WHAT MUTATION MAKES THIS RED: two widgets sharing a codename, or one named like a card.
    #[test]
    fn every_widget_has_a_name_of_its_own_to_be_called_by() {
        let names: Vec<&str> = Widget::every().iter().map(Widget::codename).collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            names.len(),
            "two widgets answer to one name: {names:?}"
        );
        for card in crate::screens::dashboards::Tile::ALL {
            assert!(
                !names.contains(&card.label()),
                "a widget is called {:?}, which is also a dashboard card",
                card.label()
            );
        }
    }
    /// A POSITION IS KEPT, AND A FILE FROM BEFORE POSITIONS LOADS WITH NONE.
    ///
    /// WHAT MUTATION MAKES THIS RED: `at` losing its `serde(default)`, or not being written out.
    #[test]
    fn an_overlay_remembers_where_it_was_left_and_an_old_file_has_no_position() {
        let old =
            r#"{"id":"dps","name":"DPS","open":true,"pinned":true,"chips":false,"w":460,"h":200}"#;
        let o: Overlay = serde_json::from_str(old).expect("an overlay from before positions loads");
        assert_eq!(
            o.at, None,
            "an old overlay loaded with a position nobody gave it"
        );
        let moved = Overlay {
            at: Some([1234.0, 56.5]),
            ..o
        };
        let back: Overlay = serde_json::from_str(&serde_json::to_string(&moved).expect("writes"))
            .expect("reads back");
        assert_eq!(
            back.at,
            Some([1234.0, 56.5]),
            "where the owner left the window was not kept"
        );
    }
    use crate::fights::Who;

    fn fighter(dealt: u64, taken: u64, healed: u64, received: u64) -> Fighter {
        Fighter {
            who: Who::You,
            dealt,
            taken,
            healed,
            received,
            swings: 0,
            landed: 0,
            avoided: 0,
            kills: 0,
            deaths: 0,
            ..Default::default()
        }
    }

    /// DEFECT: MOVING AN OVERLAY WINDOW WROTE DOWN WHAT IT SHOWS.
    ///
    /// # THE DEFECT, MEASURED
    ///
    /// The owner asked for the overlay to become a damage meter. It was built, every test was
    /// green, the release was running, and his overlay kept drawing the old ranked table. His
    /// `settings.json` held a `widgets` list byte for byte identical to the default the build
    /// before it shipped, and he had never opened the builder in his life.
    ///
    /// `main.rs` folded window edits into settings by resolving the list through [`or_default`]
    /// FIRST, which materialises the shipped overlay when the file has none, and then writing
    /// the whole thing back. So the first drag of an overlay window froze the shipped panel list
    /// into his file as though he had picked it, and every later change to the shipped default
    /// was dead on arrival for him for ever.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// A window edit against a file that has never stored an overlay stores the GEOMETRY and
    /// nothing else: `widgets` stays `None`, so the overlay still follows whatever ships. And a
    /// list somebody really did build survives an edit untouched, which is the other half: a fix
    /// that forgot every stored list would be a worse defect than the one it replaced.
    ///
    /// # WHICH CHANGE IS LOAD BEARING, MEASURED AND NOT ASSUMED
    ///
    /// Two things were changed to kill this defect and only one of them is what fixes it. Making
    /// [`apply_edits`] fold onto the STORED list instead of the resolved one was planted back as
    /// a mutation and this test stayed GREEN: once [`Overlay::widgets`] is an `Option`,
    /// [`or_default`] no longer has a concrete panel list to fabricate, so it cannot freeze one.
    /// The `Option` is the fix. That is recorded here because a comment claiming the wrong cause
    /// is worse than none: the next person would defend the wrong line.
    ///
    /// [`apply_edits`] STILL FOLDS ONTO THE STORED LIST, for the reason the last block below
    /// pins: resolving first drags the whole shipped overlay into a file that never held it, and
    /// every field on it, not just its panels, is frozen from that moment.
    ///
    /// WHAT MUTATION MAKES THIS RED: giving [`Overlay::default`] a concrete `Some(..)` panel list
    /// again (measured: red on the first assertion), clobbering a stored list with the incoming
    /// one, or resolving through [`or_default`] inside [`apply_edits`] (red on the last block).
    #[test]
    fn moving_a_window_stores_where_it_is_and_not_what_it_shows() {
        /* A WINDOW THE PERSON DRAGGED AND RESIZED. This is what the registry hands back: the
         * overlay it was seeded with, geometry updated, contents untouched at `None`. */
        let dragged = Overlay {
            w: 512.0,
            h: 260.0,
            open: true,
            ..Overlay::default_dps()
        };
        assert_eq!(
            dragged.widgets, None,
            "the shipped overlay must reach the registry with nothing chosen"
        );

        /* HIS FILE HAS NEVER STORED AN OVERLAY, which is every install until the first drag. */
        let after = apply_edits(&[], vec![dragged]);
        assert_eq!(after.len(), 1, "the moved window was not remembered at all");
        assert_eq!(after[0].w, 512.0, "the size he dragged to was not stored");
        assert_eq!(
            after[0].widgets, None,
            "dragging a window wrote down what the overlay shows, so this install can never \
             receive another shipped default again"
        );
        assert_eq!(
            after[0].panels(),
            Overlay::shipped_panels(),
            "the stored overlay stopped following the shipped panels"
        );

        /* AND A LIST HE REALLY BUILT SURVIVES ONE. */
        let mine = vec![Widget::Timeline(Timeline::default())];
        let stored = Overlay {
            widgets: Some(mine.clone()),
            ..Overlay::default_dps()
        };
        let moved = Overlay {
            w: 600.0,
            ..stored.clone()
        };
        let after = apply_edits(&[stored], vec![moved]);
        assert_eq!(
            after.len(),
            1,
            "the edit was appended instead of matched by id"
        );
        assert_eq!(after[0].w, 600.0, "the drag was lost");
        assert_eq!(
            after[0].widgets,
            Some(mine),
            "an overlay he built himself was thrown away by a window drag"
        );

        /* AND AN EDIT MUST NOT DRAG THE SHIPPED OVERLAY INTO THE FILE WITH IT.
         *
         * This is what folding onto the STORED list buys, and it is not about panels. Resolving
         * through `or_default` first means an edit to a SECOND overlay also writes the shipped
         * one out, and from then on every field of it is frozen: a later build that shipped a
         * different default width, name or pin would never reach this install either. The panel
         * list was simply the first field anybody noticed. */
        let second = Overlay {
            id: "ov2".to_owned(),
            w: 300.0,
            ..Overlay::default()
        };
        let after = apply_edits(&[], vec![second]);
        assert_eq!(
            after.len(),
            1,
            "an edit to one overlay wrote {} of them to the file: {after:?}",
            after.len()
        );
        assert_eq!(after[0].id, "ov2", "the wrong overlay was stored");
    }

    /// A PANEL LIST NOBODY CHOSE IS HANDED BACK AT LOAD, AND ONE SOMEBODY BUILT IS NOT.
    ///
    /// # WHY THIS EXISTS AT ALL
    ///
    /// Stopping the write (above) fixes every file written from now on. It does nothing for the
    /// files already on disk, and EVERY install that has ever moved an overlay window has one:
    /// the meter would have been correct, tested, and invisible to every existing user. That is
    /// this tree's signature defect wearing a settings file as a disguise.
    ///
    /// # THE RULE THIS PINS
    ///
    /// Forgotten only if the stored list EQUALS one this app has shipped as its default. The
    /// three cases that must survive are all here: a list somebody assembled, an overlay he
    /// deliberately EMPTIED (`Some(vec![])`, which is not the same as never choosing), and one
    /// already at `None`.
    ///
    /// WHAT MUTATION MAKES THIS RED: forgetting on a looser test than whole-list equality (say,
    /// anything containing a `Ranked`), dropping the history so only today's default is
    /// recognised, or treating a deliberately emptied list as unchosen.
    #[test]
    fn a_default_nobody_chose_is_forgotten_and_a_real_choice_is_kept() {
        let fossil = |w: Vec<Widget>| Overlay {
            widgets: Some(w),
            ..Overlay::default_dps()
        };

        /* EVERY DEFAULT THIS APP HAS EVER SHIPPED, including the one on disk in the owner's own
         * file on 2026-09-08. Walked from the history itself so a future default cannot be added
         * there and left unhandled here. */
        for shipped in Overlay::shipped_panels_ever() {
            let mut list = [fossil(shipped.clone())];
            forget_unchosen(&mut list);
            assert_eq!(
                list[0].widgets, None,
                "a stored copy of the shipped list {shipped:?} was treated as a choice, so this \
                 install can never receive another shipped default"
            );
        }

        /* A LIST SOMEBODY BUILT. It HOLDS the old default's widget and is not equal to it, which
         * is exactly the case a looser rule would destroy. */
        let mut mine = vec![
            Widget::Ranked(Ranked::default()),
            Widget::Timeline(Timeline::default()),
        ];
        let mut list = [fossil(mine.clone())];
        forget_unchosen(&mut list);
        assert_eq!(
            list[0].widgets,
            Some(std::mem::take(&mut mine)),
            "an overlay somebody built was thrown away because it contained a shipped widget"
        );

        /* DELIBERATELY EMPTIED IS A CHOICE. `or_default` puts the whole OVERLAY back when the
         * list of overlays is empty; an overlay with no panels in it is a different thing and it
         * is his. */
        let mut list = [fossil(Vec::new())];
        forget_unchosen(&mut list);
        assert_eq!(
            list[0].widgets,
            Some(Vec::new()),
            "an overlay emptied on purpose was read as one nobody had configured"
        );

        /* AND ALREADY UNCHOSEN STAYS UNCHOSEN. */
        let mut list = [Overlay::default_dps()];
        forget_unchosen(&mut list);
        assert_eq!(list[0].widgets, None);
    }

    /// DEFECT: A WIDGET ARM NOBODY CAN ADD.
    ///
    /// This tree's signature failure is code that compiles, passes its own tests, and is reachable
    /// from nothing. A `Widget` arm that the builder does not offer is exactly that: a panel that
    /// exists, renders, and can never appear on anybody's screen.
    ///
    /// THE MATCH IN `label` IS EXHAUSTIVE AND THE COMPILER GUARDS IT. `every()` is a hand-written
    /// list and nothing guards that but this.
    ///
    /// WHAT MUTATION MAKES THIS RED: adding an arm to `Widget` without adding it to `every()`.
    #[test]
    fn the_builder_offers_every_widget_that_exists() {
        let offered = Widget::every();

        /* One of each arm, built here so the compiler's exhaustiveness check is what enumerates
         * them: a new arm makes THIS match fail to compile, which is the loudest possible way to
         * be told to come and update the list. */
        let all = [
            Widget::Meter(Ranked::default()),
            Widget::Ranked(Ranked::default()),
            Widget::Abilities(Detail::default()),
            Widget::Targets(Detail::default()),
            Widget::Elements(Detail::default()),
            Widget::Outcomes(Detail::default()),
            Widget::Timeline(Timeline::default()),
            Widget::Coach(Coach::default()),
            Widget::Pill(Pill::default()),
        ];
        for w in &all {
            let name = match w {
                Widget::Meter(_) => "Meter",
                Widget::Ranked(_) => "Ranked",
                Widget::Abilities(_) => "Abilities",
                Widget::Targets(_) => "Targets",
                Widget::Elements(_) => "Elements",
                Widget::Outcomes(_) => "Outcomes",
                Widget::Timeline(_) => "Timeline",
                Widget::Coach(_) => "Coach",
                Widget::Pill(_) => "Pill",
            };
            assert!(
                offered
                    .iter()
                    .any(|o| std::mem::discriminant(o) == std::mem::discriminant(w)),
                "the builder never offers {name}, so that panel can never reach a screen"
            );
        }
        assert_eq!(
            offered.len(),
            all.len(),
            "the builder offers a different number of widgets than exist"
        );

        /* AND EVERY ONE OF THEM SAYS WHAT IT IS. A button with an empty face is a button nobody
         * presses on purpose. */
        for w in &offered {
            assert!(!w.label().is_empty());
        }
    }

    /// DEFECT: two widgets whose buttons read the same, so a person cannot tell what they added.
    #[test]
    fn no_two_offered_widgets_share_a_label() {
        let mut labels: Vec<String> = Widget::every().iter().map(Widget::label).collect();
        let n = labels.len();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), n, "two widgets share a label: {labels:?}");
    }

    /// DEFECT: an overlay carrying a widget that a settings file cannot round trip.
    ///
    /// Every arm is tagged by `kind` in JSON, so an arm added without a distinct tag would silently
    /// deserialise as another one and a person's Tank overlay would come back as a damage meter.
    #[test]
    fn every_widget_survives_the_settings_file() {
        for w in Widget::every() {
            let text = serde_json::to_string(&w).expect("serialises");
            let back: Widget = serde_json::from_str(&text).expect("round trips");
            assert_eq!(back, w, "{} did not survive: {text}", w.label());
        }

        /* A whole overlay with one of everything in it, which is what a Full Combat HUD is. */
        let mut hud = Overlay::fresh(&[], "HUD");
        hud.widgets = Some(Widget::every());
        let text = serde_json::to_string(&hud).expect("serialises");
        let back: Overlay = serde_json::from_str(&text).expect("round trips");
        assert_eq!(back, hud);
    }

    /// DEFECT: a metric that reads the wrong field, which is a whole overlay quietly showing
    /// somebody's damage taken under the word "healing".
    ///
    /// Four distinct values so a transposed pair cannot pass.
    #[test]
    fn every_metric_reads_its_own_field() {
        let f = fighter(1, 2, 3, 4);
        assert_eq!(Metric::Dealt.of(&f), 1);
        assert_eq!(Metric::Taken.of(&f), 2);
        assert_eq!(Metric::Healed.of(&f), 3);
        assert_eq!(Metric::Received.of(&f), 4);
    }

    /// DEFECT: a metric added to the enum and forgotten in the list the builder offers, so a
    /// person can never choose it.
    ///
    /// This is the reachability half for `Metric`. The `of` match is exhaustive and the compiler
    /// guards it; `ALL` is a hand-written list and nothing guards that but this.
    #[test]
    fn the_builder_offers_every_metric_that_exists() {
        let f = fighter(1, 2, 3, 4);
        let mut seen: Vec<u64> = Metric::ALL.iter().map(|m| m.of(&f)).collect();
        seen.sort_unstable();
        assert_eq!(
            seen,
            vec![1, 2, 3, 4],
            "ALL does not cover every field `of` can read, so a metric exists that the builder \
             never offers"
        );
        for m in Metric::ALL {
            assert!(!m.label().is_empty());
            assert!(!m.unit(true).is_empty());
            assert!(!m.unit(false).is_empty());
        }
    }

    /// DEFECT: two units that read the same, so a healing overlay and a damage one are
    /// indistinguishable once the mob name and the clock are gone from the header.
    #[test]
    fn no_two_metrics_print_the_same_unit() {
        let mut units: Vec<&str> = Metric::ALL
            .iter()
            .flat_map(|m| [m.unit(true), m.unit(false)])
            .collect();
        let n = units.len();
        units.sort_unstable();
        units.dedup();
        assert_eq!(units.len(), n, "two metrics share a unit: {units:?}");
    }

    /// DEFECT: `fresh` handing out an id something already uses, which makes two windows share one
    /// settings entry and one remembered size.
    #[test]
    fn a_fresh_overlay_never_takes_an_id_that_is_taken() {
        let a = Overlay::fresh(&[], "First");
        assert_eq!(a.id, "ov1");

        let b = Overlay::fresh(std::slice::from_ref(&a.id), "Second");
        assert_eq!(b.id, "ov2");

        /* A GAP IN THE MIDDLE IS FILLED rather than counted past, because the ids are a set and
         * not a sequence: nothing anywhere depends on them being consecutive. */
        let c = Overlay::fresh(&["ov1".to_owned(), "ov3".to_owned()], "Third");
        assert_eq!(c.id, "ov2");

        /* And the shipped id is respected like any other. */
        let d = Overlay::fresh(&["ov1".to_owned(), "dps".to_owned()], "Fourth");
        assert_eq!(d.id, "ov2");
    }

    /// DEFECT: a new overlay that is created and cannot be seen, because it defaulted to closed.
    ///
    /// Somebody who presses `+` has said they want a window. The shipped one is different: it is
    /// there on a fresh install and opening it on first launch would put a window over whatever
    /// the person was doing.
    #[test]
    fn a_new_overlay_opens_and_the_shipped_one_does_not() {
        assert!(Overlay::fresh(&[], "Mine").open);
        assert!(!Overlay::default_dps().open);
        assert!(
            Overlay::default_dps().pinned,
            "an overlay the game covers has done nothing"
        );
    }

    /// DEFECT: an empty overlay list leaving the Parser page blank with no way to tell whether the
    /// feature is broken or the owner simply deleted everything.
    #[test]
    fn an_empty_list_comes_back_as_the_shipped_one() {
        assert_eq!(or_default(&[]), vec![Overlay::default_dps()]);

        /* And a list with anything in it is left exactly alone, including one that does NOT
         * contain the shipped overlay: deleting DPS is a choice and it sticks. */
        let mine = vec![Overlay::fresh(&[], "Healing")];
        assert_eq!(or_default(&mine), mine);
    }

    /// DEFECT: a settings file written by this build that an older one cannot read, or a config
    /// that loses a field on the way to disk and back.
    #[test]
    fn an_overlay_survives_a_round_trip() {
        let mut o = Overlay::fresh(&[], "Tank");
        o.widgets = Some(vec![Widget::Ranked(Ranked {
            foot: false,
            metric: Metric::Taken,
            rate: false,
            cols: Cols {
                rank: true,
                value: true,
                share: false,
                bar: true,
                head: false,
            },
            cap: 5,
            /* THE ROUND TRIP MUST CARRY A NON DEFAULT VALUE FOR EVERY FIELD, or a field lost
             * on the way to disk and back would still compare equal. */
            fit: true,
            headline: false,
        })]);
        o.w = 380.0;
        o.h = 140.0;

        let text = serde_json::to_string(&o).expect("serialises");
        let back: Overlay = serde_json::from_str(&text).expect("round trips");
        assert_eq!(back, o);

        /* AND A PARTIAL RECORD STILL LOADS, which is what `#[serde(default)]` buys: a file written
         * before a field existed must not fail the whole settings load. */
        let thin: Overlay =
            serde_json::from_str(r#"{"id":"ov9","name":"Thin"}"#).expect("a thin record loads");
        assert_eq!(thin.id, "ov9");
        assert_eq!(thin.name, "Thin");
        assert_eq!(
            thin.widgets, None,
            "a record with no widgets key must load as `nobody chose`, not as an empty list"
        );
        assert_eq!(
            thin.panels(),
            Overlay::shipped_panels(),
            "an overlay nobody has configured draws what this build ships"
        );
    }

    /// The default table is the owner's first mockup: no rank chip, a bar, a value and a share.
    #[test]
    fn the_default_table_is_the_compact_damage_meter() {
        let r = Ranked::default();
        assert_eq!(r.metric, Metric::Dealt);
        assert!(r.rate, "the owner asked for real time DPS");
        assert!(r.headline);
        assert!(
            !r.cols.rank,
            "the rank chip was cut: the row's position says it"
        );
        assert!(r.cols.bar && r.cols.value && r.cols.share);
    }

    /// DEFECT: AN EMPTY-TABLE SENTENCE BUILT OUT OF A COLUMN ABBREVIATION.
    ///
    /// The renderer wrote "Nobody named has any {}." with [`Metric::unit`], which is meant to sit
    /// beside a number. Three of the four metrics came out as broken English:
    ///
    /// ```text
    /// Nobody named has any dmg.
    /// Nobody named has any taken.
    /// Nobody named has any healed by.
    /// ```
    ///
    /// It is the one line on a panel whose whole job is to say honestly that there is nothing to
    /// show, and it was the line a reader had to parse twice.
    ///
    /// WHAT MUTATION MAKES THIS RED: pointing the empty state back at `unit`, or writing a
    /// `nothing` arm that is a noun rather than a verb phrase.
    #[test]
    fn the_empty_table_sentence_is_a_sentence_for_every_metric() {
        for m in Metric::ALL {
            let said = format!("Nobody has {}.", m.nothing());
            /* A VERB PHRASE, WHICH IS WHAT THE SLOT NEEDS. Each of the four begins with one, and
             * an abbreviation dropped in here would not. */
            let head = m.nothing().split(' ').next().unwrap_or_default();
            assert!(
                ["dealt", "taken", "healed", "been"].contains(&head),
                "{m:?} answers {said:?}, which does not read as a sentence"
            );
            /* AND IT IS NOT THE COLUMN LABEL, which is the substitution that caused this. */
            assert_ne!(
                m.nothing(),
                m.unit(false),
                "{m:?} is using its column abbreviation as a sentence again"
            );
            assert!(said.len() > "Nobody has .".len() + 4);
        }

        /* THE FOUR ARE DISTINCT, so a reader can tell which table is empty from the words alone. */
        let mut all: Vec<&str> = Metric::ALL.iter().map(|m| m.nothing()).collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 4, "two metrics say the same thing when empty");
    }
}
