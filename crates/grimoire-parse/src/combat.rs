//! One combat line, recognised.
//!
//! The owner's logs are roughly 97 percent combat, so this is the module that decides whether
//! the Parses and Fights screens have anything behind them. The grammar below was read off a
//! real 200 KB capture (`web/fixtures/eqlog-tail-200k.txt`, cut from a 61 MB log), not guessed:
//!
//! ```text
//! [Wed Jul 15 23:16:50 2026] You frenzy on a dry bone skeleton for 22 points of damage.
//! [Wed Jul 15 23:18:02 2026] An undead brewer bashes Tanefilo for 1 point of damage.
//! [Wed Jul 15 23:17:30 2026] A lurking mummy is pierced by Tanefilo's thorns for 7 points of non-melee damage.
//! [Wed Jul 15 23:18:04 2026] A dry bone skeleton tries to crush YOU, but YOU block!
//! [Wed Jul 15 23:24:23 2026] a dry bone skeleton hit you for 26 points of fire damage by Dry Bone Fire Burst.
//! [Wed Jul 15 23:17:04 2026] Tanefilo healed himself for 32 hit points by Light Healing.
//! [Wed Jul 15 23:19:11 2026] Tanefilo has taken 1 damage from Rabies by a lurking mummy.
//! ```
//!
//! Three rules govern everything below, because they are the three things that go wrong:
//!
//! 1. **The game inflects, and not consistently.** `for 1 point of damage.` against
//!    `for 7 points of damage.`, `but miss!` against `but misses!`, `bash`/`bashes` against
//!    `frenzy`/`frenzies`. Every one of those is handled by stepping over the inflection
//!    ([`strip_points`], [`inflected`]) rather than by listing both spellings, because the
//!    counter-example `You hurt yourself for 1 points.` proves the inflection is *not* a
//!    function of the number and a rule keyed on the number would be wrong in both directions.
//! 2. **A damage shield is credited to the possessive name**, which is the opposite of how it
//!    reads. See [`shield`] for the evidence and for what reading it the natural way costs.
//! 3. **Names carry spaces and apostrophes** (`Torklar Battlemaster`, `Tanefilo's thorns`).
//!    Nothing here splits a name on whitespace or on an apostrophe: every boundary comes from a
//!    delimiter the *shape* provides, so `Tanefi` and `Tanefilo` can never merge.
//!
//! Nothing here allocates. Every field is a `&str` borrowed out of the caller's line, so a
//! 61 MB log is a scan rather than a heap.

/// Who a line is talking about.
///
/// The log writes the reader as `You`, `YOU`, `you` or `YOUR` depending on which shape it is
/// in, and the casing carries no information beyond that. It is kept as its own variant rather
/// than substituted for the character's name because the parser does not know that name: the
/// name is in the log's *filename*, not in the line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Actor<'a> {
    /// The log's owner, in any casing of the pronoun.
    You,
    /// Anyone else, spelled exactly as the line spelled them.
    Named(&'a str),
    /// The line named nobody. Falling damage and `You hurt yourself` have no attacker at all,
    /// and inventing one would be a lie the aggregator could not see through.
    Unknown,
}

impl<'a> Actor<'a> {
    /// Read a name slot. The pronoun folds; everything else is kept verbatim.
    fn of(name: &'a str) -> Self {
        if name.eq_ignore_ascii_case("you") || name.eq_ignore_ascii_case("your") {
            Actor::You
        } else if name.is_empty() {
            Actor::Unknown
        } else {
            Actor::Named(name)
        }
    }

    /// The name as written, or `None` for the reader and for an unnamed source.
    pub fn name(self) -> Option<&'a str> {
        match self {
            Actor::Named(n) => Some(n),
            _ => None,
        }
    }
}

/// Where an amount of damage came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DamageKind<'a> {
    /// A weapon swing that connected. `verb` is as written, so `bashes` stays `bashes`.
    Melee { verb: &'a str },
    /// A damage shield firing. `effect` is the shield's noun, `thorns` or `flames`.
    Shield { effect: &'a str },
    /// A damage-over-time tick.
    Dot { spell: &'a str },
    /// A direct damage spell. `resist` is the school: `fire`, `cold`, `magic`.
    Spell { spell: &'a str, resist: &'a str },
    /// `You hurt yourself for N points.` The log names no attacker and no spell, so neither
    /// does this. The capture has 97 of them, amounts 1 to 6, and three separate probes failed
    /// to tie them to any mechanic; guessing one here would put a wrong number on a screen.
    SelfInflicted,
    /// `You were hit by non-melee for N damage.` Paired with a `YOU were injured by falling.`
    /// line in 7 of 7 capture cases, but the pairing is an observation and not a law, so the
    /// cause stays off the type.
    Environmental,
}

/// Damage that landed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Damage<'a> {
    /// Who dealt it. For a shield this is the *wearer*, not the entity at the front of the line.
    pub source: Actor<'a>,
    pub target: Actor<'a>,
    pub amount: u32,
    pub kind: DamageKind<'a>,
    pub mods: Mods<'a>,
}

/// How a swing that dealt no damage ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Avoid {
    Miss,
    Parry,
    Dodge,
    Block,
    Riposte,
    /// `but <target> is INVULNERABLE!`. Documented by the reference parser, absent from the
    /// capture, so it is carried as a hypothesis and not as a measurement.
    Invulnerable,
    /// `but <target>'s magical skin absorbs the blow!`. Same standing as `Invulnerable`.
    RuneAbsorb,
}

/// A swing that produced no damage. Distinct from [`Refusal`], where no swing happened at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Swing<'a> {
    pub attacker: Actor<'a>,
    pub target: Actor<'a>,
    /// The base form, `slash` rather than `slashes`, because `tries to` always takes the base.
    pub verb: &'a str,
    pub outcome: Avoid,
    pub mods: Mods<'a>,
}

/// A heal that landed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Heal<'a> {
    pub healer: Actor<'a>,
    pub target: Actor<'a>,
    /// What the target actually gained.
    pub amount: u32,
    /// The uncapped figure the game prints in parentheses when the heal overshot the target's
    /// missing hit points. `861 (2101)` means 861 landed and 1,240 was wasted.
    pub full: Option<u32>,
    pub spell: &'a str,
    pub over_time: bool,
    pub mods: Mods<'a>,
}

/// Why the client refused to swing. Not a miss: nothing was thrown, so these must stay out of
/// any hit-versus-miss ratio.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    OutOfRange,
    NoLineOfSight,
    NoTarget,
    NeedsTarget,
    LostTarget,
}

/// Stun and knockdown state on the reader.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stun {
    Stunned,
    Recovered,
    Overcome,
    Avoided,
    KnockedUnconscious,
}

/// What the client said an entity is.
///
/// These are the only lines in the capture that declare a name to be an NPC rather than leaving
/// it to be guessed from articles and casing, and they resolve five names that are otherwise
/// structurally identical to player names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetKind {
    Npc,
    Merchant,
    Player,
    /// A parenthesised kind this parser has not seen. Kept rather than dropped.
    Other,
}

/// Why a cast did not happen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CastFailure {
    Mana,
}

/// A line that carries combat meaning.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event<'a> {
    Damage(Damage<'a>),
    Swing(Swing<'a>),
    Heal(Heal<'a>),
    Death {
        killer: Actor<'a>,
        victim: Actor<'a>,
    },
    CastStart {
        caster: Actor<'a>,
        spell: &'a str,
    },
    CastInterrupted {
        caster: Actor<'a>,
        spell: &'a str,
    },
    CastBlocked {
        spell: &'a str,
        blocked_by: &'a str,
    },
    CastFailed {
        caster: Actor<'a>,
        reason: CastFailure,
    },
    CastResumed {
        caster: Actor<'a>,
    },
    Resisted {
        caster: Actor<'a>,
        target: Actor<'a>,
        spell: &'a str,
    },
    Ability {
        who: Actor<'a>,
        name: &'a str,
    },
    AutoAttack {
        on: bool,
    },
    /// `Reviir goes into a berserker frenzy!`. The subject is the owner's character *name*, in
    /// the third person, which is why `who` is an [`Actor`] and not an assumed [`Actor::You`].
    Berserk {
        who: Actor<'a>,
        on: bool,
    },
    Stun(Stun),
    Refused(Refusal),
    Targeted {
        kind: TargetKind,
        name: &'a str,
    },
    /// Fight aggregation has to cut on this: nothing in the log marks a fight boundary, and a
    /// zone change is one of only two hard markers there are.
    Zone {
        zone: &'a str,
    },
}

/// A line that is understood and deliberately dropped. Named, so that "we ignore it" is a
/// decision on the record rather than a silent fall-through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ignored {
    /// Player chat. Matched first and explicitly: nine lines in the capture carry a literal
    /// `, but ` inside their free text and would be torn apart by the miss rule.
    Chat,
    /// NPC or player speech, including the emote-plus-speech compound where the speaker is the
    /// name before the *emote verb* rather than everything before the quote.
    Speech,
    Experience,
    SkillUp,
    AbilityPoint,
    Achievement,
    Loot,
    Coin,
    /// A consider line. Its trailing `(Lvl: 35)` is not a combat modifier.
    Consider,
    Memorise,
    ZoneLoad,
    Group,
    /// `YOU were injured by falling.` This is the *cause label* for the preceding
    /// `You were hit by non-melee` line and carries no amount. Emitting damage for both
    /// double-counts every fall.
    FallingCause,
    /// Client and interface chrome.
    Chrome,
}

/// Recognised as a class, but not typeable.
///
/// These are the per-spell English sentences the game prints when something lands or wears off.
/// They carry no number and no spell name, so turning one into an attributed event needs a
/// message-to-spell table. Reporting them as unrecognised understates coverage by five percent;
/// dropping them silently makes coverage unmeasurable. They get their own outcome instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flavour {
    LandedOnOther,
    LandedOnSelf,
    /// Stun, snare, root, lull, and NPC rage. Combat-relevant, still unattributable.
    CrowdControl,
    /// Shadows the numeric heal line: `Tanefilo feels better.` is the flavour twin of a
    /// `healed ... for N hit points by ...` line. Counting both double-counts healing, so the
    /// numeric line is the one that becomes an event.
    HealOnOther,
    BuffFaded,
}

/// What one line turned out to be. Four outcomes, not two, because "parsed or not" cannot tell
/// a deliberate drop from a hole in the parser.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reading<'a> {
    Event(Event<'a>),
    Ignored(Ignored),
    Flavour(Flavour),
    Unrecognised,
}

/// A reading and the raw timestamp it carried.
///
/// The timestamp stays a `&str`, `Wed Jul 15 23:16:50 2026`. Equality is all an aggregator
/// needs to know two events shared a second, and it dodges pulling a date library into a wasm
/// build for a string comparison.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry<'a> {
    pub at: &'a str,
    pub reading: Reading<'a>,
}

/// The trailing parenthesised group: `(Critical)`, `(Slay Undead)`, `(Lucky Critical Twincast)`.
///
/// `raw` is kept whole so a modifier this build has never heard of survives into the data
/// instead of being rounded away to "no modifier".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mods<'a> {
    raw: &'a str,
    bits: u8,
}

impl<'a> Mods<'a> {
    const CRITICAL: u8 = 1 << 0;
    const RIPOSTE: u8 = 1 << 1;
    const STRIKETHROUGH: u8 = 1 << 2;
    const SLAY_UNDEAD: u8 = 1 << 3;
    const LUCKY: u8 = 1 << 4;
    const TWINCAST: u8 = 1 << 5;
    const FLURRY: u8 = 1 << 6;
    const RAMPAGE: u8 = 1 << 7;

    /// The group exactly as written, without its parentheses. Empty when there was none.
    pub fn raw(self) -> &'a str {
        self.raw
    }

    pub fn is_empty(self) -> bool {
        self.raw.is_empty()
    }

    /// Four different words all mean a critical: the reference treats `Crippling Blow`,
    /// `Deadly Strike` and `Finishing Blow` as crits alongside `Critical`, and a meter that
    /// counted only the literal word would undercount every class that procs the others.
    pub fn critical(self) -> bool {
        self.bits & Self::CRITICAL != 0
    }

    /// A riposted swing. Note that a riposte prints twice, once as the defender's avoidance and
    /// once as the counter-swing carrying this flag, so an aggregator must not count both as
    /// attacks.
    pub fn riposte(self) -> bool {
        self.bits & Self::RIPOSTE != 0
    }

    /// The attacker struck *through* a riposte. Together with [`Mods::riposte`] it means the
    /// opposite of a riposte and must not be counted as one.
    pub fn strikethrough(self) -> bool {
        self.bits & Self::STRIKETHROUGH != 0
    }

    pub fn slay_undead(self) -> bool {
        self.bits & Self::SLAY_UNDEAD != 0
    }

    pub fn lucky(self) -> bool {
        self.bits & Self::LUCKY != 0
    }

    pub fn twincast(self) -> bool {
        self.bits & Self::TWINCAST != 0
    }

    pub fn flurry(self) -> bool {
        self.bits & Self::FLURRY != 0
    }

    pub fn rampage(self) -> bool {
        self.bits & Self::RAMPAGE != 0
    }

    /// Modifier names are multi-word (`Slay Undead`, `Crippling Blow`, `Double Bow Shot`), so
    /// the group cannot be read as a list of names split on spaces. Reading it as a bag of
    /// words instead sets every bit correctly, because no two modifiers share a leading word.
    fn read(raw: &'a str) -> Self {
        let mut bits = 0u8;
        for word in raw.split(' ') {
            bits |= match word {
                "Critical" | "Crippling" | "Deadly" | "Finishing" => Self::CRITICAL,
                "Riposte" => Self::RIPOSTE,
                "Strikethrough" => Self::STRIKETHROUGH,
                "Slay" => Self::SLAY_UNDEAD,
                "Lucky" => Self::LUCKY,
                "Twincast" => Self::TWINCAST,
                "Flurry" => Self::FLURRY,
                "Rampage" => Self::RAMPAGE,
                _ => 0,
            };
        }
        Mods { raw, bits }
    }
}

/// Recognise one line. Returns `None` only when the line carries no timestamp, which in the
/// real capture is the provenance header and nothing else.
pub fn parse(raw: &str) -> Option<Entry<'_>> {
    let (at, body) = split_stamp(raw.trim_end_matches(['\r', '\n']))?;
    Some(Entry {
        at,
        reading: read(body),
    })
}

/// Split `[stamp] body`.
///
/// The closing bracket is *found*, never assumed at a fixed offset: three messages in the
/// capture contain literal brackets of their own, so the first `] ` is the only safe boundary.
/// The stamp's shape is then checked from the right, where it is fixed, which accepts a
/// space-padded single-digit day (`Jul  5`, unproven because the capture spans two-digit days
/// only) without accepting a chat line that merely opens with a bracket.
fn split_stamp(raw: &str) -> Option<(&str, &str)> {
    let rest = raw.strip_prefix('[')?;
    let end = rest.find(']')?;
    let stamp = &rest[..end];
    if !is_stamp(stamp) {
        return None;
    }
    let body = rest[end + 1..].strip_prefix(' ')?;
    Some((stamp, body))
}

/// `Wed Jul 15 23:16:50 2026`, checked at its fixed tail: ` HH:MM:SS YYYY`.
fn is_stamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 23 || b.len() > 24 {
        return false;
    }
    let n = b.len();
    b[n - 14] == b' '
        && b[n - 11] == b':'
        && b[n - 8] == b':'
        && b[n - 5] == b' '
        && [
            n - 13,
            n - 12,
            n - 10,
            n - 9,
            n - 7,
            n - 6,
            n - 4,
            n - 3,
            n - 2,
            n - 1,
        ]
        .iter()
        .all(|&i| b[i].is_ascii_digit())
}

/// The router. Ordered by how often each family fires, with a terminal-byte gate in front of
/// the expensive branches so a line pays one comparison to reject a family it is not in.
fn read(body: &str) -> Reading<'_> {
    // Chat first, unconditionally. Free text can contain any combat shape verbatim, and nine
    // lines in the capture carry `, but ` inside their quotes. A quoted line always ends in an
    // apostrophe, which makes the gate one byte.
    if body.ends_with('\'') {
        if let Some(i) = quoted(body) {
            return Reading::Ignored(i);
        }
    }

    let (body, mods) = split_mods(body);

    if body.ends_with("damage.")
        || body.ends_with("damage!")
        || body.ends_with("points.")
        || body.ends_with("point.")
    {
        if let Some(e) = damage(body, mods) {
            return Reading::Event(e);
        }
    }
    if body.ends_with('!') {
        if let Some(e) = avoided(body, mods) {
            return Reading::Event(Event::Swing(e));
        }
    }
    if let Some(e) = spellwork(body, mods) {
        return Reading::Event(e);
    }
    if let Some(e) = states(body) {
        return Reading::Event(e);
    }
    if let Some(i) = chaff(body) {
        return Reading::Ignored(i);
    }
    if let Some(f) = flavour(body) {
        return Reading::Flavour(f);
    }
    Reading::Unrecognised
}

// -------------------------------------------------------------------------------------------
// Inflection
// -------------------------------------------------------------------------------------------

/// Step over ` point` and its optional plural, returning whatever follows.
///
/// This is the whole answer to the singular trap. `for 1 point of damage.` is 135 lines of the
/// capture and every one of them is dropped by a matcher keyed on ` points of damage`. Nothing
/// here derives the token from the number, because `You hurt yourself for 1 points.` proves the
/// game does not either.
fn strip_points(s: &str) -> Option<&str> {
    let rest = s.strip_prefix(" point")?;
    Some(rest.strip_prefix('s').unwrap_or(rest))
}

/// Does `word` equal `base` in either person?
///
/// English third person is four rules and not one: the bare form after `You`, `+s`, `+es` after
/// a sibilant (`bash`/`bashes`), and `y` to `ies` (`frenzy`/`frenzies`). Handling them here
/// means the verb tables below hold one entry per verb, so adding a verb cannot half-add it.
fn inflected(word: &str, base: &str) -> bool {
    if word == base {
        return true;
    }
    if let Some(stem) = word.strip_suffix("ies") {
        // frenzy -> frenzies, parry -> parries.
        return base.len() == stem.len() + 1 && base.ends_with('y') && base[..stem.len()] == *stem;
    }
    if let Some(stem) = word.strip_suffix("es") {
        if stem == base {
            return true;
        }
    }
    match word.strip_suffix('s') {
        Some(stem) => stem == base,
        None => false,
    }
}

/// The attack verbs, base form only.
///
/// Twelve of these have byte evidence in the capture: bash, bite, cleave, crush, frenzy, hit,
/// kick, pierce, punch, slash, smite, strike. The other fourteen come from the reference
/// parser, which targets Live and TLP rather than EverQuest Legends, so they are a hypothesis
/// and not a measurement. A verb table costs nothing, so they are carried; they are not
/// verified, and nothing downstream should report them as such.
const ATTACK_VERBS: &[&str] = &[
    "backstab", "bash", "bite", "claw", "cleave", "crush", "frenzy", "gore", "hit", "kick",
    "learn", "maul", "pierce", "punch", "reave", "rend", "shoot", "slam", "slash", "slice",
    "smash", "smite", "stab", "sting", "strike", "sweep",
];

/// Is `word` an attack verb in either person? Gated on the first byte so a token costs a
/// handful of comparisons rather than fifty-two.
fn attack_verb(word: &str) -> bool {
    let Some(&first) = word.as_bytes().first() else {
        return false;
    };
    ATTACK_VERBS
        .iter()
        .filter(|v| v.as_bytes()[0] == first)
        .any(|v| inflected(word, v))
}

// -------------------------------------------------------------------------------------------
// Modifiers
// -------------------------------------------------------------------------------------------

/// Peel a trailing `(...)` group off, but only when it is a combat modifier.
///
/// Four other shapes end in parentheses and none of them is a modifier: `(Lvl: 35)` on a
/// consider line, `(1.898%)` on experience, `(52)` on a skill-up, and
/// `(Blocked by Shield of Barbs.)` on a cast. A generic trailing-paren strip corrupts all four.
/// The test is therefore structural rather than a list of known names, so a modifier this build
/// has never seen still parses: the group must follow the sentence's own full stop or bang, it
/// must open with a capital, and it must contain none of `:`, `%` or `.`, which is exactly what
/// separates a modifier from the other four.
fn split_mods(body: &str) -> (&str, Mods<'_>) {
    let Some(head) = body.strip_suffix(')') else {
        return (body, Mods::default());
    };
    let Some(open) = head.rfind(" (") else {
        return (body, Mods::default());
    };
    if open == 0 {
        return (body, Mods::default());
    }
    let before = head.as_bytes()[open - 1];
    if before != b'.' && before != b'!' {
        return (body, Mods::default());
    }
    let group = &head[open + 2..];
    if !group.starts_with(|c: char| c.is_ascii_uppercase()) || group.contains([':', '%', '.', '('])
    {
        return (body, Mods::default());
    }
    (&head[..open], Mods::read(group))
}

// -------------------------------------------------------------------------------------------
// Damage
// -------------------------------------------------------------------------------------------

/// Read a leading run of ASCII digits.
fn number(s: &str) -> Option<(u32, &str)> {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut n = 0u32;
    while i < b.len() && b[i].is_ascii_digit() {
        n = n.checked_mul(10)?.checked_add(u32::from(b[i] - b'0'))?;
        i += 1;
    }
    (i > 0).then(|| (n, &s[i..]))
}

/// Everything that ends in an amount of damage.
///
/// The amount clause is always last, so the split is on the *last* ` for `. That is what keeps
/// a name that happens to contain the delimiter from cutting the line in the wrong place.
fn damage<'a>(body: &'a str, mods: Mods<'a>) -> Option<Event<'a>> {
    let (head, tail) = body.rsplit_once(" for ")?;
    let (amount, rest) = number(tail)?;

    if let Some(rest) = strip_points(rest) {
        if let Some(what) = rest.strip_prefix(" of ") {
            return match what {
                "damage." | "damage!" => melee(head, amount, mods),
                "non-melee damage." | "non-melee damage!" => shield(head, amount, mods),
                _ => None,
            };
        }
        // `You hurt yourself for N points.` The only shape whose amount is followed by nothing.
        if (rest == "." || rest == "!") && head == "You hurt yourself" {
            return Some(Event::Damage(Damage {
                source: Actor::Unknown,
                target: Actor::You,
                amount,
                kind: DamageKind::SelfInflicted,
                mods,
            }));
        }
        return None;
    }

    // `You were hit by non-melee for N damage.` Note the missing `points of`: the third-person
    // sibling the reference documents says `points of` and means something else entirely.
    if (rest == " damage." || rest == " damage!") && head == "You were hit by non-melee" {
        return Some(Event::Damage(Damage {
            source: Actor::Unknown,
            target: Actor::You,
            amount,
            kind: DamageKind::Environmental,
            mods,
        }));
    }
    None
}

/// `<attacker> <verb> [on] <target>`.
fn melee<'a>(head: &'a str, amount: u32, mods: Mods<'a>) -> Option<Event<'a>> {
    // A copula anywhere vetoes the match. A dead player's still-ticking effect prints
    // `<name> was hit by non-melee for N points of damage.`, which has the shape of a melee hit
    // and the meaning of a spell recourse; `hit` is a legal verb, so without this veto the
    // scanner below would report an attacker called `A lurking mummy was`.
    if head.contains(" is ")
        || head.contains(" are ")
        || head.contains(" was ")
        || head.contains(" were ")
    {
        return None;
    }
    let (attacker, verb, target) = split_verb(head)?;
    Some(Event::Damage(Damage {
        source: Actor::of(attacker),
        target: Actor::of(target),
        amount,
        kind: DamageKind::Melee { verb },
        mods,
    }))
}

/// Split `<attacker> <verb> [on] <target>` at the first token that is an attack verb.
///
/// The scan starts at the second token because a subject is at least one word, and it takes the
/// first verb rather than the last because the target is whatever follows it.
fn split_verb(head: &str) -> Option<(&str, &str, &str)> {
    let mut at = head.find(' ').map(|i| i + 1)?;
    loop {
        let end = head[at..].find(' ').map_or(head.len(), |i| at + i);
        if attack_verb(&head[at..end]) {
            if end >= head.len() {
                return None;
            }
            let target = &head[end + 1..];
            // `frenzy` is the one verb that takes a preposition: `You frenzy on a mummy`. The
            // strip is unconditional because no entity in this game is named `on something`,
            // and doing it for every verb means an unseen verb that takes `on` degrades to the
            // right target rather than to a target called `on a lurking mummy`.
            let target = target.strip_prefix("on ").unwrap_or(target);
            return Some((&head[..at - 1], &head[at..end], target));
        }
        if end >= head.len() {
            return None;
        }
        at = end + 1;
    }
}

/// `<victim> is <participle> by <wearer>'s <noun>`.
///
/// THE DIRECTION IS THE OPPOSITE OF HOW IT READS, and the next reader will assume otherwise.
/// `A lurking mummy is pierced by Tanefilo's thorns` means *Tanefilo* dealt the damage and the
/// *mummy* took it. The evidence, from the capture: `Poguhy begins casting Shield of Barbs.`
/// lands at 23:17:00 and the first `pierced by Poguhy's thorns` at 23:17:30, with zero before
/// the cast against 61 after. So the possessive is the shield's wearer, the shield fires on
/// whatever just hit the wearer, and the entity at the front of the line is the one taking it.
/// Reading it the natural way reverses 373 of the capture's 1,779 combat lines, which is 21
/// percent of every damage event in the file.
fn shield<'a>(head: &'a str, amount: u32, mods: Mods<'a>) -> Option<Event<'a>> {
    let (victim, rest) = [" is ", " are ", " was ", " were "]
        .iter()
        .find_map(|sep| head.split_once(sep))?;

    // `was chilled to the bone` has no `by` clause at all and therefore no attributable source.
    // The reference documents it; the capture has none. It degrades to an unknown source rather
    // than falling through, so a fight's damage-taken still adds up.
    let Some((_participle, wearer)) = rest.split_once(" by ") else {
        return Some(Event::Damage(Damage {
            source: Actor::Unknown,
            target: Actor::of(victim),
            amount,
            kind: DamageKind::Shield { effect: rest },
            mods,
        }));
    };

    // Compared as BYTES, not sliced as text. `wearer` is arbitrary content from the log and the
    // log is not pure ASCII, so `&wearer[..5]` panics outright the moment byte five lands inside
    // a multibyte name: `Ürsül's thorns` is a two-byte U at the front and a two-byte u straddling
    // the boundary. A byte slice has no such requirement, and once the first five bytes are known
    // to be `YOUR `, index five is a character boundary by construction.
    let yours = wearer.len() > 5 && wearer.as_bytes()[..5].eq_ignore_ascii_case(b"your ");
    let (source, effect) = if yours {
        (Actor::You, &wearer[5..])
    } else {
        // The *last* `'s ` is the possessive: a name may carry an apostrophe of its own, and
        // the shield's noun never does.
        let (name, effect) = wearer.rsplit_once("'s ")?;
        (Actor::of(name), effect)
    };
    Some(Event::Damage(Damage {
        source,
        target: Actor::of(victim),
        amount,
        kind: DamageKind::Shield { effect },
        mods,
    }))
}

// -------------------------------------------------------------------------------------------
// Swings that missed
// -------------------------------------------------------------------------------------------

/// `<attacker> tries to <verb> [on] <target>, but <outcome>!`
fn avoided<'a>(body: &'a str, mods: Mods<'a>) -> Option<Swing<'a>> {
    let (attacker, rest) = match body.strip_prefix("You try to ") {
        Some(rest) => (Actor::You, rest),
        None => {
            let (name, rest) = body.split_once(" tries to ")?;
            (Actor::of(name), rest)
        }
    };
    let (swing, tail) = rest.split_once(", but ")?;
    let (verb, target) = swing.split_once(' ')?;
    if !attack_verb(verb) {
        return None;
    }
    let target = target.strip_prefix("on ").unwrap_or(target);
    Some(Swing {
        attacker,
        target: Actor::of(target),
        verb,
        outcome: outcome(tail)?,
        mods,
    })
}

/// Which avoidance the tail describes.
///
/// Five outcomes, ten spellings: `miss!`/`misses!`, `parry!`/`parries!`, `dodge!`/`dodges!`,
/// `block!`/`blocks!`, `riposte!`/`ripostes!`. Person agreement is stepped over by
/// [`inflected`] rather than tabulated, which is why the second-person `but YOU block!` works
/// here: the reference parser has a case for `blocks!` and none for `block!`, and drops three
/// of the capture's five block lines because of it.
fn outcome(tail: &str) -> Option<Avoid> {
    let tail = tail.strip_suffix('!')?;
    if tail.ends_with("is INVULNERABLE") {
        return Some(Avoid::Invulnerable);
    }
    if tail.ends_with("magical skin absorbs the blow") {
        return Some(Avoid::RuneAbsorb);
    }
    // `but YOU block with your shield!` puts the weapon after the verb.
    let tail = match tail.find(" with ") {
        Some(i) => &tail[..i],
        None => tail,
    };
    let word = tail.rsplit(' ').next()?;
    [
        ("miss", Avoid::Miss),
        ("parry", Avoid::Parry),
        ("dodge", Avoid::Dodge),
        ("block", Avoid::Block),
        ("riposte", Avoid::Riposte),
    ]
    .into_iter()
    .find_map(|(base, out)| inflected(word, base).then_some(out))
}

// -------------------------------------------------------------------------------------------
// Heals, ticks, spells, deaths, casting
// -------------------------------------------------------------------------------------------

fn spellwork<'a>(body: &'a str, mods: Mods<'a>) -> Option<Event<'a>> {
    if body.contains(" hit point") {
        if let Some(e) = heal(body, mods) {
            return Some(e);
        }
    }
    if body.contains(" taken ") {
        if let Some(e) = dot(body, mods) {
            return Some(e);
        }
    }
    if body.contains(" damage by ") {
        if let Some(e) = direct(body, mods) {
            return Some(e);
        }
    }
    if let Some(e) = death(body) {
        return Some(e);
    }
    casting(body)
}

/// `<healer> healed <target> [over time] for N[ (M)] hit point[s] by <spell>.`
fn heal<'a>(body: &'a str, mods: Mods<'a>) -> Option<Event<'a>> {
    let (head, tail) = body.rsplit_once(" for ")?;
    let (amount, rest) = number(tail)?;
    // The uncapped figure sits *inside* the amount slot, not at the end of the line, so no
    // amount of trailing-paren handling will find it and a plain integer parse stops short.
    let (full, rest) = match rest.strip_prefix(" (") {
        Some(r) => {
            let (n, r) = number(r)?;
            (Some(n), r.strip_prefix(')')?)
        }
        None => (None, rest),
    };
    let rest = strip_hit_points(rest)?;
    let spell = rest.strip_prefix(" by ")?.strip_suffix('.')?;

    let (head, over_time) = match head.strip_suffix(" over time") {
        Some(h) => (h, true),
        None => (head, false),
    };
    // `<target> has been healed for ...` names no healer at all.
    for passive in [" has been healed", " have been healed"] {
        if let Some(target) = head.strip_suffix(passive) {
            return Some(Event::Heal(Heal {
                healer: Actor::Unknown,
                target: Actor::of(target),
                amount,
                full,
                spell,
                over_time,
                mods,
            }));
        }
    }
    let (healer, target) = head.split_once(" healed ")?;
    let healer = Actor::of(healer);
    let target = match target {
        "himself" | "herself" | "itself" | "themselves" => healer,
        other => Actor::of(other),
    };
    Some(Event::Heal(Heal {
        healer,
        target,
        amount,
        full,
        spell,
        over_time,
        mods,
    }))
}

/// ` hit point` with its optional plural. The capture has no one-hit-point heal, so the plural
/// is stepped over rather than assumed, exactly as the damage amount is.
fn strip_hit_points(s: &str) -> Option<&str> {
    let rest = s.strip_prefix(" hit point")?;
    Some(rest.strip_prefix('s').unwrap_or(rest))
}

/// `<victim> has taken N damage from <spell> by <caster>.`
///
/// Note this shape says `damage` and not `points of damage`, and its amount never inflects.
fn dot<'a>(body: &'a str, mods: Mods<'a>) -> Option<Event<'a>> {
    let (victim, rest) = [" has taken ", " have taken "]
        .iter()
        .find_map(|sep| body.split_once(sep))?;
    let (amount, rest) = number(rest)?;
    let rest = rest.strip_prefix(" damage from ")?.strip_suffix('.')?;
    // The last ` by ` is the caster: spell names run to several words, caster names do not
    // contain the delimiter, and a DoT with no `by` clause names no caster at all.
    let (spell, source) = match rest.rsplit_once(" by ") {
        Some((spell, caster)) => (spell, Actor::of(caster)),
        None => (rest, Actor::Unknown),
    };
    Some(Event::Damage(Damage {
        source,
        target: Actor::of(victim),
        amount,
        kind: DamageKind::Dot { spell },
        mods,
    }))
}

/// `<caster> hit <target> for N point[s] of <resist> damage by <spell>.`
///
/// The target pronoun here is a lowercase `you`, where melee writes `YOU` and a damage shield
/// writes `YOUR`. All three fold in [`Actor::of`].
fn direct<'a>(body: &'a str, mods: Mods<'a>) -> Option<Event<'a>> {
    let (left, spell) = body.rsplit_once(" damage by ")?;
    let spell = spell.strip_suffix('.')?;
    let (head, tail) = left.rsplit_once(" for ")?;
    let (amount, rest) = number(tail)?;
    let resist = strip_points(rest)?.strip_prefix(" of ")?;
    let (caster, target) = head.split_once(" hit ")?;
    Some(Event::Damage(Damage {
        source: Actor::of(caster),
        target: Actor::of(target),
        amount,
        kind: DamageKind::Spell { spell, resist },
        mods,
    }))
}

/// The death shapes. They are five unrelated sentences, not one inflection.
fn death(body: &str) -> Option<Event<'_>> {
    if let Some(victim) = body
        .strip_prefix("You have slain ")
        .and_then(|r| r.strip_suffix('!'))
    {
        return Some(Event::Death {
            killer: Actor::You,
            victim: Actor::of(victim),
        });
    }
    if let Some(killer) = body
        .strip_prefix("You have been slain by ")
        .and_then(|r| r.strip_suffix('!'))
    {
        return Some(Event::Death {
            killer: Actor::of(killer),
            victim: Actor::You,
        });
    }
    if let Some(inner) = body.strip_suffix('!') {
        for sep in [" has been slain by ", " was slain by "] {
            if let Some((victim, killer)) = inner.split_once(sep) {
                return Some(Event::Death {
                    killer: Actor::of(killer),
                    victim: Actor::of(victim),
                });
            }
        }
    }
    if let Some(victim) = body.strip_suffix(" died.") {
        return Some(Event::Death {
            killer: Actor::Unknown,
            victim: Actor::of(victim),
        });
    }
    None
}

fn casting(body: &str) -> Option<Event<'_>> {
    if let Some(spell) = body
        .strip_prefix("You begin casting ")
        .and_then(|r| r.strip_suffix('.'))
    {
        return Some(Event::CastStart {
            caster: Actor::You,
            spell,
        });
    }
    if let Some((caster, spell)) = body
        .strip_suffix('.')
        .and_then(|b| b.split_once(" begins casting "))
    {
        return Some(Event::CastStart {
            caster: Actor::of(caster),
            spell,
        });
    }
    if let Some(head) = body.strip_suffix(" spell is interrupted.") {
        return match head.strip_prefix("Your ") {
            Some(spell) => Some(Event::CastInterrupted {
                caster: Actor::You,
                spell,
            }),
            // `<caster>'s <spell> spell is interrupted.` The possessive taken is the last one:
            // a name may carry an apostrophe of its own.
            None => head
                .rsplit_once("'s ")
                .map(|(caster, spell)| Event::CastInterrupted {
                    caster: Actor::of(caster),
                    spell,
                }),
        };
    }
    if let Some((spell, blocked_by)) = body
        .strip_suffix(".)")
        .and_then(|h| h.split_once(" spell did not take hold. (Blocked by "))
    {
        return Some(Event::CastBlocked {
            spell: spell.strip_prefix("Your ").unwrap_or(spell),
            blocked_by,
        });
    }
    if body == "Insufficient Mana to cast this spell!" {
        return Some(Event::CastFailed {
            caster: Actor::You,
            reason: CastFailure::Mana,
        });
    }
    if body == "You regain your concentration and continue your casting." {
        return Some(Event::CastResumed { caster: Actor::You });
    }
    if let Some(rest) = body
        .strip_prefix("You resist ")
        .and_then(|r| r.strip_suffix('!'))
    {
        return rest
            .rsplit_once("'s ")
            .map(|(caster, spell)| Event::Resisted {
                caster: Actor::of(caster),
                target: Actor::You,
                spell,
            });
    }
    if let Some((target, rest)) = body
        .strip_suffix('!')
        .and_then(|b| b.split_once(" resisted "))
    {
        let (caster, spell) = match rest.strip_prefix("your ") {
            Some(spell) => (Actor::You, spell),
            None => match rest.rsplit_once("'s ") {
                Some((name, spell)) => (Actor::of(name), spell),
                None => (Actor::Unknown, rest),
            },
        };
        return Some(Event::Resisted {
            caster,
            target: Actor::of(target),
            spell,
        });
    }
    None
}

// -------------------------------------------------------------------------------------------
// State lines
// -------------------------------------------------------------------------------------------

fn states(body: &str) -> Option<Event<'_>> {
    match body {
        "Auto attack is on." => return Some(Event::AutoAttack { on: true }),
        "Auto attack is off." => return Some(Event::AutoAttack { on: false }),
        "You are stunned!" => return Some(Event::Stun(Stun::Stunned)),
        "You are no longer stunned." => return Some(Event::Stun(Stun::Recovered)),
        "You overcome the stun!" => return Some(Event::Stun(Stun::Overcome)),
        "You avoid the stunning blow." => return Some(Event::Stun(Stun::Avoided)),
        "You have been knocked unconscious!" => return Some(Event::Stun(Stun::KnockedUnconscious)),
        "You can't hit them from here." => return Some(Event::Refused(Refusal::OutOfRange)),
        "You cannot see your target." => return Some(Event::Refused(Refusal::NoLineOfSight)),
        "You no longer have a target." => return Some(Event::Refused(Refusal::LostTarget)),
        "You must first select a target for this ability!" => {
            return Some(Event::Refused(Refusal::NeedsTarget))
        }
        "You must first click on the being you wish to attack!" => {
            return Some(Event::Refused(Refusal::NoTarget))
        }
        _ => {}
    }
    if let Some(name) = body
        .strip_prefix("You activate ")
        .and_then(|r| r.strip_suffix('.'))
    {
        return Some(Event::Ability {
            who: Actor::You,
            name,
        });
    }
    if let Some((who, name)) = body
        .strip_suffix('.')
        .and_then(|b| b.split_once(" activates "))
    {
        return Some(Event::Ability {
            who: Actor::of(who),
            name,
        });
    }
    // The one combat-state line that refers to the reader in the third person, by character
    // name. A `You`-anchored rule misses it, and the name may not be hardcoded: it comes from
    // the log's filename, which this crate is not allowed to see.
    if let Some(who) = body.strip_suffix(" goes into a berserker frenzy!") {
        return Some(Event::Berserk {
            who: Actor::of(who),
            on: true,
        });
    }
    if let Some(who) = body.strip_suffix(" is no longer berserk.") {
        return Some(Event::Berserk {
            who: Actor::of(who),
            on: false,
        });
    }
    if let Some(rest) = body.strip_prefix("Targeted (") {
        let (kind, name) = rest.split_once(')')?;
        let name = name.strip_prefix(": ").or_else(|| name.strip_prefix(' '))?;
        return Some(Event::Targeted {
            kind: match kind {
                "NPC" => TargetKind::Npc,
                "Merchant" => TargetKind::Merchant,
                "Player" => TargetKind::Player,
                _ => TargetKind::Other,
            },
            name,
        });
    }
    if let Some(rest) = body.strip_prefix("You have entered ") {
        // `You have entered an area where levitation does not function.` shares the prefix and
        // means the opposite. Checked before the event is built rather than filtered after.
        if rest.starts_with("an area where") {
            return None;
        }
        return Some(Event::Zone {
            zone: rest.strip_suffix('.')?,
        });
    }
    None
}

// -------------------------------------------------------------------------------------------
// Deliberate drops
// -------------------------------------------------------------------------------------------

/// Speech verbs, base and third person both, because `You tell` and `Poguhy tells` are the same
/// line in two persons.
fn speech_verb(word: &str) -> bool {
    matches!(
        word,
        "say"
            | "says"
            | "tell"
            | "tells"
            | "shout"
            | "shouts"
            | "auction"
            | "auctions"
            | "yell"
            | "yells"
            | "whisper"
            | "whispers"
    )
}

/// Anything in quotes.
fn quoted(body: &str) -> Option<Ignored> {
    if let Some(i) = body.find(", '") {
        if body[..i].split(' ').any(speech_verb) {
            return Some(Ignored::Chat);
        }
    }
    // `<npc> <emote clause>. '<speech>'`. The speaker is the name before the emote verb, so a
    // naive split on ` says, ` would report a speaker called
    // `Guard Sheg smacks the flat of his blade against the palm of his hand and`. Nothing here
    // tries to name them: the line is dropped whole.
    body.contains(". '").then_some(Ignored::Speech)
}

const CHROME: &[&str] = &[
    "Stand close to and right click on ",
    "Manually zooming and panning is disabled",
    "You can't use that command right now",
    "Returning to Zone Safe Point",
    "A mystical path appears before you.",
    "Because you recently requested a path,",
    "You begin reciting the unyielding invocation.",
    "You begin to change your invocation.",
];

fn chaff(body: &str) -> Option<Ignored> {
    match body {
        "LOADING, PLEASE WAIT..." => return Some(Ignored::ZoneLoad),
        "YOU were injured by falling." => return Some(Ignored::FallingCause),
        _ => {}
    }
    if body.starts_with("You gain experience!") {
        return Some(Ignored::Experience);
    }
    if body.starts_with("You have become better at ") {
        return Some(Ignored::SkillUp);
    }
    if body.starts_with("You have gained an ability point!") {
        return Some(Ignored::AbilityPoint);
    }
    if body.starts_with("You have completed achievement:") {
        return Some(Ignored::Achievement);
    }
    if body.starts_with("You looted ")
        || body.starts_with("You have looted ")
        || (body.starts_with("--") && body.ends_with("--"))
        || body.contains(" has looted ")
    {
        return Some(Ignored::Loot);
    }
    if body.starts_with("You receive ") && body.ends_with(" from the corpse.") {
        return Some(Ignored::Coin);
    }
    if body.starts_with("Beginning to memorize ")
        || body.starts_with("You have finished memorizing ")
    {
        return Some(Ignored::Memorise);
    }
    if body.ends_with(" the group.")
        || body.ends_with(" the raid.")
        || body.ends_with(" of your raid.")
    {
        return Some(Ignored::Group);
    }
    // A consider line, recognised by its `--` clause rather than by its `(Lvl: N)`, since the
    // level is exactly the trailing paren group that must never be read as a modifier.
    if body.ends_with(')') && body.contains(" -- ") {
        return Some(Ignored::Consider);
    }
    CHROME
        .iter()
        .any(|p| body.starts_with(p))
        .then_some(Ignored::Chrome)
}

// -------------------------------------------------------------------------------------------
// Spell flavour
// -------------------------------------------------------------------------------------------

/// The sentences a spell prints when it lands on somebody else, as the tail that follows the
/// entity's name.
///
/// These tables are cut from the capture and are not a spell table. They name the classes the
/// game printed during one character's two hours of play; a spell nobody in that capture cast
/// prints a sentence that is not here and lands in [`Reading::Unrecognised`], which is the
/// honest answer. The real fix is a message-to-spell table, which is a licence question rather
/// than a parsing one.
const LANDED_ON_OTHER: &[&str] = &[
    " is bathed in fire.",
    " is engulfed by a swarm.",
    " is slashed by shards of ice.",
    " is surrounded by a divine aura.",
    " adheres to the ground.",
    " has been diseased.",
    "'s skin ignites.",
    "'s skin blisters as fire rains down from above.",
    "'s skin shreds as blades rain down from above.",
    "'s body spasms as the lightning bolt arcs through them.",
    "'s barbed bones glow faintly.",
];

const CROWD_CONTROL: &[&str] = &[
    " staggers.",
    " stumbles.",
    " suffers a blow to the head.",
    "'s legs buckle.",
    " yawns.",
    " looks less aggressive.",
    " begins to choke.",
    " rages.",
];

const HEAL_ON_OTHER: &[&str] = &[
    " feels better.",
    " feels a healing touch.",
    " is healed from within.",
    " is seeded with healing energy.",
];

/// Whole lines, because these carry no entity name at all.
const LANDED_ON_SELF: &[&str] = &[
    "You feel your skin smolder.",
    "You writhe in the grip of agony.",
    "You feel your life force drain away.",
    "You feel smaller.",
    "Your legs feel weak.",
    "You feel much better.",
    "Strength returns to your legs.",
    "You feel your strength return.",
    // The game's own typo for `begin`. It is in the bytes, so it is matched as written.
    "You being to feel healed by the snail.",
];

const BUFF_FADED: &[&str] = &[
    "The brambles fall away.",
    "Your skin returns to normal.",
    "You feel the snail spirit depart.",
];

fn flavour(body: &str) -> Option<Flavour> {
    if LANDED_ON_SELF.contains(&body) {
        return Some(Flavour::LandedOnSelf);
    }
    if body.starts_with("You feel the spirit of ") && body.ends_with(" enter you.") {
        return Some(Flavour::LandedOnSelf);
    }
    if BUFF_FADED.contains(&body) || body.ends_with(" fades.") || body.ends_with(" leaves you.") {
        return Some(Flavour::BuffFaded);
    }
    if HEAL_ON_OTHER.iter().any(|s| body.ends_with(s)) {
        return Some(Flavour::HealOnOther);
    }
    if CROWD_CONTROL.iter().any(|s| body.ends_with(s)) {
        return Some(Flavour::CrowdControl);
    }
    LANDED_ON_OTHER
        .iter()
        .any(|s| body.ends_with(s))
        .then_some(Flavour::LandedOnOther)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_of(raw: &str) -> Reading<'_> {
        parse(raw)
            .unwrap_or_else(|| panic!("no timestamp on {raw}"))
            .reading
    }

    fn damage_of(raw: &str) -> Damage<'_> {
        match read_of(raw) {
            Reading::Event(Event::Damage(d)) => d,
            other => panic!("{raw}\n  -> {other:?}"),
        }
    }

    fn swing_of(raw: &str) -> Swing<'_> {
        match read_of(raw) {
            Reading::Event(Event::Swing(s)) => s,
            other => panic!("{raw}\n  -> {other:?}"),
        }
    }

    /// Prefix a body with a stamp at compile time, so every case below is a `&'static str`
    /// and a test can hold one across statements.
    macro_rules! at {
        ($body:literal $(,)?) => {
            concat!("[Wed Jul 15 23:16:50 2026] ", $body)
        };
    }

    /// Both halves of the melee inflection, and the fact that only the amount tells them apart.
    #[test]
    fn one_point_and_many_points_are_the_same_shape() {
        let one = damage_of(at!(
            "An undead brewer bashes Tanefilo for 1 point of damage."
        ));
        let many = damage_of(at!(
            "An undead brewer bashes Tanefilo for 7 points of damage."
        ));
        assert_eq!(one.amount, 1);
        assert_eq!(many.amount, 7);
        assert_eq!(one.source, Actor::Named("An undead brewer"));
        assert_eq!(one.target, Actor::Named("Tanefilo"));
        assert_eq!(one.kind, DamageKind::Melee { verb: "bashes" });
        assert_eq!(one.kind, many.kind);
    }

    /// The counter-example. The game uses the plural at one point here, so the token must never
    /// be derived from the number in either direction.
    #[test]
    fn hurt_yourself_keeps_the_plural_at_one() {
        let d = damage_of(at!("You hurt yourself for 1 points."));
        assert_eq!(d.amount, 1);
        assert_eq!(d.kind, DamageKind::SelfInflicted);
        assert_eq!(d.target, Actor::You);
        // Named as nobody on purpose: the log gives no attacker and no spell.
        assert_eq!(d.source, Actor::Unknown);
        assert_eq!(damage_of(at!("You hurt yourself for 5 points.")).amount, 5);
    }

    /// The direction that reverses 21 percent of the capture's damage if it is read the natural
    /// way. Both persons of the shape, in both directions.
    #[test]
    fn a_damage_shield_is_credited_to_the_name_that_owns_it() {
        let out = damage_of(at!(
            "A lurking mummy is pierced by Tanefilo's thorns for 7 points of non-melee damage.",
        ));
        assert_eq!(out.source, Actor::Named("Tanefilo"));
        assert_eq!(out.target, Actor::Named("A lurking mummy"));
        assert_eq!(out.kind, DamageKind::Shield { effect: "thorns" });

        let mine = damage_of(at!(
            "A barbed bone skeleton is pierced by YOUR thorns for 7 points of non-melee damage.",
        ));
        assert_eq!(mine.source, Actor::You);
        assert_eq!(mine.target, Actor::Named("A barbed bone skeleton"));

        // The second-person variant ends in a bang where the third person ends in a full stop.
        let onto_me = damage_of(at!(
            "YOU are pierced by a barbed bone skeleton's thorns for 7 points of non-melee damage!",
        ));
        assert_eq!(onto_me.source, Actor::Named("a barbed bone skeleton"));
        assert_eq!(onto_me.target, Actor::You);
    }

    /// A shield line with no possessive names no source. The reference documents it; the
    /// capture has none, so this is specification and not measurement.
    #[test]
    fn a_shield_with_no_wearer_degrades_to_an_unknown_source() {
        let d = damage_of(at!(
            "A dry bone skeleton was chilled to the bone for 410 points of non-melee damage.",
        ));
        assert_eq!(d.source, Actor::Unknown);
        assert_eq!(d.target, Actor::Named("A dry bone skeleton"));
    }

    /// `frenzy` is the only verb that takes a preposition, and a parser that assumes the target
    /// starts one word after the verb reports a mob called `on a dry bone skeleton`.
    #[test]
    fn frenzy_takes_a_preposition_and_the_target_survives_it() {
        let d = damage_of(at!(
            "You frenzy on a dry bone skeleton for 22 points of damage."
        ));
        assert_eq!(d.source, Actor::You);
        assert_eq!(d.target, Actor::Named("a dry bone skeleton"));
        assert_eq!(d.kind, DamageKind::Melee { verb: "frenzy" });

        let s = swing_of(at!("You try to frenzy on a dry bone skeleton, but miss!"));
        assert_eq!(s.target, Actor::Named("a dry bone skeleton"));
        assert_eq!(s.outcome, Avoid::Miss);
    }

    /// Ten spellings, five outcomes. The second-person `block!` is the one the reference parser
    /// drops.
    #[test]
    fn both_persons_of_every_avoidance_land_on_one_outcome() {
        let cases = [
            (
                at!("A lurking mummy tries to punch Tanefilo, but misses!"),
                Avoid::Miss,
            ),
            (
                at!("You try to cleave a barbed bone skeleton, but miss!"),
                Avoid::Miss,
            ),
            (
                at!("A carrion ghoul tries to hit YOU, but YOU block!"),
                Avoid::Block,
            ),
            (
                at!("Torklar Battlemaster tries to punch Fylasem, but Fylasem blocks!"),
                Avoid::Block,
            ),
            (
                at!("A barbed bone skeleton tries to kick YOU, but YOU parry!"),
                Avoid::Parry,
            ),
            (
                at!("A lurking mummy tries to punch Tanefilo, but Tanefilo parries!"),
                Avoid::Parry,
            ),
            (
                at!("A carrion ghoul tries to pierce YOU, but YOU dodge!"),
                Avoid::Dodge,
            ),
            (
                at!("A lurking mummy tries to punch Tanefilo, but Tanefilo dodges!"),
                Avoid::Dodge,
            ),
            (
                at!("A dry bone skeleton tries to punch YOU, but YOU riposte!"),
                Avoid::Riposte,
            ),
            (
                at!("A lurking mummy tries to punch Tanefilo, but Tanefilo ripostes!"),
                Avoid::Riposte,
            ),
        ];
        for (body, want) in cases {
            assert_eq!(swing_of(body).outcome, want, "on {body}");
        }
    }

    /// Shapes the reference documents and this capture does not contain. They are carried as a
    /// hypothesis, and this test is the only thing standing behind them.
    #[test]
    fn the_avoidances_with_no_bytes_behind_them_still_parse() {
        assert_eq!(
            swing_of(at!(
                "A carrion bat tries to bite YOU, but YOU block with your shield!"
            ))
            .outcome,
            Avoid::Block
        );
        assert_eq!(
            swing_of(at!(
                "You try to crush a golem, but a golem is INVULNERABLE!"
            ))
            .outcome,
            Avoid::Invulnerable
        );
        assert_eq!(
            swing_of(at!(
                "A failed reclaimer tries to punch YOU, but YOUR magical skin absorbs the blow!"
            ))
            .outcome,
            Avoid::RuneAbsorb
        );
    }

    /// The verb is a closed set in two persons, and `bash` is not `bash` plus an `s`.
    #[test]
    fn third_person_verbs_are_not_a_plain_plus_s() {
        for (word, base) in [
            ("bashes", "bash"),
            ("punches", "punch"),
            ("crushes", "crush"),
            ("frenzies", "frenzy"),
            ("pierces", "pierce"),
            ("hits", "hit"),
            ("smites", "smite"),
        ] {
            assert!(inflected(word, base), "{word} should inflect from {base}");
            assert!(attack_verb(word) && attack_verb(base));
        }
        assert!(!attack_verb("mummy"));
        assert!(!attack_verb("Battlemaster"));
        assert!(!attack_verb(""));
    }

    /// A name with a space in it stays whole, and one that is a strict prefix of another never
    /// absorbs it.
    #[test]
    fn names_keep_their_spaces_and_their_length() {
        let d = damage_of(at!(
            "Fylasem hit Torklar Battlemaster for 65 points of fire damage by Fire Bolt.",
        ));
        assert_eq!(d.source, Actor::Named("Fylasem"));
        assert_eq!(d.target, Actor::Named("Torklar Battlemaster"));
        assert_eq!(
            d.kind,
            DamageKind::Spell {
                spell: "Fire Bolt",
                resist: "fire"
            }
        );
        assert_ne!(
            damage_of(at!("Tanefi slashes a ghoul for 3 points of damage.")).source,
            damage_of(at!("Tanefilo slashes a ghoul for 3 points of damage.")).source
        );
    }

    /// The heal's second number lives inside the amount slot, not at the end of the line.
    #[test]
    fn a_heal_reads_both_of_its_numbers_and_its_reflexive() {
        let h = match read_of(at!(
            "Tanefilo healed himself for 861 (2101) hit points by Lay on Hands III.",
        )) {
            Reading::Event(Event::Heal(h)) => h,
            other => panic!("{other:?}"),
        };
        assert_eq!((h.amount, h.full), (861, Some(2101)));
        assert_eq!(h.healer, Actor::Named("Tanefilo"));
        assert_eq!(h.target, h.healer);
        assert_eq!(h.spell, "Lay on Hands III");
        assert!(!h.over_time);

        let hot = match read_of(at!(
            "You healed Reviir over time for 5 (10) hit points by Snails Healing.",
        )) {
            Reading::Event(Event::Heal(h)) => h,
            other => panic!("{other:?}"),
        };
        assert!(hot.over_time);
        assert_eq!(hot.healer, Actor::You);
        assert_eq!(hot.target, Actor::Named("Reviir"));

        // A one-hit-point heal has no bytes behind it; the plural is stepped over, not assumed.
        let one = match read_of(at!(
            "Rykabe healed Tanefi for 1 hit point by Light Healing."
        )) {
            Reading::Event(Event::Heal(h)) => h,
            other => panic!("{other:?}"),
        };
        assert_eq!((one.amount, one.full), (1, None));

        // No healer named at all.
        let passive = match read_of(at!(
            "Poguhy has been healed for 15000 hit points by Theft of Essence.",
        )) {
            Reading::Event(Event::Heal(h)) => h,
            other => panic!("{other:?}"),
        };
        assert_eq!(passive.healer, Actor::Unknown);
        assert_eq!(passive.target, Actor::Named("Poguhy"));
    }

    /// The five death sentences. Two are in the capture, three are specification.
    #[test]
    fn every_death_shape_names_the_same_two_slots() {
        let cases: [(&str, Actor, Actor); 5] = [
            (
                at!("A dark boned skeleton has been slain by Poguhy!"),
                Actor::Named("Poguhy"),
                Actor::Named("A dark boned skeleton"),
            ),
            (
                at!("You have slain a dry bone skeleton!"),
                Actor::You,
                Actor::Named("a dry bone skeleton"),
            ),
            (
                at!("You have been slain by Guard Sheg!"),
                Actor::Named("Guard Sheg"),
                Actor::You,
            ),
            (
                at!("Poguhy`s pet was slain by a lurking mummy!"),
                Actor::Named("a lurking mummy"),
                Actor::Named("Poguhy`s pet"),
            ),
            (
                at!("A lurking mummy died."),
                Actor::Unknown,
                Actor::Named("A lurking mummy"),
            ),
        ];
        for (body, want_killer, want_victim) in cases {
            match read_of(body) {
                Reading::Event(Event::Death { killer, victim }) => {
                    assert_eq!((killer, victim), (want_killer, want_victim), "on {body}");
                }
                other => panic!("{body}\n  -> {other:?}"),
            }
        }
    }

    /// A DoT tick says `damage`, not `points of damage`, and takes its caster from the last
    /// `by` clause.
    #[test]
    fn a_dot_tick_credits_the_caster_and_not_the_spell() {
        let d = damage_of(at!(
            "Tanefilo has taken 1 damage from Rabies by a lurking mummy.",
        ));
        assert_eq!(d.source, Actor::Named("a lurking mummy"));
        assert_eq!(d.target, Actor::Named("Tanefilo"));
        assert_eq!(d.kind, DamageKind::Dot { spell: "Rabies" });
        assert_eq!(d.amount, 1);

        // No `by` clause at all: the spell is known, the caster is not, and it is not invented.
        let orphan = damage_of(at!("You have taken 2354 damage from Flashbroil Singe III."));
        assert_eq!(orphan.source, Actor::Unknown);
        assert_eq!(
            orphan.kind,
            DamageKind::Dot {
                spell: "Flashbroil Singe III"
            }
        );
    }

    /// Modifiers ride after the terminal punctuation, on hits and on misses alike, and they are
    /// multi-word.
    #[test]
    fn modifiers_attach_to_hits_and_to_misses() {
        let crit = damage_of(at!(
            "Tanefilo slashes a lurking mummy for 37 points of damage. (Critical)",
        ));
        assert!(crit.mods.critical() && !crit.mods.riposte());
        assert_eq!(crit.amount, 37);

        let slay = damage_of(at!(
            "Tanefilo slashes a lurking mummy for 72 points of damage. (Slay Undead)",
        ));
        assert!(slay.mods.slay_undead() && !slay.mods.critical());
        assert_eq!(slay.mods.raw(), "Slay Undead");

        let missed = swing_of(at!(
            "You try to slash a dry bone skeleton, but miss! (Riposte)",
        ));
        assert!(missed.mods.riposte());
        assert_eq!(missed.outcome, Avoid::Miss);

        // Multi-word and stacked, which this capture never shows but the game does emit.
        let stacked = damage_of(at!(
            "You crush a golem for 20581 points of damage. (Lucky Critical Twincast)",
        ));
        assert!(stacked.mods.critical() && stacked.mods.lucky() && stacked.mods.twincast());
        let crippling = damage_of(at!(
            "You crush a golem for 20581 points of damage. (Crippling Blow)",
        ));
        assert!(crippling.mods.critical());
    }

    /// The trailing-paren shapes that are not modifiers. A generic strip corrupts all of them.
    #[test]
    fn a_trailing_paren_is_not_automatically_a_modifier() {
        assert_eq!(
            read_of(at!(
                "Glorin Binfurr regards you indifferently -- what would you like your \
                 tombstone to say? (Lvl: 35)"
            )),
            Reading::Ignored(Ignored::Consider)
        );
        assert_eq!(
            read_of(at!("You gain experience! (1.898%)")),
            Reading::Ignored(Ignored::Experience)
        );
        assert_eq!(
            read_of(at!("You have become better at Swimming! (52)")),
            Reading::Ignored(Ignored::SkillUp)
        );
        assert_eq!(
            read_of(at!(
                "Your Shield of Fire spell did not take hold. (Blocked by Shield of Barbs.)"
            )),
            Reading::Event(Event::CastBlocked {
                spell: "Shield of Fire",
                blocked_by: "Shield of Barbs"
            })
        );
    }

    /// A player can type any of these. None of them may become an event.
    #[test]
    fn chat_cannot_forge_an_event() {
        let hostile = [
            at!("Mogging tells General:3, 'You slash a dry bone skeleton for 9999 points of damage.'"),
            at!("Dias says, 'A lurking mummy is pierced by Dias's thorns for 9999 points of non-melee damage.'"),
            at!("Poguhy tells you, 'i tried to punch him, but missed!'"),
            at!("You tell general1:2, 'You have slain a dry bone skeleton!'"),
        ];
        for body in hostile {
            assert_eq!(read_of(body), Reading::Ignored(Ignored::Chat), "on {body}");
        }
        // The emote-plus-speech compound, where a naive `says, ` split invents a speaker.
        assert_eq!(
            read_of(at!(
                "Guard Sheg smirks and shakes his head. 'That's what you get for messing with the Freeport Militia!'"
            )),
            Reading::Ignored(Ignored::Speech)
        );
    }

    /// A swing refusal is not a miss: nothing was thrown, so it must stay out of any ratio.
    #[test]
    fn a_refusal_is_not_a_miss() {
        assert_eq!(
            read_of(at!("You can't hit them from here.")),
            Reading::Event(Event::Refused(Refusal::OutOfRange))
        );
        assert_eq!(
            read_of(at!("You cannot see your target.")),
            Reading::Event(Event::Refused(Refusal::NoLineOfSight))
        );
    }

    /// The berserk line names the reader in the third person, by a name this crate cannot know.
    #[test]
    fn berserk_carries_the_name_the_log_used() {
        assert_eq!(
            read_of(at!("Reviir goes into a berserker frenzy!")),
            Reading::Event(Event::Berserk {
                who: Actor::Named("Reviir"),
                on: true
            })
        );
        assert_eq!(
            read_of(at!("Reviir is no longer berserk.")),
            Reading::Event(Event::Berserk {
                who: Actor::Named("Reviir"),
                on: false
            })
        );
    }

    /// A zone change cuts a fight. The line that shares its prefix and means the opposite must
    /// not.
    #[test]
    fn entering_an_area_is_not_entering_a_zone() {
        assert_eq!(
            read_of(at!("You have entered Dagnor's Cauldron.")),
            Reading::Event(Event::Zone {
                zone: "Dagnor's Cauldron"
            })
        );
        assert_ne!(
            read_of(at!(
                "You have entered an area where levitation does not function."
            )),
            Reading::Event(Event::Zone {
                zone: "an area where levitation does not function"
            })
        );
    }

    /// The stamp is found, never assumed at an offset, and a chat line that merely opens with a
    /// bracket is not a stamped line.
    #[test]
    fn the_timestamp_is_located_and_not_assumed() {
        let bracketed = at!(
            "Translocator Fithop says, 'If you need to [travel to Ocean of Tears] I can help.'"
        );
        let e = parse(bracketed).expect("stamped");
        assert_eq!(e.at, "Wed Jul 15 23:16:50 2026");
        assert_eq!(e.reading, Reading::Ignored(Ignored::Chat));

        // A space-padded single-digit day. The capture spans two-digit days only, so this is
        // the shape rule being exercised rather than an observation.
        assert!(parse("[Wed Jul  5 23:16:50 2026] Auto attack is on.").is_some());
        for bad in [
            "[bogus] You have slain a dry bone skeleton!",
            "You have slain a dry bone skeleton!",
            "[Wed Jul 15 23:16:50 2026]",
            "",
        ] {
            assert!(parse(bad).is_none(), "accepted {bad:?}");
        }
    }

    /// CRLF is how these files are written. A trailing carriage return breaks every
    /// terminal-anchored match if it is not trimmed.
    #[test]
    fn a_carriage_return_does_not_defeat_the_terminal_anchors() {
        let d =
            damage_of("[Wed Jul 15 23:16:50 2026] You cleave a ghoul for 54 points of damage.\r\n");
        assert_eq!(d.amount, 54);
        assert_eq!(
            read_of(
                "[Wed Jul 15 23:16:50 2026] A lurking mummy tries to punch Tanefilo, but misses!\r"
            ),
            Reading::Event(Event::Swing(Swing {
                attacker: Actor::Named("A lurking mummy"),
                target: Actor::Named("Tanefilo"),
                verb: "punch",
                outcome: Avoid::Miss,
                mods: Mods::default(),
            }))
        );
    }

    /// The four casings of the same pronoun, one per shape family.
    #[test]
    fn every_casing_of_the_pronoun_folds_to_one_actor() {
        assert_eq!(
            damage_of(at!("You slash a ghoul for 5 points of damage.")).source,
            Actor::You
        );
        assert_eq!(
            damage_of(at!("A ghoul punches YOU for 5 points of damage.")).target,
            Actor::You
        );
        assert_eq!(
            damage_of(at!(
                "a dry bone skeleton hit you for 26 points of fire damage by Dry Bone Fire Burst."
            ))
            .target,
            Actor::You
        );
        assert_eq!(
            damage_of(at!(
                "A ghoul is pierced by YOUR thorns for 7 points of non-melee damage."
            ))
            .source,
            Actor::You
        );
    }

    /// The log is not pure ASCII, and a byte index chosen by arithmetic rather than found as a
    /// delimiter will eventually land inside a codepoint and panic.
    ///
    /// The real capture carries its non-ASCII inside a chat line, which every combat rule skips,
    /// so the coverage test over those bytes cannot reach this. It took a name built for the
    /// purpose: `Ürsül's thorns` puts a two-byte `ü` across byte five, which is exactly where the
    /// damage-shield rule used to slice to test for `YOUR `.
    #[test]
    fn a_multibyte_name_does_not_split_a_codepoint() {
        let d = damage_of(at!(
            "A ghoul is pierced by Ürsül's thorns for 7 points of non-melee damage."
        ));
        assert_eq!(d.source, Actor::Named("Ürsül"));
        assert_eq!(d.target, Actor::Named("A ghoul"));
        assert_eq!(d.kind, DamageKind::Shield { effect: "thorns" });

        // And the same name in every other slot the parser cuts.
        assert_eq!(
            damage_of(at!("Ürsül slashes a ghoul for 5 points of damage.")).source,
            Actor::Named("Ürsül")
        );
        assert_eq!(
            swing_of(at!("Ürsül tries to slash a ghoul, but misses!")).attacker,
            Actor::Named("Ürsül")
        );
        assert_eq!(
            damage_of(at!(
                "Ürsül has taken 3 damage from Rabies by a lurking mummy."
            ))
            .target,
            Actor::Named("Ürsül")
        );

        // Nothing here may panic, whatever the shape turns out to be.
        for body in [
            "Ürsül is pierced by YOUR thorns for 1 point of non-melee damage.",
            "Ürsül healed himself for 3 hit points by Läuterung.",
            "Ürsül has been slain by Ürsül!",
            "Ürsül begins casting Läuterung.",
            "Ürsül",
            "Ü",
            "'",
            "Ürsül tries to slash Ürsül, but Ürsül parries!",
        ] {
            let _ = parse(&format!("[Wed Jul 15 23:16:50 2026] {body}"));
        }
    }

    /// A dead player's still-ticking effect prints a line with the grammar of a melee hit and
    /// the meaning of a spell recourse. `hit` is a legal verb, so the copula veto is the only
    /// thing keeping it from being read as an attacker called `A lurking mummy was`.
    #[test]
    fn a_copula_vetoes_a_melee_match_rather_than_inventing_an_attacker() {
        let r = read_of(at!(
            "A lurking mummy was hit by non-melee for 6734 points of damage.",
        ));
        assert_eq!(
            r,
            Reading::Unrecognised,
            "an unhandled shape must stay unrecognised rather than parse into something false"
        );
    }
}
