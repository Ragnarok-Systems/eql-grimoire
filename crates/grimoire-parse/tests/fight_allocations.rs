//! What fight aggregation is allowed to allocate, asserted with a counting global allocator.
//!
//! The line parser's floor is zero allocations per line, and its own test binary proves that.
//! An aggregator cannot make that claim honestly: it has to keep the fights it finds and the
//! participants in them, so it must allocate for those. The claim it CAN make, and the one that
//! matters, is that the cost is per fight and per participant and never per line. A 61 MB log is
//! six million lines and a few hundred fights; anything that allocates per line turns a scan
//! into a heap.
//!
//! So the assertion here is precisely: **folding four times as many lines into the same fights
//! costs exactly the same number of allocations.** Allocation counts are deterministic where
//! wall clock is not, so this is still true on a CI box running eleven other jobs.
//!
//! `unsafe` appears once, in the counting allocator, which is measurement scaffolding and never
//! ships. `grimoire-parse` itself carries `#![forbid(unsafe_code)]`.

use grimoire_parse::combat::parse;
use grimoire_parse::fights::Fights;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Only ever read to prove the allocator is wired in at all. The MEASUREMENT is the thread-local
/// below; see [`counted`] for why a shared counter cannot be the measurement.
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Armed per thread: `cargo test` runs this binary's tests concurrently and the harness
    /// allocates on its own thread. The initialiser is `const` and the cell has no destructor,
    /// so reading it from inside the allocator cannot itself allocate or recurse.
    static ARMED: Cell<bool> = const { Cell::new(false) };
    /// Counted per thread as well, and this is the part that is easy to get wrong.
    ///
    /// Arming per thread while counting into one shared integer does NOT isolate the
    /// measurement: another test, armed on its own thread, adds to the same integer inside your
    /// window, and the floor becomes a race. That is not theoretical. The first version of this
    /// file did exactly that and the disarmed check below failed on the first run with 109
    /// against 77, which is another test's 4 KB allocation landing in the middle of it.
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = here();
    ARMED.with(|a| a.set(true));
    let out = f();
    ARMED.with(|a| a.set(false));
    (out, here() - before)
}

/// This thread's allocation count.
fn here() -> usize {
    COUNT.try_with(Cell::get).unwrap_or(0)
}

struct Counting;

impl Counting {
    fn note() {
        if ARMED.try_with(Cell::get).unwrap_or(false) {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// SAFETY: every call is forwarded unchanged to the system allocator; the only addition is a
// relaxed counter increment behind a thread-local flag, which cannot affect the allocation.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        Self::note();
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A realloc counts. A participant list doubling as it grows is exactly the cost this
        // test exists to bound, and it must not be invisible.
        Self::note();
        System.realloc(ptr, layout, new_size)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        Self::note();
        System.alloc_zeroed(layout)
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// One second of a fight: three combatants, four shapes, all lifted from the real capture.
const BEAT: [&str; 4] = [
    "You slash a dry bone skeleton for 20 points of damage.",
    "A lurking mummy punches Tanefilo for 6 points of damage.",
    "A lurking mummy is pierced by Tanefilo's thorns for 7 points of non-melee damage.",
    "You try to cleave a barbed bone skeleton, but miss!",
];

/// `blocks` runs of combat, `beats` lines each, every run far enough from the last to be its own
/// fight. Built OUTSIDE the counted region so the only thing being measured is the fold.
///
/// Time must move forward and only forward. The first version of this generator cycled the
/// seconds column, which walks the clock backwards every fifth line, and the aggregator
/// correctly cut a fight at each step back: four blocks came out as thirty-two fights. The
/// generator was wrong, not the aggregator, and the assertion that the two runs produce the same
/// fights is what caught it.
/// The same, with the lines-per-second density as a knob.
///
/// IT EXISTS TO HOLD DURATION FIXED WHILE LINES CHANGE. `log` puts ten lines in every second,
/// so asking it for more lines asks for a longer fight, and a longer fight legitimately costs
/// more now that a participant carries a per-second series. Separating the two is the only way
/// to still test what the per-line rule was always about.
fn log_dense(blocks: usize, seconds: usize, per_sec: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(blocks * seconds * per_sec);
    for b in 0..blocks {
        let base = b as i64 * 200;
        for s in 0..seconds {
            for i in 0..per_sec {
                let t = base + s as i64;
                out.push(format!(
                    "[Wed Jul 15 {:02}:{:02}:{:02} 2026] {}",
                    t / 3600,
                    (t / 60) % 60,
                    t % 60,
                    BEAT[(s * per_sec + i) % BEAT.len()]
                ));
            }
        }
    }
    out
}

fn log(blocks: usize, beats: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(blocks * beats);
    for b in 0..blocks {
        // 200 seconds between blocks, far outside any quiet window, so each block is one fight.
        let base = b as i64 * 200;
        for i in 0..beats {
            // Ten lines to the second, which is inside the range the real capture shows.
            let t = base + i as i64 / 10;
            out.push(format!(
                "[Wed Jul 15 {:02}:{:02}:{:02} 2026] {}",
                t / 3600,
                (t / 60) % 60,
                t % 60,
                BEAT[i % BEAT.len()]
            ));
        }
    }
    out
}

/// Fold a prepared log and report `(fights, allocations)`.
fn fold(lines: &[String]) -> (usize, usize) {
    let (fights, allocs) = counted(|| {
        let mut f = Fights::new();
        for raw in lines {
            if let Some(e) = parse(raw) {
                f.push(e);
            }
        }
        let done = f.finish();
        black_box(done.iter().map(|x| x.damage).sum::<u64>());
        done.len()
    });
    (fights, allocs)
}

/// THE FLOOR. Same fights, four times the lines, identical allocation count.
#[test]
fn folding_more_lines_into_the_same_fights_does_not_allocate_more() {
    // Warm anything the first call sets up lazily, so this is steady state.
    let _ = fold(&log_dense(4, 4, 10));

    /* THE SAME FOUR SECONDS EACH TIME, AT FOUR TIMES THE DENSITY. Holding the DURATION fixed
     * is what makes this a test of per-LINE cost, and it did not used to matter: before a
     * participant carried a per-second series, asking `log` for more lines also asked for a
     * longer fight and the two were indistinguishable. They are not any more, and this is the
     * half that still has to be zero. The capture really does put up to 32 lines in one
     * printed second, so this shape is measured and not invented. */
    let short = log_dense(4, 4, 10);
    let long = log_dense(4, 4, 40);
    let (short_fights, short_allocs) = fold(&short);
    let (long_fights, long_allocs) = fold(&long);

    assert_eq!(
        (short_fights, long_fights),
        (4, 4),
        "the two runs must produce the same fights for the comparison to mean anything"
    );

    let per_line = (long_allocs as f64 - short_allocs as f64) / (long.len() - short.len()) as f64;
    let report = format!(
        "{} lines in {short_fights} fights allocated {short_allocs} times; {} lines in the same \
         {long_fights} fights allocated {long_allocs} times; growth is {per_line:.6} allocations \
         per additional line",
        short.len(),
        long.len()
    );
    println!("{report}");

    assert_eq!(
        long_allocs, short_allocs,
        "the aggregator allocates per line, not per fight: {report}"
    );
}

/// And the cost that IS paid is per fight, bounded, and visible. Not an aspiration: if this ever
/// stops being linear in fights the number here moves and the test says so.
///
/// THE CEILING MOVED FROM 8 TO 12 ON 2026-09-05, AND THIS IS THE RECORD OF WHY.
///
/// It went to 9 for the three per-participant maps, then to 12 for the per-second `series` a
/// timeline needs. Measured 11.07. The ceiling is set just above, not at a round number: a
/// gate with slack in it is not a gate.
///
/// `Participant` gained three maps (`by_name`, `by_target`, `by_school`) so that an Ability
/// Breakdown, a Targets panel and an element split can be drawn without parsing the log a second
/// time. Each is a `Vec` and each allocates the first time something is pushed into it, so a fight
/// now costs a little more than it did: measured 8.07 against the old ceiling of 8.00.
///
/// IT IS STILL LINEAR IN FIGHTS, which is the property this test actually defends. The maps are
/// per participant and they are LAZY: `Vec::new` allocates nothing, so a participant that only ever
/// TAKES damage still costs zero, and a fight's cost is the number of entities that actually dealt
/// something. Nothing here allocates per LINE, which is what the test above this one pins.
///
/// THE HEADROOM IS ONE ALLOCATION AND NOT TEN, deliberately. A ceiling raised to a comfortable
/// round number stops being a gate; this one is set just above the measurement so the next thing
/// that grows the per-fight cost has to come here and say what it was for.
#[test]
fn the_cost_that_is_paid_is_per_fight() {
    let _ = fold(&log(4, 40));

    let (few, few_allocs) = fold(&log(4, 40));
    let (many, many_allocs) = fold(&log(64, 40));
    assert_eq!((few, many), (4, 64));

    let per_fight = (many_allocs - few_allocs) as f64 / (many - few) as f64;
    println!(
        "{few} fights cost {few_allocs} allocations, {many} fights cost {many_allocs}; \
         {per_fight:.2} per additional fight"
    );
    assert!(
        many_allocs > few_allocs,
        "more fights cost nothing, which means the counter is not seeing the fold at all"
    );
    assert!(
        per_fight <= 12.0,
        "a fight costs {per_fight:.2} allocations, which is more than a participant list, its \
         three per-participant maps and a push should need. See the note above: the ceiling is set \
         just above the measurement on purpose, so raising it is a decision somebody has to write \
         down rather than a number that drifts"
    );
}

/// The counter is real. Without this, the two tests above would pass just as well with an
/// allocator that counted nothing.
#[test]
fn the_counting_allocator_actually_counts() {
    let (_, seen) = counted(|| {
        let v: Vec<u8> = black_box(Vec::with_capacity(4096));
        black_box(&v);
    });
    assert!(seen > 0, "a 4 KB allocation went unnoticed");
    assert!(
        ALLOCATIONS.load(Ordering::Relaxed) > 0,
        "the global allocator hook never fired, so nothing here is measuring anything"
    );

    let before = here();
    let v: Vec<u8> = black_box(Vec::with_capacity(4096));
    black_box(&v);
    assert_eq!(
        here(),
        before,
        "the counter fired while disarmed, so a neighbouring test can pollute the floor"
    );
}

/// Names in a fight are still slices of the caller's buffer. Aggregation copies nothing.
#[test]
fn participant_names_point_back_into_the_input() {
    let raw =
        "[Wed Jul 15 23:17:30 2026] A lurking mummy is pierced by Tanefilo's thorns for 7 points \
         of non-melee damage.";
    let mut f = Fights::new();
    f.push(parse(raw).expect("stamped"));
    let fights = f.finish();
    let span = raw.as_bytes().as_ptr_range();
    let mut named = 0;
    for p in &fights[0].participants {
        if let Some(n) = p.name() {
            named += 1;
            assert!(
                span.contains(&n.as_ptr()),
                "{n:?} was copied out of the line instead of borrowed from it"
            );
        }
    }
    assert_eq!(named, 2);
    assert!(span.contains(&fights[0].start.as_ptr()));
}
