//! The throughput floor: 200 MB/s over a real corpus, attribution included.
//!
//! The reference point is the existing crafting scan, which runs at 591 MB/s and would chew a
//! 61 MB log in 0.10s. A full combat parse does more work per line, so the floor sits lower,
//! but it is a floor and it ships as a test that goes red rather than as a note in a README:
//! 200 MB/s is the whole 61 MB log in under 0.31 seconds.
//!
//! The corpus is the real capture repeated to about 50 MB in the system temp directory. It is
//! not committed: 50 MB of duplicated bytes in a repository buys nothing that the generator does
//! not, and the bytes it is built from are the same real bytes the coverage test reads. It is
//! written once and reused, so a second run of the suite pays for the parse and not for the
//! write.
//!
//! This test lives under `grimoire-forge` because that is the crate the purity gate allows
//! `fs`, `env` and `time`. `grimoire-parse` may not touch any of the three, which is also why
//! the parser cannot time itself.
//!
//! **Wall clock is not deterministic.** A loaded box will produce a smaller number than an idle
//! one, and this test will say so with the number in the message rather than pretending
//! otherwise. The assertion that holds everywhere is the allocation floor in
//! `grimoire-parse/tests/combat_allocations.rs`; this one is the honest timing companion to it.

use grimoire_parse::combat::{parse, DamageKind, Event, Reading};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

const FIXTURE: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");

/// Bytes of corpus to build. Enough that a single run is not measuring the clock's resolution.
const TARGET_BYTES: usize = 50 * 1024 * 1024;

/// The floor, in bytes per second.
const FLOOR: f64 = 200.0 * 1e6;

/// Build (or reuse) the repeated corpus and return its path and size.
///
/// The provenance header is stripped first: it is 29 comment lines describing the cut, and
/// repeating it 245 times would measure the parser rejecting comments rather than reading
/// combat.
fn corpus() -> PathBuf {
    let body: String = FIXTURE
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .fold(String::new(), |mut acc, l| {
            acc.push_str(l);
            acc.push('\n');
            acc
        });

    let path = std::env::temp_dir().join(format!("eql-grimoire-combat-corpus-{TARGET_BYTES}.log"));
    let want = (TARGET_BYTES / body.len() + 1) * body.len();
    let already = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if already != want as u64 {
        let mut out = String::with_capacity(want);
        while out.len() < want {
            out.push_str(&body);
        }
        std::fs::write(&path, &out).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
    }
    path
}

/// One full read: every line parsed, every event attributed into a running total. The fold is
/// what makes this a parse-and-attribute measurement rather than a parse-and-discard one.
fn read_all(text: &str) -> (u64, u64) {
    let mut lines = 0u64;
    let mut damage = 0u64;
    for raw in text.lines() {
        let Some(entry) = parse(raw) else { continue };
        lines += 1;
        if let Reading::Event(Event::Damage(d)) = entry.reading {
            damage += u64::from(d.amount);
            damage = damage.wrapping_add(match d.kind {
                DamageKind::Melee { verb } => verb.len() as u64,
                DamageKind::Shield { effect } => effect.len() as u64,
                DamageKind::Dot { spell } => spell.len() as u64,
                DamageKind::Spell { spell, .. } => spell.len() as u64,
                DamageKind::SelfInflicted | DamageKind::Environmental => 0,
            });
            if let Some(n) = d.source.name() {
                damage = damage.wrapping_add(n.len() as u64);
            }
        }
    }
    (lines, damage)
}

#[test]
fn the_parser_clears_two_hundred_megabytes_a_second() {
    let path = corpus();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let bytes = text.len();

    // One untimed pass so the measurement is of a warm page cache and a warm branch predictor,
    // which is the state a real run is in after the first megabyte.
    black_box(read_all(&text));

    // Three runs, best of. A single sample on a shared box measures the neighbours.
    let mut best = f64::MAX;
    let mut lines = 0u64;
    for _ in 0..3 {
        let started = Instant::now();
        let (n, sum) = read_all(&text);
        let elapsed = started.elapsed().as_secs_f64();
        black_box(sum);
        lines = n;
        best = best.min(elapsed);
    }

    let rate = bytes as f64 / best;
    let report = format!(
        "{:.1} MB over {lines} lines in {best:.4}s = {:.0} MB/s \
         (floor {:.0} MB/s; a 61 MB log would take {:.3}s)",
        bytes as f64 / 1e6,
        rate / 1e6,
        FLOOR / 1e6,
        61.5e6 / rate,
    );
    println!("{report}");

    assert!(rate >= FLOOR, "throughput below its floor: {report}");
}
