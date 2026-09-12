//! Fights, cut out of a stream of combat lines.
//!
//! EverQuest does not write a fight boundary. There is no "combat begins" line, no encounter id,
//! no instance id anywhere in the capture. What the log gives is a one-second stamp on every
//! line, thirty-four death lines and seven zone changes, and that is the whole of it. So a fight
//! here is defined the only honest way it can be: a run of combat lines with no long quiet in it.
//!
//! [`QUIET_SECONDS`] is that "long", and it was measured rather than picked. See the constant.
//!
//! What this module deliberately does not do:
//!
//! * It does not group by mob instance. Zero lines in the capture contain a `#`, several
//!   `a lurking mummy` were demonstrably alive at once, and nothing distinguishes them. Grouping
//!   is by folded NAME, and per-instance stats are simply not available from these bytes.
//! * It does not decide who is a player and who is an NPC. The log does not say, except on the
//!   ten `Targeted (NPC)` lines, and guessing from articles and casing gets six names wrong.
//! * It does not invent a source for damage the log left unattributed. `You hurt yourself` keeps
//!   [`Actor::Unknown`] as its source and shows up under its own row, so the dealt column and the
//!   taken column still reconcile instead of quietly disagreeing.

use crate::combat::{Actor, Avoid, DamageKind, Entry, Event, Reading};

/// How long a fight may go quiet before the next combat line counts as a different fight.
///
/// MEASURED, not chosen by taste, and measured with this code rather than with a sketch of it.
/// Over the real capture (`web/fixtures/eqlog-tail-200k.txt`) there are 1,779 lines that open or
/// extend a fight and therefore 1,778 gaps between them. The gaps are sharply bimodal:
///
/// ```text
///   <= 1s      1729     inside a pull: auto-attack lands about every two seconds
///   2..29s       43     pauses inside a pull: 2s x20, 3s x11, 4s x3, then 6, 8, 10, 13, 14,
///                       19, 20, 27, 29, one each
///   30..131s      0     nothing at all. 102 seconds wide, and empty.
///   >= 132s       6     132, 143, 168, 270, 289, 297: the spaces between sessions
/// ```
///
/// So the window is not balanced on a knife edge, it sits in a hole in the data. Every value
/// from 29 to 167 cuts the capture into exactly the same four fights, a plateau 139 wide.
/// The neighbouring plateaus are much worse places to stand: 14..28 gives 5 fights because the
/// 29-second pause inside the first camp gets split, and 168 and up gives 3 because a 168-second
/// walk between zones gets swallowed.
///
/// ```text
///   N   1     44 fights        N  13      6 fights
///   N   2     24               N  14..28  5
///   N   3     13               N  29..167 4      <- the plateau, 30 lives here
///   N   4..5  10               N 168+     3
///   N   6..12  9..7
/// ```
///
/// 30 is deliberately near the low end of that plateau. The two errors are not symmetric:
/// splitting one fight in two leaves both halves readable and the damage still adds up, while
/// merging two pulls into one silently doubles a fight's duration and halves its DPS with
/// nothing on screen to say it happened. 30 also matches the convention the mature C# reference
/// uses for a fight that took damage, which is worth something on its own.
///
/// The margin is honestly asymmetric: one second above the largest gap ever seen inside a fight
/// here, and 102 seconds below the smallest gap seen between them. One capture, one character,
/// two hours of solo and small-group play. A raid log would very likely want a wider window,
/// which is what [`Fights::with_quiet`] is for. This is a judgement inside a measured plateau,
/// not a law.
pub const QUIET_SECONDS: i64 = 30;

/// HOW LONG A FIGHT WHOSE LAST OPPONENT DIED IS HELD OPEN BEFORE IT IS CLOSED.
///
/// A PULL THAT ENDS IS NOT ALWAYS OVER. A camp sends the next mob while the last one is still
/// warm, and a reader standing in one spot killing the same things does not think of that as a new
/// fight. Anything that engages him inside this window joins the run that was closing.
///
/// IT ALSO CATCHES WHAT WAS ALREADY IN THE AIR. A swing thrown before the kill, a damage shield
/// answering one, a pet finishing its round: all of them land after the slay line, and without a
/// window they would each open a fight of their own.
///
/// SIX SECONDS IS THE OWNER'S NUMBER AND HIS REASON. It is not measured, and it is not pretending
/// to be: the measurement says what the log does, and how long a camp is worth calling one
/// encounter is a judgement about how a person plays.
pub const HOLD_SECONDS: i64 = 6;

/// Why a fight stopped. Kept on the fight because "the parser decided this ended here" is a
/// claim a reader is entitled to see the reason for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ended {
    /// EVERYTHING THIS FIGHT WAS FIGHTING IS DEAD, and the log said so in as many words.
    ///
    /// The other arms are all inferences from silence or from a stamp. This one is a statement:
    /// `A thunder spirit princess has been slain by Reviir!` is the client telling the reader the
    /// pull is over, and a parser that then waits [`QUIET_SECONDS`] to agree is arguing with it.
    ///
    /// MEASURED ON THE NIGHT CAPTURE: of 34 fights, 28 close here, and 27 of those 28 close on the
    /// death line itself with no overhang at all. The 6 that do not are honest survivors, and the
    /// leftovers name them: `a thunder spirit princess` that lived, `an azarack`, and in the tail
    /// capture the six Freeport guards who kill the reader and walk away. Nothing died there, so
    /// nothing closes, and [`Ended::Quiet`] still catches them.
    Killed,
    /// No combat line for longer than the quiet window.
    Quiet,
    /// The reader zoned. Nothing survives a zone line, however busy the second was.
    Zone,
    /// The next stamp was earlier than this one. Two logs concatenated, or a clock stepping
    /// back. A negative duration is not a fight, so the run is cut rather than folded.
    Backwards,
    /// The log ran out while this fight was still going.
    EndOfLog,
}

/// WHICH FAMILY OF LINE A NAME CAME FROM, so a renderer can scope a percentage without parsing
/// the name back apart. Mirrors [`DamageKind`]'s four damaging arms and nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameKind {
    Melee,
    Shield,
    Dot,
    Spell,
}

/// ONE NAME THE LOG USED FOR A SOURCE OF DAMAGE, AND WHAT IT DID.
///
/// `key` IS THE LOG'S OWN WORD, UNSTEMMED AND UNFOLDED. `slash` and `slashes` are the log's second
/// and third person, and measured over the reference capture every one of the base-form landed
/// lines has the attacker `You` while no other actor ever uses one. So inside ONE participant's map
/// there is exactly one spelling per weapon, and the two spellings are two ROWS OF A GROUP TABLE
/// rather than a merge bug. Stemming them would be this crate asserting that two words the log
/// printed are the same thing.
///
/// `hits` IS LANDED DAMAGE LINES AND NOT LINES. `grimoire-forge`'s own `named` counter bumps on
/// swings as well, which is why its `slash` reads 148 against 63 owner hits, a 2.3x overstatement.
/// Nothing here counts a swing that dealt nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NameTally<'a> {
    pub key: &'a str,
    pub kind: NameKind,
    pub amount: u64,
    pub hits: u32,
    pub crits: u32,
}

/// WHAT ONE PARTICIPANT DID TO ANOTHER, KEYED ON THE OTHER'S SLOT.
///
/// A SLOT INDEX AND NOT A NAME, WHICH IS THE WHOLE POINT. [`same`] folds participants with
/// `eq_ignore_ascii_case`, so `a dry bone skeleton` (121 lines in the capture) and
/// `A dry bone skeleton` (168) are ALREADY one participant. A map keyed on the string would split
/// them back apart: one mob, two rows, every percentage wrong. The index resolves against
/// [`Fight::participants`], which any mirror copies at the same index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TargetTally {
    pub slot: u32,
    pub amount: u64,
    pub hits: u32,
}

/// DAMAGE BY THE LOG'S ELEMENT WORD: `fire`, `cold`, `magic` and the rest of the corpus.
///
/// FROM [`DamageKind::Spell`]'s `resist` FIELD ONLY. That field is named for the check and its
/// CONTENT is the school. Melee lines carry no element word at all, so they are absent here and are
/// a renderer's stated remainder, never a slice of a pie that claims to cover everything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SchoolTally<'a> {
    pub school: &'a str,
    pub amount: u64,
    pub hits: u32,
}

/// SWINGS THIS ENTITY THREW THAT WERE STOPPED, AND BY WHAT.
///
/// ON THE ATTACKER, WHICH [`Participant::avoided`] IS NOT. That field counts swings this entity was
/// on the RECEIVING end of; reading it as "attacks I avoided" overstates the owner's by about six
/// times. Both are real and they are different numbers.
///
/// `swings - landed` IS NOT `missed`, AND THAT IS WHY THIS EXISTS. For the owner in the reference
/// capture that difference is 110, of which 20 is the TARGET parrying or dodging. A Miss slice
/// built by subtraction is 22 percent wrong and credits the defender's skill to the attacker's aim.
///
/// `invulnerable` AND `rune_absorbed` CAN ONLY READ ZERO ON THIS CAPTURE, and they are here anyway:
/// they are already [`Avoid`] arms, so counting them makes no new claim, while leaving them out
/// would silently fold them into nothing.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
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
    /// AN IDENTITY HELPER AND NOT A FIGURE ANY SCREEN DRAWS. `landed + total() == swings` is
    /// what makes a hit-results column trustworthy, and `tests/fight_maps` is what pins it. The
    /// desktop mirror carries the same method with the same note; both have been flagged as
    /// uncalled by an audit that was reading production callers alone, which is the right
    /// question to ask and the wrong answer for this one.
    pub fn total(self) -> u32 {
        self.missed
            + self.parried
            + self.dodged
            + self.blocked
            + self.riposted
            + self.invulnerable
            + self.rune_absorbed
    }

    fn bump(&mut self, how: Avoid) {
        match how {
            Avoid::Miss => self.missed += 1,
            Avoid::Parry => self.parried += 1,
            Avoid::Dodge => self.dodged += 1,
            Avoid::Block => self.blocked += 1,
            Avoid::Riposte => self.riposted += 1,
            Avoid::Invulnerable => self.invulnerable += 1,
            Avoid::RuneAbsorb => self.rune_absorbed += 1,
        }
    }
}

/// ONE ENTITY'S PART IN ONE FIGHT.
///
/// `dealt` and `taken` are separate columns rather than a signed total because a damage shield
/// makes an entity deal damage without swinging and take damage without being swung at, and
/// collapsing them loses exactly that.
///
/// THIS PARAGRAPH SPENT A WHILE ON `NameKind`, four types above, where it read as a claim that a
/// four-variant tag has damage columns. Inserting a type between a doc block and the item it
/// documents is how that happens, and it is quiet: the compiler is content and the sentence is
/// still true of something, just not of the thing under it.

/* NO `Copy`, BECAUSE A PARTICIPANT NOW OWNS THREE VECTORS. It was `Copy` while it was nine
 * counters; the per-name, per-target and per-school maps are what a detailed panel reads and they
 * cannot be bitwise duplicated. Callers clone or borrow. */
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Participant<'a> {
    pub who: Actor<'a>,
    /// THE READER'S CHARMED PET, apart from any hostile mob that shares its name. See
    /// [`Fights::push_with_pet`].
    pub pet: bool,
    pub dealt: u64,
    pub taken: u64,
    pub healed: u64,
    pub received: u64,
    /// Melee swings thrown, landed or not. Shield ticks and spell damage are not swings.
    pub swings: u32,
    /// Of those swings, the ones that did damage.
    pub landed: u32,
    /// Swings this entity was on the receiving end of and took no damage from.
    pub avoided: u32,
    pub kills: u32,
    pub deaths: u32,
    /// Every named source of damage this entity used, in first-use order.
    pub by_name: Vec<NameTally<'a>>,
    /// What it dealt to each other participant, keyed on that participant's slot.
    pub by_target: Vec<TargetTally>,
    /// Spell damage by the log's element word.
    pub by_school: Vec<SchoolTally<'a>>,
    /// Its own swings that were stopped, by how. See [`Outcomes`].
    pub outcomes: Outcomes,
    /// SECONDS FROM THE FIGHT'S START WHEN THIS ENTITY FIRST AND LAST TOOK DAMAGE.
    ///
    /// # WHAT THEY ARE FOR: SAYING WHAT IS BEING FOUGHT *NOW*
    ///
    /// A fight is a run of combat with no [`QUIET_SECONDS`] gap in it, so on a raid night, where
    /// combat never goes quiet for thirty seconds, an eight minute chain of pulls is ONE fight.
    /// [`Fight::headline`] names the biggest damage sponge in the whole of it, which is the right
    /// label for a LIST and is badly wrong over a live meter: it can name something that died six
    /// minutes ago while the reader is looking at what he is hitting right now.
    ///
    /// TAKEN AND NOT DEALT, because the question is what is being FOUGHT. [`Participant::series`]
    /// is the damage this entity dealt, which answers who is working; these answer what is being
    /// worked on.
    ///
    /// `None` FOR AN ENTITY NOTHING HAS HIT, which is a real state: a healer in a clean fight
    /// takes nothing all night and is not a target.
    pub first_taken_at: Option<u32>,
    pub last_taken_at: Option<u32>,
    /// Melee damage lines that carried `(Critical)`. Melee only, because the flag also rides
    /// heals: one of the capture's 25 is on a heal, and a `landed - crits` subtraction over a
    /// healer goes negative.
    pub melee_crits: u32,
    /// EVERY DAMAGE EVENT THIS ENTITY DEALT: seconds from the fight start, and how much.
    ///
    /// RAW POINTS AND NOT BUCKETS. Where the buckets fall is a decision about a chart width and a
    /// reader zoom, and baking one in here would fix every future chart to whatever looked right
    /// on the day. The log stamps to the second, so this is already the finest grain the file
    /// supports and a renderer can only ever group these, never split them.
    pub series: Vec<(u32, u64)>,
}

impl<'a> Participant<'a> {
    fn new(who: Actor<'a>, pet: bool) -> Self {
        Participant {
            who,
            pet,
            dealt: 0,
            taken: 0,
            healed: 0,
            received: 0,
            swings: 0,
            landed: 0,
            avoided: 0,
            kills: 0,
            deaths: 0,
            by_name: Vec::new(),
            by_target: Vec::new(),
            by_school: Vec::new(),
            outcomes: Outcomes::default(),
            melee_crits: 0,
            first_taken_at: None,
            last_taken_at: None,
            series: Vec::new(),
        }
    }

    /// Add to the tally for `key`, or start one. Linear, because a participant uses a handful of
    /// names: the capture's busiest has five.
    fn tally_name(&mut self, key: &'a str, kind: NameKind, amount: u64, crit: bool) {
        let at = match self
            .by_name
            .iter()
            .position(|t| t.key == key && t.kind == kind)
        {
            Some(at) => at,
            None => {
                self.by_name.push(NameTally {
                    key,
                    kind,
                    amount: 0,
                    hits: 0,
                    crits: 0,
                });
                self.by_name.len() - 1
            }
        };
        self.by_name[at].amount += amount;
        self.by_name[at].hits += 1;
        if crit {
            self.by_name[at].crits += 1;
        }
    }

    fn tally_target(&mut self, slot: u32, amount: u64) {
        match self.by_target.iter_mut().find(|t| t.slot == slot) {
            Some(t) => {
                t.amount += amount;
                t.hits += 1;
            }
            None => self.by_target.push(TargetTally {
                slot,
                amount,
                hits: 1,
            }),
        }
    }

    /// ADD TO THIS SECOND, OR START IT. Appended in order, so the last entry is always the second
    /// being written and a linear search is never needed.
    ///
    /// PER SECOND AND NOT PER EVENT, AND THAT LOSES NOTHING. The log stamps to the second and
    /// nothing finer, so two damage lines in one second are already indistinguishable in time: a
    /// series of raw events would carry duplicate x values and call it resolution. Accumulating is
    /// the finest grain the FILE supports, which is a different claim from the finest grain a
    /// renderer might want, and the file is the one that gets to decide.
    ///
    /// IT IS ALSO WHAT KEEPS THE FOLD OFF A PER-LINE ALLOCATION. The capture puts up to 32 lines
    /// in one printed second; an event-per-entry series would grow with LINES, and
    /// `fight_allocations` exists to stop exactly that.
    fn add_to_series(&mut self, at: u32, amount: u64) {
        match self.series.last_mut() {
            Some(last) if last.0 == at => last.1 += amount,
            _ => self.series.push((at, amount)),
        }
    }

    fn tally_school(&mut self, school: &'a str, amount: u64) {
        match self.by_school.iter_mut().find(|t| t.school == school) {
            Some(t) => {
                t.amount += amount;
                t.hits += 1;
            }
            None => self.by_school.push(SchoolTally {
                school,
                amount,
                hits: 1,
            }),
        }
    }

    /// The name as written, or `None` for the reader and for unattributed damage.
    pub fn name(&self) -> Option<&'a str> {
        self.who.name()
    }
}

/// SOMETHING WORTH A MARK ON A TIMELINE, AND WHEN IN THE FIGHT IT HAPPENED.
///
/// SLOTS AND NOT NAMES, for the same reason [`TargetTally`] uses one: the participant vector has
/// already folded `a dry bone skeleton` and `A dry bone skeleton` into one entry, and a name here
/// would let a renderer resolve the same entity two ways.
///
/// WHAT IS NOT HERE IS AS DELIBERATE AS WHAT IS. There is no `Proc` arm: the reference capture
/// contains no proc line, and a rule of "damage with no cast line before it" would misclassify
/// every one of its 26 DoT ticks. There is no `AddSpawn` arm: nothing in the grammar announces a
/// spawn. Both are in the mockups and both wait for a capture that contains them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum What<'a> {
    /// Somebody died. `killer` is the slot that got the credit.
    Death { killer: u32, victim: u32 },
    /// A named ability fired: `Reviir performs a flying kick`.
    Ability { who: u32, name: &'a str },
    /// `Reviir goes into a berserker frenzy!` and its end.
    Berserk { who: u32, on: bool },
    /// A damage line that carried `(Critical)`. `what` is the name it was dealt with.
    Crit { who: u32, what: &'a str },
}

/// One [`What`], stamped in seconds from the fight's own start.
///
/// AN OFFSET AND NOT A CLOCK TIME, because the log carries no zone offset and a timeline's x axis
/// is a duration. `at` is seconds since [`Fight::start`], so it is directly a position on the axis
/// and no consumer has to subtract two stamps and get the timezone question wrong.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FightEvent<'a> {
    pub at: u32,
    pub what: What<'a>,
}

/// A run of combat with no long quiet in it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fight<'a> {
    /// The raw stamp of the first combat line, `Wed Jul 15 23:16:50 2026`.
    pub start: &'a str,
    /// The raw stamp of the last one.
    pub end: &'a str,
    pub participants: Vec<Participant<'a>>,
    /// Combat lines folded into this fight. Not the number of lines the file spent on it: chat,
    /// loot and spell flavour in the middle of a pull are not combat and are not counted here.
    pub lines: u32,
    /// Every point of damage anybody dealt to anybody.
    pub damage: u64,
    pub deaths: u32,
    pub ended: Ended,
    /// Marks for a timeline, oldest first. See [`FightEvent`].
    pub events: Vec<FightEvent<'a>>,
    /// The zone this fight happened in, as the log spelled it, or `None` when no zone line has
    /// been seen yet in this file.
    ///
    /// STAMPED WHEN THE FIGHT OPENS AND NOT WHEN IT CLOSES. A zone line is what CUTS a fight, so
    /// the zone named at the cut is the one being entered, not the one the fight happened in.
    pub zone: Option<&'a str>,
    /// When a zone line ended this fight, how many seconds after its last combat line that line
    /// arrived.
    ///
    /// THE TERMINATOR STATES ITS OWN GAP OR IT IS A LIE. Measured on the capture: fight #3 ends at
    /// 23:40:45 and the zone line that closed it is at 23:41:53, sixty-eight seconds later, which
    /// is longer than the sixty-one second fight. A screen saying "ended: zoned" with no gap
    /// invites a reader to believe the zoning interrupted the pull.
    pub zone_gap: Option<u32>,
    start_secs: i64,
    end_secs: i64,
}

impl<'a> Fight<'a> {
    fn new(at: &'a str, secs: i64, zone: Option<&'a str>) -> Self {
        Fight {
            start: at,
            end: at,
            participants: Vec::new(),
            lines: 0,
            damage: 0,
            deaths: 0,
            ended: Ended::EndOfLog,
            events: Vec::new(),
            zone,
            zone_gap: None,
            start_secs: secs,
            end_secs: secs,
        }
    }

    /// Seconds from this fight's start, saturating at zero.
    ///
    /// SATURATING BECAUSE THE CLOCK CAN STEP BACK. `Ended::Backwards` exists precisely because two
    /// logs concatenated, or a daylight saving edge, can hand the aggregator a stamp earlier than
    /// the one before it. An event at a negative offset is not a thing a timeline can draw.
    fn offset(&self, now: i64) -> u32 {
        u32::try_from(now.saturating_sub(self.start_secs)).unwrap_or(0)
    }

    /// # A MARK MAY NOT LAND AFTER THE FIGHT IT IS IN
    ///
    /// The caller's liveness guard is `now >= self.last && now - self.last <= self.quiet`, and
    /// `self.last` is the last COMBAT second, which is also `end_secs`. So the guard bounds a mark
    /// at `end_secs + quiet` and NOT at `end_secs`: a `Reviir is no longer berserk.` line five
    /// seconds after the last blow, with no further combat, is inside the window and lands five
    /// seconds past a fight whose clock stopped.
    ///
    /// AND THE RENDERER CANNOT TELL. `screens::widgets::timeline` computes `x_of(sec)` from
    /// `sec / span` and clamps, so an event past the end is drawn ON the closing edge, exactly
    /// where a real last-second event goes. A tick for something that happened after the fight,
    /// indistinguishable from one that happened during it.
    ///
    /// THE COMMENT ABOVE THE GUARD CLAIMS THIS IS ALREADY HANDLED, and it half is: it caught a
    /// berserk at 431 seconds into a 266 second fight, which is the case the quiet window fixes.
    /// What it does not fix is the last `quiet` seconds of overhang, and a clamp is the honest
    /// answer rather than a wider guard: the event IS in this fight, its second is not.
    ///
    /// CLAMPED AND NOT DROPPED, because the event happened and the fight is the one it belongs to.
    /// What the log cannot support is a position past the end, so the position is pinned to the
    /// end and the event keeps its place in the list.
    fn mark(&mut self, now: i64, what: What<'a>) {
        let at = self.offset(now.min(self.end_secs));
        self.events.push(FightEvent { at, what });
    }

    /// How long the fight lasted, floored at one second.
    ///
    /// The log stamps to the second and nothing finer, and up to 32 lines share a single second
    /// in the capture. A fight that begins and ends inside one second therefore has a true
    /// duration this log cannot express, and the floor reports the shortest interval the file
    /// can actually distinguish rather than a zero that would make DPS infinite.
    pub fn seconds(&self) -> i64 {
        (self.end_secs - self.start_secs).max(1)
    }

    /// Damage from every source divided by the duration.
    pub fn dps(&self) -> f64 {
        self.damage as f64 / self.seconds() as f64
    }

    /// One participant's share of the same denominator, so the rows add up to [`Fight::dps`].
    pub fn dps_of(&self, p: &Participant<'a>) -> f64 {
        p.dealt as f64 / self.seconds() as f64
    }

    /// What the fight was about: the named entity that took the most damage, or, when nothing
    /// named took any, the one that dealt the most.
    ///
    /// The fallback is not decoration. In the real capture one fight is the reader being killed
    /// by six Freeport guards: 1,700 damage, all of it taken by `You`, not one point taken by
    /// anything with a name. On a taken-only rule that fight is nameless, which is the least
    /// useful label available for the most memorable thing in the log. Taken first because
    /// usually the fight is about the thing being killed; dealt second because when the reader is
    /// the thing being killed, the killer is the answer.
    ///
    /// A judgement, and a shallow one. A pull with three mobs in it gets named after the biggest
    /// and the other two are still in `participants`. It is a label for a list, not a claim about
    /// the encounter. `None` only when no named entity dealt or took anything.
    pub fn headline(&self) -> Option<&'a str> {
        let best = |f: fn(&Participant<'a>) -> u64| {
            self.participants
                .iter()
                .filter(|p| !p.pet)
                .filter_map(|p| match (f(p), p.name()) {
                    (n, Some(name)) if n > 0 => Some((n, name)),
                    _ => None,
                })
                // Ties broken by name, descending, so the answer does not depend on the order
                // participants happened to appear in the file.
                .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(a.1)))
                .map(|(_, n)| n)
        };
        best(|p| p.taken).or_else(|| best(|p| p.dealt))
    }

    /// WHAT IS BEING FOUGHT NOW: the named entity hit most recently.
    ///
    /// # THIS IS NOT [`Fight::headline`] AND THE DIFFERENCE IS THE WHOLE POINT
    ///
    /// `headline` is the biggest damage sponge over the WHOLE fight, and a fight runs until
    /// combat goes quiet for [`QUIET_SECONDS`]. On a raid night that is one unbroken run: the
    /// owner's own screen read `IN COMBAT / A SPITE GOLEM / 08:06 / 15 players named`, where the
    /// golem was the biggest thing in eight minutes rather than the thing in front of him. Every
    /// figure on that line was correct about the chain and none of it was about now.
    ///
    /// # TIES GO TO DAMAGE, AND GOING TO THE NAME WAS A BUG THE OWNER CAUGHT ON SCREEN
    ///
    /// The log stamps to the SECOND and this capture puts up to thirty-two lines inside one, so
    /// in a raid the thing being killed and every add, pet and stray around it share the newest
    /// second constantly. The tie-break is therefore not a rare case, it decides the answer
    /// almost every frame.
    ///
    /// BREAKING IT ON THE NAME HANDED THE HEADER TO THE ALPHABET. `'A'` is 65 in ASCII and `'a'`
    /// is 97, so a mob the log capitalises beats every lowercase-named one: the owner's screen
    /// read `IN COMBAT  A REVULTANT RAT  02:02  9 players named` while nine people were killing
    /// something else entirely. The rat only had to be clipped once in the newest second.
    ///
    /// SO THE BIGGEST SOAKER OF THE ONES HIT MOST RECENTLY WINS. That is the question the line
    /// is really asking, it is a measured quantity rather than a spelling, and a stray cleave on
    /// a rat cannot take the header off the thing absorbing a raid's damage.
    ///
    /// THE NAME IS STILL THE LAST RESORT, ascending, because two entities can genuinely tie on
    /// both and an answer that depends on the order the file listed them in would flicker.
    ///
    /// IT DOES NOT DECIDE WHO IS A PLAYER, because this module deliberately never does: see the
    /// module note. A caller that wants the MOB filters the answer, which is what the desktop's
    /// `FightRow::current_target` does with `Who::player`.
    /// THE FALLBACK IS THE SAME ONE [`Fight::headline`] MAKES AND FOR THE SAME REASON. One
    /// fight in the capture is the reader being killed by six Freeport guards: every point of
    /// damage in it is taken by `You`, and `Actor::You` has no name, so a taken-only rule
    /// answers `None` for the one fight a reader would most want a name on. When nothing NAMED
    /// has been hit, the thing hitting is the answer, and `series` already carries the second
    /// each entity last dealt damage in.
    pub fn current_target(&self) -> Option<&'a str> {
        let newest = |at: fn(&Participant<'a>) -> Option<u32>| {
            self.participants
                .iter()
                .filter(|p| !p.pet)
                .filter_map(|p| Some((at(p)?, p.taken, p.name()?)))
                .max_by(|a, b| {
                    a.0.cmp(&b.0)
                        .then_with(|| a.1.cmp(&b.1))
                        .then_with(|| b.2.cmp(a.2))
                })
                .map(|(_, _, n)| n)
        };
        newest(|p| p.last_taken_at).or_else(|| newest(|p| p.series.last().map(|(at, _)| *at)))
    }

    /// `now` IS THE LINE'S OWN SECOND, and it is passed in rather than read off `end_secs`
    /// because `push` sets that field before calling this and a later change to that order would
    /// silently stamp every event with the wrong time.
    fn fold(&mut self, event: Event<'a>, owner: Option<&'a str>, now: i64, pet: Option<&str>) {
        match event {
            Event::Damage(d) => {
                let amount = u64::from(d.amount);
                self.damage += amount;
                /* THE PET DEALS NOTHING TO THE READER, so a line of its name hitting him is a hostile
                 * mob of its kind: 1,087 damage in 8 of the owner's 45 charms. */
                let src_pet = is_pet(d.source, pet) && fold_owner(d.target, owner) != Actor::You;
                let src = self.slot(d.source, owner, src_pet);
                self.participants[src].dealt += amount;
                // A shield firing and a tick landing are not swings, so only melee moves the
                // swing counters. Getting this wrong turns every damage shield into a 100 percent
                // hit rate for whoever was wearing it.
                if matches!(d.kind, DamageKind::Melee { .. }) {
                    self.participants[src].swings += 1;
                    self.participants[src].landed += 1;
                    if d.mods.critical() {
                        self.participants[src].melee_crits += 1;
                    }
                }

                /* BOTH SLOTS BEFORE EITHER TALLY. `slot` takes `&mut self` because it may push a
                 * new participant, and pushing while `participants[src]` is borrowed will not
                 * compile. Resolving the target first also means the index is stable for the
                 * tally below it. */
                /* AND THE READER DOES NOT HIT HIS PET, nor does it hit itself: 2,411 damage of his
                 * in 5 charms went into a hostile mob of its name. */
                let tgt_pet =
                    is_pet(d.target, pet) && !src_pet && fold_owner(d.source, owner) != Actor::You;
                let tgt = self.slot(d.target, owner, tgt_pet);
                self.participants[tgt].taken += amount;
                /* WHEN, AS WELL AS HOW MUCH. Beside the tally rather than in a second pass, so
                 * the two can never disagree about whether this entity was hit. */
                let hit_at = self.offset(now);
                let tgt_p = &mut self.participants[tgt];
                tgt_p.first_taken_at.get_or_insert(hit_at);
                tgt_p.last_taken_at = Some(hit_at);

                /* THE NAME IS THE LOG'S OWN WORD, taken off whichever arm of `DamageKind` this
                 * was. The two arms with no name (`SelfInflicted`, `Environmental`) are named by
                 * the log as nothing, and inventing a word for them here would put a row in a
                 * table that the file does not support. */
                let named = match d.kind {
                    DamageKind::Melee { verb } => Some((verb, NameKind::Melee)),
                    DamageKind::Shield { effect } => Some((effect, NameKind::Shield)),
                    DamageKind::Dot { spell } => Some((spell, NameKind::Dot)),
                    DamageKind::Spell { spell, .. } => Some((spell, NameKind::Spell)),
                    DamageKind::SelfInflicted | DamageKind::Environmental => None,
                };
                if let Some((key, kind)) = named {
                    self.participants[src].tally_name(key, kind, amount, d.mods.critical());
                }
                if let DamageKind::Spell { resist, .. } = d.kind {
                    self.participants[src].tally_school(resist, amount);
                }
                let tgt_slot = u32::try_from(tgt).unwrap_or(u32::MAX);
                self.participants[src].tally_target(tgt_slot, amount);

                /* THE SERIES IS RAW POINTS AND NOT BUCKETS. Where the buckets fall is a decision
                 * about a chart's width and a reader's zoom, and baking one here would fix every
                 * future chart to whatever looked right today. The log stamps to the second, so
                 * this is already the finest grain the file supports. */
                let at = self.offset(now);
                self.participants[src].add_to_series(at, amount);
                if d.mods.critical() {
                    if let Some((key, _)) = named {
                        self.mark(
                            now,
                            What::Crit {
                                who: src_u32(src),
                                what: key,
                            },
                        );
                    }
                }
            }
            Event::Swing(s) => {
                let a_pet = is_pet(s.attacker, pet) && fold_owner(s.target, owner) != Actor::You;
                let a = self.slot(s.attacker, owner, a_pet);
                self.participants[a].swings += 1;
                /* WHAT STOPPED IT, ON THE ONE WHO THREW IT. `avoided` below is the other half and
                 * sits on the DEFENDER; see `Outcomes` for why subtracting to get either is wrong. */
                self.participants[a].outcomes.bump(s.outcome);
                let t_pet =
                    is_pet(s.target, pet) && !a_pet && fold_owner(s.attacker, owner) != Actor::You;
                let t = self.slot(s.target, owner, t_pet);
                self.participants[t].avoided += 1;
            }
            Event::Heal(h) => {
                let a = self.slot(h.healer, owner, false);
                self.participants[a].healed += u64::from(h.amount);
                let t = self.slot(h.target, owner, false);
                self.participants[t].received += u64::from(h.amount);
            }
            Event::Death { killer, victim } => {
                self.deaths += 1;
                let k_pet = is_pet(killer, pet) && fold_owner(victim, owner) != Actor::You;
                let k = self.slot(killer, owner, k_pet);
                self.participants[k].kills += 1;
                let v_pet =
                    is_pet(victim, pet) && !k_pet && fold_owner(killer, owner) != Actor::You;
                let v = self.slot(victim, owner, v_pet);
                self.participants[v].deaths += 1;
                /* THE KILLER IS ON THE MARK, which `ingest::KillEvent` cannot say: that type has
                 * no killer field at all, so a screen built on it can show what died and never
                 * who killed it. */
                self.mark(
                    now,
                    What::Death {
                        killer: src_u32(k),
                        victim: src_u32(v),
                    },
                );
            }
            // Ability and Berserk are marked in `push`, before its `Beat::Idle` return, so that
            // they can be drawn without counting as combat lines. See there.
            // Unreachable in practice: `beat` has already decided that everything else neither
            // opens nor extends a fight, and only the families above are ever folded.
            _ => {}
        }
    }

    /// WHERE THIS ENTITY ALREADY SITS, OR NOTHING. Never pushes.
    ///
    /// THE COUNTERPART TO `slot`, AND THE DIFFERENCE IS A REAL DEFECT IT CAUGHT. A mark is not
    /// evidence that somebody took part: `Reviir goes into a berserker frenzy!` at the exact second
    /// a fight ends says nothing about whether he dealt or took anything in it. Marking through
    /// `slot` ENROLLED him, and the capture's third fight went from eight participants to nine,
    /// gaining a row with nothing in it. A timeline must be able to name an actor without adding
    /// one to the table beside it.
    fn find_slot(&self, who: Actor<'a>, owner: Option<&'a str>) -> Option<usize> {
        let who = fold_owner(who, owner);
        self.participants.iter().position(|p| same(p.who, who))
    }

    /// THE SLOT OF THIS ENTITY, pushing one if it has none. `pet` is whether this is the reader's
    /// charmed pet, and a pet and a hostile mob of the same name are two slots.
    fn slot(&mut self, who: Actor<'a>, owner: Option<&'a str>, pet: bool) -> usize {
        let who = fold_owner(who, owner);
        match self
            .participants
            .iter()
            .position(|p| same(p.who, who) && p.pet == pet)
        {
            Some(i) => i,
            None => {
                self.participants.push(Participant::new(who, pet));
                self.participants.len() - 1
            }
        }
    }
}

/// A SLOT INDEX AS THE WIDTH EVERY MARK AND TALLY USES.
///
/// SATURATING RATHER THAN PANICKING. A fight with more than four billion participants is not a
/// thing, and the alternative to saturating is an `unwrap` in the hot path of a fold that runs over
/// forty megabytes.
fn src_u32(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// The log names the reader two ways in one sentence: `You healed Reviir over time for 61 hit
/// points` is one person healing himself. The line parser cannot know that, because the
/// character's name is in the log's FILENAME and never in the line. The caller does know it, and
/// passing it in through [`Fights::with_owner`] is what stops the owner appearing twice in every
/// table. Whole-string and case-folded, so `Tanefi` can never be absorbed into `Tanefilo`.
fn fold_owner<'a>(who: Actor<'a>, owner: Option<&'a str>) -> Actor<'a> {
    match (who, owner) {
        (Actor::Named(n), Some(o)) if n.eq_ignore_ascii_case(o) => Actor::You,
        _ => who,
    }
}

/// IS THIS ACTOR NAMED AS THE READER'S CHARMED PET? By name only, and the name is shared with every
/// mob of its kind, so each caller also asks who the line's other party is.
fn is_pet(who: Actor<'_>, pet: Option<&str>) -> bool {
    matches!((who, pet), (Actor::Named(n), Some(p)) if n.eq_ignore_ascii_case(p))
}

/// Two name slots meaning the same entity.
///
/// The game sentence-capitalises inconsistently: `a dry bone skeleton` and `A dry bone skeleton`
/// are one mob, and sixteen lines in the capture begin with a lowercase article, so line position
/// settles nothing either. The comparison is whole-string and length-exact, never a prefix, which
/// is the difference between telling `Tanefi` and `Tanefilo` apart and merging two live
/// combatants into one row.
fn same(a: Actor<'_>, b: Actor<'_>) -> bool {
    match (a, b) {
        (Actor::You, Actor::You) | (Actor::Unknown, Actor::Unknown) => true,
        (Actor::Named(x), Actor::Named(y)) => x.eq_ignore_ascii_case(y),
        _ => false,
    }
}

/// What one event does to the run of combat in progress.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Beat {
    /// Something was thrown. This can begin a fight on its own.
    Opens,
    /// Real combat, but not proof a fight started here. Four shapes qualify and each has the
    /// same defect: they name no opponent, so on their own they cannot evidence that anybody is
    /// fighting anybody.
    ///
    /// * A heal. It fires while medding between pulls as readily as under a mob.
    /// * A death. `Guard Topplo` kills `An orc centurion` across the zone and the reader never
    ///   touched either of them.
    /// * Falling damage. Left as an opener it invents fights, and it demonstrably did: before
    ///   this rule the real capture cut into six fights and two of them were nothing but the
    ///   reader falling down a cliff on the run to Dagnor's Cauldron, four and eight points of
    ///   damage split between two rows called `you` and `(unattributed)`. A cliff is not an
    ///   encounter. All seven of the capture's falling lines now sit outside every fight.
    /// * `You hurt yourself for N points.` The log names no attacker and no spell, three
    ///   separate probes failed to tie it to any mechanic, and it fires while running around.
    ///   Graded here on that argument rather than on evidence: all 97 of its occurrences in the
    ///   capture happen to land inside a fight anyway, so these bytes do not test the rule.
    ///
    /// All four extend a fight already running, because getting hurt during a pull belongs to
    /// that pull. None of them starts one.
    Extends,
    /// Ends whatever was running, immediately.
    Cuts,
    /// Carries no information about whether a fight is happening. Casts, stuns, auto-attack
    /// toggles, refusals, target changes: all of these fire outside combat as readily as inside
    /// it, and letting them extend a fight would stretch a pull across a bank trip.
    Idle,
}

fn beat(event: &Event<'_>) -> Beat {
    match event {
        // Damage that names an opponent is the strongest evidence a fight exists. Damage that
        // names nobody is not evidence of anything, so it is graded with the heals and deaths.
        Event::Damage(d) => match d.kind {
            DamageKind::SelfInflicted | DamageKind::Environmental => Beat::Extends,
            DamageKind::Melee { .. }
            | DamageKind::Shield { .. }
            | DamageKind::Dot { .. }
            | DamageKind::Spell { .. } => Beat::Opens,
        },
        Event::Swing(_) => Beat::Opens,
        Event::Heal(_) | Event::Death { .. } => Beat::Extends,
        Event::Zone { .. } => Beat::Cuts,
        _ => Beat::Idle,
    }
}

/// Folds a stream of parsed lines into fights, one forward pass, in log order.
///
/// Streaming rather than batch on purpose: the caller already walks the file once to count
/// coverage, and a 61 MB log has no business being walked twice or held as a `Vec` of events.
pub struct Fights<'a> {
    quiet: i64,
    owner: Option<&'a str>,
    open: Option<Fight<'a>>,
    last: i64,
    unreadable: u32,
    done: Vec<Fight<'a>>,
    /// The most recent zone line seen in this file, which is the zone the NEXT fight opens in.
    ///
    /// ON THE AGGREGATOR AND NOT ON THE FIGHT, because a zone line arrives BETWEEN fights: it is
    /// what cuts one, and by the time the next one opens the line is long gone. Nothing else can
    /// remember it.
    last_zone: Option<&'a str>,
    /// HOSTILES THIS FIGHT HAS ENGAGED AND NOT YET SEEN DIE, which is the whole of what
    /// [`Ended::Killed`] turns on.
    ///
    /// A NAME JOINS ON A DAMAGE LINE WITH EXACTLY ONE FRIENDLY END, and friendly means the reader
    /// or his charmed pet. A line between two names the reader is not part of engages nobody: a
    /// guard killing an orc across the zone is not his fight, and enrolling it would leave a
    /// corpse on the set that never dies and never closes anything.
    ///
    /// CASE FOLDED ON COMPARISON, NOT ON INSERT, through the same [`same`] every slot uses. The
    /// game sentence-capitalises as it pleases: `a thunder spirit princess` appears 29,238 times
    /// in the night capture and `A thunder spirit princess` 4,833, and they are one mob. A set
    /// that compared bytes read the boss as two entities, one of which never died, and the rule
    /// fired on 3 fights out of 34 instead of 28.
    engaged: Vec<&'a str>,

    /// NAMES THAT HAVE DIED IN THIS FIGHT, so a blow landing on a corpse cannot re-engage it.
    dead: Vec<&'a str>,

    /// THE SECOND THE LAST THING THIS FIGHT WAS FIGHTING DIED, AND THE STAMP IT DIED ON.
    ///
    /// The fight is over from here, and stays open anyway for [`HOLD_SECONDS`]. Two things need
    /// that window. A mob arriving inside it is the same encounter continuing, which is what a
    /// reader standing in a camp means by "the fight"; and damage already in flight lands inside
    /// the fight that earned it instead of opening a one line fight of its own.
    killed_at: Option<(i64, &'a str)>,
}

impl Default for Fights<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Fights<'a> {
    /// A fresh aggregator using [`QUIET_SECONDS`].
    pub fn new() -> Self {
        Fights {
            quiet: QUIET_SECONDS,
            owner: None,
            open: None,
            last: 0,
            unreadable: 0,
            done: Vec::new(),
            last_zone: None,
            engaged: Vec::new(),
            dead: Vec::new(),
            killed_at: None,
        }
    }

    /// A different quiet window. Exists because [`QUIET_SECONDS`] is a judgement inside a
    /// measured plateau and a different log may have a different shape, not because 30 is
    /// arbitrary.
    pub fn with_quiet(mut self, seconds: i64) -> Self {
        self.quiet = seconds.max(0);
        self
    }

    /// The reader's own character name, so that `Reviir` and `You` stop being two people.
    /// The name comes from outside the log's text, which is why the line parser cannot do this.
    pub fn with_owner(mut self, name: &'a str) -> Self {
        self.owner = if name.is_empty() { None } else { Some(name) };
        self
    }

    /// Lines whose stamp could not be turned into a time, and which were therefore folded into
    /// no fight at all. Not silently zero: a month name this build does not know would otherwise
    /// delete combat with nothing on screen to show for it.
    pub fn unreadable(&self) -> u32 {
        self.unreadable
    }

    /// Offer one parsed line. Everything that is not a combat event is ignored here; coverage
    /// accounting is the caller's job and this type does not duplicate it.
    pub fn push(&mut self, entry: Entry<'a>) {
        self.push_with_pet(entry, None);
    }

    /// [`Fights::push`], with the reader's charmed pet as of this line: `grimoire_parse::group`'s
    /// `Party::charmed`, asked after the party has seen the same line.
    ///
    /// # THE PET'S OWN ROW, AND NOT THE READER'S, AND NOT A HOSTILE MOB'S
    ///
    /// A charmed mob keeps its name, and that name is every mob of its kind in the zone. So a line
    /// naming it goes to the pet only when the line's other party says it can: the pet does not hit
    /// the reader and the reader does not hit his pet. Measured on the owner's 45 charms, that keeps
    /// 1,087 damage dealt to him and 2,411 dealt by him out of the pet's row. A hostile mob of the
    /// pet's name hitting somebody else while the charm is up is still taken for the pet; nothing in
    /// one line can tell them apart.
    ///
    /// NOT FOLDED INTO THE READER. The pet stays a participant of its own (`Participant::pet`), so a
    /// table can show it, and a page that wants the reader's side adds it to him.
    pub fn push_with_pet(&mut self, entry: Entry<'a>, pet: Option<&str>) {
        let Reading::Event(event) = entry.reading else {
            return;
        };
        let beat = beat(&event);
        if beat == Beat::Idle {
            /* A MARK THAT DOES NOT EXTEND THE FIGHT, AND THE DISTINCTION IS THE WHOLE POINT.
             *
             * An ability and a berserk are worth a mark on a timeline, but neither is evidence
             * that a fight is happening: one performed in a quiet stretch must not hold a fight
             * open past its quiet window, and neither is a combat LINE.
             *
             * MEASURED, NOT ASSUMED. Grading these as `Beat::Extends` was tried and the CLI moved:
             * fight #1 went from 1,626 combat lines to 1,628 and the file's left-over count went
             * from 9 to 4. Nothing about damage, duration, participants or cuts changed, but the
             * counts a person READS did, and the engine does not get to quietly restate them for
             * the convenience of a chart. So the mark is taken here, before the early return, and
             * `last`, `lines` and the open/close machinery are all left exactly alone.
             */
            /* AND ONLY WHILE THE FIGHT IS STILL RUNNING, WHICH IS NOT THE SAME AS STILL OPEN.
             *
             * A fight stays in `self.open` until the NEXT combat line arrives and finds the quiet
             * window exceeded, so between two pulls there is always a stale fight sitting there
             * waiting to be closed. A mark taken against it lands past its own end: this test
             * caught a berserk at 431 seconds into a fight that lasted 266.
             *
             * The condition is the same one `close` is driven by a few lines below, asked early.
             * A mark outside the quiet window belongs to no fight and is dropped, which is the
             * honest answer: the next fight has not started and the last one is over. */
            /* AND NOT WHILE THE FIGHT IS BEING HELD OPEN AFTER ITS LAST OPPONENT DIED. The run is
             * over from the kill and `close` stamps its end back to that second, so an ability
             * performed in the hold would land past the end of the fight carrying it: exactly the
             * defect this guard was written for, arriving through a second door. It belongs to no
             * fight, because the last one has ended and the next has not begun. */
            let live = self.killed_at.is_none()
                && seconds(entry.at)
                    .is_some_and(|now| now >= self.last && now - self.last <= self.quiet);
            if let (true, Some(now), Some(f)) = (live, seconds(entry.at), self.open.as_mut()) {
                let owner = self.owner;
                match event {
                    Event::Ability { who, name } => {
                        let Some(w) = f.find_slot(who, owner) else {
                            return;
                        };
                        f.mark(
                            now,
                            What::Ability {
                                who: src_u32(w),
                                name,
                            },
                        );
                    }
                    Event::Berserk { who, on } => {
                        let Some(w) = f.find_slot(who, owner) else {
                            return;
                        };
                        f.mark(
                            now,
                            What::Berserk {
                                who: src_u32(w),
                                on,
                            },
                        );
                    }
                    _ => {}
                }
            }
            return;
        }
        let Some(now) = seconds(entry.at) else {
            self.unreadable += 1;
            return;
        };
        if beat == Beat::Cuts {
            /* THE GAP IS STAMPED BEFORE THE FIGHT CLOSES, because after `close` the fight is in
             * `done` and this is the only moment both the zone line's time and the fight's last
             * combat second are in scope. Measured on the capture: fight #3's zone line is 68
             * seconds after its last swing, on a fight that ran 61. */
            if let Some(f) = self.open.as_mut() {
                f.zone_gap = Some(u32::try_from(now.saturating_sub(f.end_secs)).unwrap_or(0));
            }
            /* AND THE ZONE IS REMEMBERED FOR THE NEXT FIGHT. The binding is still live here and
             * nowhere else: a zone line is what CUTS a fight, so by the time one opens again the
             * line has been consumed. */
            if let Event::Zone { zone } = event {
                self.last_zone = Some(zone);
            }
            self.close(Ended::Zone);
            return;
        }
        if self.open.is_some() {
            if now < self.last {
                self.close(Ended::Backwards);
            } else if self.killed_at.is_some_and(|(k, _)| now - k > HOLD_SECONDS) {
                /* THE HOLD RAN OUT AND NOTHING CAME. Checked BEFORE the quiet window and not after
                 * it: the hold is the shorter of the two, and asking the wider question first
                 * would keep a finished pull open for the rest of the quiet window, which is the
                 * wait this whole rule exists to remove. */
                self.close(Ended::Killed);
            } else if now - self.last > self.quiet {
                self.close(Ended::Quiet);
            }
        }
        if self.open.is_none() {
            if beat == Beat::Extends {
                return;
            }
            self.open = Some(Fight::new(entry.at, now, self.last_zone));
        }
        let owner = self.owner;
        self.last = now;
        let fight = self.open.as_mut().expect("a fight is open on this path");
        fight.end = entry.at;
        fight.end_secs = now;
        fight.lines += 1;
        fight.fold(event, owner, now, pet);
        self.note_engagement(event, entry.at, now, owner, pet);
    }

    /// KEEP [`Fights::engaged`] IN STEP WITH THE LINE JUST FOLDED, AND CLOSE THE FIGHT WHEN THE
    /// LAST THING IT WAS FIGHTING DIES.
    ///
    /// AFTER THE FOLD AND NOT BEFORE IT. The death line belongs to the fight it ends: folded
    /// first, it lands on the participant who got the kill and bumps his `kills`, and only then
    /// does the run close. Reversed, the slay line would be the first line of the NEXT fight.
    ///
    /// THE READER'S OWN DEATH CLOSES NOTHING HERE. He dies and the mob walks away alive, so there
    /// is still something to fight and the set is not empty. That case is the quiet window's, and
    /// the live overlay's short combat window, not this rule's.
    fn note_engagement(
        &mut self,
        event: Event<'a>,
        at: &'a str,
        now: i64,
        owner: Option<&'a str>,
        pet: Option<&str>,
    ) {
        let ours =
            |who: Actor<'a>| matches!(fold_owner(who, owner), Actor::You) || is_pet(who, pet);
        match event {
            Event::Damage(d) => {
                /* EXACTLY ONE FRIENDLY END. Both ends ours is the reader and his own pet, which
                 * engages nobody; neither end ours is two strangers, which is not his fight. */
                let (s, t) = (ours(d.source), ours(d.target));
                if s == t {
                    return;
                }
                let foe = if s { d.target } else { d.source };
                let Actor::Named(n) = foe else {
                    return;
                };
                /* A CORPSE IS NOT AN OPPONENT, AND WITHOUT THIS THE WHOLE RULE UNWINDS. A swing
                 * already in flight, a damage shield answering one, or a pet finishing its round
                 * all land AFTER the slay line and all name the thing that just died. Taken as an
                 * engagement, each one puts the dead mob back on the set, which cancels the hold,
                 * which means the fight can only end on the quiet window again: the exact defect
                 * this rule exists to remove, reintroduced by its own success. */
                if self.dead.iter().any(|e| e.eq_ignore_ascii_case(n)) {
                    return;
                }
                if !self.engaged.iter().any(|e| e.eq_ignore_ascii_case(n)) {
                    self.engaged.push(n);
                }
                /* AND A LIVE OPPONENT DURING THE HOLD IS THE SAME ENCOUNTER CONTINUING. This is
                 * what the hold is FOR: the pull ended, the next mob is already on its way, and a
                 * reader who is still standing in the same spot fighting the same camp has not
                 * started a new fight. The pending end is dropped and the run carries on. */
                self.killed_at = None;
            }
            Event::Death { victim, .. } => {
                let Actor::Named(n) = victim else {
                    return;
                };
                let before = self.engaged.len();
                self.engaged.retain(|e| !e.eq_ignore_ascii_case(n));
                if self.engaged.len() < before {
                    self.dead.push(n);
                }
                /* ONLY WHEN THIS DEATH IS THE ONE THAT EMPTIED IT. A stray corpse across the zone
                 * removes nothing, and a set that was already empty was never fighting anything:
                 * neither is evidence that the reader's pull is over. */
                if self.engaged.len() < before && self.engaged.is_empty() {
                    /* MARKED, NOT CLOSED. The fight is over as of this second, and it is held open
                     * for `HOLD_SECONDS` anyway so that the next mob can join it and so that a
                     * blow already in flight lands inside the fight it belongs to rather than
                     * opening one of its own. `close` stamps the end back to this second. */
                    self.killed_at = Some((now, at));
                }
            }
            _ => {}
        }
    }

    /// Every fight, in log order. The one still running when the file ended is included and
    /// carries [`Ended::EndOfLog`], because dropping it would lose the last pull of every log.
    pub fn finish(mut self) -> Vec<Fight<'a>> {
        /* A PULL THAT ENDED ON A KILL SAYS SO EVEN IF THE FILE STOPPED INSIDE ITS HOLD. Closing it
         * as `EndOfLog` would tell the reader his last fight might still be running when the log
         * already said everything in it was dead. */
        let why = if self.killed_at.is_some() {
            Ended::Killed
        } else {
            Ended::EndOfLog
        };
        self.close(why);
        self.done
    }

    fn close(&mut self, why: Ended) {
        if let Some(mut f) = self.open.take() {
            f.ended = why;
            /* THE FIGHT ENDED WHEN THE LAST THING IN IT DIED, NOT WHEN THE HOLD RAN OUT. Anything
             * that landed in the hold is folded into this fight, because it belongs to it, but the
             * fight did not go on for another six seconds and its duration must not say it did: a
             * rate divides by this. */
            /* THE END IS THE LAST LINE THIS FIGHT ACTUALLY FOLDED, AND IS NOT PULLED BACK TO THE
             * KILL. Stamping it back read well and was incoherent: the hold folds a blow already
             * in the air into the fight that earned it, so a fight whose end had moved back
             * carried damage and marks stamped after its own end. `Ended::Killed` is what says the
             * run ended because everything in it died; the span is what says how long it took, and
             * a rate divides by that. What keeps the span honest is what the hold REFUSES to fold,
             * a few lines above, not a correction applied here. */
            self.done.push(f);
        }
        self.killed_at = None;
        self.dead.clear();
        /* THE SET BELONGS TO THE FIGHT AND NOT TO THE FILE. Left standing, a mob the reader ran
         * away from would still be "engaged" two zones later, and the first thing to die there
         * would close a fight it was never in. */
        self.engaged.clear();
    }
}

/// A log stamp as seconds.
///
/// `Wed Jul 15 23:16:50 2026`, and also `Wed Jul  5 ...` and `Wed Jul 5 ...`: the capture spans
/// two-digit days only, so which of the two short forms the game emits is unproven, and both are
/// accepted rather than guessed at. Everything is read from a delimiter the shape provides, so a
/// space-padded day cannot shift the clock by a byte.
///
/// The answer is seconds since 1970 computed as civil arithmetic, with no library and no clock.
/// It is a difference engine, not a calendar: only gaps between two stamps in the same log are
/// ever used, so the absence of a zone offset in the log costs nothing. A log written across a
/// daylight-saving step back produces one stamp earlier than the one before it, and
/// [`Ended::Backwards`] is what happens then.
pub fn seconds(stamp: &str) -> Option<i64> {
    let b = stamp.as_bytes();
    if b.len() < 23 || b.len() > 24 {
        return None;
    }
    let n = b.len();
    if b[3] != b' ' || b[7] != b' ' {
        return None;
    }
    if b[n - 14] != b' ' || b[n - 11] != b':' || b[n - 8] != b':' || b[n - 5] != b' ' {
        return None;
    }
    let month = month(&b[4..7])?;
    let day = number(&b[8..n - 14])?;
    let hour = number(&b[n - 13..n - 11])?;
    let minute = number(&b[n - 10..n - 8])?;
    let second = number(&b[n - 7..n - 5])?;
    let year = number(&b[n - 4..])?;
    if !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn month(name: &[u8]) -> Option<i64> {
    Some(match name {
        b"Jan" => 1,
        b"Feb" => 2,
        b"Mar" => 3,
        b"Apr" => 4,
        b"May" => 5,
        b"Jun" => 6,
        b"Jul" => 7,
        b"Aug" => 8,
        b"Sep" => 9,
        b"Oct" => 10,
        b"Nov" => 11,
        b"Dec" => 12,
        _ => return None,
    })
}

/// A small run of digits, tolerating the leading space the game uses to pad a single-digit day.
fn number(bytes: &[u8]) -> Option<i64> {
    let digits = match bytes.first() {
        Some(b' ') => &bytes[1..],
        _ => bytes,
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(
        digits
            .iter()
            .fold(0i64, |n, d| n * 10 + i64::from(d - b'0')),
    )
}

/// Days from 1970-01-01 to a civil date, by the standard shift-the-year-to-March algorithm.
/// Integer only, correct for every proleptic Gregorian date, and no branch on leap years beyond
/// the ones the era arithmetic already does.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let year_of_era = y - era * 400;
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::parse;

    /// Fold a whole little log. Every body below is a shape lifted from the real capture.
    fn fights_of(lines: &[String]) -> Vec<Fight<'_>> {
        let mut f = Fights::new();
        for l in lines {
            if let Some(e) = parse(l) {
                f.push(e);
            }
        }
        f.finish()
    }

    /// `at(0, ..)` is 23:16:50; `at(31, ..)` is 31 seconds later. Keeps the arithmetic out of the
    /// reader's head, which is where an off-by-one would hide.
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
    const MOB: &str = "A lurking mummy punches Tanefilo for 6 points of damage.";

    /// THE READER'S CHARMED PET IS A ROW OF ITS OWN, APART FROM A HOSTILE MOB WITH ITS NAME, AND IS
    /// NEVER WHAT THE FIGHT IS ABOUT.
    ///
    /// WHAT MUTATION MAKES THIS RED: the pet and the hostile mob sharing a slot; the reader's own hit
    /// on the hostile one, or its hit on him, going to the pet; the pet's kill not the pet's; the
    /// headline or the current target naming the pet.
    #[test]
    fn a_charmed_pet_is_apart_from_a_hostile_mob_with_its_name() {
        let lines = [
            at(
                0,
                "A tormented dead cleaves a death beetle for 22 points of damage.",
            ),
            at(1, "You slash a tormented dead for 20 points of damage."),
            at(1, "A tormented dead slashes YOU for 7 points of damage."),
            at(
                2,
                "A tormented dead kicks a death beetle for 9 points of damage.",
            ),
            at(
                3,
                "A death beetle hits a tormented dead for 50 points of damage.",
            ),
            at(4, "A death beetle has been slain by a tormented dead!"),
        ];
        let mut f = Fights::new();
        for l in &lines {
            if let Some(e) = parse(l) {
                f.push_with_pet(e, Some("a tormented dead"));
            }
        }
        let fights = f.finish();
        let fight = &fights[0];
        let named = |pet: bool| {
            fight
                .participants
                .iter()
                .find(|p| {
                    p.pet == pet
                        && p.name()
                            .is_some_and(|n| n.eq_ignore_ascii_case("a tormented dead"))
                })
                .expect("a tormented dead is a participant")
        };
        let (pet, hostile) = (named(true), named(false));
        assert_eq!(
            (pet.dealt, pet.taken, pet.kills),
            (31, 50, 1),
            "the pet's row is not what the pet did and had done to it"
        );
        assert_eq!(
            (hostile.dealt, hostile.taken),
            (7, 20),
            "the hostile mob's trade with the reader is not on its own row"
        );
        assert_eq!(
            fight.headline(),
            Some("a death beetle"),
            "the fight is named after the pet"
        );
        assert_eq!(
            fight.current_target(),
            Some("a death beetle"),
            "what is being fought is the pet"
        );

        let plain = fights_of(&lines);
        assert!(
            plain[0].participants.iter().all(|p| !p.pet),
            "a fold told of no pet marked one"
        );
    }

    // ---- the stamp reader -------------------------------------------------------------

    #[test]
    fn a_stamp_becomes_the_second_it_names() {
        assert_eq!(seconds("Thu Jan 01 00:00:00 1970"), Some(0));
        assert_eq!(seconds("Wed Jul 15 23:16:50 2026"), Some(1_784_157_410));
        assert_eq!(seconds("Thu Jul 16 00:00:00 2026"), Some(1_784_160_000));
        // A leap day, a year boundary, and a date before the epoch: the arithmetic is civil,
        // not a table of the two days this capture happens to span.
        assert_eq!(seconds("Thu Feb 29 12:00:00 2024"), Some(1_709_208_000));
        assert_eq!(seconds("Fri Jan 01 00:00:00 2027"), Some(1_798_761_600));
        assert_eq!(seconds("Thu Mar 01 00:00:00 1900"), Some(-2_203_891_200));
    }

    /// The capture spans two-digit days only, so what the game does with a single-digit day is
    /// unproven. Both plausible forms are accepted and both must land on the same second, which
    /// is the point: a padded day must not shift the clock by a byte.
    #[test]
    fn both_single_digit_day_forms_read_the_same_second() {
        let padded = seconds("Sun Jul  5 23:16:50 2026");
        let bare = seconds("Sun Jul 5 23:16:50 2026");
        assert_eq!(padded, bare);
        assert_eq!(padded, seconds("Sun Jul 05 23:16:50 2026"));
        assert!(padded.is_some());
    }

    #[test]
    fn a_stamp_that_is_not_one_is_refused_rather_than_guessed() {
        for bad in [
            "",
            "Wed Jul 15 23:16:50",
            "Wed Xxx 15 23:16:50 2026",
            "WedxJul 15 23:16:50 2026",
            "Wed Julx15 23:16:50 2026",
            "Wed Jul xx 23:16:50 2026",
            "Wed Jul 15 24:16:50 2026",
            "Wed Jul 15 23:60:50 2026",
            "Wed Jul 15 23:16:60 2026",
            "Wed Jul 32 23:16:50 2026",
            "Wed Jul 00 23:16:50 2026",
            "Wed Jul 15 23-16:50 2026",
        ] {
            assert!(seconds(bad).is_none(), "accepted a non-stamp: {bad:?}");
        }
    }

    // ---- the boundary, which is the whole module ---------------------------------------

    #[test]
    fn a_gap_of_exactly_the_quiet_window_is_still_one_fight() {
        let log = [at(0, HIT), at(QUIET_SECONDS, HIT)];
        let f = fights_of(&log);
        assert_eq!(f.len(), 1, "{f:#?}");
        assert_eq!(f[0].seconds(), QUIET_SECONDS);
        assert_eq!(f[0].lines, 2);
    }

    #[test]
    fn a_gap_one_second_past_the_quiet_window_is_two_fights() {
        let log = [at(0, HIT), at(QUIET_SECONDS + 1, HIT)];
        let f = fights_of(&log);
        assert_eq!(f.len(), 2, "{f:#?}");
        assert_eq!(f[0].ended, Ended::Quiet);
        assert_eq!(f[1].ended, Ended::EndOfLog);
    }

    /// The boundary walked one second at a time, so a comparison shifted by one cannot pass by
    /// luck on a single sample.
    #[test]
    fn the_split_happens_at_exactly_one_second_past_the_window() {
        for gap in 0..=QUIET_SECONDS + 3 {
            let log = [at(0, HIT), at(gap, HIT)];
            let want = if gap > QUIET_SECONDS { 2 } else { 1 };
            assert_eq!(
                fights_of(&log).len(),
                want,
                "a gap of {gap}s should be {want} fight(s)"
            );
        }
    }

    #[test]
    fn a_shorter_window_can_be_asked_for_and_is_obeyed() {
        let log = [at(0, HIT), at(10, HIT)];
        assert_eq!(fights_of(&log).len(), 1);
        let mut f = Fights::new().with_quiet(5);
        for l in &log {
            f.push(parse(l).unwrap());
        }
        assert_eq!(f.finish().len(), 2);
    }

    // ---- the other two ways a fight can stop --------------------------------------------

    #[test]
    fn zoning_cuts_a_fight_even_in_the_same_second() {
        let log = [
            at(0, HIT),
            at(0, "You have entered West Freeport."),
            at(0, HIT),
        ];
        let f = fights_of(&log);
        assert_eq!(f.len(), 2, "{f:#?}");
        assert_eq!(f[0].ended, Ended::Zone);
    }

    #[test]
    fn a_stamp_that_goes_backwards_cuts_rather_than_producing_a_negative_fight() {
        let log = [at(10, HIT), at(0, HIT)];
        let f = fights_of(&log);
        assert_eq!(f.len(), 2, "{f:#?}");
        assert_eq!(f[0].ended, Ended::Backwards);
        assert!(f.iter().all(|x| x.seconds() >= 1));
    }

    // ---- what may and may not begin a fight ----------------------------------------------

    #[test]
    fn a_heal_or_a_death_alone_never_opens_a_fight() {
        let log = [
            at(
                0,
                "Tanefilo healed himself for 32 hit points by Light Healing.",
            ),
            at(1, "A dark boned skeleton has been slain by Poguhy!"),
        ];
        assert!(fights_of(&log).is_empty());
    }

    /// Damage that names nobody is not evidence of a fight. Before this rule the real capture
    /// produced two fights that were nothing but the reader falling down a cliff.
    #[test]
    fn unattributed_damage_alone_never_opens_a_fight() {
        for body in [
            "You were hit by non-melee for 1 damage.",
            "You hurt yourself for 5 points.",
        ] {
            let log = [at(0, body), at(1, body)];
            assert!(
                fights_of(&log).is_empty(),
                "{body:?} invented a fight out of damage with no opponent in it"
            );
        }
    }

    /// It still belongs to a pull that is already running, and it must not stretch one either.
    #[test]
    fn unattributed_damage_folds_into_a_fight_that_is_already_running() {
        let log = [
            at(0, HIT),
            at(5, "You hurt yourself for 5 points."),
            at(6, "You were hit by non-melee for 1 damage."),
        ];
        let f = fights_of(&log);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].damage, 26);
        assert_eq!(f[0].seconds(), 6);

        // But it cannot hold one open across the quiet window on its own.
        let stretched = [
            at(0, HIT),
            at(20, "You hurt yourself for 5 points."),
            at(45, HIT),
        ];
        let g = fights_of(&stretched);
        assert_eq!(
            g.len(),
            1,
            "self damage inside a running fight should still refresh it"
        );
        let alone = [at(0, "You hurt yourself for 5 points."), at(45, HIT)];
        assert_eq!(
            fights_of(&alone).len(),
            1,
            "the self damage at 0s opened nothing, so only the swing at 45s is a fight"
        );
    }

    #[test]
    fn a_heal_extends_a_fight_that_is_already_running() {
        let log = [
            at(0, HIT),
            at(
                25,
                "Tanefilo healed himself for 32 hit points by Light Healing.",
            ),
            at(50, HIT),
        ];
        let f = fights_of(&log);
        assert_eq!(f.len(), 1, "the heal at 25s should bridge the two swings");
        assert_eq!(f[0].seconds(), 50);
    }

    /// Casting, auto-attack toggles, stuns and refusals happen out of combat as readily as in it.
    /// If any of them extended a fight, a pull would stretch across a bank trip.
    #[test]
    fn idle_chatter_neither_opens_a_fight_nor_holds_one_open() {
        let idle = [
            "You begin casting Snails Healing.",
            "Auto attack is on.",
            "You can't hit them from here.",
            "Targeted (NPC): Guard Sheg",
            "You avoid the stunning blow.",
            "You activate Skull Bash.",
        ];
        for body in idle {
            assert!(
                fights_of(&[at(0, body)]).is_empty(),
                "{body:?} opened a fight"
            );
            let log = [at(0, HIT), at(20, body), at(40, HIT)];
            assert_eq!(
                fights_of(&log).len(),
                2,
                "{body:?} held a fight open across a 40s gap"
            );
        }
    }

    // ---- folding -------------------------------------------------------------------------

    #[test]
    fn the_same_mob_in_two_casings_is_one_participant() {
        let log = [
            at(0, "You slash a dry bone skeleton for 20 points of damage."),
            at(1, "You slash A dry bone skeleton for 5 points of damage."),
        ];
        let f = fights_of(&log);
        let mob = f[0]
            .participants
            .iter()
            .find(|p| p.name().is_some())
            .unwrap();
        assert_eq!(f[0].participants.len(), 2, "{:#?}", f[0].participants);
        assert_eq!(mob.taken, 25);
    }

    /// Forty-three pairs of names in the capture are strict prefixes of one another, and
    /// `Tanefi` and `Tanefilo` are both live combatants. A prefix or substring match merges them.
    #[test]
    fn a_name_that_is_a_prefix_of_another_stays_its_own_participant() {
        let log = [
            at(0, "A lurking mummy punches Tanefi for 3 points of damage."),
            at(
                1,
                "A lurking mummy punches Tanefilo for 7 points of damage.",
            ),
        ];
        let f = fights_of(&log);
        let mut taken: Vec<(Option<&str>, u64)> = f[0]
            .participants
            .iter()
            .map(|p| (p.name(), p.taken))
            .collect();
        taken.sort();
        assert_eq!(
            taken,
            vec![
                (Some("A lurking mummy"), 0),
                (Some("Tanefi"), 3),
                (Some("Tanefilo"), 7)
            ]
        );
    }

    #[test]
    fn the_owner_and_the_pronoun_become_one_person_when_the_caller_says_who_they_are() {
        let log = [
            at(0, HIT),
            at(
                1,
                "You healed Reviir over time for 61 hit points by Snails Healing.",
            ),
        ];
        let without = fights_of(&log);
        assert_eq!(
            without[0]
                .participants
                .iter()
                .filter(|p| matches!(p.who, Actor::You) || p.name() == Some("Reviir"))
                .count(),
            2,
            "with no owner given, Reviir and You are two rows, and that is the honest answer"
        );

        let mut f = Fights::new().with_owner("Reviir");
        for l in &log {
            f.push(parse(l).unwrap());
        }
        let with = f.finish();
        let me = with[0]
            .participants
            .iter()
            .find(|p| matches!(p.who, Actor::You))
            .unwrap();
        assert!(with[0]
            .participants
            .iter()
            .all(|p| p.name() != Some("Reviir")));
        assert_eq!((me.healed, me.received), (61, 61));
    }

    /// A damage shield fires without a swing. Counting it as one gives its wearer a 100 percent
    /// hit rate out of nothing.
    #[test]
    fn only_melee_moves_the_swing_counters() {
        let log = [
            at(0, HIT),
            at(
                1,
                "A lurking mummy is pierced by Tanefilo's thorns for 7 points of non-melee damage.",
            ),
            at(
                2,
                "Tanefilo has taken 1 damage from Rabies by a lurking mummy.",
            ),
            at(3, "You try to cleave a barbed bone skeleton, but miss!"),
        ];
        let f = fights_of(&log);
        let me = f[0]
            .participants
            .iter()
            .find(|p| matches!(p.who, Actor::You))
            .unwrap();
        let tan = f[0]
            .participants
            .iter()
            .find(|p| p.name() == Some("Tanefilo"))
            .unwrap();
        assert_eq!((me.swings, me.landed), (2, 1));
        assert_eq!((tan.swings, tan.landed, tan.dealt), (0, 0, 7));
    }

    #[test]
    fn dealt_and_taken_reconcile_even_when_the_log_named_no_attacker() {
        let log = [at(0, HIT), at(1, "You hurt yourself for 5 points.")];
        let f = fights_of(&log);
        let dealt: u64 = f[0].participants.iter().map(|p| p.dealt).sum();
        let taken: u64 = f[0].participants.iter().map(|p| p.taken).sum();
        assert_eq!((dealt, taken, f[0].damage), (25, 25, 25));
        let unattributed = f[0]
            .participants
            .iter()
            .find(|p| matches!(p.who, Actor::Unknown))
            .expect("the unattributed source keeps its own row");
        assert_eq!(unattributed.dealt, 5);
    }

    // ---- the numbers that reach the screen ---------------------------------------------------

    #[test]
    fn a_fight_inside_one_second_lasts_one_second_rather_than_zero() {
        let log = [at(0, HIT), at(0, HIT)];
        let f = fights_of(&log);
        assert_eq!(f[0].seconds(), 1);
        assert_eq!(f[0].damage, 40);
        assert!(f[0].dps().is_finite());
        assert_eq!(f[0].dps(), 40.0);
    }

    #[test]
    fn dps_divides_by_the_span_and_the_rows_add_up_to_the_total() {
        let log = [at(0, HIT), at(10, MOB)];
        let f = fights_of(&log);
        assert_eq!(f[0].seconds(), 10);
        assert_eq!(f[0].damage, 26);
        assert!((f[0].dps() - 2.6).abs() < 1e-9);
        let sum: f64 = f[0].participants.iter().map(|p| f[0].dps_of(p)).sum();
        assert!((sum - f[0].dps()).abs() < 1e-9);
    }

    #[test]
    fn the_headline_is_whatever_took_the_most_damage() {
        let log = [
            at(0, "You slash a dry bone skeleton for 20 points of damage."),
            at(1, "You slash a lurking mummy for 90 points of damage."),
            at(2, MOB),
        ];
        let f = fights_of(&log);
        assert_eq!(f[0].headline(), Some("a lurking mummy"));
    }

    #[test]
    fn a_fight_where_nothing_named_was_hurt_has_no_headline() {
        let log = [
            at(0, "You try to cleave YOU, but miss!"),
            at(1, "You hurt yourself for 5 points."),
        ];
        let f = fights_of(&log);
        assert_eq!(f.len(), 1);
        assert!(f[0].damage > 0);
        assert_eq!(f[0].headline(), None);
    }

    // ---- the accounting the caller prints ----------------------------------------------------

    #[test]
    fn a_stamp_the_reader_cannot_decode_is_counted_rather_than_dropped_in_silence() {
        // `parse` accepts the shape, `seconds` rejects the month: the two checks are not the
        // same check, and a line that clears one and fails the other must be visible.
        let line =
            "[Wed Zzz 15 23:16:50 2026] You slash a dry bone skeleton for 20 points of damage.";
        let entry = parse(line).expect("the stamp is well shaped");
        let mut f = Fights::new();
        f.push(entry);
        assert_eq!(f.unreadable(), 1);
        assert!(f.finish().is_empty());
    }

    #[test]
    fn the_fight_still_running_when_the_log_stops_is_kept() {
        let log = [at(0, HIT)];
        let f = fights_of(&log);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].ended, Ended::EndOfLog);
    }

    #[test]
    fn an_empty_log_produces_no_fights_and_does_not_panic() {
        assert!(Fights::new().finish().is_empty());
        assert!(fights_of(&[]).is_empty());
    }
}
