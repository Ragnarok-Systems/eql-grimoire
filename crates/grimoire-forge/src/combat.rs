//! `grimoire combat` - what a log knows about fighting.
//!
//! The workbench end of [`grimoire_parse::combat`]. It reads a real `eqlog_<char>_<server>.txt`
//! off the player's own disk and prints, in one pass: the span the log covers, the four-way
//! coverage split, damage dealt and taken by entity, how swings ended, which modifiers fired,
//! what was healed and overhealed, and a named ledger of everything that was deliberately
//! dropped.
//!
//! The coverage split and the drop ledger are printed every time rather than hidden behind a
//! flag, because a parser that cannot say how much of a file it failed to understand is not
//! reporting, it is guessing. `--unknown` prints the lines behind the last number.
//!
//! Fight segmentation is not here. Nothing in the log marks a fight boundary, and the only hard
//! markers are the death lines and the zone changes; turning those into fights is its own piece
//! of work and it would be wrong to imply it exists by printing something that looked like it.

use crate::{flag, positionals, read};
use grimoire_parse::combat::{
    parse, Actor, Avoid, CastFailure, DamageKind, Event, Flavour, Ignored, Mods, Reading, Refusal,
    Stun, TargetKind,
};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// An entity name, compared with its letter casing folded away.
///
/// The game sentence-capitalises inconsistently: the same mob appears as both
/// `a dry bone skeleton` and `A dry bone skeleton`, and sixteen lines in the reference capture
/// *begin* with a lowercase article. Folding is whole-string and length-exact, so it can never
/// merge `Tanefi` into `Tanefilo` the way a prefix or substring match would.
#[derive(Clone, Copy, Eq, Debug)]
struct Entity<'a>(&'a str);

impl PartialEq for Entity<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(other.0)
    }
}

impl Hash for Entity<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for b in self.0.bytes() {
            state.write_u8(b.to_ascii_lowercase());
        }
        state.write_u8(0xff);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Who<'a> {
    You,
    Other(Entity<'a>),
    Unknown,
}

impl<'a> Who<'a> {
    /// [`Actor::name`] is `None` for the reader and for an unnamed source, and that is the whole
    /// distinction this type needs: a name is always somebody who is not the reader, because the
    /// log writes the reader as a pronoun and never as a name.
    fn of(a: Actor<'a>) -> Self {
        match a.name() {
            Some(n) => Who::Other(Entity(n)),
            None if matches!(a, Actor::You) => Who::You,
            None => Who::Unknown,
        }
    }

    fn label(self) -> &'a str {
        match self {
            Who::You => "you",
            Who::Other(Entity(n)) => n,
            Who::Unknown => "(unattributed)",
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Totals {
    dealt: u64,
    taken: u64,
    landed: u32,
    swings: u32,
    /// Swings this entity was on the receiving end of and survived without taking damage.
    avoided: u32,
    healed: u64,
    received: u64,
    overhealed: u64,
    kills: u32,
    deaths: u32,
}

/// The six ways damage arrives. Kept apart because they behave differently: a shield fires
/// without a swing, a tick fires without an attacker present, and self damage has no source at
/// all, so folding them into one number hides the thing worth seeing.
const KINDS: [&str; 6] = [
    "melee",
    "damage shield",
    "damage over time",
    "direct spell",
    "self inflicted",
    "environmental",
];

/// Counts of the things that are not damage numbers but change how a log reads.
#[derive(Default)]
struct Tallies<'a> {
    kinds: [(u64, u32); 6],
    /// Damage by the name the line gave it: a weapon verb, a shield's noun, a spell.
    named: HashMap<&'a str, (u64, u32)>,
    /// Healing by spell, and what each one wasted.
    spells: HashMap<&'a str, (u64, u64)>,
    outcomes: [u32; 7],
    mods: [u32; 8],
    other_mods: Vec<String>,
    casts: u32,
    interrupted: u32,
    blocked: u32,
    resumed: u32,
    resisted: u32,
    no_mana: u32,
    abilities: u32,
    auto_attack: u32,
    berserk: u32,
    stuns: [u32; 5],
    refusals: [u32; 5],
    targets: [u32; 4],
    zones: u32,
    hots: u32,
    ignored: HashMap<&'static str, u32>,
    flavour: HashMap<&'static str, u32>,
}

#[derive(Default)]
struct Report<'a> {
    typed: u64,
    ignored: u64,
    flavour: u64,
    unrecognised: Vec<&'a str>,
    by: HashMap<Who<'a>, Totals>,
    tallies: Tallies<'a>,
    lines: u64,
    first: &'a str,
    last: &'a str,
}

impl<'a> Report<'a> {
    fn entry(&mut self, who: Who<'a>) -> &mut Totals {
        self.by.entry(who).or_default()
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let files = positionals(args);
    if files.is_empty() {
        return Err("give me at least one eqlog".into());
    }
    let top: usize = flag(args, "--top")
        .and_then(|p| p.to_str().and_then(|s| s.parse().ok()))
        .unwrap_or(12);
    let show_unknown = args.iter().any(|a| a == "--unknown");

    for f in &files {
        let text = read(f)?;
        let started = std::time::Instant::now();
        let report = harvest(&text);
        let elapsed = started.elapsed();

        println!("{f}");
        println!(
            "  {} bytes, {} stamped lines, {:.3}s, {:.0} MB/s",
            text.len(),
            report.lines,
            elapsed.as_secs_f64(),
            text.len() as f64 / elapsed.as_secs_f64() / 1e6
        );
        if !report.first.is_empty() {
            println!("  [{}] .. [{}]", report.first, report.last);
        }
        coverage(&report);
        entities(&report, top);
        sources(&report.tallies, top);
        swings(&report.tallies);
        healing(&report, top);
        dropped(&report.tallies);

        if show_unknown {
            for line in &report.unrecognised {
                println!("  ? {line}");
            }
        } else if !report.unrecognised.is_empty() {
            println!("\n  run again with --unknown to see the unrecognised lines");
        }
    }
    Ok(())
}

/// One forward pass. Everything below borrows out of `text`, so a 61 MB log costs one read and
/// no per-line allocation.
fn harvest(text: &str) -> Report<'_> {
    let mut r = Report::default();
    for raw in text.lines() {
        let Some(entry) = parse(raw) else { continue };
        r.lines += 1;
        if r.first.is_empty() {
            r.first = entry.at;
        }
        r.last = entry.at;
        match entry.reading {
            Reading::Event(e) => {
                r.typed += 1;
                fold(&mut r, e);
            }
            Reading::Ignored(why) => {
                r.ignored += 1;
                *r.tallies.ignored.entry(ignored_name(why)).or_default() += 1;
            }
            Reading::Flavour(what) => {
                r.flavour += 1;
                *r.tallies.flavour.entry(flavour_name(what)).or_default() += 1;
            }
            Reading::Unrecognised => r.unrecognised.push(raw),
        }
    }
    r
}

fn fold<'a>(r: &mut Report<'a>, event: Event<'a>) {
    match event {
        Event::Damage(d) => {
            let (source, target) = (Who::of(d.source), Who::of(d.target));
            let amount = u64::from(d.amount);
            note_mods(&mut r.tallies, d.mods);
            // The name the line gave this damage. A melee verb, a shield's noun and a spell are
            // different vocabularies, so they are kept as written rather than normalised into a
            // single made-up label.
            let (slot, name) = match d.kind {
                DamageKind::Melee { verb } => (0, Some(verb)),
                DamageKind::Shield { effect } => (1, Some(effect)),
                DamageKind::Dot { spell } => (2, Some(spell)),
                DamageKind::Spell { spell, resist } => {
                    let _ = resist;
                    (3, Some(spell))
                }
                DamageKind::SelfInflicted => (4, None),
                DamageKind::Environmental => (5, None),
            };
            r.tallies.kinds[slot].0 += amount;
            r.tallies.kinds[slot].1 += 1;
            if let Some(name) = name {
                let e = r.tallies.named.entry(name).or_default();
                e.0 += amount;
                e.1 += 1;
            }
            {
                let t = r.entry(source);
                t.dealt += amount;
                if slot == 0 {
                    t.swings += 1;
                    t.landed += 1;
                }
            }
            r.entry(target).taken += amount;
        }
        Event::Swing(s) => {
            note_mods(&mut r.tallies, s.mods);
            r.tallies.outcomes[outcome_slot(s.outcome)] += 1;
            let e = r.tallies.named.entry(s.verb).or_default();
            e.1 += 1;
            r.entry(Who::of(s.attacker)).swings += 1;
            r.entry(Who::of(s.target)).avoided += 1;
        }
        Event::Heal(h) => {
            note_mods(&mut r.tallies, h.mods);
            if h.over_time {
                r.tallies.hots += 1;
            }
            // The parenthesised figure is what the heal *would* have restored. The difference is
            // waste, and it is the number a healer actually wants.
            let waste = u64::from(h.full.unwrap_or(h.amount).saturating_sub(h.amount));
            {
                let e = r.tallies.spells.entry(h.spell).or_default();
                e.0 += u64::from(h.amount);
                e.1 += waste;
            }
            let t = r.entry(Who::of(h.healer));
            t.healed += u64::from(h.amount);
            t.overhealed += waste;
            r.entry(Who::of(h.target)).received += u64::from(h.amount);
        }
        Event::Death { killer, victim } => {
            r.entry(Who::of(killer)).kills += 1;
            r.entry(Who::of(victim)).deaths += 1;
        }
        Event::CastStart { caster, spell } => {
            let _ = (caster, spell);
            r.tallies.casts += 1;
        }
        Event::CastInterrupted { caster, spell } => {
            let _ = (caster, spell);
            r.tallies.interrupted += 1;
        }
        Event::CastBlocked { spell, blocked_by } => {
            let _ = (spell, blocked_by);
            r.tallies.blocked += 1;
        }
        Event::CastFailed { caster, reason } => {
            let _ = caster;
            match reason {
                CastFailure::Mana => r.tallies.no_mana += 1,
            }
        }
        Event::CastResumed { caster } => {
            let _ = caster;
            r.tallies.resumed += 1;
        }
        Event::Resisted {
            caster,
            target,
            spell,
        } => {
            let _ = (caster, target, spell);
            r.tallies.resisted += 1;
        }
        Event::Ability { who, name } => {
            let _ = (who, name);
            r.tallies.abilities += 1;
        }
        Event::AutoAttack { on } => {
            let _ = on;
            r.tallies.auto_attack += 1;
        }
        Event::Berserk { who, on } => {
            let _ = (who, on);
            r.tallies.berserk += 1;
        }
        Event::Stun(state) => {
            r.tallies.stuns[match state {
                Stun::Stunned => 0,
                Stun::Recovered => 1,
                Stun::Overcome => 2,
                Stun::Avoided => 3,
                Stun::KnockedUnconscious => 4,
            }] += 1;
        }
        Event::Refused(why) => {
            r.tallies.refusals[match why {
                Refusal::OutOfRange => 0,
                Refusal::NoLineOfSight => 1,
                Refusal::NoTarget => 2,
                Refusal::NeedsTarget => 3,
                Refusal::LostTarget => 4,
            }] += 1;
        }
        Event::Targeted { kind, name } => {
            let _ = name;
            r.tallies.targets[match kind {
                TargetKind::Npc => 0,
                TargetKind::Merchant => 1,
                TargetKind::Player => 2,
                TargetKind::Other => 3,
            }] += 1;
        }
        Event::Zone { zone } => {
            let _ = zone;
            r.tallies.zones += 1;
        }
    }
}

fn outcome_slot(o: Avoid) -> usize {
    match o {
        Avoid::Miss => 0,
        Avoid::Parry => 1,
        Avoid::Dodge => 2,
        Avoid::Block => 3,
        Avoid::Riposte => 4,
        Avoid::Invulnerable => 5,
        Avoid::RuneAbsorb => 6,
    }
}

/// A modifier group this build does not have a bit for is kept by name rather than rounded to
/// nothing, so an unfamiliar proc shows up as a line to read instead of as silence.
fn note_mods(t: &mut Tallies<'_>, m: Mods<'_>) {
    if m.is_empty() {
        return;
    }
    for (i, set) in [
        m.critical(),
        m.riposte(),
        m.strikethrough(),
        m.slay_undead(),
        m.lucky(),
        m.twincast(),
        m.flurry(),
        m.rampage(),
    ]
    .into_iter()
    .enumerate()
    {
        t.mods[i] += u32::from(set);
    }
    let raw = m.raw();
    if !t.other_mods.iter().any(|k| k == raw) && t.other_mods.len() < 32 {
        t.other_mods.push(raw.to_string());
    }
}

fn ignored_name(why: Ignored) -> &'static str {
    match why {
        Ignored::Chat => "chat",
        Ignored::Speech => "npc speech",
        Ignored::Experience => "experience",
        Ignored::SkillUp => "skill-up",
        Ignored::AbilityPoint => "ability point",
        Ignored::Achievement => "achievement",
        Ignored::Loot => "loot",
        Ignored::Coin => "coin",
        Ignored::Consider => "consider",
        Ignored::Memorise => "memorisation",
        Ignored::ZoneLoad => "zone load",
        Ignored::Group => "group",
        Ignored::FallingCause => "falling (cause label)",
        Ignored::Chrome => "client chrome",
    }
}

fn flavour_name(what: Flavour) -> &'static str {
    match what {
        Flavour::LandedOnOther => "spell landed on other",
        Flavour::LandedOnSelf => "spell landed on self",
        Flavour::CrowdControl => "crowd control",
        Flavour::HealOnOther => "heal landed on other",
        Flavour::BuffFaded => "buff faded",
    }
}

fn coverage(r: &Report<'_>) {
    let total = r.typed + r.ignored + r.flavour + r.unrecognised.len() as u64;
    let pct = |n: u64| {
        if total == 0 {
            0.0
        } else {
            100.0 * n as f64 / total as f64
        }
    };
    println!(
        "  {:>7} typed ({:.1}%)   {:>6} ignored ({:.1}%)   {:>5} spell flavour ({:.1}%)   \
         {:>5} unrecognised ({:.1}%)",
        r.typed,
        pct(r.typed),
        r.ignored,
        pct(r.ignored),
        r.flavour,
        pct(r.flavour),
        r.unrecognised.len(),
        pct(r.unrecognised.len() as u64),
    );
}

fn entities(r: &Report<'_>, top: usize) {
    let mut rows: Vec<(&Who, &Totals)> = r.by.iter().collect();
    rows.sort_by_key(|(w, t)| (std::cmp::Reverse(t.dealt + t.taken), w.label()));
    if rows.is_empty() {
        println!("\n  nothing fought in this file");
        return;
    }
    println!(
        "\n  {:<32} {:>10} {:>10} {:>7} {:>6} {:>8} {:>7} {:>5} {:>5}",
        "entity", "dealt", "taken", "swings", "landed", "avoided", "healed", "kills", "died"
    );
    for (who, t) in rows.iter().take(top) {
        let landed = if t.swings > 0 {
            format!("{:.0}%", 100.0 * f64::from(t.landed) / f64::from(t.swings))
        } else {
            "-".to_string()
        };
        println!(
            "  {:<32} {:>10} {:>10} {:>7} {:>6} {:>8} {:>7} {:>5} {:>5}",
            truncate(who.label(), 32),
            t.dealt,
            t.taken,
            t.swings,
            landed,
            t.avoided,
            t.healed,
            t.kills,
            t.deaths
        );
    }
    if rows.len() > top {
        println!("  ... and {} more", rows.len() - top);
    }
}

fn swings(t: &Tallies<'_>) {
    let names = [
        "missed",
        "parried",
        "dodged",
        "blocked",
        "riposted",
        "invulnerable",
        "absorbed",
    ];
    let avoided: u32 = t.outcomes.iter().sum();
    if avoided == 0 && t.mods.iter().all(|&n| n == 0) {
        return;
    }
    println!("\n  swings that produced no damage");
    for (name, n) in names.iter().zip(t.outcomes) {
        if n > 0 {
            println!("    {name:<14} {n:>6}");
        }
    }
    let mod_names = [
        "critical",
        "riposte",
        "strikethrough",
        "slay undead",
        "lucky",
        "twincast",
        "flurry",
        "rampage",
    ];
    if t.mods.iter().any(|&n| n > 0) {
        println!("\n  modifiers");
        for (name, n) in mod_names.iter().zip(t.mods) {
            if n > 0 {
                println!("    {name:<14} {n:>6}");
            }
        }
        if !t.other_mods.is_empty() {
            println!("    groups seen: {}", t.other_mods.join(", "));
        }
    }
}

/// Where the damage came from, by mechanism and then by the name the game used.
fn sources(t: &Tallies<'_>, top: usize) {
    if t.kinds.iter().all(|&(sum, n)| sum == 0 && n == 0) {
        return;
    }
    println!("\n  {:<18} {:>12} {:>8}", "damage by kind", "total", "hits");
    for (name, (sum, n)) in KINDS.iter().zip(t.kinds) {
        if n > 0 {
            println!("    {name:<16} {sum:>12} {n:>8}");
        }
    }
    let mut rows: Vec<(&&str, &(u64, u32))> = t.named.iter().collect();
    rows.sort_by_key(|(name, (sum, _))| (std::cmp::Reverse(*sum), **name));
    println!("\n  {:<26} {:>12} {:>8}", "by name", "total", "lines");
    for (name, (sum, n)) in rows.iter().take(top) {
        println!("    {:<24} {sum:>12} {n:>8}", truncate(name, 24));
    }
    if rows.len() > top {
        println!("    ... and {} more", rows.len() - top);
    }
}

fn healing(r: &Report<'_>, top: usize) {
    let healed: u64 = r.by.values().map(|t| t.healed).sum();
    if healed == 0 {
        return;
    }
    let wasted: u64 = r.by.values().map(|t| t.overhealed).sum();
    let received: u64 = r.by.values().map(|t| t.received).sum();
    println!(
        "\n  healing: {healed} landed on {received} received, {wasted} overhealed, {} over time",
        r.tallies.hots
    );
    let mut rows: Vec<(&&str, &(u64, u64))> = r.tallies.spells.iter().collect();
    rows.sort_by_key(|(name, (sum, _))| (std::cmp::Reverse(*sum), **name));
    for (name, (sum, waste)) in rows.iter().take(top) {
        println!(
            "    {:<24} {sum:>10} landed {waste:>8} wasted",
            truncate(name, 24)
        );
    }
}

fn dropped(t: &Tallies<'_>) {
    let line = |label: &str, n: u32| {
        if n > 0 {
            println!("    {label:<24} {n:>6}");
        }
    };
    println!("\n  other events");
    line("casts started", t.casts);
    line("casts interrupted", t.interrupted);
    line("casts blocked", t.blocked);
    line("casts resumed", t.resumed);
    line("spells resisted", t.resisted);
    line("out of mana", t.no_mana);
    line("abilities used", t.abilities);
    line("auto attack toggled", t.auto_attack);
    line("berserk toggled", t.berserk);
    line("zone changes", t.zones);
    for (label, n) in [
        "stunned",
        "no longer stunned",
        "overcame the stun",
        "avoided the stun",
        "knocked unconscious",
    ]
    .iter()
    .zip(t.stuns)
    {
        line(label, n);
    }
    for (label, n) in [
        "refused: out of range",
        "refused: no line of sight",
        "refused: no target",
        "refused: needs a target",
        "refused: lost the target",
    ]
    .iter()
    .zip(t.refusals)
    {
        line(label, n);
    }
    for (label, n) in [
        "declared NPC",
        "declared merchant",
        "declared player",
        "declared other",
    ]
    .iter()
    .zip(t.targets)
    {
        line(label, n);
    }

    if !t.ignored.is_empty() {
        println!("\n  deliberately dropped, by reason");
        let mut rows: Vec<_> = t.ignored.iter().collect();
        rows.sort_by_key(|(k, n)| (std::cmp::Reverse(**n), **k));
        for (k, n) in rows {
            println!("    {k:<24} {n:>6}");
        }
    }
    if !t.flavour.is_empty() {
        println!(
            "\n  recognised but not attributable, by class\n  \
             (the game prints a per-spell English sentence with no spell name and no number, so \
             naming these\n  needs a message-to-spell table this build does not have)"
        );
        let mut rows: Vec<_> = t.flavour.iter().collect();
        rows.sort_by_key(|(k, n)| (std::cmp::Reverse(**n), **k));
        for (k, n) in rows {
            println!("    {k:<24} {n:>6}");
        }
    }
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}
