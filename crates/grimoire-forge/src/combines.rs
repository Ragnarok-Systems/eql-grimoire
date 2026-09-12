//! `grimoire combines` — what a log knows about crafting.
//!
//! Also the honesty check on the combine model: it prints observed against predicted for
//! every attempt at a pinned trivial, so a drift shows up as a number rather than as a
//! surprise in someone's quote.

use crate::{flag, positionals, read};
use grimoire_core::combine::success_chance;
use grimoire_parse::{harvest, Harvest};

pub fn run(args: &[String]) -> Result<(), String> {
    let files = positionals(args);
    if files.is_empty() {
        return Err("give me at least one eqlog".into());
    }

    let mut all = Harvest::default();
    for f in &files {
        let text = read(f)?;
        let h = harvest(&text);
        println!(
            "{f}\n  {:>7} combine attempts   {:>5} items   {:>3} tradeskills",
            h.attempts.len(),
            h.items.len(),
            h.skills.len()
        );
        merge(&mut all, h);
    }
    if files.len() > 1 {
        println!("\nall logs together");
    }
    report(&all);

    if let Some(p) = flag(args, "--csv") {
        let mut s = String::from("# trivial,skill,attempts,successes\n");
        for (t, sk, n, k) in all.calibration_buckets() {
            s.push_str(&format!("{t},{sk},{n},{k}\n"));
        }
        std::fs::write(&p, s).map_err(|e| format!("{}: {e}", p.display()))?;
        println!("\ncalibration fixture -> {}", p.display());
    }
    if let Some(p) = flag(args, "--json") {
        let out = serde_json::json!({
            "attempts": all.attempts,
            "items": all.items,
        });
        std::fs::write(&p, serde_json::to_vec_pretty(&out).unwrap())
            .map_err(|e| format!("{}: {e}", p.display()))?;
        println!("harvest -> {}", p.display());
    }
    Ok(())
}

fn merge(into: &mut Harvest, from: Harvest) {
    into.events_seen += from.events_seen;
    into.attempts.extend(from.attempts);
    for (k, v) in from.items {
        let e = into.items.entry(k).or_default();
        e.attempts += v.attempts;
        e.successes += v.successes;
        e.skill = e.skill.or(v.skill);
        e.trivial_above = max_opt(e.trivial_above, v.trivial_above);
        e.trivial_at_most = min_opt(e.trivial_at_most, v.trivial_at_most);
    }
    for (k, v) in from.skills {
        let e = into.skills.entry(k).or_insert(v);
        *e = (*e).max(v);
    }
}

fn max_opt(a: Option<u16>, b: Option<u16>) -> Option<u16> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, y) => x.or(y),
    }
}
fn min_opt(a: Option<u16>, b: Option<u16>) -> Option<u16> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, y) => x.or(y),
    }
}

fn report(h: &Harvest) {
    let mut skills: Vec<_> = h.skills.iter().collect();
    skills.sort();
    if !skills.is_empty() {
        println!("\n  skills reached");
        for (s, v) in skills {
            println!("    {:<16} {v}", s.as_str());
        }
    }

    let mut pinned: Vec<(&String, u16)> = h
        .items
        .iter()
        .filter_map(|(k, v)| v.pinned_trivial().map(|t| (k, t)))
        .collect();
    pinned.sort_by_key(|&(_, t)| t);

    if pinned.is_empty() {
        println!("\n  no trivial was pinned — nothing was crafted from under trivial to over it");
    } else {
        println!("\n  trivials this log pins exactly");
        for (name, t) in &pinned {
            let st = &h.items[*name];
            println!(
                "    {:<38} {:>4}   {:>4} attempts  {:>5.0}% landed",
                truncate(name, 38),
                t,
                st.attempts,
                st.success_rate().unwrap_or(0.0) * 100.0
            );
        }
    }

    calibration(h);
}

/// Observed against predicted, banded by `skill − trivial`.
fn calibration(h: &Harvest) {
    let buckets = h.calibration_buckets();
    if buckets.is_empty() {
        return;
    }
    let total: u32 = buckets.iter().map(|&(_, _, n, _)| n).sum();
    println!("\n  the combine model against this log  ({total} attempts at pinned trivials)");
    println!("    skill−trivial      n   observed   model");

    for (lo, hi) in [
        (-200i32, -40i32),
        (-40, -25),
        (-25, -15),
        (-15, -5),
        (-5, 200),
    ] {
        let (mut n, mut k, mut e) = (0.0f64, 0.0f64, 0.0f64);
        for &(t, s, an, ak) in &buckets {
            let d = s as i32 - t as i32;
            if d < lo || d >= hi {
                continue;
            }
            n += an as f64;
            k += ak as f64;
            e += success_chance(s, t) * an as f64;
        }
        if n == 0.0 {
            continue;
        }
        println!(
            "    {:>5}…{:<5}  {:>6}     {:>5.2}   {:>5.2}",
            lo.max(-99),
            hi.min(99),
            n as u32,
            k / n,
            e / n
        );
    }

    let n: f64 = buckets.iter().map(|&(_, _, n, _)| n as f64).sum();
    let k: f64 = buckets.iter().map(|&(_, _, _, k)| k as f64).sum();
    let e: f64 = buckets
        .iter()
        .map(|&(t, s, n, _)| success_chance(s, t) * n as f64)
        .sum();
    let gap = (k / n - e / n).abs();
    println!(
        "    overall          {:>6}     {:>5.2}   {:>5.2}   {}",
        n as u32,
        k / n,
        e / n,
        if gap < 0.06 {
            "model holds"
        } else {
            "MODEL HAS DRIFTED — re-check before trusting a quote"
        }
    );
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}
