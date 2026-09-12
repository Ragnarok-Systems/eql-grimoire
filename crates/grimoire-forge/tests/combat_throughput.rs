//! The throughput floor, measured against the machine it runs on.
//!
//! The claim is that a full combat parse, attribution included, keeps pace with the hardware: it
//! moves at least [`RATIO_FLOOR`] of the bytes per second that a plain scalar pass over the same
//! bytes moves, on the same machine, in the same build, in the same run.
//!
//! WHY A RATIO AND NOT A NUMBER OF MEGABYTES A SECOND. This file used to assert 200 MB/s, set on
//! the owner's desktop, where the debug build measured 435 to 445 MB/s. The first release cut on a
//! GitHub Windows runner measured the same code at 194 MB/s and stopped the release, with the
//! parser untouched since the floor was written (2026-09-12, run 34722938681). An absolute floor
//! answers "is this machine fast", which is not the question. A ratio against a yardstick timed
//! in the same process answers "did the parser get slower", on whatever box runs it.
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
//! **Wall clock is still wall clock.** A ratio cancels the speed of the machine, not a neighbour
//! stealing it halfway through, which is why the two passes are interleaved and each keeps its
//! best of several rounds. The assertion that holds with no clock at all is the allocation floor
//! in `grimoire-parse/tests/combat_allocations.rs`; this one is the honest timing companion to it.

use grimoire_parse::combat::{parse, DamageKind, Event, Reading};
use std::hint::black_box;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

const FIXTURE: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");

/// Bytes of corpus to build. Enough that a single run is not measuring the clock's resolution.
const TARGET_BYTES: usize = 50 * 1024 * 1024;

/// The floor: parser bytes per second over yardstick bytes per second.
///
/// SET FROM MEASUREMENTS, 2026-09-12, each the best of five interleaved rounds, in the debug build
/// that every gate runs its tests in:
///
/// | machine                          | parser MB/s | yardstick MB/s | ratio         |
/// |----------------------------------|-------------|----------------|---------------|
/// | owner's desktop, seven runs      | 437 to 456  | 565 to 570     | 0.773 - 0.800 |
/// | GitHub ubuntu-latest, CI         | 210         | 178            | 1.183         |
/// | GitHub windows-latest, release   | 189         | 212            | 0.891         |
///
/// The ratio still moves by half again between machines, because the yardstick is one tight
/// multiply loop and CPUs differ in how much they favour that over branchy parsing. That is still
/// much less than the 2.3x the raw megabytes a second moved. 0.60 sits 22% under the lowest ratio
/// seen, so it catches a parser that has slowed by a fifth or more on the desktop, a third on the
/// Windows runner and half on the Linux one, without failing a release because a runner is slow.
///
/// RELEASE PROFILE HAS ITS OWN FLOOR. With optimisations on, the yardstick speeds up far more than
/// the parser: the desktop measured parser 385, yardstick 1386, ratio 0.278 under `--release`, so
/// the debug floor would fail every optimised run for no reason. 0.15 is one measurement with the
/// same kind of margin under it; no gate runs tests in release, so it has not been measured on a
/// runner, and if a gate ever does, that is the measurement to add here.
const RATIO_FLOOR: f64 = if cfg!(debug_assertions) { 0.60 } else { 0.15 };

/// Interleaved rounds of the two passes. Each keeps its best, because a slow sample on a shared
/// box measures the neighbours.
const ROUNDS: usize = 5;

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

/// The yardstick: one scalar pass over every byte, a hash step per byte and a little more per
/// line, so it touches exactly the memory the parser touches.
///
/// WRITTEN HERE AND NOT BORROWED FROM STD. `str::lines`, `memchr` and std's checksums arrive
/// precompiled with optimisations on, while this file and the parser are built in whatever
/// profile the run uses. A yardstick from std would stay fast in a debug build while the parser
/// slowed down, and the ratio would measure the build profile rather than the parser. A loop
/// written in this crate is compiled with the same settings as the code it is measuring.
fn yardstick(text: &str) -> (u64, u64) {
    let mut lines = 0u64;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in text.as_bytes() {
        if b == b'\n' {
            lines += 1;
            hash = hash.rotate_left(5);
        } else {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    (lines, hash)
}

#[test]
fn the_parser_keeps_pace_with_a_plain_pass_over_the_same_bytes() {
    let path = corpus();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let bytes = text.len() as f64;

    // One untimed pass of each, so both are measured on a warm page cache and a warm branch
    // predictor, which is the state a real run is in after the first megabyte.
    black_box(yardstick(black_box(&text)));
    black_box(read_all(black_box(&text)));

    let mut yard_best = f64::MAX;
    let mut parse_best = f64::MAX;
    let mut lines = 0u64;
    for _ in 0..ROUNDS {
        let started = Instant::now();
        black_box(yardstick(black_box(&text)));
        yard_best = yard_best.min(started.elapsed().as_secs_f64());

        let started = Instant::now();
        let (n, sum) = read_all(black_box(&text));
        parse_best = parse_best.min(started.elapsed().as_secs_f64());
        black_box(sum);
        lines = n;
    }

    let parse_rate = bytes / parse_best;
    let yard_rate = bytes / yard_best;
    let ratio = parse_rate / yard_rate;
    let report = format!(
        "{:.1} MB over {lines} lines: parser {:.0} MB/s, yardstick {:.0} MB/s on this machine, \
         ratio {ratio:.3} (floor {RATIO_FLOOR:.3})",
        bytes / 1e6,
        parse_rate / 1e6,
        yard_rate / 1e6,
    );
    /* EVERY RUN ADDS A MEASUREMENT, PASSING OR NOT. The test harness swallows `println!` from a
     * test that passes, so a green CI run would keep none of the numbers the floor is set from.
     * Writing to the stderr handle directly goes around that capture, which only intercepts the
     * print macros, so the line lands in the job log every time. On GitHub it also goes into the
     * run's summary page. Failing to write either is not a test failure: the assertion below is
     * the test. */
    let _ = writeln!(std::io::stderr().lock(), "combat throughput: {report}");
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(summary)
        {
            let _ = writeln!(f, "combat throughput ({}): {report}", std::env::consts::OS);
        }
    }

    assert!(
        ratio >= RATIO_FLOOR,
        "the parser has fallen behind the machine it runs on: {report}"
    );
}
