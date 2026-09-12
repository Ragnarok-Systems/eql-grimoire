//! The zero-allocation floor, asserted with a counting global allocator.
//!
//! This is the assertion that separates this parser from the field. Every mature EQ parser is
//! constrained by per-line allocation rather than by algorithm: a .NET string is UTF-16 while an
//! EQ log is effectively ASCII, so a line costs twice the memory it needs and a live tail pays
//! the collector for it. Rust can simply not do that, and the only way to know it did not is to
//! count.
//!
//! Wall clock is not the assertion because wall clock is not deterministic. An allocation count
//! is: it is the same number on an idle laptop and on a CI box running eleven other jobs, so
//! this test can be believed when a timing test can only be suspected.
//!
//! The claim is precisely: **the number of allocations does not grow with the number of lines
//! parsed.** Parsing four times as many lines must allocate exactly as many times as parsing
//! one, which can only be true if the per-line cost is zero.
//!
//! `unsafe` appears once, in the counting allocator, which is measurement scaffolding and never
//! ships. The parser crate itself carries `#![forbid(unsafe_code)]`, which is the compiler's own
//! statement that nothing on the hot path can reach for it.

use grimoire_parse::combat::{parse, DamageKind, Event, Reading};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Only ever read to prove the allocator hook is wired in at all. The MEASUREMENT is the
/// thread-local counter below; see [`counted`].
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Counting is armed per thread, not globally.
    ///
    /// `cargo test` runs the tests in this binary on separate threads at the same time, and the
    /// harness allocates on its own thread while they run. The initialiser is `const` and the
    /// cell has no destructor, so reading this from inside the allocator cannot itself allocate
    /// and cannot recurse.
    static ARMED: Cell<bool> = const { Cell::new(false) };
    /// The count is per thread as well, and it has to be.
    ///
    /// Arming per thread while counting into one shared integer does NOT isolate the
    /// measurement: a neighbouring test, armed on its own thread, adds to the same integer
    /// inside this thread's window. `the_counting_allocator_actually_counts` allocates 4 KB
    /// while armed, so on an unlucky interleaving it lands inside the zero-allocation floor
    /// above and turns a deterministic assertion into a race. Observed as a real failure while
    /// the fight aggregator's own floor was being written, at 109 counted against 77 expected.
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Run `f` with allocation counting armed on this thread, and return what it allocated.
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
// relaxed counter increment on a thread-local flag, which cannot affect the allocation itself.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        Self::note();
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A realloc is a fresh allocation as far as this floor is concerned: a growing `Vec` on
        // the hot path would show up here and it should.
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

const CAPTURE: &str = include_str!("../../../web/fixtures/eqlog-tail-200k.txt");

/// Parse `passes` sweeps of the capture, folding every field of every event into one integer so
/// the optimiser cannot delete the work, and return `(lines parsed, allocations)`.
///
/// The fold deliberately touches the borrowed `&str` fields: reading a name's bytes proves the
/// name is a real slice of the input rather than something the parser could have skipped
/// building.
fn sweep(passes: usize) -> (u64, usize) {
    counted(|| parse_passes(passes))
}

fn parse_passes(passes: usize) -> u64 {
    let mut lines = 0u64;
    let mut checksum = 0u64;
    for _ in 0..passes {
        for raw in CAPTURE.lines() {
            let Some(entry) = parse(raw) else { continue };
            lines += 1;
            checksum = checksum.wrapping_add(entry.at.len() as u64);
            match entry.reading {
                Reading::Event(Event::Damage(d)) => {
                    checksum = checksum.wrapping_add(u64::from(d.amount));
                    if let Some(n) = d.source.name() {
                        checksum = checksum.wrapping_add(n.len() as u64);
                    }
                    if let Some(n) = d.target.name() {
                        checksum = checksum.wrapping_add(n.len() as u64);
                    }
                    checksum = checksum.wrapping_add(match d.kind {
                        DamageKind::Melee { verb } => verb.len() as u64,
                        DamageKind::Shield { effect } => effect.len() as u64,
                        DamageKind::Dot { spell } => spell.len() as u64,
                        DamageKind::Spell { spell, resist } => (spell.len() + resist.len()) as u64,
                        DamageKind::SelfInflicted | DamageKind::Environmental => 1,
                    });
                    checksum = checksum.wrapping_add(d.mods.raw().len() as u64);
                }
                Reading::Event(Event::Swing(s)) => {
                    checksum = checksum.wrapping_add(s.verb.len() as u64);
                }
                Reading::Event(Event::Heal(h)) => {
                    checksum = checksum
                        .wrapping_add(u64::from(h.amount))
                        .wrapping_add(h.spell.len() as u64);
                }
                _ => checksum = checksum.wrapping_add(1),
            }
        }
    }
    black_box(checksum);
    lines
}

/// The floor. One sweep and four sweeps must cost the same number of allocations, and that
/// number must be zero.
#[test]
fn parsing_more_lines_does_not_allocate_more() {
    // Warm anything the first call would set up lazily, so the measurement is steady state.
    let _ = sweep(1);

    let (one_lines, one_allocs) = sweep(1);
    let (four_lines, four_allocs) = sweep(4);

    assert_eq!(
        four_lines,
        one_lines * 4,
        "the sweeps did not do equal work"
    );

    let per_line = (four_allocs as f64 - one_allocs as f64) / (four_lines - one_lines) as f64;
    let report = format!(
        "{one_lines} lines allocated {one_allocs} times; {four_lines} lines allocated \
         {four_allocs} times; growth is {per_line:.6} allocations per additional line"
    );
    println!("{report}");

    assert_eq!(
        four_allocs, one_allocs,
        "allocation count grew with line count, so the parser allocates per line: {report}"
    );
    assert_eq!(
        one_allocs, 0,
        "the parser allocated on the hot path at all: {report}"
    );
}

/// The counter is real. If this fails, the test above proves nothing, because an allocator that
/// counts nothing would pass it trivially.
#[test]
fn the_counting_allocator_actually_counts() {
    let (_, seen) = counted(|| {
        let v: Vec<u8> = black_box(Vec::with_capacity(4096));
        black_box(&v);
    });
    assert!(
        seen > 0,
        "the counting allocator missed a 4 KB allocation, so it would miss the parser's too"
    );
    assert!(
        ALLOCATIONS.load(Ordering::Relaxed) > 0,
        "the global allocator hook never fired, so nothing here is measuring anything"
    );
    // And it counts nothing when it is not armed, which is what keeps a busy neighbouring test
    // out of the measurement above.
    let before = here();
    let v: Vec<u8> = black_box(Vec::with_capacity(4096));
    black_box(&v);
    assert_eq!(
        here(),
        before,
        "the counter fired while disarmed, so another thread's work can pollute the floor"
    );
}

/// The parser really is handing back slices of the caller's buffer rather than copies.
///
/// Pointer identity is the proof: a name that lives inside the input line has an address inside
/// that line. A `String` would not, however carefully it was built.
#[test]
fn every_name_points_back_into_the_input() {
    let raw = "[Wed Jul 15 23:17:30 2026] A lurking mummy is pierced by Tanefilo's thorns \
               for 7 points of non-melee damage.";
    let entry = parse(raw).expect("stamped");
    let Reading::Event(Event::Damage(d)) = entry.reading else {
        panic!("{entry:?}");
    };
    let span = raw.as_bytes().as_ptr_range();
    for slice in [
        entry.at,
        d.source.name().expect("named source"),
        d.target.name().expect("named target"),
        match d.kind {
            DamageKind::Shield { effect } => effect,
            other => panic!("{other:?}"),
        },
    ] {
        let p = slice.as_ptr();
        assert!(
            span.contains(&p),
            "{slice:?} was copied out of the line instead of borrowed from it"
        );
    }
}
