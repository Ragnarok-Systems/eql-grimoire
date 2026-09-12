//! WHAT EACH THING THE APP LOADS COSTS IN HEAP, MEASURED ONE AT A TIME. Ignored; run by hand.
//!
//! The question it answers: of the app's working set, how much is DATA (the wiki snapshot, the
//! stored fight history, the log tail and what is parsed out of it) and how much is the app
//! itself. An earlier reading compared a run with the wiki snapshot against a run without it and
//! called the remainder "the app", which was wrong: the run without the snapshot still held the
//! fight history and the log tail. This measures every load on its own instead of by subtraction.
//!
//! HOW. A counting global allocator tracks LIVE heap bytes (allocations minus frees). Each load
//! runs, its result is kept alive, and the growth in live bytes is its cost. That is bytes the
//! program asked for; resident memory adds allocator slack on top, so these are floors.
//!
//!     GRIMOIRE_MEASURE_LOG="<path to eqlog_Name_server.txt>" \
//!     cargo test --release -p grimoire-desktop --test memory_breakdown -- --ignored --nocapture --test-threads=1
//!
//! The wiki snapshot is read from `crates/grimoire-desktop/data`; fight history from the app's own
//! store for the character and server the log's file name names.

use grimoire_desktop::data::Snapshot;
use grimoire_desktop::ingest::{parse_log, read_tail, Roster};
use grimoire_desktop::store::{Owner, Store};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;

static LIVE: AtomicIsize = AtomicIsize::new(0);

struct Live;

// SAFETY: forwards every call to `System` unchanged; the only addition is an atomic counter,
// which does not allocate. Measurement scaffolding, never shipped.
unsafe impl GlobalAlloc for Live {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            LIVE.fetch_add(new as isize - l.size() as isize, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static A: Live = Live;

fn mb(b: isize) -> f64 {
    b as f64 / 1_048_576.0
}

/// Run `f`, keep what it returns, and report the live heap it added.
fn cost<T>(what: &str, on_disk: Option<u64>, f: impl FnOnce() -> T) -> (T, isize) {
    let before = LIVE.load(Ordering::Relaxed);
    let out = f();
    let grew = LIVE.load(Ordering::Relaxed) - before;
    match on_disk {
        Some(d) => println!(
            "{what:<44} {:>8.1} MB heap   from {:>7.1} MB on disk   ({:.1}x)",
            mb(grew),
            d as f64 / 1_048_576.0,
            grew as f64 / d.max(1) as f64
        ),
        None => println!("{what:<44} {:>8.1} MB heap", mb(grew)),
    }
    (out, grew)
}

fn dir_bytes(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    rd.flatten()
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                dir_bytes(&path)
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// `eqlog_Reviir_neriak.txt` -> Reviir, neriak. The store keys its folders the same way.
fn owner_of(log: &Path) -> Owner {
    let stem = log
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("a log file name");
    let rest = stem.strip_prefix("eqlog_").expect("an eqlog_ file");
    let (character, server) = rest.split_once('_').expect("eqlog_<character>_<server>");
    Owner {
        character: character.to_owned(),
        server: server.to_owned(),
    }
}

#[test]
#[ignore = "measurement against the owner's real files; run by hand"]
fn what_each_load_costs_in_heap() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let log =
        PathBuf::from(std::env::var("GRIMOIRE_MEASURE_LOG").expect("set GRIMOIRE_MEASURE_LOG"));
    let fights = dirs::config_dir()
        .expect("a config dir")
        .join(grimoire_desktop::settings::APP_DIR)
        .join("fights");
    let who = owner_of(&log);
    let mut total = 0isize;

    println!();
    let (snapshot, n) = cost(
        "wiki snapshot (Snapshot::load)",
        Some(dir_bytes(&data)),
        || Snapshot::load(&data).expect("the snapshot loads"),
    );
    total += n;

    let kills = std::fs::metadata(data.join("kills-data.json"))
        .map(|m| m.len())
        .ok();
    let (roster, n) = cost("kill roster (Roster::load, a 2nd read)", kills, || {
        Roster::load(&data).expect("the roster loads")
    });
    total += n;

    let store = Store::at(&fights);
    let (history, n) = cost(
        "stored fight history (Store::all)",
        Some(dir_bytes(&fights)),
        || store.all(&who),
    );
    total += n;

    let size = std::fs::metadata(&log).expect("the log exists").len();
    let (tail, n) = cost("log tail text (read_tail, capped)", None, || {
        read_tail(&log, size).expect("the tail reads")
    });
    total += n;
    println!(
        "{:<44} {:>8.1} MB of a {:.1} MB log",
        "  (tail length)",
        mb(tail.text.len() as isize),
        size as f64 / 1_048_576.0
    );

    let (parsed, n) = cost("parsed tail (parse_log)", None, || {
        parse_log(&tail.text, Arc::new(HashMap::new()))
    });
    total += n;

    println!("{:<44} {:>8.1} MB heap", "SUM OF THE ABOVE", mb(total));
    println!("history rows: {}   torn: {}", history.0.len(), history.1);
    drop((snapshot, roster, history, tail, parsed));
}
