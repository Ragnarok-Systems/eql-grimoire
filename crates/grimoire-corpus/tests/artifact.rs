//! The artifact has to survive being read badly, over a network, by an old client.

use grimoire_corpus::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
struct Row {
    id: u32,
    name: String,
}

fn build(n: u32) -> (Vec<u8>, Manifest) {
    let mut w = Writer::new();
    w.source("test fixture");
    for i in 0..n {
        w.put(
            format!("item/{i:05}"),
            &Row {
                id: i,
                name: format!("Thing {i}"),
            },
        );
    }
    for i in 0..n / 2 {
        w.put(
            format!("recipe/{i:05}"),
            &Row {
                id: i,
                name: "r".into(),
            },
        );
    }
    w.finish("2026-08-09")
}

#[test]
fn round_trips() {
    let (bytes, m) = build(50);
    let r = Reader::open(InMemory(bytes)).unwrap();
    assert_eq!(r.len().unwrap(), m.records as usize);
    let row: Row = r.get("item/00007").unwrap();
    assert_eq!(
        row,
        Row {
            id: 7,
            name: "Thing 7".into()
        }
    );
}

#[test]
fn a_missing_key_is_not_found_rather_than_a_panic() {
    let (bytes, _) = build(5);
    let r = Reader::open(InMemory(bytes)).unwrap();
    assert_eq!(
        r.get::<Row>("item/99999").unwrap_err(),
        CorpusError::NotFound
    );
}

/// The point of the whole format: one lookup must not read the corpus, and the cost of a
/// lookup must not grow with how much is in it.
#[test]
fn a_lookup_costs_four_reads_and_a_sliver_of_the_file() {
    let (bytes, _) = build(4000);
    let total = bytes.len() as u64;
    let f = Counting::new(InMemory(bytes));

    let r = Reader::open(f).unwrap();
    let after_open = r_reads(&r);
    let _: Row = r.get("item/02000").unwrap();

    assert_eq!(after_open, 2, "opening should be footer + directory");
    assert_eq!(
        r_reads(&r),
        4,
        "a record should be one index block + the record"
    );
    assert!(
        r_bytes(&r) * 20 < total,
        "read {} of {total} bytes — that is a scan, not a range read",
        r_bytes(&r)
    );
}

/// A ten-times bigger corpus must not make a lookup ten times dearer.
#[test]
fn lookup_cost_barely_moves_as_the_corpus_grows() {
    let cost = |n: u32, key: &str| {
        let f = Counting::new(InMemory(build(n).0));
        let r = Reader::open(f).unwrap();
        let _: Row = r.get(key).unwrap();
        (r_reads(&r), r_bytes(&r))
    };
    let (small_reads, small_bytes) = cost(400, "item/00200");
    let (big_reads, big_bytes) = cost(8000, "item/00200");
    assert_eq!(small_reads, big_reads, "read count should not scale");
    assert!(
        big_bytes < small_bytes * 3,
        "bytes went from {small_bytes} to {big_bytes} for a 20x corpus"
    );
}

/// A second lookup in the same neighbourhood should reuse the index block it already has.
#[test]
fn a_nearby_second_lookup_is_nearly_free() {
    let f = Counting::new(InMemory(build(4000).0));
    let r = Reader::open(f).unwrap();
    let _: Row = r.get("item/02000").unwrap();
    let after_first = r_reads(&r);
    let _: Row = r.get("item/02001").unwrap();
    assert_eq!(r_reads(&r), after_first + 1, "index block was re-fetched");
}

fn r_reads(r: &Reader<Counting<InMemory>>) -> u32 {
    r.fetch_ref().reads.get()
}
fn r_bytes(r: &Reader<Counting<InMemory>>) -> u64 {
    r.fetch_ref().bytes.get()
}

#[test]
fn prefix_walks_only_its_own_keys() {
    let (bytes, _) = build(20);
    let r = Reader::open(InMemory(bytes)).unwrap();
    let recipes = r.prefix("recipe/").unwrap();
    assert_eq!(recipes.len(), 10);
    assert!(recipes.iter().all(|k| k.starts_with("recipe/")));
}

#[test]
fn the_same_content_hashes_the_same_and_different_content_does_not() {
    let (_, a) = build(30);
    let (_, b) = build(30);
    let (_, c) = build(31);
    assert_eq!(a.content_hash, b.content_hash, "build is not reproducible");
    assert_ne!(a.content_hash, c.content_hash);
}

/// Insertion order must not change the bytes, or two builders produce two "versions" of an
/// identical corpus and the content address stops meaning anything.
#[test]
fn insertion_order_does_not_change_the_artifact() {
    let mut fwd = Writer::new();
    let mut rev = Writer::new();
    let rows: Vec<Row> = (0..40)
        .map(|i| Row {
            id: i,
            name: format!("n{i}"),
        })
        .collect();
    for r in &rows {
        fwd.put(format!("item/{:05}", r.id), r);
    }
    for r in rows.iter().rev() {
        rev.put(format!("item/{:05}", r.id), r);
    }
    assert_eq!(fwd.finish("d").0, rev.finish("d").0);
}

#[test]
fn a_truncated_file_is_refused_not_misread() {
    let (bytes, _) = build(30);
    for cut in [0usize, 1, 8, 15] {
        let short = bytes[..cut].to_vec();
        assert!(
            Reader::open(InMemory(short)).is_err(),
            "accepted {cut} bytes"
        );
    }
    // Chopping the tail off destroys the footer.
    let chopped = bytes[..bytes.len() - 4].to_vec();
    assert!(Reader::open(InMemory(chopped)).is_err());
}

#[test]
fn something_that_is_not_a_corpus_is_rejected_by_magic() {
    let junk = vec![0u8; 512];
    assert_eq!(
        Reader::open(InMemory(junk)).unwrap_err(),
        CorpusError::BadMagic
    );
}

#[test]
fn a_future_format_is_refused_rather_than_guessed_at() {
    let (mut bytes, _) = build(10);
    let n = bytes.len();
    bytes[n - 10..n - 8].copy_from_slice(&99u16.to_le_bytes());
    assert_eq!(
        Reader::open(InMemory(bytes)).unwrap_err(),
        CorpusError::UnknownFormat(99)
    );
}

#[test]
fn a_bogus_index_offset_does_not_read_out_of_bounds() {
    let (mut bytes, _) = build(10);
    let n = bytes.len();
    bytes[n - 8..].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        Reader::open(InMemory(bytes)),
        Err(CorpusError::Corrupt(_)) | Err(CorpusError::Truncated { .. })
    ));
}

#[test]
fn an_empty_corpus_is_legal() {
    let (bytes, m) = Writer::new().finish("2026-08-09");
    assert_eq!(m.records, 0);
    let r = Reader::open(InMemory(bytes)).unwrap();
    assert!(r.is_empty());
    assert_eq!(r.len().unwrap(), 0);
    assert_eq!(r.get::<Row>("anything").unwrap_err(), CorpusError::NotFound);
}
