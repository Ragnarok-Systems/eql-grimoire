//! The desktop's OWNED MIRROR of `grimoire_parse::fights`, and the one fold that fills it.
//!
//! WHY A MIRROR AND NOT THE ENGINE'S OWN TYPES, WHICH IS THE ONLY REAL DECISION IN THIS FILE.
//! `Entry`, `Fight` and `Participant` all borrow the log text. That is what lets the parser walk a
//! 61 MB log at a few hundred MB/s with no per-line allocation, and it is not a wart to be worked
//! around: it is the reason the engine is fast. But this crate does its bootstrap read and parse on
//! a WORKER THREAD (`ingest::scan`) and carries the answer home over an `mpsc::Sender<Scan>`, and a
//! channel demands `Send + 'static`. A `Fight<'a>` whose `'a` is a `String` the worker owns is
//! neither. So an owned mirror is not a preference here, it is the only shape that can cross that
//! channel, and every field of [`FightRow`] is COPIED out of the engine's own accessor before the
//! text goes out of scope. Nothing borrowed escapes [`fold_text`].
//!
//! WHY EVERY NUMBER IS COPIED AND NOT ONE OF THEM RECOMPUTED. Two of these fields look trivially
//! derivable from the others and are not:
//!
//!   * `secs` is [`Fight::seconds`], which floors a span at ONE second. The log stamps to the
//!     second and up to 32 lines in the capture share one; a fight that opens and closes inside a
//!     single printed second has a true duration these bytes cannot express. Recomputing it here as
//!     `seconds(end) - seconds(start)` gives 0 for that fight, and 0 is not a smaller number than
//!     1, it is a different claim: it says the fight took no time, and it makes every rate ever
//!     divided by it infinite.
//!   * `headline` is [`Fight::headline`], which is taken-first, dealt-as-fallback, and breaks a tie
//!     by name so the answer cannot depend on the order participants happened to appear in the
//!     file. Recomputing it with the obvious `max_by_key` forks that tie-break silently: the same
//!     log names a different mob on this screen than it does in the CLI, and neither is visibly
//!     wrong.
//!
//! Both are exactly the kind of fork that stays green for ever, because a desktop test written
//! against the desktop's own arithmetic agrees with itself. The parity sweep at the bottom of this
//! file exists for that and nothing else: it re-runs `grimoire_parse` over the same bytes, at six
//! windows, with and without the owner, and demands the mirror match the engine field by field.
//!
//! NO RATE COLUMN, HERE OR ANYWHERE THIS FEEDS. [`Fight::dps`] divides by that floored span, and
//! the engine's own test asserts 40.0 dps for a fight that lasted less than one printed second.
//! That is the single largest misreport the engine can make, it is honest about it, and putting it
//! on a screen is not. Rates come back behind a publishability floor, with the floor drawn beside
//! them. That is why this struct carries `damage` and `secs` and no quotient of the two: a row that
//! does not carry the number cannot draw it by accident.

use grimoire_parse::combat::parse;
use grimoire_parse::combat::Actor;
use grimoire_parse::fights::{Ended, Fight, Fights, NameKind, Participant, What, QUIET_SECONDS};
use grimoire_parse::group::Party;

/// The quiet window this build cuts fights at, in the width [`fold_text`] takes.
///
/// A `30` TYPED HERE WOULD BE A SECOND OPINION ABOUT A MEASURED JUDGEMENT, and a restatement rots:
/// the day [`QUIET_SECONDS`] moved, the screen and `grimoire fights` would cut the same log into
/// different numbers of fights and both would look right. A `QUIET_SECONDS as u32` is no better,
/// because `as` is a silent wrap and a wrapped window cuts a log into thousands of one-line fights
/// with nothing on screen to say why. `try_from` cannot wrap, and the fallback saturates UP on
/// purpose: a too-long window merges two fights into one honestly-labelled row, where a zero window
/// invents hundreds of encounters that never happened. The arm is unreachable while the constant is
/// a small positive number, and the test below is what says so out loud.
pub fn quiet_window() -> u32 {
    u32::try_from(QUIET_SECONDS).unwrap_or(u32::MAX)
}

/// HOW LONG THE READER GOES WITHOUT A COMBAT LINE BEFORE HE IS OUT OF COMBAT.
///
/// NOT [`QUIET_SECONDS`], AND THE TWO QUESTIONS ARE NOT THE SAME ONE. The quiet window answers
/// "is this the same fight as the last pull", where being wrong merges two pulls into one and
/// halves a DPS figure, so it is deliberately generous. This answers "am I swinging RIGHT NOW",
/// where being wrong leaves `IN COMBAT` burning over a corpse. A reader watched that for half a
/// minute after every kill because one constant was doing both jobs.
///
/// MEASURED OFF THE SAME DISTRIBUTION THE QUIET WINDOW WAS. Of the 1,778 gaps between combat
/// lines in the reference capture, 1,729 are a second or less, because auto-attack lands about
/// every two seconds. Ten seconds covers all but six of the rest: the gaps of 13, 14, 19, 20, 27
/// and 29 seconds, six lulls in two hours of play. Each one costs a few seconds of the mark going
/// out mid-pull and coming back, which is what the log actually said happened.
///
/// THE OVERWHELMING CASE IS NOT THIS CONSTANT AT ALL. A pull that ends with something dying is
/// closed by `Ended::Killed` on the death line itself, with no window and no wait. This is the
/// fallback for the pulls where nothing dies: the reader fled, the mob fled, or the reader lost.
pub const COMBAT_SECONDS: i64 = 10;

/// [`COMBAT_SECONDS`] as the width a comparison against a log stamp wants.
pub fn combat_window() -> u32 {
    u32::try_from(COMBAT_SECONDS).unwrap_or(u32::MAX)
}

/// HOW LONG A FINISHED ENCOUNTER IS HELD ON SCREEN BEFORE IT IS CALLED CLOSED.
///
/// THE OWNER'S NUMBER AND THE OWNER'S REASON: a pull that ends is not always over, because the
/// next mob can be on its way. The totals stay up either way, so this window does not decide what
/// is drawn, only what the mark beside it claims: held, or done.
pub const HOLD_SECONDS: i64 = 6;

/// WHAT THE MARK BESIDE A LIVE METER IS ENTITLED TO CLAIM.
///
/// A BOOLEAN COULD NOT SAY THE MIDDLE THING. `live` was true or false, so the dot was green or
/// grey, and a pull that had just ended had to be called one or the other: green lied about a
/// corpse, grey threw away the fact that the reader is still standing in the camp with the next
/// mob incoming. The state between them is real and now has a name.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Pulse {
    /// Blows are landing. The figures beside this are still growing.
    Fighting,
    /// The fight is over and the encounter is held open for [`HOLD_SECONDS`] in case another mob
    /// is on its way. The figures are final for this pull and are still worth reading.
    Holding,
    /// The encounter is closed. Whatever is drawn is history until the next blow lands.
    #[default]
    Closed,
}

impl Pulse {
    /// Is the reader swinging? The narrow question the old boolean asked, kept because plenty of
    /// the drawing turns on it and only the mark needs all three answers.
    pub fn fighting(self) -> bool {
        self == Pulse::Fighting
    }

    /// THE OLD BOOLEAN, WIDENED, FOR THE SURFACES THAT ONLY HAVE ONE.
    ///
    /// The Analysis page, the dashboard's preview tiles and the Live page draw widgets from a row
    /// they already hold rather than from a running ingest, so all they can honestly say is
    /// whether it is the fight in progress. `false` becomes [`Pulse::Closed`] and not
    /// [`Pulse::Holding`]: a page that cannot time the hold must not claim one.
    pub fn from_live(live: bool) -> Self {
        if live {
            Pulse::Fighting
        } else {
            Pulse::Closed
        }
    }

    /// The mark's colour: green swinging, amber held, red closed.
    pub fn tint(self) -> egui::Color32 {
        match self {
            Pulse::Fighting => crate::theme::SETTLED,
            Pulse::Holding => crate::theme::GOLD,
            Pulse::Closed => crate::theme::WRONG,
        }
    }

    /// What the mark means, for a tooltip and for a test that would otherwise assert on a colour.
    pub fn words(self) -> &'static str {
        match self {
            Pulse::Fighting => "in combat",
            Pulse::Holding => "fight over, encounter held",
            Pulse::Closed => "encounter closed",
        }
    }
}

/// WHO A FIGHTER IS, OWNED. An arm-for-arm mirror of [`Actor`], which borrows the log text.
///
/// THE READER IS A VARIANT AND NOT A NAME, because `Fights::with_owner` folds the owner's
/// character name into [`Actor::You`] before the aggregator ever sees it. Flattening that to a
/// string here would either invent a name the fold deliberately removed or print the pronoun as
/// though it were one, and an overlay whose top row says `You` next to a row that says `Reviir` is
/// the exact double-count `with_owner` exists to prevent.
///
/// [`Actor::Unknown`] IS KEPT AND IS NOT DROPPED. Falling damage and `You hurt yourself` name
/// nobody, and the engine's note says inventing an attacker is a lie the aggregator cannot see
/// through. A screen that silently discarded these rows would report a smaller total than the
/// fight's own `damage`, and the reader would have no way to tell which of the two numbers lied.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Who {
    /// The log's owner, with his character name already folded in by the aggregator.
    You,
    /// Anyone else, spelled exactly as the line spelled them.
    Named(String),
    /// The line named nobody.
    Unknown,
}

impl Who {
    /// An owned mirror of one [`Actor`]. One arm per variant and no default, so a new variant in
    /// the engine is a compile error here rather than a fighter quietly becoming `Unknown`.
    fn of(a: Actor<'_>) -> Who {
        match a {
            Actor::You => Who::You,
            Actor::Named(n) => Who::Named(n.to_owned()),
            Actor::Unknown => Who::Unknown,
        }
    }

    /// What to print. `You` is the word the log itself uses for the reader, and the empty-source
    /// case is named rather than blank so a row with no attacker is visibly a row with no attacker.
    pub fn text(&self) -> &str {
        match self {
            Who::You => "You",
            Who::Named(n) => n,
            Who::Unknown => "(nobody named)",
        }
    }

    /// IS THIS A PLAYER, OR IS IT SOMETHING THE WORLD SPAWNED?
    ///
    /// THE RULE IS A SPACE, AND IT IS A RULE OF THE GAME RATHER THAN A GUESS ABOUT THE LOG. An
    /// EverQuest character name is a single word: the client will not create one with a space in
    /// it. Everything the world spawns is free of that constraint, and nearly all of it breaks it,
    /// either with an article (`a dry bone skeleton`) or with a title (`Guard Ullindin`).
    ///
    /// MEASURED, NOT ASSUMED, over the reference capture's 2,414 lines. Every entity that dealt
    /// damage in it, run through the shipping CLI:
    ///
    ///   NO SPACE, and all six are players. (This line said "players in the owner's own group", and
    ///   that is not what the rule measures: `Losumyda` never was, his only other lines being
    ///   NewPlayers chat. Who was grouped is `FightRow::group`'s question, never this rule's.)
    ///     `Fylasem` `Losumyda` `Poguhy` `Rykabe` `Tanefi` `Tanefilo`
    ///   A SPACE, and not one of the twenty-five is a player:
    ///     `a dry bone skeleton` `A crazed ghoul` `An undead brewer` `a large spider` `a skeleton`
    ///     `Guard Ullindin` `Guard Sheg` `Guard V`Lex` `Torklar Battlemaster` `Trolon Lightleer`
    ///     `Reclusive ghoul magus` `Reclusive ghoul magus pet` and thirteen more
    ///
    /// AN ARTICLE TEST ALONE WOULD NOT DO. `Guard Ullindin` and `Trolon Lightleer` carry no article
    /// and are not players; fight #3 of the capture is the guards of Qeynos killing the reader.
    ///
    /// WHERE IT CAN BE WRONG, SAID OUT LOUD. A player who has earned a SURNAME would be two words
    /// if the client ever wrote one into a combat line. Every combat line in the capture uses the
    /// first name alone, including for players who speak in chat, so this has not been observed;
    /// it is the failure this rule has, and a log that shows one is the thing that would fix it.
    ///
    /// A NAMED MOB WITH A ONE-WORD NAME BREAKS THE RULE, AND THE OWNER'S LOG HAS SEVERAL.
    /// `Xicotl` (182 lines of `Xicotl punches YOU`), `Gearheart`, `Enynti` and `Ssynthi` are mobs
    /// that fought him, and a name cannot tell any of them from a player: they sat on his damage
    /// roster, `Xicotl` with a class chip, and their deaths were flagged as a person's. So a ROW
    /// can overrule this with evidence, and every roster asks the row: see [`proven_foes`] and
    /// [`FightRow::player`]. This stays the rule for a name nothing has proven either way.
    ///
    /// A CHARMED PET IS A MOB BY NAME AND THIS CANNOT SEE THE DIFFERENCE, so the fighter says which
    /// one is the reader's: [`Fighter::pet`].
    pub fn player(&self) -> bool {
        match self {
            Who::You => true,
            /* Nobody at all is not a player, and it is not a mob either. It is excluded from a
             * roster of people by being nobody, which is the honest reason rather than a guess. */
            Who::Unknown => false,
            Who::Named(n) => !n.contains(' '),
        }
    }
}

/// WHICH FAMILY A NAMED SOURCE OF DAMAGE CAME FROM. Owned mirror of `NameKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Family {
    Melee,
    Shield,
    Dot,
    Spell,
}

impl Family {
    fn of(k: NameKind) -> Family {
        match k {
            NameKind::Melee => Family::Melee,
            NameKind::Shield => Family::Shield,
            NameKind::Dot => Family::Dot,
            NameKind::Spell => Family::Spell,
        }
    }

    /// What a column header calls it.
    pub fn label(self) -> &'static str {
        match self {
            Family::Melee => "melee",
            Family::Shield => "shield",
            Family::Dot => "dot",
            Family::Spell => "spell",
        }
    }
}

/// ONE NAMED SOURCE OF DAMAGE AND WHAT IT DID. The Ability Breakdown panel's row.
///
/// THE NAME IS THE LOG'S OWN WORD. See `grimoire_parse::fights::NameTally`: `slash` and `slashes`
/// are the log's second and third person and are two rows of a group table, not a merge bug.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Ability {
    pub name: String,
    pub family: Family,
    pub amount: u64,
    /// Landed damage lines. Never swings: a swing that missed is not a use of an ability that hit.
    pub hits: u32,
    pub crits: u32,
}

/// WHAT ONE FIGHTER DID TO ANOTHER. The Targets panel's row.
///
/// `slot` INDEXES [`FightRow::fighters`] AND IS NOT A NAME. The engine folds two spellings of one
/// mob into one participant, and the mirror copies participants at the same index, so the index is
/// the only key that keeps them together. Resolve it as `row.fighters[slot].who`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TargetShare {
    pub slot: usize,
    pub amount: u64,
    pub hits: u32,
}

/// SPELL DAMAGE BY THE LOG'S ELEMENT WORD. Melee is absent, deliberately: see the engine's note.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchoolShare {
    pub school: String,
    pub amount: u64,
    pub hits: u32,
}

/// SWINGS THIS FIGHTER THREW THAT WERE STOPPED, BY WHAT. The Hit Results donut.
///
/// NOT `swings - landed`. For the owner in the reference capture that subtraction is 110, a fifth
/// of which is the TARGET parrying or dodging, so a Miss slice built by subtracting credits the
/// defender's skill to the attacker's aim.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Outcomes {
    pub missed: u32,
    pub parried: u32,
    pub dodged: u32,
    pub blocked: u32,
    pub riposted: u32,
    pub invulnerable: u32,
    pub rune_absorbed: u32,
}

impl Outcomes {
    /// EVERY SWING THAT WAS STOPPED, HOWEVER.
    ///
    /// # NO SCREEN DRAWS THIS, AND THAT IS DELIBERATE RATHER THAN AN OVERSIGHT
    ///
    /// The Hit results panel takes its denominator from `Fighter::swings` and its rows from
    /// `slices()` chained with `hypothesised()`; it never asks for a total, because a total
    /// beside a column of parts is a number a reader has to check rather than read.
    ///
    /// WHAT IT IS FOR IS THE IDENTITY `landed + total() == swings`, which is the thing that
    /// makes that column trustworthy: the parts cannot be made to add up by fudging, because
    /// the denominator is measured separately. `widgets::every_swing_is_in_exactly_one_slice`
    /// and the engine's `fight_maps` both pin it, and they are the only callers by design.
    ///
    /// KEPT RATHER THAN DELETED, and this note is the difference between a known test-support
    /// API and a `pub fn` nobody noticed had no callers. An audit has now flagged it twice.
    pub fn total(self) -> u32 {
        self.missed
            + self.parried
            + self.dodged
            + self.blocked
            + self.riposted
            + self.invulnerable
            + self.rune_absorbed
    }

    /// Every slice a donut draws, with the log's own word for each. Zero slices are kept: a
    /// reader can tell "none of these happened" from "this app did not look" only if the row is
    /// there.
    /// THE FIVE OUTCOMES THE GRAMMAR HAS ACTUALLY MEASURED, always drawn, zero included.
    ///
    /// A ZERO HERE IS A REAL ANSWER. All five appear in the reference capture (misses 323,
    /// parries 15, dodges 20, blocks 2, ripostes 2), so a fighter who was never parried has a
    /// measured zero and drawing it says the app checked.
    pub fn slices(self) -> [(&'static str, u32); 5] {
        [
            ("miss", self.missed),
            ("parry", self.parried),
            ("dodge", self.dodged),
            ("block", self.blocked),
            ("riposte", self.riposted),
        ]
    }

    /// THE TWO THE GRAMMAR CARRIES AS A HYPOTHESIS, drawn only when one actually fires.
    ///
    /// `combat.rs` says of both in as many words that they are "documented by the reference
    /// parser, absent from the capture, so it is carried as a hypothesis and not as a
    /// measurement". Neither has ever been seen in real bytes.
    ///
    /// SO A PERMANENT `invulnerable 0 0%` ROW IS THE IMMUNE-SLICE MISTAKE UNDER ANOTHER WORD. A
    /// drawn zero means the app looked and found none; these two mean the app has never once
    /// seen the line that would increment them, which is a different statement and must not be
    /// printed as the first. They appear the moment one fires and not before.
    pub fn hypothesised(self) -> Vec<(&'static str, u32)> {
        [
            ("invulnerable", self.invulnerable),
            ("absorbed", self.rune_absorbed),
        ]
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .collect()
    }
}

/// SOMETHING WORTH A MARK ON A TIMELINE. Owned mirror of `grimoire_parse::fights::What`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mark {
    Death { killer: usize, victim: usize },
    Ability { who: usize, name: String },
    Berserk { who: usize, on: bool },
    Crit { who: usize, what: String },
}

/// One [`Mark`], in seconds from the fight's own start.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Moment {
    pub at: u32,
    pub what: Mark,
}

/// ONE FIGHTER IN ONE FIGHT, OWNED. An owned mirror of [`Participant`], which borrows the log.
///
/// THIS IS THE HALF THAT USED TO BE THROWN AWAY. `grimoire_parse` has aggregated all nine of these
/// numbers since the engine was written, and [`FightRow`] mirrored `participants.len()` and dropped
/// the rest, so the desktop could say twenty-three took part and could not say what any one of them
/// did. Every combat overlay the owner asked for is a projection of these fields: DPS is `dealt`,
/// the healing view is `healed` and `received`, the tank view is `taken`, and the hit-rate columns
/// are `swings`, `landed` and `avoided`.
///
/// NO RATE HERE EITHER, AND FOR THE SAME REASON AS [`FightRow`]. Every field is a COUNT the log
/// stated. A `dps` on this struct would divide by [`FightRow::secs`], which is floored at one
/// second, and the module note above explains why that number must not reach a screen yet.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fighter {
    pub who: Who,
    /// THE READER'S OWN PET: charmed (the fold's `Participant::pet`, one line at a time) or a pet
    /// that answered him as `Master` in the fight's session ([`FightRow::pets`]). On the reader's
    /// side of every rule that asks [`FightRow::on_side`], and never a mob.
    ///
    /// DEFAULT FALSE FOR A STORED ROW FROM BEFORE IT EXISTED, which is what that row's fold said: it
    /// knew no charm.
    #[serde(default)]
    pub pet: bool,
    /// WHAT THIS CHARACTER HAS BEEN SEEN TO BE, or `None` when the log has not shown them
    /// casting anything the corpus can place. See [`crate::class`].
    ///
    /// ON THE ROW AND NOT LOOKED UP BY THE RENDERER, because `dps::draw_widget` is pure in
    /// (config, fight) and that is what makes the overlay builder's preview free: nothing it
    /// draws reads a snapshot or a settings key. A class is a fact about a fighter, so it
    /// travels with the fighter. `Ingest` stamps it after each fold.
    pub class: Option<String>,
    /// Damage this entity dealt to anybody.
    pub dealt: u64,
    /// Damage anybody dealt to this entity.
    pub taken: u64,
    /// Healing this entity put out.
    pub healed: u64,
    /// Healing this entity got, from anybody including itself.
    pub received: u64,
    /// Melee swings thrown, landed or not. Shield ticks and spell damage are not swings.
    pub swings: u32,
    /// Of those swings, the ones that did damage.
    pub landed: u32,
    /// Swings this entity was on the receiving end of and took no damage from.
    pub avoided: u32,
    pub kills: u32,
    pub deaths: u32,
    /// Every named source of damage this fighter used. The Ability Breakdown.
    pub abilities: Vec<Ability>,
    /// What it dealt to each other fighter, keyed on that fighter's index in the same row.
    pub targets: Vec<TargetShare>,
    /// Spell damage by element. Empty for anyone who cast nothing.
    pub schools: Vec<SchoolShare>,
    /// Its own swings that were stopped, and by what.
    pub outcomes: Outcomes,
    /// SECONDS FROM THE FIGHT'S START WHEN THIS ENTITY FIRST AND LAST TOOK DAMAGE.
    /// [`grimoire_parse::fights::Participant::first_taken_at`], copied. See there for why they
    /// exist: a live meter has to be able to say what is being fought NOW, and the fight's
    /// headline is about the whole run.
    pub first_taken_at: Option<u32>,
    pub last_taken_at: Option<u32>,

    /// Melee damage lines that carried `(Critical)`. Melee only: the flag also rides heals.
    pub melee_crits: u32,
    /// Damage dealt, one entry per SECOND of the fight in which it dealt any. The timeline.
    pub series: Vec<(u32, u64)>,
}

/// A FIGHTER WHO DID NOTHING, as a base for a fixture to override.
///
/// `Who::Unknown` AND NOT A NAME, because a default person would be a person this app invented.
/// Nothing in the app constructs one of these: `fighter_of` fills every field from the engine,
/// and this exists so a test can say what it is ABOUT and leave the rest alone. A field added
/// to `Fighter` later then does not break every fixture in the tree, which is what happened the
/// first time these maps landed.
impl Default for Fighter {
    fn default() -> Self {
        Fighter {
            who: Who::Unknown,
            pet: false,
            class: None,
            dealt: 0,
            taken: 0,
            healed: 0,
            received: 0,
            swings: 0,
            landed: 0,
            avoided: 0,
            kills: 0,
            deaths: 0,
            abilities: Vec::new(),
            targets: Vec::new(),
            schools: Vec::new(),
            outcomes: Outcomes::default(),
            melee_crits: 0,
            first_taken_at: None,
            last_taken_at: None,
            series: Vec::new(),
        }
    }
}

/// An empty fight, as a base for a fixture. Same reasoning as [`Fighter`]'s.
impl Default for FightRow {
    fn default() -> Self {
        FightRow {
            start: String::new(),
            end: String::new(),
            secs: 1,
            damage: 0,
            deaths: 0,
            lines: 0,
            ended: String::new(),
            headline: None,
            fighters: Vec::new(),
            moments: Vec::new(),
            zone: None,
            zone_gap: None,
            cut: false,
            /* NOT KNOWN, and never solo: a fixture that means solo says so. */
            group: None,
            pets: Vec::new(),
            foes: Vec::new(),
        }
    }
}

fn fighter_of(p: &Participant<'_>) -> Fighter {
    Fighter {
        who: Who::of(p.who),
        pet: p.pet,
        /* NOT KNOWN HERE. The fold sees damage lines; the classes come out of cast lines and
         * a corpus, neither of which this crate has. `Ingest::stamp_classes` fills it. */
        class: None,
        dealt: p.dealt,
        taken: p.taken,
        healed: p.healed,
        received: p.received,
        swings: p.swings,
        landed: p.landed,
        avoided: p.avoided,
        kills: p.kills,
        deaths: p.deaths,
        abilities: p
            .by_name
            .iter()
            .map(|t| Ability {
                name: t.key.to_owned(),
                family: Family::of(t.kind),
                amount: t.amount,
                hits: t.hits,
                crits: t.crits,
            })
            .collect(),
        targets: p
            .by_target
            .iter()
            .map(|t| TargetShare {
                slot: t.slot as usize,
                amount: t.amount,
                hits: t.hits,
            })
            .collect(),
        schools: p
            .by_school
            .iter()
            .map(|t| SchoolShare {
                school: t.school.to_owned(),
                amount: t.amount,
                hits: t.hits,
            })
            .collect(),
        outcomes: Outcomes {
            missed: p.outcomes.missed,
            parried: p.outcomes.parried,
            dodged: p.outcomes.dodged,
            blocked: p.outcomes.blocked,
            riposted: p.outcomes.riposted,
            invulnerable: p.outcomes.invulnerable,
            rune_absorbed: p.outcomes.rune_absorbed,
        },
        melee_crits: p.melee_crits,
        first_taken_at: p.first_taken_at,
        last_taken_at: p.last_taken_at,
        series: p.series.clone(),
    }
}

/// One fight, owned, ready to cross a thread boundary.
///
/// Every field is what `grimoire_parse` said, in the CLI's own words where the CLI has words. There
/// is deliberately no `dps`, no per-participant rate and no hit rate: see the module note.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FightRow {
    /// The stamp of the first combat line, exactly as the log printed it. Kept as text rather than
    /// as a parsed instant because the log carries no zone offset: the engine's clock is a
    /// difference engine, not a calendar, and a `DateTime` here would be a timezone claim these
    /// bytes cannot support.
    pub start: String,
    /// The stamp of the last one.
    pub end: String,
    /// [`Fight::seconds`], copied. Never `end - start`: see the module note.
    pub secs: i64,
    /// Every point of damage anybody dealt to anybody inside the fight.
    pub damage: u64,
    pub deaths: u32,
    /// Combat lines folded into this fight. Not lines the file spent on it: chat, loot and spell
    /// flavour in the middle of a pull are not combat and are not counted.
    pub lines: u32,
    /// Why the fight stopped, in the words `grimoire fights` prints. A screen that says "zoned"
    /// beside a CLI that says the same thing is checkable; two vocabularies for one enum are not.
    pub ended: String,
    /// [`Fight::headline`], copied. `None` when no named entity dealt or took anything, which is a
    /// real state (a fight that is nothing but the reader hurting himself) and not an error.
    pub headline: Option<String>,
    /// EVERY FIGHTER, IN THE ENGINE'S OWN ORDER, WHICH IS THE ORDER THEY FIRST APPEARED.
    ///
    /// NOT SORTED HERE. A screen that wants the biggest hitter first sorts its own copy; sorting in
    /// the mirror would mean the desktop and the CLI print the same fight in two different orders
    /// off the same bytes, and the sweep test that compares this mirror to the engine field by
    /// field would have to be taught to un-sort it before it could compare anything.
    pub fighters: Vec<Fighter>,
    /// Marks for a timeline, oldest first.
    pub moments: Vec<Moment>,
    /// The zone this fight happened in, as the log spelled it.
    pub zone: Option<String>,
    /// When a zone line ended this fight, how many seconds after its last combat line.
    pub zone_gap: Option<u32>,
    /// This is the oldest fight in the fold AND the text it came from was cut by the tail cap, so
    /// its opening lines are not in the bytes this app read and its numbers are a floor.
    ///
    /// [`fold_text`] cannot know this: it is handed a `&str`, and a `&str` remembers nothing about
    /// where it was cut from. Only the caller that did the read knows, so it is left false here and
    /// stamped by [`mark_clipped`]. A screen that shows this on a whole log is crying wolf; one
    /// that hides it on a clipped tail is quietly reporting half a fight as a fight.
    pub cut: bool,
    /// EVERYONE PROVABLY IN THE READER'S GROUP AT ANY SECOND OF THIS FIGHT, or `None` when the log
    /// does not say who that was for every second of it.
    ///
    /// # THREE ANSWERS, AND TWO OF THEM LOOK ALIKE ON A SCREEN THAT IS NOT CAREFUL
    ///
    ///   * `Some(names)`: the group, spelled as the log spelled it, never including the reader. A
    ///     member who left halfway through is kept, because he was in the group for part of it.
    ///   * `Some(vec![])`: SOLO, AND THAT IS A MEASUREMENT. The log proved nobody was with him.
    ///   * `None`: NOT KNOWN. A group the reader joined (the log names at most the inviter), an
    ///     invite nothing answered, raid evidence, a login, a tail that began mid-session. It must
    ///     never be drawn, filtered or summed as "nobody"; a filter that reads this field acts on
    ///     `Some` and shows every player, exactly as it did before the field existed, on `None`.
    ///
    /// COMPARE NAMES IGNORING CASE. The members come from lines the game wrote, but the engine
    /// matches an invite as the reader TYPED it (`You invite flagg`), and `Who::Named` is the
    /// spelling of a combat line.
    ///
    /// # STAMPED BY THE FOLD, NEVER WORKED OUT HERE
    ///
    /// [`fold_text`] pushes every line into a `grimoire_parse::group::Party` beside the combat
    /// aggregator and asks `Party::during(start, end)` once the whole text is in. After, and not
    /// while folding, because that engine revokes: a member nobody announced turning up later
    /// proves the spans since the group formed were never complete, so an answer can move from
    /// `Some` to `None` as more text arrives. It never moves the other way.
    ///
    /// # `serde(default)`, AND THE DEFAULT IS `None`
    ///
    /// Every row in the store before this field existed was written by a build that never looked
    /// at the group. `None` is what that build knew. A default of `Some(vec![])` would load the
    /// whole history as solo and hide every player in it, and a default that is silently
    /// persisted is indistinguishable from a choice.
    ///
    /// # MEASURED ON THE CAPTURE
    ///
    /// Fight 1 is `None`: the file has no group line before it, and the removal 38 seconds after
    /// it proves he WAS grouped without saying with whom. Fights 2, 3 and 4 are `Some(vec![])`,
    /// after `You have been removed from the group.` at 23:21:54, and fight 2 still has Fylasem
    /// and Poguhy, group mates in fight 1, swinging at a dry bone skeleton two minutes after the
    /// removal. Fight 4's Losumyda was never in the group: his only other lines are NewPlayers
    /// chat. Those are the people a group filter exists to take off the reader's meter.
    ///
    /// # WHERE IT CAN BE WRONG, SAID OUT LOUD
    ///
    ///   * A member someone else invited, never announced, who never speaks in the group, never
    ///     leads it and leaves without a line, is missing from `Some`. The log has nothing that
    ///     would catch him; see `Party`'s own note.
    ///   * A STORED ROW KEEPS THE ANSWER IT WAS WRITTEN WITH. A later line could revoke that span
    ///     and the store will not rewrite it. The engine measured this never happening across the
    ///     owner's four logs (every fight asked 30 seconds after it ended and again after the whole
    ///     file gave the same answer), and every row this app stores is one a later line had
    ///     already closed. Rare and measured at zero is not impossible.
    #[serde(default)]
    pub group: Option<Vec<String>>,
    /// THE READER'S OWN PETS IN THIS FIGHT'S SESSION, spelled as the log spelled them: every
    /// one-word name that answered him as `Master` between the logins either side of the fight
    /// (`grimoire_parse::group::Party::pets_during`).
    ///
    /// # WHY THE ROSTER NEEDS IT
    ///
    /// A summoned pet is one word (`Gabtik`), so `Who::player` calls it a player, and a roster that
    /// keeps only the reader and [`FightRow::group`] took the reader's own pet off his meter
    /// whenever the group was known: 63 known fights and 122,294 damage across the owner's four
    /// logs, while the same pets stayed on every fight whose group was not known. [`FightRow::ours`]
    /// keeps these names beside the reader.
    ///
    /// # `serde(default)`, AND EMPTY MEANS NONE PROVEN
    ///
    /// Not "he had no pet". A keep list can only err one way on a missing entry, and that way is the
    /// row it exists to keep being hidden; that is why an old stored row cannot be hurt by it: every
    /// one of them has `group: None`, and `None` shows every player whatever this holds.
    ///
    /// A GROUP MEMBER'S PET IS NEVER HERE, because nothing the reader's log prints ties a pet to an
    /// owner who is not the reader. A fight whose group is known leaves members' pets off.
    #[serde(default)]
    pub pets: Vec<String>,
    /// ONE-WORD NAMES IN THIS FIGHT THAT ARE PROVEN MOBS, spelled as the log spelled them.
    ///
    /// [`Who::player`] calls every one-word name a player, and a named mob can have one word
    /// for a name. [`proven_foes`] is this fight's own evidence; [`crate::ingest::Ingest`] adds
    /// every name any of the character's fights proved, because the game will not give a player
    /// an NPC's name, so a name that is a mob in one fight is a mob in all of them.
    ///
    /// # `serde(default)`, AND EMPTY MEANS NOTHING PROVEN
    ///
    /// A row stored before this existed loads with no foes and is stamped again when it is read,
    /// from the damage it already recorded. Empty never hides anybody: it leaves every one-word
    /// name a player, which is what every page did before.
    #[serde(default)]
    pub foes: Vec<String>,
}

/// Fold a whole log text into owned rows, in LOG ORDER, oldest first.
///
/// `quiet` is the window from [`Fights::with_quiet`] and [`quiet_window`] is the measured default.
/// `owner` is the reader's own character name, which lives in the log's FILENAME and never in a
/// line: without it the owner is two rows in every table, once as `you` and once under his own
/// name. `None` is the honest answer when the filename does not yield one, not a guess.
///
/// ONE LIFETIME FOR BOTH `&str`s, WHICH IS A RESTRICTION AND IS THE CHEAP ONE. `Fights<'a>` borrows
/// the text through the entries pushed into it AND takes the owner name as `&'a str`, so the two
/// have to meet. Two independent lifetimes would also compile (every type in `Fights<'a>` is
/// covariant in `'a`, so inference could pick the intersection), but making the caller's obligation
/// explicit costs nothing at any call site in this crate and takes a variance argument off the
/// critical path of a change nobody can compile before landing it.
///
/// Returns the rows and the number of lines that carried a stamp this build could not read. That
/// count comes from [`Fights::unreadable`] and is NOT recounted here. A second stamp reader in this
/// crate would be a second chance to disagree with the first, and the disagreement would show up on
/// screen as combat vanishing with a confident zero beside it.
pub fn fold_text<'a>(text: &'a str, quiet: u32, owner: Option<&'a str>) -> (Vec<FightRow>, u32) {
    fold_text_after(text, quiet, owner, Party::new())
}

/// [`fold_text`], for a text that is NOT the start of what the reader's log has said.
///
/// `party` IS THE GROUP AS OF THE LINE BEFORE `text`'s FIRST, and it is the one piece of state a
/// fold can carry in from earlier bytes. `Fights` cannot be carried (see below), but a `Party` owns
/// every string it holds and is `Clone`, so the live window can start from what the bootstrap
/// already settled instead of from `Unknown` on every poll. A whole text passes `Party::new()`,
/// which knows nothing, and that is exactly what [`fold_text`] does.
///
/// BY VALUE, SO THE CLONE IS THE CALLER'S AND IS VISIBLE THERE. The fold pushes every line of
/// `text` into it, and pushing a line twice is not harmless: a member leaving twice looks like
/// somebody who was never there leaving, which revokes the group, and a text that repeats earlier
/// stamps is a clock stepping back, which fogs every span it overlaps. So `party` must have seen
/// exactly the lines before `text` and none of `text` itself.
pub fn fold_text_after<'a>(
    text: &'a str,
    quiet: u32,
    owner: Option<&'a str>,
    mut party: Party,
) -> (Vec<FightRow>, u32) {
    let mut agg = Fights::new().with_quiet(i64::from(quiet));
    if let Some(name) = owner {
        // `with_owner` already refuses an empty name, so an empty `--me` cannot fold every unnamed
        // actor into the reader.
        agg = agg.with_owner(name);
    }

    for raw in text.lines() {
        /* EVERY LINE TO THE PARTY, OUTSIDE THE COMBAT TEST AND NOT FILTERED BY IT. `combat::parse`
         * answers `None` only for a line with no stamp, so today a push inside the `if` below sees
         * the same lines (mutation-checked: it stays green). It is out here so that stays true the
         * day this fold starts skipping readings it has no use for: no group line is a combat
         * reading (`combat` grades them `Ignored::Group` or chat), and a party fed only combat
         * would stamp every fight not known. The party also watches the clock on every stamped
         * line, so a backward step between two fights is never missed. */
        party.push(raw);
        // Lines that are not log lines at all yield `None` and are simply not offered. Coverage
        // accounting (parsed / ignored / flavour / unrecognised) is the CLI's job and is not
        // duplicated here; this screen lists fights.
        if let Some(entry) = parse(raw) {
            /* AFTER THE PARTY HAS SEEN THIS SAME LINE, so a charm landing on it is already known. */
            agg.push_with_pet(entry, party.charmed());
        }
    }

    // READ THE COUNT BEFORE FINISHING, and that order is forced rather than stylistic. `finish`
    // takes the aggregator BY VALUE, so there is no aggregator left to ask afterwards. The same
    // consumption is why there is no live incremental fold: `Fights` cannot be held open across
    // frames and read for finished fights, so a refresh re-folds the whole text.
    let unreadable = agg.unreadable();
    /* THE GROUP IS ASKED HERE, AFTER THE LAST LINE, and never inside the loop. See
     * `FightRow::group`: a later line can revoke a span, so an answer taken mid-fold could claim a
     * group the rest of the text disproves. */
    let rows = agg.finish().iter().map(|f| row_of(f, &party)).collect();
    (rows, unreadable)
}

/// Say that the text handed to [`fold_text`] was a cut tail, so its oldest fight is missing its
/// opening lines.
///
/// A SEPARATE CALL RATHER THAN A FOURTH ARGUMENT because the two facts come from different places:
/// the fold knows the fights, and only `ingest::read_tail` knows whether it started at byte zero.
/// Passing `false` clears the flag, so re-marking a re-fold cannot leave a stale warning on a log
/// that has since been read whole.
pub fn mark_clipped(rows: &mut [FightRow], clipped: bool) {
    // ONLY THE OLDEST ROW. Everything after it opened inside bytes this app actually read, so
    // flagging the whole list would say "these numbers are a floor" about fights that are exact.
    // This is why the fold must never be reversed before marking: index 0 is the oldest.
    if let Some(first) = rows.first_mut() {
        first.cut = clipped;
    }
}

/// One fight, copied out of the borrow, with the group the party saw during it.
///
/// THE PARTY IS AN ARGUMENT SO NO ROW CAN BE BUILT WITHOUT ASKING IT. A `group: None` here with a
/// stamp added afterwards would compile, pass every test that folds a log with no group lines in
/// it, and ship a field that is `None` on every row, which reads as "not known" for ever and is
/// indistinguishable from a fold that never looked.
fn row_of(f: &Fight<'_>, party: &Party) -> FightRow {
    let mut row = FightRow {
        start: f.start.to_owned(),
        end: f.end.to_owned(),
        secs: f.seconds(),
        damage: f.damage,
        deaths: f.deaths,
        lines: f.lines,
        ended: ended_words(f.ended).to_owned(),
        headline: f.headline().map(str::to_owned),
        fighters: f.participants.iter().map(fighter_of).collect(),
        moments: f
            .events
            .iter()
            .map(|e| Moment {
                at: e.at,
                what: match e.what {
                    What::Death { killer, victim } => Mark::Death {
                        killer: killer as usize,
                        victim: victim as usize,
                    },
                    What::Ability { who, name } => Mark::Ability {
                        who: who as usize,
                        name: name.to_owned(),
                    },
                    What::Berserk { who, on } => Mark::Berserk {
                        who: who as usize,
                        on,
                    },
                    What::Crit { who, what } => Mark::Crit {
                        who: who as usize,
                        what: what.to_owned(),
                    },
                },
            })
            .collect(),
        zone: f.zone.map(str::to_owned),
        zone_gap: f.zone_gap,
        cut: false,
        group: party.during(f.start, f.end),
        pets: party.pets_during(f.start, f.end),
        foes: Vec::new(),
    };
    /* A PET THAT ANSWERED HIM IS HIS FOR THE SESSION, as `FightRow::pets` says, and the fighter says so
     * too so every rule can ask the fighter. */
    for x in &mut row.fighters {
        if matches!(&x.who, Who::Named(n) if row.pets.iter().any(|p| p.eq_ignore_ascii_case(n))) {
            x.pet = true;
        }
    }
    /* AFTER THE GROUP AND THE PETS, which are who the reader's side is. */
    row.foes = proven_foes(&row.fighters, row.group.as_deref(), &row.pets);
    row
}

/// THE ONE-WORD NAMES IN ONE FIGHT THAT TRADED DAMAGE WITH THE READER'S SIDE, which makes them mobs.
///
/// # THE EVIDENCE
///
/// A player cannot hurt the reader, his group or his pets, and they cannot hurt a player: this is
/// a game with no fighting between players outside a duel. So a one-word name that dealt damage
/// to the reader's side, or took damage from it, is a mob whatever its name looks like. Measured
/// on the owner's own store: `Xicotl` dealt 1,045 to him and took 1,069 from him on Sep 7, and
/// `Ssynthi` dealt 43 and took 1,004. Every fighter's `targets` already holds exactly this.
///
/// # WHO THE READER'S SIDE IS
///
/// The reader; the reader's own pets; and the group, only when the log proved it (`group` is
/// `Some`). With the group not known a player beside the reader is not assumed to be on his
/// side, so a mob that only ever hit him proves nothing here.
///
/// # WHERE IT CAN BE WRONG, SAID OUT LOUD
///
///   * A DUEL. A player the reader duels trades damage with him and is called a mob.
///   * A MOB NOBODY ON THE READER'S SIDE TOUCHED in this fight is not proven here; the ingest's
///     book of every fight is what catches it (see [`FightRow::foes`]).
pub fn proven_foes(fighters: &[Fighter], group: Option<&[String]>, pets: &[String]) -> Vec<String> {
    let named_in = |x: &Fighter, list: &[String]| matches!(&x.who, Who::Named(n) if list.iter().any(|m| m.eq_ignore_ascii_case(n)));
    let ally: Vec<bool> = fighters
        .iter()
        .map(|x| {
            x.pet
                || matches!(x.who, Who::You)
                || named_in(x, pets)
                || group.is_some_and(|g| named_in(x, g))
        })
        .collect();
    let mut out: Vec<String> = Vec::new();
    for (j, x) in fighters.iter().enumerate() {
        if ally[j] || !x.who.player() {
            continue;
        }
        let hit_ally = x
            .targets
            .iter()
            .any(|t| t.amount > 0 && ally.get(t.slot).copied().unwrap_or(false));
        let hit_by_ally = fighters
            .iter()
            .enumerate()
            .any(|(i, a)| ally[i] && a.targets.iter().any(|t| t.slot == j && t.amount > 0));
        if hit_ally || hit_by_ally {
            let name = x.who.text();
            if !out.iter().any(|o| o.eq_ignore_ascii_case(name)) {
                out.push(name.to_owned());
            }
        }
    }
    out
}

/// IS THIS A PLAYER, GIVEN THE NAMES ITS FIGHT HAS PROVEN ARE MOBS? [`Who::player`], overruled by
/// evidence. See [`proven_foes`].
pub fn player_in(foes: &[String], who: &Who) -> bool {
    who.player() && !matches!(who, Who::Named(n) if foes.iter().any(|f| f.eq_ignore_ascii_case(n)))
}

/// [`FightRow::ours`] for a GROUP rather than a row, and the one body that rule has.
///
/// # WHY A FREE FUNCTION AS WELL AS THE METHOD
///
/// A scope of several fights has a group (`reports::rolled_group`) and no single row that owns it:
/// the dashboard's timeline draws one line per fight and has to filter every one of them by the
/// SCOPE's answer, or its lines stop adding up to the roster beside it. Building a throwaway
/// `FightRow` just to carry a group into the method would work and would be a row that is not a
/// fight. So the rule lives here, the method is one line onto it, and the two cannot disagree.
///
/// `group` IS THE FIELD'S OWN THREE ANSWERS: `None` is not known (every player), `Some(&[])` is
/// solo (the reader only), `Some(names)` is the reader and those names, compared ignoring case.
///
/// `pets` IS [`FightRow::pets`], and a name there is on the roster beside the reader whatever the
/// group is: solo with a pet is still a pet dealing the reader's damage.
///
/// `foes` IS [`FightRow::foes`]: a one-word name proven to be a mob is on nobody's roster.
pub fn on_roster(group: Option<&[String]>, pets: &[String], foes: &[String], who: &Who) -> bool {
    if !player_in(foes, who) {
        return false;
    }
    match (who, group) {
        (Who::You, _) | (_, None) => true,
        (Who::Named(n), Some(members)) => members
            .iter()
            .chain(pets)
            .any(|m| m.eq_ignore_ascii_case(n)),
        /* `player()` already said no to this one; the arm is here so the match is whole. */
        (Who::Unknown, Some(_)) => false,
    }
}

impl FightRow {
    /// THE LAST NAMED THING KILLED IN THIS FIGHT, and the second it went down.
    ///
    /// # WHAT THE HEADER SHOWS BETWEEN PULLS
    ///
    /// A fight stays open until combat has been quiet for `QUIET_SECONDS`, so for half a minute
    /// after a kill there is a live fight with nothing alive in it. Measured on the owner's own
    /// log: `You have slain a thunder spirit princess!` at 00:08:24, then sixty-nine seconds of
    /// looting, chat and memorising before the next combat line.
    ///
    /// WITHOUT THIS THE HEADER FALLS BACK TO `headline`, which names the biggest thing in the
    /// fight -- usually the very mob that just died -- with no sign it is a corpse. The reader
    /// sees the same name he saw mid-pull and cannot tell the difference between fighting it
    /// and having killed it.
    ///
    /// PLAYERS ARE EXCLUDED. A raid death is a real event and belongs in the deaths column; it
    /// is not what the group just killed.
    pub fn last_slain(&self) -> Option<(&str, u32)> {
        self.moments
            .iter()
            .filter_map(|m| match m.what {
                Mark::Death { victim, .. } => Some((victim, m.at)),
                _ => None,
            })
            .filter_map(|(slot, at)| {
                let f = self.fighters.get(slot)?;
                (!f.pet && !self.player(&f.who)).then(|| (f.who.text(), at))
            })
            /* THE NEWEST, and ties on the name so a chain that killed two things in one
             * printed second does not flicker. */
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
    }

    /// WHAT IS BEING FOUGHT NOW, and how long it has been engaged, or `None` when the log has
    /// not shown anything named being hit.
    ///
    /// # THIS DOC WAS ATTACHED TO THE FUNCTION ABOVE, WHICH IS ITS OWN SMALL DEFECT
    ///
    /// The whole of it, opening paragraph and all, sat on [`FightRow::last_slain`] with that
    /// function's own doc running straight on from it: one blank `///` line was missing, so
    /// rustdoc printed "WHAT IS BEING FOUGHT NOW" over the corpse finder and printed nothing at
    /// all over this one. Two functions that are deliberately different, one of them explaining
    /// itself under the other's name.
    ///
    /// # WHY THIS IS NOT [`FightRow::headline`], WHICH IS WHAT THE HEADER USED TO SHOW
    ///
    /// A fight is a run of combat with no thirty second gap in it, so on a raid night an eight
    /// minute chain of pulls is ONE fight and `headline` names the biggest damage sponge in the
    /// whole of it. The owner's screen read `IN COMBAT / A SPITE GOLEM / 08:06 / 15 players`,
    /// where the golem was the biggest thing in eight minutes rather than the thing in front of
    /// him and may have died six minutes earlier. Every figure was right about the chain and none
    /// of it was about now.
    ///
    /// THE MOB AND NOT A PLAYER. `Who::player` is the same filter every table here ranks by, and
    /// on a raid the most recently damaged entity is very often a person taking a hit. The engine
    /// deliberately never decides who is a player, so the filter belongs on this side.
    ///
    /// # THE SECONDS ARE THE NAME'S OWN, WHICH IS NOT THE SAME AS THE MOB'S OWN
    ///
    /// This said "THE SECONDS ARE THIS TARGET'S OWN and not the chain's", and half of that is
    /// still the point: it is `last_taken_at - first_taken_at` for this entity, not
    /// [`FightRow::secs`], so a clock beside a name is the clock for that name and not for the
    /// eight minute chain around it.
    ///
    /// THE OTHER HALF WAS NEVER TRUE OF A MOB. The engine folds by NAME and says so in its own
    /// module note: nothing in an EverQuest log tells two `a thunder spirit princess` apart, so
    /// both land in one `Fighter` and its two stamps span BOTH engagements with the gap between
    /// them inside. This rule is precisely the one that lets that row through, since its whole
    /// test is that a hit strictly after the last death mark means one of them is up again. So on
    /// a repeat pull of a name the figure returned here is how long the NAME has been under fire
    /// this fight, and the mob standing in front of the reader has been up for less than that.
    /// [`FightRow::deaths_of`] is what says whether that is happening; a caller that shows this
    /// clock without asking is quoting a duration for a mob that is partly a previous mob's.
    pub fn current_target(&self) -> Option<(&str, u32)> {
        /* A CORPSE IS NOT WHAT YOU ARE FIGHTING.
         *
         * `You have slain a thunder spirit princess!` is in the log, the grammar reads it as an
         * `Event::Death`, and the fold already keeps it as a `Mark::Death` with the second it
         * happened in. None of that reached this rule: a mob stayed the newest thing hit until
         * something ELSE was hit, so after a kill the header went on naming the body -- through
         * the looting, through the buff-up, until the next pull landed.
         *
         * THE COMPARISON IS AGAINST THE HIT AND NOT A FLAG, because a folded NAME can die more
         * than once in one fight: this engine groups by name and nothing in the log distinguishes
         * two `a thunder spirit princess` instances (see the module note on mob instances). A
         * `deaths > 0` test would bury the second one for the rest of the chain. Taking damage
         * strictly AFTER the last death mark is the honest reading of "this one is up again".
         *
         * STRICTLY AFTER, and the killing blow is why: the last hit and the death land in the same
         * printed second almost every time, so `>=` would keep every corpse.
         */
        let died_at = |slot: usize| -> Option<u32> {
            self.moments
                .iter()
                .filter_map(|m| match m.what {
                    Mark::Death { victim, .. } if victim == slot => Some(m.at),
                    _ => None,
                })
                .max()
        };
        let (who, _, first, last) = self
            .fighters
            .iter()
            .enumerate()
            .filter(|(_, x)| !x.pet && !self.player(&x.who))
            .filter(|(i, x)| match (died_at(*i), x.last_taken_at) {
                (Some(dead), Some(hit)) => hit > dead,
                (Some(_), None) => false,
                (None, _) => true,
            })
            .map(|(_, x)| x)
            .filter_map(|x| Some((x.who.text(), x.taken, x.first_taken_at?, x.last_taken_at?)))
            /* TIES GO TO DAMAGE, AND GOING TO THE NAME PUT A RAT IN THE HEADER.
             *
             * The log stamps to the second, so in a raid the thing being killed and every add
             * around it share the newest second constantly and the tie-break decides almost
             * every frame. On the name, the alphabet wins: `'A'` is 65 and `'a'` is 97, so a
             * capitalised mob beats every lowercase one. The owner's screen read `IN COMBAT  A
             * REVULTANT RAT  02:02  9 players named` while nine people were killing something
             * else; the rat only had to be clipped once in the newest second.
             *
             * SO THE BIGGEST SOAKER AMONG THE ONES HIT MOST RECENTLY WINS. The name stays as
             * the last resort, because two mobs can tie on both. */
            .max_by(|a, b| {
                a.3.cmp(&b.3)
                    .then_with(|| a.1.cmp(&b.1))
                    .then_with(|| b.0.cmp(a.0))
            })?;
        Some((who, last.saturating_sub(first)))
    }

    /// HOW MANY MOBS OF THIS NAME THIS FIGHT HAS ALREADY BURIED.
    ///
    /// # A FACT ABOUT THE ROW, AND TWO PAGES NEED IT
    ///
    /// It was a private helper on `screens::live`, where it exists because of a measured defect:
    /// the header printed `34,102 of ~20,016`, a numerator counting two mobs against a denominator
    /// measured on one (`hp::read` throws away any fight where a name died twice, exactly so a
    /// reading is one mob's worth). An app that never invents a number can still print one by
    /// division. The Dashboards live tile draws the same subject off the same row and needs the
    /// same test, and a second copy of a rule on a second screen is how those two came to disagree
    /// about a mob in the first place.
    ///
    /// # DEATH MARKS AND NOT [`Fighter::deaths`], AND THEY ANSWER DIFFERENT QUESTIONS
    ///
    /// `Fighter::deaths` is the engine's count for that participant. This walks the `moments`,
    /// which is the timeline the fold kept, and it is the timeline this question needs: the same
    /// marks carry the SECOND each death happened, which is what [`FightRow::current_target`]
    /// compares a hit against to decide that one of them is up again. Both are read off the same
    /// `Mark::Death` events, so they agree; asking the timeline keeps the two rules reading one
    /// source.
    ///
    /// # PLAYERS ARE NOT LOOKED UP, AND THAT IS PART OF THE RULE
    ///
    /// The slot lookup skips anybody `Who::player` calls a person, so a raid death never lands
    /// here. That is deliberate and it is the same filter `current_target` and `last_slain` use:
    /// the question is "how many of that mob have gone down", and a player who shares a mob's name
    /// would otherwise fold his own death into the count and shorten the mob's history. A caller
    /// asking about a PERSON wants `Fighter::deaths`, and gets `0` from here.
    ///
    /// `0` FOR A NAME NOT IN THIS FIGHT, which is the honest answer and not an absence: nothing of
    /// that name has died here, because nothing of that name is here.
    pub fn deaths_of(&self, who: &str) -> usize {
        let Some(slot) = self
            .fighters
            .iter()
            .position(|x| !x.pet && !self.player(&x.who) && x.who.text() == who)
        else {
            return 0;
        };
        self.moments
            .iter()
            .filter(|m| matches!(m.what, Mark::Death { victim, .. } if victim == slot))
            .count()
    }

    /// WHAT THE PLAYERS DID, WHICH IS THE POPULATION EVERY TABLE IN THIS APP RANKS.
    ///
    /// # ON THE ROW, BECAUSE TWO PAGES DREW THE SAME FIGHT AND DISAGREED ABOUT IT
    ///
    /// `screens::analysis` worked this out first and kept it to itself as a private helper, and
    /// `screens::live` then printed `FightRow::damage` in a header six pixels above three tables
    /// built from `dps::ranked_dealers`. On the capture's first fight the header read `16,526
    /// damage` and the panel under it listed the players who dealt 12,976: the missing 3,550 is
    /// the lurking mummy and the skeletons hitting the group, and NO ROW ANYWHERE ON THE PAGE
    /// accounts for it. The share column is computed against 12,976, so the shares add to a
    /// hundred percent of a number twenty-seven percent smaller than the heading above them, and
    /// a reader hunting the difference is hunting rows that were never going to be there.
    ///
    /// SO IT LIVES HERE AND NOT ON A PAGE. Two screens holding two copies of one filter is how
    /// they came to disagree; a method on the row they both read is the version that cannot.
    ///
    /// THE POPULATION IS [`FightRow::ours`], which is the players when the group is not known and
    /// the reader and his group when it is.
    pub fn group_sum(&self, pick: fn(&Fighter) -> u64) -> u64 {
        self.fighters
            .iter()
            .filter(|x| self.on_side(x))
            .map(pick)
            .sum()
    }

    /// DOES THIS FIGHTER BELONG ON THE READER'S ROSTER FOR THIS FIGHT? The one rule every roster,
    /// meter, share denominator and group total in the app asks.
    ///
    /// # THREE WAYS IN, AND A MOB HAS NONE OF THEM
    ///
    ///   * `Who::You`, always. A roster the reader is missing from is not his roster.
    ///   * ANY PLAYER WHEN [`FightRow::group`] IS `None`. Not known is not nobody: every player the
    ///     log named is shown, exactly as every screen did before the field existed.
    ///   * A PLAYER WHOSE NAME IS IN THE GROUP when it is `Some`. `Some(vec![])` is solo, so the
    ///     reader is the whole roster, and that is a measurement and not an empty answer.
    ///
    /// `Who::player` STILL GATES ALL THREE, so a mob that shares a spelling with a member (the
    /// engine never stops a mob being called `Hert`) stays off the meter, and `Who::Unknown` is
    /// nobody's.
    ///
    /// # CASE IGNORED, BECAUSE THE TWO SPELLINGS COME FROM DIFFERENT LINES
    ///
    /// `Who::Named` is spelled by a combat line and a member can be spelled by an invite the reader
    /// TYPED: 28 of the 183 `You invite` lines in the owner's four logs name the invitee in lower
    /// case (`You invite flagg`). The engine folds names with `eq_ignore_ascii_case`, and so does
    /// this. A byte comparison would take `Flagg` off the meter of the group he is in.
    ///
    /// # WHERE IT CAN BE WRONG, SAID OUT LOUD
    ///
    /// A MULTI-WORD CHARMED MOB IS A MOB BY NAME and is off this name-only roster whether the group is
    /// known or not; [`FightRow::on_side`] is the fighter's rule, and keeps it on. A ONE-WORD PET is a player by name
    /// and stays on when the group is not known; when it is known the pet stays only if it is in
    /// [`FightRow::pets`], so a pet of the reader's that never answered him in its session, and
    /// every group member's pet, is off a known roster. A group member someone else
    /// invited and the log never announced is not in `Some`, so he is off the roster of a fight he
    /// was in; `FightRow::group`'s own note says why nothing in the log can catch him.
    ///
    /// NOT FOR MOB LOOKUPS. [`FightRow::current_target`], [`FightRow::last_slain`] and
    /// [`FightRow::deaths_of`] ask `FightRow::player` on purpose: a player outside the group is still
    /// not a mob, and this answering `false` for him must never turn him into one.
    pub fn ours(&self, who: &Who) -> bool {
        on_roster(self.group.as_deref(), &self.pets, &self.foes, who)
    }

    /// [`FightRow::ours`] FOR A FIGHTER, which can also know it is the reader's pet.
    ///
    /// A charmed `a tormented dead` is a mob by name and off every name-only roster, and a hostile
    /// one beside it has the same name, so only the fighter can say which is his: [`Fighter::pet`].
    /// Every roster that holds the fighter asks this.
    pub fn on_side(&self, x: &Fighter) -> bool {
        x.pet || self.ours(&x.who)
    }

    /// IS THIS FIGHTER A PLAYER IN THIS FIGHT? [`Who::player`] overruled by [`FightRow::foes`].
    ///
    /// EVERY "IS IT A PERSON" QUESTION A PAGE ASKS OF A ROW ASKS THIS, and never the name alone: a
    /// roster that asked the name put `Xicotl` on the owner's damage table with a class chip.
    pub fn player(&self, who: &Who) -> bool {
        player_in(&self.foes, who)
    }

    /// The same, for a count rather than an amount.
    ///
    /// `deaths` IS WHY THIS EXISTS SEPARATELY. `FightRow::deaths` counts every `Event::Death`
    /// inside the fight, whoever died, so on a clean camp it is a second copy of the kill count:
    /// the reports page measured 33 against a real 1.
    pub fn group_count(&self, pick: fn(&Fighter) -> u32) -> u32 {
        self.fighters
            .iter()
            .filter(|x| self.on_side(x))
            .map(pick)
            .sum()
    }

    /// HOW MANY PLAYERS ARE IN THIS FIGHT, which is not [`FightRow::participants`].
    ///
    /// A participant is anything that was HIT. The capture's first fight has twenty-two of them
    /// and four are players, so a header reading `22 took part` over a four row table is two
    /// counts of two different populations with nothing on screen saying so.
    ///
    /// THE PLAYERS ON THE READER'S ROSTER, which is [`FightRow::ours`]: every player when the group
    /// is not known, and the reader with his group when it is.
    pub fn players(&self) -> usize {
        self.fighters.iter().filter(|x| self.ours(&x.who)).count()
    }

    /// How many entities took part.
    ///
    /// A METHOD AND NO LONGER A FIELD, WHICH IS THE POINT. It was a `usize` mirrored beside the
    /// fighters it counts, and two mirrors of one fact are two chances to disagree: a fold that
    /// filtered a fighter out, or a screen that pushed one in, would leave a row saying
    /// twenty-three took part above a table of twenty-two. Derived from the vector, it cannot.
    ///
    /// NOT [`FightRow::players`]. A participant is anything that was HIT; the capture's first
    /// fight has twenty-two of them and four are players.
    pub fn participants(&self) -> usize {
        self.fighters.len()
    }
}

/// Why a fight stopped, in the words `grimoire fights` prints.
///
/// EXHAUSTIVE ON PURPOSE, WITH NO `_` ARM. These four strings are duplicated from the CLI, whose
/// own `why` is private to a bin crate and cannot be called from here, and a duplicate is only safe
/// if adding a fifth reason cannot pass silently. Without the wildcard a new [`Ended`] variant is a
/// compile error here rather than a fight that ends "quiet" because that was the nearest arm. A
/// REWORDING in the CLI would still drift with nothing to catch it; the real fix is an `as_words`
/// on `Ended` in grimoire-parse that both callers read, and that is a change to a crate this piece
/// does not touch.
fn ended_words(e: Ended) -> &'static str {
    match e {
        Ended::Killed => "everything died",
        Ended::Quiet => "quiet",
        Ended::Zone => "zoned",
        Ended::Backwards => "the clock stepped back",
        Ended::EndOfLog => "the log stopped",
    }
}

/* ============================================== the planted log, shared == */

/// A real eqlog on disk, and an [`Ingest`] booted off it. Test only.
///
/// IT LIVES HERE AND NOT IN `ingest.rs` BECAUSE THREE MODULES NEED THE SAME LOG. The guard below
/// proves the ingest reaches the engine; `ingest`'s own tests prove the worker folds and `adopt`
/// replaces; `screens::parser` proves the Fights section paints what the ingest found. Three copies
/// of the planting code would drift the day one of them changed the file name.
///
/// NOTHING HERE GOES NEAR `%APPDATA%/eql-grimoire`. That folder holds the owner's real
/// settings.json and his real snapshot. `Settings` is CONSTRUCTED here, never loaded and never
/// saved, and both roots are pointed at a folder this module made, so no code path can read or
/// write his files. `data_root` is set for that reason and not because the roster matters: leaving
/// it `None` sends `Ingest::new` to `data::Snapshot::locate()`, which walks his machine.
#[cfg(test)]
pub(crate) mod probe {
    use crate::ingest::Ingest;
    use crate::settings::Settings;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    /// The only real bytes that exist: 2,385 lines cut from the owner's own 61 MB log, the same
    /// file `grimoire-parse/tests/fight_coverage.rs` measures. Included rather than read at run
    /// time so a test cannot pass because a path resolved to something else. The relative depth is
    /// identical to that test's, which is how it is known to resolve.
    pub(crate) const CAPTURE: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");

    /// The character the planted log's FILE NAME carries, which is the only place a character name
    /// exists: no line of any log states it. `ingest::character_of_log` reads it back out.
    pub(crate) const OWNER: &str = "Reviir";

    /* THREE SYNTHETIC LOGS, AND EACH EXISTS BECAUSE THE CAPTURE CANNOT REACH A RULE.
     *
     * The capture's four fights run 266s, 38s, 61s and 36s, so `Fight::seconds()`'s floor at one
     * second never fires in it and a mirror that recomputed `end - start` would agree with the
     * engine on every byte of it. Nothing in it ties, so the headline tie-break is never exercised
     * either, and no fight in it ends `Backwards`.
     *
     * THE TWO TIE LOGS ARE THE SAME FIGHT IN THE TWO POSSIBLE FILE ORDERS, and they are a pair on
     * purpose. `Fight::headline` breaks a damage tie by name ascending, so it answers `a dry bone
     * skeleton` for both. A recompute written with `max_by_key` (which keeps the LAST maximum)
     * fails only one of them; one written to keep the FIRST maximum fails only the other. One log
     * lets half the wrong implementations through.
     *
     * The third line of each is a combat line whose stamp names a month no build knows. `parse`
     * accepts its SHAPE and `fights::seconds` refuses its VALUE, so the engine counts it unreadable
     * rather than folding 99 points of damage in at an invented time. MEASURED through the shipping
     * CLI, not predicted: `grimoire fights` on these exact bytes prints one fight, 1s, 20 damage, 2
     * combat lines, 3 participants, headline `a dry bone skeleton`, ended `the log stopped`, and
     * "1 lines carried a stamp this build could not read". */

    /// Two named mobs take ten each inside ONE printed second, alphabetically first named first.
    pub(crate) const TIE_ALPHA_FIRST: &str = concat!(
        "[Wed Jul 15 23:16:50 2026] You slash a dry bone skeleton for 10 points of damage.\n",
        "[Wed Jul 15 23:16:50 2026] You slash a lurking mummy for 10 points of damage.\n",
        "[Wed Zzz 15 23:16:50 2026] You slash a dry bone skeleton for 99 points of damage.\n",
    );

    /// The same fight with the two mobs swapped in the file.
    pub(crate) const TIE_ALPHA_LAST: &str = concat!(
        "[Wed Jul 15 23:16:50 2026] You slash a lurking mummy for 10 points of damage.\n",
        "[Wed Jul 15 23:16:50 2026] You slash a dry bone skeleton for 10 points of damage.\n",
        "[Wed Zzz 15 23:16:50 2026] You slash a dry bone skeleton for 99 points of damage.\n",
    );

    /// The second stamp is ten seconds EARLIER than the first: two logs concatenated, or a clock
    /// stepping back over a daylight saving edge. The only way to reach `Ended::Backwards`, which
    /// the capture never does. Measured: two fights, `the clock stepped back` then `the log
    /// stopped`, 1s and 10 damage, then 1s and 7.
    pub(crate) const CLOCK_STEPS_BACK: &str = concat!(
        "[Wed Jul 15 23:16:50 2026] You slash a lurking mummy for 10 points of damage.\n",
        "[Wed Jul 15 23:16:40 2026] You slash a dry bone skeleton for 7 points of damage.\n",
    );

    /// Every tag handed out this run. A REPEATED TAG IS A PANIC AND NOT A RACE, and this tree has
    /// paid for that lesson once already: `screens::parser`'s roster helper keyed its temp folder on
    /// a zone COUNT, so any two tests building rosters of the same size wrote one another's files,
    /// and cargo runs tests as threads of one process so the pid did not separate them. The folder
    /// name here is pid plus tag, which is what makes each run wipe the previous run's folder rather
    /// than accumulate (stale temp dirs filling the disk have shown up in this tree as fake compile
    /// errors), and that self-cleaning is only safe while the tags differ.
    static TAGS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

    /// A fresh, empty Logs folder for this tag, wiped if a previous run left one.
    ///
    /// THE LOGS FOLDER IS NESTED ONE DEEP ON PURPOSE. `ingest::newest_dump` looks for
    /// `*-Inventory.txt` in the Logs folder AND in its parent, which it reads as the game folder. A
    /// Logs folder placed directly in the system temp directory would make the whole of that
    /// directory the game folder, and the scan would read a stranger's dump out of it.
    pub(crate) fn logs_dir(tag: &str) -> PathBuf {
        {
            let mut seen = TAGS.lock().unwrap_or_else(|e| e.into_inner());
            assert!(
                seen.insert(tag.to_owned()),
                "two tests asked for the probe tag {tag:?}; they would share one folder and wipe \
                 each other. Give one of them a different tag."
            );
        }
        let game =
            std::env::temp_dir().join(format!("grimoire-fights-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&game);
        let logs = game.join("Logs");
        std::fs::create_dir_all(&logs).unwrap_or_else(|e| panic!("{}: {e}", logs.display()));
        logs
    }

    /// The same, with `text` written into it as the character's own log.
    ///
    /// THE FILE NAME IS THE WHOLE OF WHAT THE APP KNOWS ABOUT WHO IS PLAYING.
    /// `eqlog_<who>_<where>.txt` is what `ingest::character_of_log` reads, and it is the only input
    /// to the fold that merges `Reviir` and `You` into one participant.
    pub(crate) fn planted(tag: &str, text: &str) -> PathBuf {
        let logs = logs_dir(tag);
        let file = logs.join(format!("eqlog_{OWNER}_freeport.txt"));
        std::fs::write(&file, text).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        logs
    }

    /// An [`Ingest`] pointed at `dir` with its bootstrap already landed.
    ///
    /// `Ingest::new` returns before the worker has read anything, and the scan lands only when a
    /// later `tail()` drains the channel. A test that read `fights()` straight after `new()` would
    /// find it empty on every build, working or broken, and would then be asserting nothing.
    pub(crate) fn booted(dir: &Path) -> Ingest {
        let settings = Settings {
            log_dir: Some(dir.to_path_buf()),
            data_root: Some(dir.to_path_buf()),
            ..Settings::default()
        };
        let mut ing = Ingest::new(&settings);
        /* Ten seconds is not a measurement, it is a ceiling: the real bootstrap over 206 KB is two
         * hundredths of a second. It is here so a broken scan fails with a sentence naming the
         * folder instead of hanging a CI runner. */
        for _ in 0..1_000 {
            let _ = ing.tail();
            if !ing.scanning() {
                return ing;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "the bootstrap never landed for {} in 10s; log folder problem: {:?}",
            dir.display(),
            ing.log_dir_problem()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::probe;
    use super::{
        fighter_of, fold_text, mark_clipped, quiet_window, FightRow, Fighter, Mark, Moment, Who,
    };
    use grimoire_parse::combat::parse;
    use grimoire_parse::fights::{Ended, Fight, Fights, Participant, QUIET_SECONDS};
    use grimoire_parse::group::Party;
    use std::sync::mpsc;

    /// DEFECT: THE ONE ROSTER RULE GETTING ONE OF ITS THREE ANSWERS WRONG.
    ///
    /// Every roster, meter, share and group total in the app asks `on_roster` through
    /// `FightRow::ours`, so each way it can be wrong is a wrong list on every screen at once:
    ///
    ///   * NOT KNOWN READ AS NOBODY: a `None` group dropping a player. Every player stays.
    ///   * A MEMBER DROPPED BY HIS SPELLING: the group spells him as the reader typed the invite
    ///     (28 of 183 invites in the owner's logs are lower case), the combat line capitalises.
    ///   * A STRANGER KEPT: a player the log proved was not in the group.
    ///   * THE READER DROPPED: `Who::You` is never in the group list and is always on the roster.
    ///   * A MOB LET IN: `Who::player` gates all three answers, whatever the group list holds.
    ///
    /// AND THE METHOD IS THE FUNCTION, asked over every kind of fighter, so the two entry points
    /// cannot come to disagree.
    ///
    ///   * THE READER'S OWN PET DROPPED: a one-word pet in [`FightRow::pets`] is his, whatever the
    ///     group says, and nobody else gets in through that list.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `player()` gate; `(_, None) => false`;
    /// `m == n` for `eq_ignore_ascii_case`; dropping the `Who::You` arm; `ours` answering
    /// `who.player()`; `on_roster` ignoring `pets`; `ours` handing `on_roster` an empty pet list.
    #[test]
    fn a_roster_is_the_reader_and_his_group_when_known_and_every_player_when_not() {
        let hert = Who::Named("Hert".to_owned());
        let stranger = Who::Named("Losumyda".to_owned());
        let mob = Who::Named("a dry bone skeleton".to_owned());
        let group = vec!["hert".to_owned()];
        let known = Some(group.as_slice());
        let solo: Option<&[String]> = Some(&[]);

        assert!(
            super::on_roster(known, &[], &[], &Who::You),
            "the reader is off his own roster in a fight whose group the log proved"
        );
        assert!(
            super::on_roster(known, &[], &[], &hert),
            "Hert is in the group, spelled `hert` as the invite typed him, and is off the roster"
        );
        assert!(
            !super::on_roster(known, &[], &[], &stranger),
            "a player the log proved was not in the group is on the reader's roster"
        );
        for who in [&Who::You, &hert, &stranger] {
            assert!(
                super::on_roster(None, &[], &[], who),
                "{who:?} is off a roster whose group is NOT KNOWN, which reads not known as nobody"
            );
        }
        assert!(
            super::on_roster(solo, &[], &[], &Who::You),
            "a solo reader is off his own roster"
        );
        assert!(
            !super::on_roster(solo, &[], &[], &hert),
            "the log proved the reader solo and a player is still on his roster"
        );
        let named_like = vec!["a dry bone skeleton".to_owned()];
        assert!(
            !super::on_roster(Some(named_like.as_slice()), &named_like, &[], &mob),
            "a mob whose name is on the group or pet list reached the roster, so `Who::player` no \
             longer gates the rule"
        );
        /* THE READER'S OWN PET, whose one-word name `Who::player` calls a player. A known group
         * that does not name it must still keep it when the log proved it answers the reader, and
         * the pet list must not become a way in for anybody else. */
        let gabtik = Who::Named("Gabtik".to_owned());
        let pets = vec!["gabtik".to_owned()];
        assert!(
            super::on_roster(solo, &pets, &[], &gabtik),
            "the reader's own pet, proven by its answer and spelled in another case, is off a solo \
             roster"
        );
        assert!(
            super::on_roster(known, &pets, &[], &gabtik),
            "the reader's own pet is off the roster of a group the log proved"
        );
        assert!(
            !super::on_roster(solo, &pets, &[], &stranger),
            "a pet list let a player the log proved was not in the group onto a solo roster"
        );
        assert!(
            !super::on_roster(None, &[], &[], &mob)
                && !super::on_roster(None, &[], &[], &Who::Unknown),
            "a mob or nobody-at-all reached a roster whose group is not known"
        );

        let row = FightRow {
            group: Some(group.clone()),
            pets: pets.clone(),
            ..FightRow::default()
        };
        for who in [&Who::You, &hert, &stranger, &mob, &Who::Unknown, &gabtik] {
            assert_eq!(
                row.ours(who),
                super::on_roster(row.group.as_deref(), &row.pets, &[], who),
                "`FightRow::ours` and `on_roster` disagree about {who:?}"
            );
        }

        /* AND THE THREE FIGURES ON THE ROW THAT EVERY HEADER, TILE AND COLUMN READS. `group_sum`
         * is the Live page's `group damage` and the parser table's `group dmg`, `group_count` is
         * every `player deaths`, `players` is the Analysis `Players` tile. Each was a copy of the
         * `Who::player` filter, and each has to be the roster's rule now or the header over a
         * meter counts people the meter under it does not draw. */
        let who_dealt = |who: &Who, dealt: u64, deaths: u32| Fighter {
            who: who.clone(),
            dealt,
            deaths,
            ..Fighter::default()
        };
        let fought = FightRow {
            group: Some(group.clone()),
            fighters: vec![
                who_dealt(&Who::You, 500, 1),
                who_dealt(&hert, 300, 1),
                who_dealt(&stranger, 200, 1),
                who_dealt(&mob, 900, 3),
            ],
            ..FightRow::default()
        };
        assert_eq!(
            fought.group_sum(|x| x.dealt),
            800,
            "`group_sum` over a known group counted the stranger or the mob"
        );
        assert_eq!(
            fought.group_count(|x| x.deaths),
            2,
            "`group_count` over a known group counted the stranger's death or the mob's"
        );
        assert_eq!(
            fought.players(),
            2,
            "`players` over a known group counted a player outside it"
        );
        let everyone = FightRow {
            group: None,
            ..fought.clone()
        };
        assert_eq!(
            (
                everyone.group_sum(|x| x.dealt),
                everyone.group_count(|x| x.deaths),
                everyone.players()
            ),
            (1_000, 3, 3),
            "a fight whose group is not known dropped a player from its totals"
        );
    }

    /// The end reason in words, WRITTEN OUT A SECOND TIME rather than borrowed from the module under
    /// test. A parity check that called `ended_words` would compare it with itself and pass over any
    /// wording at all, including an empty string. These four are the shipping CLI's, read off
    /// `grimoire-forge/src/fights.rs`, and the match is exhaustive so a fifth variant is a compile
    /// error here too.
    fn words(e: Ended) -> &'static str {
        match e {
            Ended::Killed => "everything died",
            Ended::Quiet => "quiet",
            Ended::Zone => "zoned",
            Ended::Backwards => "the clock stepped back",
            Ended::EndOfLog => "the log stopped",
        }
    }

    /// The engine's own answer over the same bytes, for the parity guard.
    fn engine<'a>(text: &'a str, quiet: u32, owner: Option<&'a str>) -> Vec<Fight<'a>> {
        let mut agg = Fights::new().with_quiet(i64::from(quiet));
        if let Some(n) = owner {
            agg = agg.with_owner(n);
        }
        for raw in text.lines() {
            if let Some(e) = parse(raw) {
                agg.push(e);
            }
        }
        agg.finish()
    }

    /// `at(0, ..)` is 23:16:50, `at(31, ..)` is 31 seconds later. The same helper the engine's own
    /// tests use, so an off-by-one cannot hide in the fixture text.
    fn at(offset: i64, body: &str) -> String {
        let base = 23 * 3600 + 16 * 60 + 50 + offset;
        format!(
            "[Wed Jul 15 {:02}:{:02}:{:02} 2026] {body}",
            base / 3600,
            (base / 60) % 60,
            base % 60
        )
    }

    const HIT: &str = "You slash a dry bone skeleton for 20 points of damage.";

    /// A row reduced to what a reader sees, so a failure prints the whole disagreement rather than
    /// the first field that differs.
    #[derive(PartialEq, Eq, Debug)]
    struct Seen {
        start: String,
        end: String,
        secs: i64,
        damage: u64,
        deaths: u32,
        lines: u32,
        ended: String,
        headline: Option<String>,
        participants: usize,
        cut: bool,
    }

    fn seen(r: &FightRow) -> Seen {
        Seen {
            start: r.start.clone(),
            end: r.end.clone(),
            secs: r.secs,
            damage: r.damage,
            deaths: r.deaths,
            lines: r.lines,
            ended: r.ended.clone(),
            headline: r.headline.clone(),
            participants: r.participants(),
            cut: r.cut,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn want(
        start: &str,
        end: &str,
        secs: i64,
        damage: u64,
        deaths: u32,
        lines: u32,
        ended: &str,
        headline: &str,
        participants: usize,
    ) -> Seen {
        Seen {
            start: start.to_owned(),
            end: end.to_owned(),
            secs,
            damage,
            deaths,
            lines,
            ended: ended.to_owned(),
            headline: Some(headline.to_owned()),
            participants,
            cut: false,
        }
    }

    /// THE FIXTURE, WHOLE. Every number here was printed by `grimoire fights` over these bytes.
    ///
    /// DEFECT: a mirror that drops a fight, reorders them, renumbers them newest-first, or copies
    /// the wrong field into the wrong slot. All four are invisible to a test that only counts rows,
    /// and all four are shippable: the fold is four lines of plumbing and nothing about it fails
    /// loudly.
    #[test]
    fn the_capture_folds_into_the_twelve_encounters_the_engine_reports() {
        let (rows, unreadable) = fold_text(probe::CAPTURE, quiet_window(), None);
        let got: Vec<Seen> = rows.iter().map(seen).collect();
        assert_eq!(
            got,
            vec![
                want(
                    "Wed Jul 15 23:16:50 2026",
                    "Wed Jul 15 23:17:09 2026",
                    19,
                    1_533,
                    3,
                    171,
                    "everything died",
                    "a lurking mummy",
                    10,
                ),
                want(
                    "Wed Jul 15 23:17:10 2026",
                    "Wed Jul 15 23:17:54 2026",
                    44,
                    3_404,
                    7,
                    428,
                    "everything died",
                    "A carrion ghoul",
                    15,
                ),
                want(
                    "Wed Jul 15 23:17:55 2026",
                    "Wed Jul 15 23:18:36 2026",
                    41,
                    3_928,
                    5,
                    370,
                    "everything died",
                    "a lurking mummy",
                    14,
                ),
                want(
                    "Wed Jul 15 23:18:37 2026",
                    "Wed Jul 15 23:19:45 2026",
                    68,
                    3_167,
                    5,
                    286,
                    "everything died",
                    "A dry bone skeleton",
                    12,
                ),
                want(
                    "Wed Jul 15 23:19:47 2026",
                    "Wed Jul 15 23:20:14 2026",
                    27,
                    1_394,
                    2,
                    125,
                    "everything died",
                    "A dry bone skeleton",
                    6,
                ),
                want(
                    "Wed Jul 15 23:20:16 2026",
                    "Wed Jul 15 23:21:05 2026",
                    49,
                    2_793,
                    4,
                    209,
                    "everything died",
                    "A crazed ghoul",
                    9,
                ),
                want(
                    "Wed Jul 15 23:21:06 2026",
                    "Wed Jul 15 23:21:16 2026",
                    10,
                    307,
                    0,
                    35,
                    "quiet",
                    "A crazed ghoul",
                    3,
                ),
                want(
                    "Wed Jul 15 23:24:04 2026",
                    "Wed Jul 15 23:24:06 2026",
                    2,
                    304,
                    1,
                    20,
                    "everything died",
                    "A greater skeleton",
                    3,
                ),
                want(
                    "Wed Jul 15 23:24:14 2026",
                    "Wed Jul 15 23:24:34 2026",
                    20,
                    734,
                    2,
                    48,
                    "everything died",
                    "A tormented dead",
                    6,
                ),
                want(
                    "Wed Jul 15 23:24:38 2026",
                    "Wed Jul 15 23:24:42 2026",
                    4,
                    310,
                    0,
                    19,
                    "zoned",
                    "A dry bone skeleton",
                    3,
                ),
                want(
                    "Wed Jul 15 23:39:44 2026",
                    "Wed Jul 15 23:40:45 2026",
                    61,
                    1_700,
                    1,
                    41,
                    "zoned",
                    "Guard Ullindin",
                    8,
                ),
                want(
                    "Wed Jul 15 23:45:42 2026",
                    "Wed Jul 15 23:46:18 2026",
                    36,
                    121,
                    2,
                    16,
                    "the log stopped",
                    "a skeleton",
                    5,
                ),
            ],
            "the fights the desktop mirrors changed"
        );
        assert_eq!(unreadable, 0, "every stamp in the capture reads");

        /* And the totals reconcile with what the CLI's own left-over accounting prints, so a row
         * silently losing lines or damage cannot pass by keeping the count at four. */
        /* 1,768 AND 19,693, NOT 1,770 AND 19,695. A pull that ends on its kill leaves gaps between
         * pulls, and two lines land in one: the falling-damage line and a 2 point `You hurt
         * yourself` at 23:24:25, neither of which can open a fight of its own. `grimoire-parse`s
         * own `every_point_of_damage_reconciles` names them and proves nothing else moved. */
        assert_eq!(rows.iter().map(|r| u64::from(r.lines)).sum::<u64>(), 1_768);
        assert_eq!(rows.iter().map(|r| r.damage).sum::<u64>(), 19_695);
        assert_eq!(rows.iter().map(|r| r.deaths).sum::<u32>(), 32);
    }

    /// THE PARITY GUARD. The mirror is re-derived from the engine, field by field, over the real
    /// bytes, at six windows and with and without the owner folded.
    ///
    /// DEFECT: any field quietly recomputed in the desktop instead of copied. The two that would
    /// actually be written that way are `secs` (an `end - start` that loses the one second floor)
    /// and `headline` (a `max_by_key` that forks the tie-break), and each of them keeps the fixture
    /// test above green at the default window, because on those four fights every span is longer
    /// than a second and no headline is tied. A hardcoded expectation cannot catch a fork that does
    /// not happen to bite the fixture; asking the engine at a narrower window can.
    ///
    /// `ended` IS COMPARED AGAINST THIS MODULE'S OWN `words`, NOT AGAINST `ended_words`. Comparing
    /// the mirror's mapping with itself would pass over any wording, empty strings included.
    ///
    /// `group` IS THE GROUP ENGINE'S ANSWER, re-derived the same way: a `Party` fed the same bytes
    /// apart from the mirror, asked about each fight's own two stamps. The capture has one group
    /// line, a removal between fights 1 and 2, so the comparison is between `None` and
    /// `Some(vec![])` and not two `None`s, and at the wider windows it covers a run that straddles
    /// the removal.
    #[test]
    fn every_row_field_is_the_engines_own_answer_and_not_a_second_calculation() {
        let mut party = Party::new();
        for raw in probe::CAPTURE.lines() {
            party.push(raw);
        }
        for owner in [None, Some(probe::OWNER)] {
            for quiet in [1u32, 5, 28, quiet_window(), 168, 400] {
                let (rows, unreadable) = fold_text(probe::CAPTURE, quiet, owner);
                let truth = engine(probe::CAPTURE, quiet, owner);
                assert_eq!(
                    rows.len(),
                    truth.len(),
                    "quiet={quiet} owner={owner:?}: the mirror lost or invented a fight"
                );
                for (r, f) in rows.iter().zip(truth.iter()) {
                    let at = format!("quiet={quiet} owner={owner:?} fight [{}]", f.start);
                    assert_eq!(r.start, f.start, "{at}: start");
                    assert_eq!(r.end, f.end, "{at}: end");
                    assert_eq!(
                        r.secs,
                        f.seconds(),
                        "{at}: secs is not Fight::seconds(), so the one second floor is gone"
                    );
                    assert_eq!(r.damage, f.damage, "{at}: damage");
                    assert_eq!(r.deaths, f.deaths, "{at}: deaths");
                    assert_eq!(r.lines, f.lines, "{at}: lines");
                    assert_eq!(r.ended, words(f.ended), "{at}: ended");
                    assert_eq!(
                        r.headline.as_deref(),
                        f.headline(),
                        "{at}: headline is not Fight::headline(), so its tie-break has forked"
                    );
                    assert_eq!(r.participants(), f.participants.len(), "{at}: participants");
                    /* THE FIGHTERS THEMSELVES, FIELD BY FIELD. The count agreeing proves only that
                     * the vector is the right LENGTH; a mirror that put `taken` where `dealt`
                     * belongs, or dropped the owner fold, has the right length and the wrong
                     * numbers, and every overlay built on it would be confidently wrong. */
                    let want: Vec<Fighter> = f.participants.iter().map(fighter_of).collect();
                    assert_eq!(r.fighters, want, "{at}: fighters");
                    assert_eq!(
                        r.fighters.iter().map(|x| x.dealt).sum::<u64>(),
                        f.damage,
                        "{at}: the fighters' dealt does not add up to the fight's own damage, so a \
                         row is missing from the mirror or one is counted twice"
                    );
                    assert!(!r.cut, "{at}: fold_text cannot know a tail was clipped");
                    assert_eq!(
                        r.group,
                        party.during(f.start, f.end),
                        "{at}: group is not the party's own answer over the same bytes"
                    );
                }
                assert_eq!(unreadable, 0, "quiet={quiet}: the capture grew a bad stamp");
            }
        }
    }

    /// The reader forms his own group, in the shape the owner's logs print it: the invite, then
    /// `You have joined`, `You are now the leader` and the member's join all on one second (58
    /// times across the four logs; see `grimoire_parse::group::Party`). Zarmin is the one member.
    fn zarmin_joins(t: i64) -> Vec<String> {
        vec![
            at(t, "You invite Zarmin to join your group."),
            at(t + 1, "You have joined the group."),
            at(t + 1, "You are now the leader of your group."),
            at(t + 1, "Zarmin has joined the group."),
        ]
    }

    fn log_of(lines: &[String]) -> String {
        let mut s = lines.join("\n");
        s.push('\n');
        s
    }

    /// DEFECT: THE FOLD NEVER SHOWS THE PARTY A GROUP LINE, SO EVERY ROW IS `None` FOR EVER.
    ///
    /// A fold that leaves the push out, or feeds the party only the lines the combat grammar reads
    /// as combat (no group line is one), compiles, passes every test that folds a log with no group
    /// in it, and stamps `None` on every row the app will ever make. `None` is a real answer, which
    /// is what makes that failure invisible: it reads as "the group was not known", on every fight,
    /// including the ones the log watched form.
    ///
    /// WHAT MUTATION MAKES THIS RED: the `party.push(raw)` line deleted; `row_of` stamping `None`
    /// rather than asking the party. WHAT DOES NOT: moving the push inside `if let Some(entry)`,
    /// because `combat::parse` returns an entry for every stamped line and that move changes nothing.
    #[test]
    fn a_fight_in_a_group_the_log_watched_form_carries_its_members() {
        let mut lines = zarmin_joins(0);
        lines.push(at(5, HIT));
        lines.push(at(
            8,
            "Zarmin hits a dry bone skeleton for 10 points of damage.",
        ));
        let (rows, _) = fold_text(&log_of(&lines), quiet_window(), Some(probe::OWNER));
        assert_eq!(rows.len(), 1, "one pull");
        assert!(
            rows[0]
                .fighters
                .iter()
                .any(|x| x.who == Who::Named("Zarmin".into())),
            "the fixture's member line did not fold as combat, so the fight is not the one this is \
             about"
        );
        assert_eq!(
            rows[0].group,
            Some(vec!["Zarmin".to_owned()]),
            "a fight inside a group the log watched form does not name its one member, so the fold \
             never asked the party or never showed it the group lines"
        );
    }

    /// DEFECT: A GROUP THE READER JOINED REPORTED AS KNOWN, OR NOT KNOWN REPORTED AS SOLO.
    ///
    /// Joining somebody else's group names the inviter and nobody else. 79 of the owner's 140
    /// joins follow a `You notify` line like this one, and the rest of that group is never listed,
    /// so the membership is partial and the answer is `None`. Bada is in the fight and nothing
    /// says whether he is in the group, which is the whole point.
    ///
    /// The two ways the desktop gets this wrong are each one line in `row_of`: filling the field
    /// from the party's current member list when `during` says not known, or
    /// `unwrap_or_default()` on the answer, which makes every not known fight solo and hides
    /// everybody in it. The second is this app's cardinal sin, "unknown" drawn as "nobody".
    ///
    /// WHAT MUTATION MAKES THIS RED: `Some(party.during(..).unwrap_or_default())` in `row_of`.
    #[test]
    fn a_fight_after_joining_somebody_elses_group_is_not_known() {
        let lines = vec![
            at(0, "Hert invites you to join a group."),
            at(
                0,
                "To join the group, click on the 'FOLLOW' option, or 'DECLINE' to cancel.",
            ),
            at(1, "You notify Hert that you agree to join the group."),
            at(1, "You have joined the group."),
            at(5, HIT),
            at(8, "Bada hits a dry bone skeleton for 10 points of damage."),
        ];
        let (rows, _) = fold_text(&log_of(&lines), quiet_window(), Some(probe::OWNER));
        assert_eq!(rows.len(), 1, "one pull");
        assert_eq!(
            rows[0].group, None,
            "joining Hert's group names only Hert; who else is in it is not in these bytes, so \
             this fight's group is not known and must be neither a list nor solo"
        );
    }

    /// DEFECT: SOLO TREATED AS NOT KNOWN, OR A GROUP OUTLIVING ITS OWN REMOVAL.
    ///
    /// `You have been removed from the group.` is 133 lines in the owner's logs, and after it he is
    /// provably alone. `Some(vec![])` is that measurement and it is not `None`: a filter shows
    /// every player on `None` and only the reader on `Some(vec![])`. A mirror that tidies an empty
    /// list into `None` throws the measurement away; one that stamps the group as it stood at
    /// formation keeps Zarmin on a meter he has left.
    ///
    /// WHAT MUTATION MAKES THIS RED: `.filter(|g| !g.is_empty())` on the answer in `row_of`.
    #[test]
    fn a_fight_after_removal_from_the_group_is_known_solo_and_not_unknown() {
        let mut lines = zarmin_joins(0);
        lines.push(at(5, HIT));
        lines.push(at(50, "You have been removed from the group."));
        lines.push(at(100, HIT));
        let (rows, _) = fold_text(&log_of(&lines), quiet_window(), Some(probe::OWNER));
        assert_eq!(
            rows.len(),
            2,
            "the removal sits in a quiet gap between two pulls"
        );
        assert_eq!(
            rows[0].group,
            Some(vec!["Zarmin".to_owned()]),
            "the pull before the removal was in the group"
        );
        assert_eq!(
            rows[1].group,
            Some(Vec::new()),
            "after `You have been removed from the group.` the reader is provably alone, and this \
             fight says otherwise"
        );
    }

    /// DEFECT: THE READER'S OWN PET TAKEN OFF HIS METER THE MOMENT THE GROUP IS KNOWN.
    ///
    /// A summoned pet's name is one word, so `Who::player` calls it a player, and a roster of the
    /// reader and his group left it off: 63 known fights and 122,294 damage of the reader's pets
    /// over the owner's four logs, the largest `Vibartik` at 46,544. On Jul 18 16:29 neriak, known
    /// solo, `Gabtik`'s 9,493 was gone and the meter showed the reader alone. The pet says so
    /// itself (`Following you, Master.`), and a stranger beside them says nothing of the kind.
    ///
    /// WHAT MUTATION MAKES THIS RED: `row_of` stamping no pets; `FightRow::ours` handing `on_roster`
    /// an empty pet list.
    #[test]
    fn a_known_solo_fight_keeps_the_readers_own_pet_and_drops_the_stranger() {
        let lines = vec![
            at(0, "You have been removed from the group."),
            at(2, "Gabtik says, 'Following you, Master.'"),
            at(5, HIT),
            at(
                6,
                "Gabtik hits a dry bone skeleton for 40 points of damage.",
            ),
            at(
                7,
                "Losumyda hits a dry bone skeleton for 25 points of damage.",
            ),
        ];
        let (rows, _) = fold_text(&log_of(&lines), quiet_window(), Some(probe::OWNER));
        assert_eq!(rows.len(), 1, "one pull");
        assert_eq!(
            rows[0].group,
            Some(Vec::new()),
            "the pull after the removal is not known solo, so this is not the case under test"
        );
        assert_eq!(
            rows[0].pets,
            vec!["Gabtik".to_owned()],
            "the fold did not stamp the pet the party proved answers the reader"
        );
        assert_eq!(
            rows[0].group_sum(|x| x.dealt),
            60,
            "the solo reader's roster is his 20 and his pet Gabtik's 40; the pet was dropped, or \
             the stranger Losumyda's 25 was kept"
        );
    }

    /// DEFECT: A NAMED MOB WITH A ONE-WORD NAME WAS ON THE READER'S ROSTER AS A PLAYER.
    ///
    /// The owner's damage table listed `Xicotl` as a Druid and his timeline flagged its death as a
    /// person's. Driven from log lines of the shapes his own log prints: the mob punches him and he
    /// slashes it, while a real player beside him only hits a mob.
    ///
    /// WHAT MUTATION MAKES THIS RED: `row_of` not stamping `foes`; `on_roster` asking the name
    /// instead of `player_in`; `proven_foes` ignoring damage the reader dealt.
    #[test]
    fn a_one_word_mob_that_trades_blows_with_the_reader_is_not_a_player() {
        let text = [
            "[Mon Sep 07 16:11:53 2026] Xicotl punches YOU for 30 points of damage.",
            "[Mon Sep 07 16:11:54 2026] You slash Xicotl for 40 points of damage.",
            "[Mon Sep 07 16:11:55 2026] Losumyda hits a dry bone skeleton for 10 points of damage.",
            "[Mon Sep 07 16:11:56 2026] You slash a dry bone skeleton for 20 points of damage.",
        ]
        .join("\n");
        let (rows, _) = fold_text(&text, 30, None);
        assert_eq!(rows.len(), 1, "the fixture is not one fight");
        let r = &rows[0];
        let xicotl = Who::Named("Xicotl".to_owned());
        let losumyda = Who::Named("Losumyda".to_owned());
        assert!(
            r.fighters.iter().any(|x| x.who == xicotl)
                && r.fighters.iter().any(|x| x.who == losumyda),
            "the fixture's lines did not fold into the fighters it is about: {:?}",
            r.fighters.iter().map(|x| x.who.text()).collect::<Vec<_>>()
        );
        assert_eq!(
            r.foes,
            vec!["Xicotl".to_owned()],
            "the mob that fought the reader is not a proven foe"
        );
        assert!(
            !r.player(&xicotl),
            "a mob that punched the reader is still a player"
        );
        assert!(
            !r.ours(&xicotl),
            "a mob that punched the reader is on his roster"
        );
        assert!(
            r.ours(&losumyda),
            "a player who only hit a mob was taken off a roster whose group is not known"
        );
    }

    /// WHO THE READER'S SIDE IS, AND WHAT COUNTS AS PROOF. See [`proven_foes`].
    ///
    /// WHAT MUTATION MAKES THIS RED: a group member or a pet that can be a foe; the group used when
    /// it is not known; damage to a mob counted as proof.
    #[test]
    fn a_foe_is_proven_by_damage_to_or_from_the_readers_side_and_nothing_else() {
        let hit = |slot: usize, amount: u64| crate::fights::TargetShare {
            slot,
            amount,
            hits: 1,
        };
        let named = |n: &str, targets: Vec<crate::fights::TargetShare>| Fighter {
            who: Who::Named(n.to_owned()),
            targets,
            ..Fighter::default()
        };
        let fighters = vec![
            Fighter {
                who: Who::You,
                targets: vec![hit(7, 40)],
                ..Fighter::default()
            },
            /* A GROUP MEMBER WHOSE DAMAGE LANDED ON THE READER, which cannot happen, and he is still
             * never a foe: the side is decided before the evidence is read. */
            named("Hert", vec![hit(0, 1)]),
            named("Jebobab", vec![]),
            named("Gearheart", vec![hit(1, 50)]),
            named("Enynti", vec![hit(2, 10)]),
            named("Kinettic", vec![hit(6, 99)]),
            named("a dry bone skeleton", vec![hit(0, 5)]),
            named("Ssynthi", vec![]),
        ];
        let group = vec!["Hert".to_owned()];
        let pets = vec!["Jebobab".to_owned()];
        assert_eq!(
            crate::fights::proven_foes(&fighters, Some(&group), &pets),
            vec!["Gearheart".to_owned(), "Enynti".to_owned(), "Ssynthi".to_owned()],
            "with the group known: the mob that hit a member, the mob that hit the pet and the mob \
             the reader hit, and nobody else"
        );
        assert_eq!(
            crate::fights::proven_foes(&fighters, None, &pets),
            vec!["Hert".to_owned(), "Enynti".to_owned(), "Ssynthi".to_owned()],
            "with the group not known, Gearheart hitting Hert proves nothing (Hert is not proven the reader's side) while Hert's own damage on the reader is evidence like anybody's"
        );
    }

    /// THE CAPTURE'S ONE GROUP LINE, ON REAL BYTES, AT THE WINDOW THE APP USES AND AT ONE THAT
    /// MERGES ACROSS IT.
    ///
    /// Line 1942 of the fixture is `[Wed Jul 15 23:21:54 2026] You have been removed from the
    /// group.`, and it is the only group line in the file. Fight 1 ends 38 seconds before it, so
    /// nothing in the bytes says who was with the reader then: `None`. Fights 2, 3 and 4 come after
    /// it and are solo, and fight 2 is the case a group filter is for: Fylasem and Poguhy, his
    /// group mates for the whole of fight 1, are still swinging at a dry bone skeleton two minutes
    /// after he left them.
    ///
    /// AT A WINDOW OF 168 FIGHTS 1 AND 2 ARE ONE RUN (`the_quiet_window_reaches_the_aggregator`),
    /// and that run opened five minutes BEFORE the removal, while he was still in the group. It
    /// is not solo and it is not known. A party that answers from the first line it saw onward and
    /// forgets that nothing was known before that line calls it solo, and a filter would take
    /// Fylasem and Poguhy off the meter for the half of the run they spent in his group.
    ///
    /// WHAT MUTATION MAKES THIS RED: `unwrap_or_default()` on the answer (fight 1 becomes solo);
    /// the `party.push(raw)` line deleted (fights 2 to 4 become not known); a `Party::during`
    /// that counts a span as covered when the query starts before the party's first span.
    #[test]
    fn the_captures_fights_are_not_known_before_its_removal_line_and_solo_after_it() {
        let groups = |quiet: u32| -> Vec<Option<Vec<String>>> {
            fold_text(probe::CAPTURE, quiet, Some(probe::OWNER))
                .0
                .into_iter()
                .map(|r| r.group)
                .collect()
        };
        assert_eq!(
            groups(quiet_window()),
            {
                // Seven pulls before the removal line, five after it.
                let mut v = vec![None; 7];
                v.extend(std::iter::repeat_n(Some(Vec::new()), 5));
                v
            },
            "the capture's one group line is a removal between fights 1 and 2"
        );
        let merged = groups(168);
        assert_eq!(
            merged.len(),
            11,
            "168 merges the pulls a walk between zones separates"
        );
        assert_eq!(
            merged[0], None,
            "one run that opened five minutes before `You have been removed from the group.`, \
             while Fylasem and Poguhy were in the reader's group, was reported as {:?}",
            merged[0]
        );
        /* SEVEN MORE PULLS BEFORE THE REMOVAL LINE AT THIS WIDER WINDOW, then the two after it. The
         * removal is one line in the middle of a camp, so which side of it a pull falls on moves
         * with the boundary, and 168 still swallows the walk between zones that 30 does not. */
        assert!(merged[1..7].iter().all(Option::is_none));
        assert_eq!(
            merged[7..],
            [
                Some(Vec::new()),
                Some(Vec::new()),
                Some(Vec::new()),
                Some(Vec::new())
            ]
        );
    }

    /// DEFECT: `secs` recomputed as `seconds(end) - seconds(start)`.
    ///
    /// Called out on its own because the sweep proves AGREEMENT and this proves the VALUE, on the
    /// one shape where the two calculations differ. Thirty-two lines in the real capture share a
    /// single printed second; a fight that opens and closes inside one has a duration these bytes
    /// cannot express, and reporting 0 says it took no time at all.
    #[test]
    fn a_fight_inside_one_printed_second_lasts_one_second_and_not_zero() {
        let log = format!("{}\n{}\n", at(0, HIT), at(0, HIT));
        let (rows, _) = fold_text(&log, quiet_window(), None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].start, rows[0].end, "both lines share one stamp");
        assert_eq!(
            rows[0].secs, 1,
            "a span the log cannot express must floor at one second, never zero"
        );
        assert_eq!(rows[0].damage, 40);
    }

    /// DEFECT: `headline` recomputed with the obvious `max_by_key`, which forks the tie-break.
    ///
    /// BOTH ORDERS, AND THAT IS THE WHOLE TEST. The engine breaks a damage tie by NAME, ascending,
    /// precisely so the answer cannot depend on the order participants happened to appear in the
    /// file, so a single ordering proves nothing: `max_by_key` returns the LAST maximum and a
    /// hand-rolled loop usually keeps the FIRST, and each of those agrees with the engine on one of
    /// these two logs. `Alpha` is the answer to both, and a screen naming `Zeta` for one of them
    /// looks entirely reasonable. Measured through the CLI on these exact bytes.
    #[test]
    fn a_tie_on_damage_names_the_same_mob_whichever_order_the_log_wrote_them_in() {
        for (first, second) in [("Zeta", "Alpha"), ("Alpha", "Zeta")] {
            let log = format!(
                "{}\n{}\n",
                at(0, &format!("You slash {first} for 10 points of damage.")),
                at(1, &format!("You slash {second} for 10 points of damage."))
            );
            let (rows, _) = fold_text(&log, quiet_window(), None);
            assert_eq!(rows.len(), 1);
            let truth = engine(&log, quiet_window(), None);
            let taken: Vec<(Option<&str>, u64)> = truth[0]
                .participants
                .iter()
                .map(|p: &Participant<'_>| (p.name(), p.taken))
                .collect();
            assert!(
                taken.contains(&(Some("Zeta"), 10)) && taken.contains(&(Some("Alpha"), 10)),
                "the tie this test needs did not happen: {taken:?}"
            );
            assert_eq!(
                rows[0].headline.as_deref(),
                Some("Alpha"),
                "{first} was written first and moved the headline"
            );
            assert_eq!(rows[0].headline.as_deref(), truth[0].headline());
        }
    }

    /// DEFECT: counting unreadable stamps here instead of asking the aggregator.
    ///
    /// `parse` accepts the SHAPE of a stamp and `seconds` reads its VALUE, and they are not the same
    /// check: `Wed Zzz 15 ..` clears the first and fails the second. A line like that folds into no
    /// fight, so a desktop that reported zero here would delete combat from the screen with a
    /// confident 0 beside it. The count has to come from the one place that knows.
    #[test]
    fn a_stamp_the_engine_cannot_read_is_reported_and_not_silently_dropped() {
        let log = "[Wed Zzz 15 23:16:50 2026] You slash a dry bone skeleton for 20 points of \
                   damage.\n";
        let (rows, unreadable) = fold_text(log, quiet_window(), None);
        assert!(rows.is_empty(), "a line with no clock cannot open a fight");
        assert_eq!(unreadable, 1);
    }

    /// DEFECT: every end reason the capture cannot reach, and the unreadable count left at a hard
    /// zero.
    ///
    /// `Ended` has four variants and the capture reaches three. `Ended::Backwards` is what happens
    /// when the next stamp is EARLIER than the one before it, and a mirror arm that is never
    /// executed can carry any string at all, an empty one included, and stay green everywhere else
    /// in this file. The second half is the other return value of the contract and the easiest thing
    /// in it to hardcode: a mirror returning 0 says the log was fully understood when it was not,
    /// and this tree's rule is that a parser which cannot say what it failed to read is guessing.
    #[test]
    fn the_mirror_carries_every_end_reason_and_the_unreadable_count() {
        let (back, unreadable) = fold_text(probe::CLOCK_STEPS_BACK, quiet_window(), None);
        assert_eq!(
            back.iter().map(|r| r.ended.as_str()).collect::<Vec<_>>(),
            vec!["the clock stepped back", "the log stopped"],
            "a stamp earlier than the one before it cuts the fight, and the reader is entitled to \
             be told which of the four reasons it was"
        );
        assert_eq!(unreadable, 0);

        /* All four reasons produced, so no arm of the mapping is unexercised. Quiet and Zone come
         * from the capture, which is the only place either is reachable from real bytes. */
        let (capture, _) = fold_text(probe::CAPTURE, quiet_window(), None);
        let mut said: Vec<&str> = capture
            .iter()
            .chain(back.iter())
            .map(|r| r.ended.as_str())
            .collect();
        said.sort_unstable();
        said.dedup();
        let mut every = [
            words(Ended::Quiet),
            words(Ended::Zone),
            words(Ended::Backwards),
            words(Ended::EndOfLog),
            words(Ended::Killed),
        ];
        every.sort_unstable();
        assert_eq!(
            said,
            every.to_vec(),
            "one of the four end reasons is never produced by the desktop, so its wording is \
             untested and could be anything"
        );

        let (rows, one) = fold_text(probe::TIE_ALPHA_FIRST, quiet_window(), None);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            one, 1,
            "the third line of that log is 99 points of damage under a month name no build knows. \
             It must be reported as unread, not folded in and not silently dropped."
        );
        assert_eq!(rows[0].damage, 20, "and the 99 stayed out of the total");
        assert_eq!(
            rows[0].headline.as_deref(),
            Some("a dry bone skeleton"),
            "the tie is broken by name whichever order the file wrote them in"
        );
        let (swapped, _) = fold_text(probe::TIE_ALPHA_LAST, quiet_window(), None);

        /* WHAT THIS PAIR ACTUALLY PROVES, NARROWED ON PURPOSE ONCE IT COULD NO LONGER BE `rows ==
         * swapped`.
         *
         * It used to compare the whole rows, which held only a participant COUNT and so could not
         * tell the two orders apart. `fighters` can: `Fight::participants` is in FIRST-APPEARANCE
         * order, so swapping the two lines swaps two entries in that vector, and the whole-row
         * comparison started failing the moment the mirror got honest about what the engine
         * returns.
         *
         * THAT IS THE ENGINE'S ORDER AND NOT A DEFECT IN THE MIRROR, and it is not silently sorted
         * away here: sorting in the mirror would make the desktop and the CLI print one fight in
         * two orders off one set of bytes (see `FightRow::fighters`). What must not depend on the
         * file's order are the ANSWERS, so every answer is compared and the vector's order is
         * asserted to be exactly the thing that moved.
         *
         * A SCREEN THAT RANKS THESE ROWS OWNS ITS OWN TIE-BREAK, which is why
         * `screens::dps::equal_dealers_rank_by_name_whichever_order_the_fold_built_them_in` drives
         * both of these orders through the sort a reader actually sees. */
        assert_eq!(swapped.len(), rows.len(), "the cut moved");
        let answers = |r: &FightRow| {
            (
                r.damage,
                r.deaths,
                r.lines,
                r.secs,
                r.ended.clone(),
                r.headline.clone(),
                r.participants(),
            )
        };
        assert_eq!(
            answers(&swapped[0]),
            answers(&rows[0]),
            "swapping the two lines moved an answer"
        );

        /* And every fighter is still there with the same numbers, in a different order. */
        let sorted = |r: &FightRow| {
            let mut v = r.fighters.clone();
            v.sort_by(|a, b| a.who.text().cmp(b.who.text()));
            v
        };
        assert_eq!(
            sorted(&swapped[0]),
            sorted(&rows[0]),
            "swapping the two lines changed WHAT a fighter did, not just where he sits"
        );
        assert_ne!(
            swapped[0].fighters, rows[0].fighters,
            "first-appearance order is the engine's, and if it has stopped moving with the file \
             then this mirror has quietly started sorting and the CLI no longer agrees with it"
        );
    }

    /// DEFECT: `owner` accepted and thrown away.
    ///
    /// The signature would still compile, the four fights would still be four, every damage total
    /// would still be right, and the reader would appear twice in every participant count for ever.
    /// The count is the one field this piece carries that moves when the fold happens, and it moves
    /// by exactly the single row the fold removes. Measured: `--me Reviir` takes fight #1 from 23
    /// participant rows to 22 and leaves the other three untouched.
    #[test]
    fn naming_the_owner_folds_his_two_names_into_one_participant() {
        let (without, _) = fold_text(probe::CAPTURE, quiet_window(), None);
        let (with, _) = fold_text(probe::CAPTURE, quiet_window(), Some(probe::OWNER));
        assert_eq!(with.len(), without.len(), "folding a name moved a boundary");
        assert_eq!(
            without.iter().map(|r| r.participants()).collect::<Vec<_>>(),
            vec![10, 15, 14, 12, 6, 9, 3, 3, 6, 3, 8, 5],
            "with no owner given, Reviir is his own row, which is the honest answer"
        );
        assert_eq!(
            with.iter().map(|r| r.participants()).collect::<Vec<_>>(),
            vec![9, 14, 14, 12, 6, 9, 3, 3, 6, 3, 8, 5],
            "naming the owner must fold his row away"
        );
        /* And it folds a row rather than losing damage with it. */
        assert_eq!(
            with.iter().map(|r| r.damage).sum::<u64>(),
            without.iter().map(|r| r.damage).sum::<u64>()
        );
        /* An empty name is refused rather than folding every unnamed actor into the reader. */
        let (empty, _) = fold_text(probe::CAPTURE, quiet_window(), Some(""));
        assert_eq!(
            empty.iter().map(|r| r.participants()).collect::<Vec<_>>(),
            vec![10, 15, 14, 12, 6, 9, 3, 3, 6, 3, 8, 5]
        );
    }

    /// DEFECT: `quiet` accepted and thrown away, so the desktop always cuts at the default.
    ///
    /// A settings control for the window would appear to do nothing, and worse, the CLI and the
    /// screen would disagree about the same log with no way to see why. The three windows are the
    /// engine.s own plateau map, re-measured since `Ended::Killed` cuts each pull at its kill: 28
    /// splits a camp into sixteen, 30 sits in the hole at fifteen, 168 swallows a walk between zones
    /// down to eleven.
    #[test]
    fn the_quiet_window_reaches_the_aggregator() {
        assert_eq!(fold_text(probe::CAPTURE, 28, None).0.len(), 13);
        assert_eq!(fold_text(probe::CAPTURE, quiet_window(), None).0.len(), 12);
        assert_eq!(fold_text(probe::CAPTURE, 168, None).0.len(), 11);
    }

    /// DEFECT: the desktop's window drifting from, or silently wrapping, the engine's measured one.
    ///
    /// [`quiet_window`] converts rather than restating, and its fallback arm saturates at
    /// `u32::MAX`. If [`QUIET_SECONDS`] were ever retuned negative (which `with_quiet` clamps to
    /// zero, cutting a fight at every line) or past `u32::MAX`, the conversion would silently hand
    /// the screen a window that cuts the same log the CLI reads into a different number of fights,
    /// and both would look right. This is the only thing in the desktop that would notice.
    #[test]
    fn the_desktop_window_is_the_engines_measured_window() {
        assert!(
            QUIET_SECONDS > 0 && QUIET_SECONDS <= i64::from(u32::MAX),
            "QUIET_SECONDS is {QUIET_SECONDS}, which does not survive the trip through the \
             desktop's u32 window"
        );
        assert_eq!(i64::from(quiet_window()), QUIET_SECONDS);
    }

    /// DEFECT: an empty or combat-free log taking a different path from a real one.
    ///
    /// Both happen on the first launch of every install: the app reads whatever log is newest, and a
    /// fresh character's log is chat and zone lines. An empty list is the answer, and the unreadable
    /// count is 0 because nothing was unreadable, not because nothing was checked. A zone line CUTS
    /// a fight but never opens one; casts, auto-attack toggles and target changes are graded Idle
    /// and touch nothing.
    #[test]
    fn a_log_with_no_combat_in_it_yields_no_fights_rather_than_a_surprise() {
        for text in [
            "",
            "\n\n\n",
            "not a log line at all\n",
            "[Wed Jul 15 23:16:50 2026] You have entered West Freeport.\n\
             [Wed Jul 15 23:16:51 2026] Auto attack is on.\n\
             [Wed Jul 15 23:16:52 2026] You begin casting Snails Healing.\n",
        ] {
            let (rows, unreadable) = fold_text(text, quiet_window(), None);
            assert!(rows.is_empty(), "invented a fight out of {text:?}");
            assert_eq!(unreadable, 0);
        }
    }

    /// DEFECT: the clipped warning drawn on every row, or on none, or left stale across a re-fold.
    ///
    /// Only the oldest fight can have lost its head to the tail cap; the rest opened inside bytes
    /// this app actually read and their numbers are exact. Flagging all of them tells a reader that
    /// four exact fights are estimates. The order this depends on is `Fights::finish`'s: oldest
    /// first, which the fixture test above pins.
    #[test]
    fn only_the_oldest_row_carries_the_tail_cap_warning() {
        let (mut rows, _) = fold_text(probe::CAPTURE, quiet_window(), None);
        assert!(rows.iter().all(|r| !r.cut), "the fold cannot know this");

        mark_clipped(&mut rows, true);
        assert!(rows[0].cut);
        assert!(
            rows[1..].iter().all(|r| !r.cut),
            "a fight that opened inside the bytes we read is not a floor"
        );

        /* A re-read that reached byte zero must clear it rather than leave the old warning up. */
        mark_clipped(&mut rows, false);
        assert!(rows.iter().all(|r| !r.cut));

        /* And an empty fold is not a panic. */
        let mut none: Vec<FightRow> = Vec::new();
        mark_clipped(&mut none, true);
        assert!(none.is_empty());
    }

    /// DEFECT: A MOB IN THE DAMAGE METER, OR A PLAYER MISSING FROM IT.
    ///
    /// THE NAMES ARE THE CAPTURE'S OWN, not names invented to suit the rule. Every entity that
    /// dealt damage in `probe::CAPTURE`, as the shipping CLI prints them, sorted into the two
    /// answers a reader would give. If `Who::player` ever disagrees with this list it is wrong
    /// about real bytes, not about a hypothetical.
    ///
    /// WHAT MUTATION MAKES THIS RED: an article test (`starts_with("a ")`) instead of the space
    /// test. Qeynos's guards carry no article and killed the reader in fight #3.
    #[test]
    fn the_players_in_the_capture_are_the_ones_without_a_space_in_their_name() {
        /* The six that dealt damage and are people. */
        for n in [
            "Fylasem", "Losumyda", "Poguhy", "Rykabe", "Tanefi", "Tanefilo",
        ] {
            assert!(
                Who::Named(n.to_owned()).player(),
                "{n} is a player in the owner's own group and the meter must show them"
            );
        }

        /* And everything else that swung. Articles in both cases, titles, and a mob's pet. */
        for n in [
            "a dry bone skeleton",
            "A crazed ghoul",
            "An undead brewer",
            "a large spider",
            "a skeleton",
            "A dark boned skeleton",
            "a lurking mummy",
            "A jack o lantern",
            "Guard Ullindin",
            "Guard Sheg",
            "Guard V`Lex",
            "Torklar Battlemaster",
            "Trolon Lightleer",
            "Reclusive ghoul magus",
            "Reclusive ghoul magus pet",
        ] {
            assert!(
                !Who::Named(n.to_owned()).player(),
                "{n} is not a player and must stay out of the damage meter"
            );
        }

        /* THE READER IS ALWAYS A PLAYER and nobody-at-all never is. `Actor::You` is what
         * `with_owner` folds his character name into, so this is the arm that carries him. */
        assert!(Who::You.player());
        assert!(!Who::Unknown.player());
    }

    /// THE READER'S CHARMED PET IS HIS, APART FROM THE HOSTILE MOB OF ITS NAME, THROUGH THE WHOLE FOLD.
    ///
    /// The owner's own Sep 11 lines: his charm lands, the pet fights, the charm wears off and a mob of
    /// the same name turns on him seconds later.
    ///
    /// WHAT MUTATION MAKES THIS RED: the fold not handing the charm over; the fighter not copying
    /// `pet`; a group sum not counting the pet; the current target being the pet.
    #[test]
    fn the_readers_charmed_pet_is_on_his_side_and_the_mob_after_it_is_not() {
        let text = [
            "[Fri Sep 11 03:30:36 2026] You begin casting Charm VII.",
            "[Fri Sep 11 03:30:37 2026] a tormented dead has been charmed.",
            "[Fri Sep 11 03:30:42 2026] A tormented dead cleaves a death beetle for 22 points of damage.",
            "[Fri Sep 11 03:30:43 2026] A death beetle hits a tormented dead for 9 points of damage.",
            "[Fri Sep 11 03:30:44 2026] A death beetle has been slain by a tormented dead!",
            "[Fri Sep 11 03:30:50 2026] Your Charm spell has worn off of a tormented dead.",
            "[Fri Sep 11 03:30:51 2026] A tormented dead slashes YOU for 7 points of damage.",
        ]
        .join("\n");
        /* TWO FIGHTS NOW, AND THE SPLIT IS THE POINT. The pet kills the beetle at 03:30:44 and
         * `Ended::Killed` closes the pull on that line; the charm wears off and the same-named mob
         * turns on the reader seven seconds later, which is a different fight and now reads as one.
         * The pet and the hostile were always two rows, and they are now two rows in two pulls. */
        let (rows, _) = fold_text(&text, 30, Some("Reviir"));
        assert_eq!(
            rows.len(),
            2,
            "the kill did not cut the charm fight from the one after it"
        );
        let row = &rows[0];
        let after = &rows[1];
        let tormented = |pet: bool| {
            let row = if pet { row } else { after };
            row.fighters
                .iter()
                .find(|x| x.pet == pet && x.who.text().eq_ignore_ascii_case("a tormented dead"))
                .expect("a tormented dead is on the row")
        };
        assert_eq!(
            (tormented(true).dealt, tormented(true).taken),
            (22, 9),
            "the charmed pet's row is not what it did"
        );
        assert_eq!(
            tormented(false).dealt,
            7,
            "the mob that hit the reader after the charm wore off was taken for his pet"
        );
        assert_eq!(
            row.group_sum(|x| x.dealt),
            22,
            "the reader's side is not his pet's damage and nothing else"
        );
        assert!(
            row.current_target()
                .is_none_or(|(n, _)| !n.eq_ignore_ascii_case("a tormented dead")),
            "the reader's pet is named as what he is fighting"
        );
    }

    /// DEFECT: a borrowed field creeping back into `FightRow`, which is the whole reason this module
    /// exists.
    ///
    /// This is the exact shape `ingest::scan` uses: the worker owns the text, folds it, sends the
    /// rows and drops the text. If any field borrowed the log the send would not compile, so the
    /// guard is really the compiler; the test is here so the requirement is written down beside the
    /// struct instead of living in a reviewer's head, and so the text is genuinely dropped before
    /// the rows are read.
    #[test]
    fn the_rows_outlive_the_text_they_were_folded_from() {
        let (tx, rx) = mpsc::channel::<Vec<FightRow>>();
        let worker = std::thread::spawn(move || {
            let owned: String = probe::CAPTURE.to_owned();
            let (rows, _) = fold_text(&owned, quiet_window(), Some(probe::OWNER));
            drop(owned);
            let _ = tx.send(rows);
        });
        let rows = rx.recv().unwrap_or_default();
        let _ = worker.join();
        assert_eq!(rows.len(), 12);
        assert_eq!(rows[0].headline.as_deref(), Some("a lurking mummy"));
        assert_eq!(rows[0].participants(), 9);
    }

    /// THE REACHABILITY GUARD: A COMBAT ENGINE THAT COMPILES, PASSES ITS OWN TESTS, AND IS CALLED BY
    /// NOTHING IN THE APP.
    ///
    /// That is not hypothetical here. `grimoire-parse` is 4,043 lines with 112 green tests and it
    /// cuts the reference capture into four fights, and for the whole of round one the desktop's
    /// Fights view said the engine was something "nothing in this build implements", about a crate in
    /// the same workspace. Every test that engine has is a test of the engine. None of them can see
    /// whether anything calls it, and neither can any test above this one.
    ///
    /// SO THIS DOES NOT CALL `fold_text`. It plants a log on disk, builds an `Ingest` against that
    /// folder exactly as `App::new` builds one against the owner's, pumps `tail()` until the worker's
    /// bootstrap lands, and reads `Ingest::fights()`. Everything between the bytes and the rows is
    /// the real path: `list_logs`, `read_tail`, `scan` on its own thread, the mpsc channel that
    /// forces an owned mirror, and `adopt`.
    ///
    /// THE PARTICIPANT COUNT IS THE SECOND ASSERTION AND IT IS NOT DECORATION. 22 is the count with
    /// the owner folded, 23 without. The character name exists only in the FILE NAME, so the only
    /// way to 22 is `scan` reading `LogFile::character` and handing it to the fold. Drop that
    /// argument and the reader appears twice in his own fight; that mutation moves nothing else in
    /// this row, so nothing else here would catch it.
    ///
    /// IT PAIRS WITH THE FRAME TEST IN `screens::parser`. This one proves the ingest reaches the
    /// engine; that one proves the screen reaches the ingest. Neither half is reachability alone.
    #[test]
    fn the_desktop_ingest_actually_calls_the_combat_engine_end_to_end() {
        let dir = probe::planted("ingest-e2e", probe::CAPTURE);
        let ing = probe::booted(&dir);

        let rows = ing.fights();
        assert!(
            !rows.is_empty(),
            "the ingest bootstrapped {} and found no fights at all, so nothing in the app is \
             calling the combat engine. Active log: {:?}, problem: {:?}",
            dir.display(),
            ing.active_log().map(|f| f.name()),
            ing.active_problem()
        );
        assert_eq!(rows.len(), 12, "the capture cuts into twelve encounters");
        assert_eq!(
            ing.active_character(),
            Some(probe::OWNER),
            "the character name comes out of the file name and is what folds `you` and `Reviir` \
             into one row"
        );
        assert_eq!(
            ing.tail_start(),
            Some(0),
            "206 KB is under the 40 MB cap, so the file was read whole and nothing is clipped"
        );
        assert_eq!(
            ing.fights_unreadable(),
            0,
            "every stamp in the capture reads"
        );

        let got = &rows[0];
        assert_eq!(
            (
                got.start.as_str(),
                got.end.as_str(),
                got.secs,
                got.damage,
                got.deaths,
                got.lines,
                got.ended.as_str(),
                got.headline.as_deref(),
                got.participants(),
                got.cut,
            ),
            (
                "Wed Jul 15 23:16:50 2026",
                "Wed Jul 15 23:17:09 2026",
                19,
                1_533,
                3,
                171,
                "everything died",
                Some("a lurking mummy"),
                /* 9 and not 10: see the note above. */
                9,
                false,
            ),
            "the first fight the desktop found is not the first fight the CLI prints for the same \
             bytes"
        );

        /* THE FIGHTERS CROSSED THE CHANNEL, WHICH IS THE HALF THIS TEST EXISTS FOR NOW. Every field
         * on `Participant` borrows the log text, the worker drops that text before the rows are
         * read, and a mirror that quietly shipped an empty vector would satisfy every assertion
         * above it: `participants()` would be 0 and only the count comparison would notice. */
        assert_eq!(
            got.fighters.len(),
            9,
            "the fighters came across with the row"
        );
        assert_eq!(
            got.fighters.iter().map(|x| x.dealt).sum::<u64>(),
            1_533,
            "the fighters' dealt must add up to the fight's own damage"
        );
        assert!(
            got.fighters.iter().any(|x| x.who == Who::You),
            "the owner is one row and it is the `You` row: {:?}",
            got.fighters
                .iter()
                .map(|x| x.who.text())
                .collect::<Vec<_>>()
        );
    }

    /// DEFECT: THE ONE QUESTION THAT SAYS A ROW HOLDS TWO MOBS WAS A PRIVATE HELPER ON ONE SCREEN.
    ///
    /// The engine folds by NAME, so a second pull of `a thunder spirit princess` lands in the
    /// FIRST one's row: `taken` becomes both mobs' damage and the two stamps span both
    /// engagements. `screens::live` prints damage into a target against `hp`'s measured
    /// denominator, and `hp` throws away any fight where a name died twice precisely so a reading
    /// is one mob's worth, so on a repeat pull the header read `34,102 of ~20,016`: more damage
    /// into a mob than the mob can absorb, by division, in an app that never invents a number. Its
    /// answer was to refuse the comparison, and the test it refuses on lived in that file. The
    /// Dashboards live tile draws the same subject off the same row and had no way to ask.
    ///
    /// # WHAT THIS PINS THAT A COUNT OF `Fighter::deaths` WOULD NOT
    ///
    /// The count is read off `moments`, which is the timeline `FightRow::current_target` compares
    /// hits against; both rules therefore read one source and cannot disagree about whether the
    /// name has gone down. The fixture below is exactly the state that matters: a name that died
    /// and was hit again, which `current_target` deliberately lets through.
    ///
    /// WHAT MUTATION MAKES THIS RED: counting the death MARKS of every slot rather than this
    /// name's (the player's own death is in this fixture for that reason), dropping the
    /// `!who.player()` filter from the slot lookup, or returning `fighters[slot].deaths`, which
    /// this fixture sets to a different number on purpose.
    #[test]
    fn a_repeat_pull_counts_only_the_deaths_of_that_name() {
        let died = |victim: usize, at: u32| Moment {
            at,
            what: Mark::Death { killer: 0, victim },
        };
        let row = FightRow {
            fighters: vec![
                Fighter {
                    who: Who::You,
                    /* A RAID DEATH IS A REAL EVENT AND IT IS NOT A MOB GOING DOWN. */
                    deaths: 1,
                    ..Fighter::default()
                },
                Fighter {
                    who: Who::Named("a thunder spirit princess".into()),
                    taken: 34_102,
                    /* DELIBERATELY NOT 2: this is the engine's per-participant field and this
                     * test is about the timeline. A `deaths_of` that returned it would pass every
                     * other assertion here. */
                    deaths: 7,
                    ..Fighter::default()
                },
                Fighter {
                    who: Who::Named("a spite golem".into()),
                    ..Fighter::default()
                },
            ],
            moments: vec![died(1, 10), died(0, 33), died(1, 90)],
            ..FightRow::default()
        };

        assert_eq!(
            row.deaths_of("a thunder spirit princess"),
            2,
            "the row holds two mobs of one name and the page cannot tell"
        );
        assert_eq!(
            row.deaths_of("a spite golem"),
            0,
            "a name that is in the fight and has not died is not a corpse"
        );
        assert_eq!(
            row.deaths_of("You"),
            0,
            "a raid death is not one of the mobs the run buried"
        );
        assert_eq!(
            row.deaths_of("a gnoll pup"),
            0,
            "a name that is not in this fight has buried nothing here"
        );
    }
}
