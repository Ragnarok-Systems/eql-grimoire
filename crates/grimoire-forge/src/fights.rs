//! `grimoire fights` - what a log knows about who fought what.
//!
//! The workbench end of [`grimoire_parse::fights`]. It reads a real `eqlog_<char>_<server>.txt`
//! off the player's own disk and prints, in one pass, the coverage split and then every fight
//! the log contains: how long it ran, what it was against, and what each participant dealt,
//! took, healed and avoided.
//!
//! THE COVERAGE LINE IS NOT BEHIND A FLAG. A parser that cannot say how much of a file it failed
//! to understand is not reporting, it is guessing, and the owner has to be able to see at a
//! glance whether these numbers rest on 5 percent of their log or 95.
//!
//! The footer is the other half of that. Not every combat line lands in a fight, on purpose:
//! falling off a cliff is not an encounter and a mob dying across the zone is not the reader's
//! kill. Those lines are counted at the bottom rather than quietly vanishing, because
//! "1,770 of 1,779" is checkable and "the fights are these" is not.

use crate::{flag, positionals, read};
use grimoire_parse::combat::{parse, Actor, Event, Reading};
use grimoire_parse::fights::{seconds, Ended, Fight, Fights, Participant, QUIET_SECONDS};
use std::path::Path;

/// How many fights to list before saying how many were left. A two-hour log has four; a raid
/// night has hundreds and nobody reads past the top of that.
const DEFAULT_FIGHTS: usize = 20;

/// How many participants to list inside one fight. The first camp in the reference capture has
/// twenty-three, most of them a mob that swung once.
const DEFAULT_TOP: usize = 10;

/// What the parser made of the file, carried alongside the fights so the two are printed
/// together and cannot drift apart.
#[derive(Default)]
struct Coverage<'a> {
    stamped: u64,
    typed: u64,
    ignored: u64,
    flavour: u64,
    unrecognised: u64,
    /// Lines belonging to one of the four families that can open or extend a fight, whether or
    /// not one was open at the time.
    activity: u64,
    /// Every point of damage in the file, including points that land in no fight.
    damage: u64,
    unreadable: u32,
    first: &'a str,
    last: &'a str,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let files = positionals(args);
    if files.is_empty() {
        return Err("give me at least one eqlog".into());
    }
    let top = number(args, "--top").unwrap_or(DEFAULT_TOP);
    let most = number(args, "--fights").unwrap_or(DEFAULT_FIGHTS);
    let quiet = number(args, "--quiet")
        .map(|n| n as i64)
        .unwrap_or(QUIET_SECONDS);
    let named_me = flag(args, "--me").and_then(|p| p.to_str().map(str::to_string));

    for f in &files {
        let text = read(f)?;
        // The owner's character name is in the FILENAME and never in a line, so the parser
        // cannot know it and the aggregator has to be told. Without it the owner appears twice
        // in every table, once as `you` and once under his own name.
        let me = named_me.clone().or_else(|| owner_of(f));

        let started = std::time::Instant::now();
        let (cover, fights) = harvest(&text, quiet, me.as_deref());
        let elapsed = started.elapsed();

        println!("{f}");
        println!(
            "  {} bytes, {} stamped lines, {:.3}s, {:.0} MB/s",
            text.len(),
            cover.stamped,
            elapsed.as_secs_f64(),
            text.len() as f64 / elapsed.as_secs_f64().max(f64::MIN_POSITIVE) / 1e6
        );
        if !cover.first.is_empty() {
            match session_seconds(cover.first, cover.last) {
                Some(s) => println!("  [{}] .. [{}]   {s}s", cover.first, cover.last),
                None => println!("  [{}] .. [{}]", cover.first, cover.last),
            }
        }
        match &me {
            Some(name) => println!("  reading as {name}, folded together with `you`"),
            None => println!(
                "  no character name given, so `you` and the owner's own name stay separate \
                 rows (pass --me NAME)"
            ),
        }
        coverage(&cover);
        if cover.unreadable > 0 {
            println!(
                "  {} lines carried a stamp this build could not read and were folded into no \
                 fight",
                cover.unreadable
            );
        }
        report(&fights, &cover, quiet, most, top);
    }
    Ok(())
}

/// One forward pass. Everything below borrows out of `text`, so a 61 MB log costs one read and
/// no per-line allocation.
fn harvest<'a>(text: &'a str, quiet: i64, me: Option<&'a str>) -> (Coverage<'a>, Vec<Fight<'a>>) {
    let mut cover = Coverage::default();
    let mut agg = Fights::default().with_quiet(quiet);
    if let Some(name) = me {
        agg = agg.with_owner(name);
    }

    for raw in text.lines() {
        let Some(entry) = parse(raw) else { continue };
        cover.stamped += 1;
        if cover.first.is_empty() {
            cover.first = entry.at;
        }
        cover.last = entry.at;
        match entry.reading {
            Reading::Event(event) => {
                cover.typed += 1;
                if let Event::Damage(d) = event {
                    cover.damage += u64::from(d.amount);
                }
                if opens_or_extends(&event) {
                    cover.activity += 1;
                }
            }
            Reading::Ignored(_) => cover.ignored += 1,
            Reading::Flavour(_) => cover.flavour += 1,
            Reading::Unrecognised => cover.unrecognised += 1,
        }
        agg.push(entry);
    }

    cover.unreadable = agg.unreadable();
    (cover, agg.finish())
}

/// Whether an event is one of the four families that can open or extend a fight. It mirrors the
/// aggregator's grading and is here only to count; it never decides anything.
fn opens_or_extends(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Damage(_) | Event::Swing(_) | Event::Heal(_) | Event::Death { .. }
    )
}

/// The whole session, using the aggregator's own clock so the span in the header and the
/// durations under each fight cannot disagree.
fn session_seconds(first: &str, last: &str) -> Option<i64> {
    Some(seconds(last)? - seconds(first)?)
}

fn coverage(c: &Coverage<'_>) {
    let total = c.typed + c.ignored + c.flavour + c.unrecognised;
    let pct = |n: u64| {
        if total == 0 {
            0.0
        } else {
            100.0 * n as f64 / total as f64
        }
    };
    println!(
        "  {:>7} parsed ({:.1}%)   {:>6} ignored ({:.1}%)   {:>5} spell flavour ({:.1}%)   \
         {:>5} unrecognised ({:.1}%)",
        c.typed,
        pct(c.typed),
        c.ignored,
        pct(c.ignored),
        c.flavour,
        pct(c.flavour),
        c.unrecognised,
        pct(c.unrecognised),
    );
}

fn report(fights: &[Fight<'_>], c: &Coverage<'_>, quiet: i64, most: usize, top: usize) {
    if fights.is_empty() {
        println!("\n  no fights: nothing in this file attacked anything");
        return;
    }
    println!(
        "\n  {} fights, cut where combat goes quiet for more than {quiet}s",
        fights.len()
    );

    for (i, f) in fights.iter().enumerate().take(most) {
        let name = f.headline().unwrap_or("(nothing named)");
        println!(
            "\n  #{:<3} {:<34} {:>5}s {:>7} damage {:>8.1} dps {:>3} deaths   ended: {}",
            i + 1,
            truncate(name, 34),
            f.seconds(),
            f.damage,
            f.dps(),
            f.deaths,
            why(f.ended)
        );
        println!(
            "       [{}] .. [{}]  {} combat lines",
            f.start, f.end, f.lines
        );
        participants(f, top);
    }
    if fights.len() > most {
        println!("\n  ... and {} more fights", fights.len() - most);
    }

    let folded: u64 = fights.iter().map(|f| u64::from(f.lines)).sum();
    let inside: u64 = fights.iter().map(|f| f.damage).sum();
    println!("\n  what did not land in a fight");
    println!("    {:<38} {:>8}", "combat lines in the file", c.activity);
    println!("    {:<38} {:>8}", "folded into a fight", folded);
    println!(
        "    {:<38} {:>8}",
        "left over",
        c.activity.saturating_sub(folded)
    );
    println!("    {:<38} {:>8}", "damage in the file", c.damage);
    println!("    {:<38} {:>8}", "inside a fight", inside);
    println!(
        "    {:<38} {:>8}",
        "outside every fight",
        c.damage.saturating_sub(inside)
    );
    println!(
        "\n  Damage that names an opponent always opens a fight, so anything left over is a heal,\n  \
         a death, or damage the log left unattributed: falling, or `You hurt yourself`. Each of\n  \
         those extends a fight already running but never starts one, and a line of any of them\n  \
         with no fight open is credited to nobody rather than to whoever was nearest."
    );
}

fn participants(f: &Fight<'_>, top: usize) {
    let mut rows: Vec<&Participant<'_>> = f.participants.iter().collect();
    rows.sort_by(|a, b| {
        b.dealt
            .cmp(&a.dealt)
            .then_with(|| b.taken.cmp(&a.taken))
            .then_with(|| label(a).cmp(label(b)))
    });
    println!(
        "       {:<30} {:>8} {:>8} {:>8} {:>7} {:>7} {:>8} {:>8} {:>6} {:>5} {:>4}",
        "entity",
        "dealt",
        "dps",
        "taken",
        "swings",
        "landed",
        "avoided",
        "healed",
        "healed+",
        "kills",
        "died"
    );
    for p in rows.iter().take(top) {
        let landed = if p.swings > 0 {
            format!("{:.0}%", 100.0 * f64::from(p.landed) / f64::from(p.swings))
        } else {
            "-".to_string()
        };
        println!(
            "       {:<30} {:>8} {:>8.1} {:>8} {:>7} {:>7} {:>8} {:>8} {:>6} {:>5} {:>4}",
            truncate(label(p), 30),
            p.dealt,
            f.dps_of(p),
            p.taken,
            p.swings,
            landed,
            p.avoided,
            p.healed,
            p.received,
            p.kills,
            p.deaths
        );
    }
    if rows.len() > top {
        println!("       ... and {} more", rows.len() - top);
    }
}

/// `healed` is what an entity put out and `healed+` is what it took in. Two columns rather than
/// one, because a healer and the person being healed are not the same number read two ways.
fn label<'a>(p: &Participant<'a>) -> &'a str {
    match p.name() {
        Some(n) => n,
        None if matches!(p.who, Actor::You) => "you",
        None => "(unattributed)",
    }
}

fn why(e: Ended) -> &'static str {
    match e {
        Ended::Killed => "everything died",
        Ended::Quiet => "quiet",
        Ended::Zone => "zoned",
        Ended::Backwards => "the clock stepped back",
        Ended::EndOfLog => "the log stopped",
    }
}

/// `eqlog_Reviir_freeport.txt` names the character between the first and last underscore. A file
/// not shaped like that yields nothing rather than a guess, because a wrong name silently merges
/// two people into one row.
fn owner_of(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_str()?;
    let rest = stem.strip_prefix("eqlog_")?;
    let (name, server) = rest.rsplit_once('_')?;
    if name.is_empty() || server.is_empty() || name.contains('_') {
        return None;
    }
    Some(name.to_string())
}

fn number(args: &[String], name: &str) -> Option<usize> {
    flag(args, name).and_then(|p| p.to_str().and_then(|s| s.parse().ok()))
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wrong character name silently merges two people, so the shape has to be exact.
    #[test]
    fn the_character_name_comes_out_of_the_filename_or_not_at_all() {
        assert_eq!(
            owner_of(r"C:\EQ\Logs\eqlog_Reviir_freeport.txt").as_deref(),
            Some("Reviir")
        );
        assert_eq!(
            owner_of("/home/j/eqlog_Tanefilo_legends.txt").as_deref(),
            Some("Tanefilo")
        );
        for not_a_log in [
            "web/fixtures/eqlog-tail-200k.txt",
            "eqlog_freeport.txt",
            "eqlog__freeport.txt",
            "eqlog_Reviir_.txt",
            "eqlog_A_B_C.txt",
            "combat.txt",
            "",
        ] {
            assert_eq!(
                owner_of(not_a_log),
                None,
                "{not_a_log:?} produced a character name out of nothing"
            );
        }
    }
}
