//! Does the combine model still describe the actual game?
//!
//! Runs against `pinned_combines.csv` — 343 real attempts at eleven trivials the log pins
//! exactly. This is the test that stops someone "improving" the formula into something that
//! no longer matches EverQuest Legends.
//!
//! Re-cut the fixture from a fresh log with `grimoire-forge combines`.

use grimoire_core::combine::success_chance;

struct Bucket {
    trivial: u16,
    skill: u16,
    attempts: u32,
    successes: u32,
}

fn fixture() -> Vec<Bucket> {
    include_str!("pinned_combines.csv")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            assert_eq!(f.len(), 4, "malformed fixture row: {l}");
            Bucket {
                trivial: f[0].parse().unwrap(),
                skill: f[1].parse().unwrap(),
                attempts: f[2].parse().unwrap(),
                successes: f[3].parse().unwrap(),
            }
        })
        .collect()
}

fn log_likelihood(rows: &[Bucket]) -> f64 {
    rows.iter()
        .map(|b| {
            let p = success_chance(b.skill, b.trivial).clamp(1e-6, 1.0 - 1e-6);
            let k = b.successes as f64;
            let n = b.attempts as f64;
            k * p.ln() + (n - k) * (1.0 - p).ln()
        })
        .sum()
}

#[test]
fn fixture_is_the_dataset_we_think_it_is() {
    let rows = fixture();
    let attempts: u32 = rows.iter().map(|b| b.attempts).sum();
    assert_eq!(attempts, 343, "fixture changed size");
    assert!(rows.iter().all(|b| b.successes <= b.attempts));
}

/// Aggregate calibration by `skill − trivial` band. Each band is a few dozen attempts, so
/// the tolerance is wide on purpose — this catches a model that has stopped describing the
/// game, not a model that is a couple of points off.
#[test]
fn calibrated_across_the_difficulty_range() {
    let rows = fixture();
    let bands: [(i32, i32); 5] = [(-90, -40), (-40, -25), (-25, -15), (-15, -5), (-5, 20)];

    for (lo, hi) in bands {
        let mut n = 0.0;
        let mut k = 0.0;
        let mut expected = 0.0;
        for b in &rows {
            let d = b.skill as i32 - b.trivial as i32;
            if d < lo || d >= hi {
                continue;
            }
            n += b.attempts as f64;
            k += b.successes as f64;
            expected += success_chance(b.skill, b.trivial) * b.attempts as f64;
        }
        if n < 20.0 {
            continue;
        }
        let observed = k / n;
        let predicted = expected / n;
        assert!(
            (observed - predicted).abs() < 0.12,
            "band {lo}..{hi}: n={n} observed {observed:.2}, model {predicted:.2}"
        );
    }
}

/// The classic formula has no free parameters. A two-parameter curve fitted directly to
/// this data scores −202.5, so anything materially worse than that means the model has
/// drifted away from the game.
#[test]
fn likelihood_has_not_regressed() {
    let ll = log_likelihood(&fixture());
    assert!(
        ll > -215.0,
        "log-likelihood {ll:.1} — was −200.4 when measured; the model has drifted"
    );
}

/// Overall hit rate. 343 attempts, 63% of them successful.
#[test]
fn overall_success_rate_is_predicted() {
    let rows = fixture();
    let n: f64 = rows.iter().map(|b| b.attempts as f64).sum();
    let k: f64 = rows.iter().map(|b| b.successes as f64).sum();
    let e: f64 = rows
        .iter()
        .map(|b| success_chance(b.skill, b.trivial) * b.attempts as f64)
        .sum();
    let (observed, predicted) = (k / n, e / n);
    assert!(
        (observed - predicted).abs() < 0.06,
        "observed {observed:.3} vs predicted {predicted:.3}"
    );
}
