//! The purity gate: `grimoire-core`, `grimoire-parse`, `grimoire-corpus` and `grimoire-wasm`
//! must never name `std::fs`, `std::net`, `std::env` or `std::time`. The engine is pure maths
//! and pure `&str -> struct` parsing, so the same code runs natively, in wasm and under test with
//! no filesystem, socket, environment or clock behind it. This file is the enforcement.
//!
//! TWO CRATES ARE ALLOWED SOME OF THE FOUR, AND EACH SAYS WHICH.
//! `grimoire-forge` is the I/O crate and takes all four. `grimoire-desktop` is the native
//! application: it reads the game's log folder and the snapshot from disk (`fs`), it resolves
//! those folders from the environment and the executable's own location (`env`), and it ages the
//! live pill and debounces the settings writes against a clock (`time`).
//!
//! `net` IS ALLOWED IN TWO NAMED FILES AND NOWHERE ELSE, and the narrowness is the point.
//! Twitch's chat is IRC over TLS, which is a raw socket: `ureq` speaks HTTP and cannot carry it,
//! so `chat.rs` and `irc.rs` genuinely need `std::net::TcpStream`. Granting the whole CRATE `net`
//! to say that would retire the rule, because a bare socket appearing in a SCREEN is exactly the
//! change this gate exists to fail on. `FILE_ALLOW` names the two files instead; a third file
//! naming `std::net` still fails, and so does a new namespace in either of these two.
//!
//! THIS GATE CAUGHT ITS OWN CASE AND THE CATCH WAS LATE. The IRC socket was written without the
//! table being told, and it sat red for a while because that work was checked with
//! `cargo test -p grimoire-desktop` rather than over the workspace. The failure was correct; the
//! fix is this entry, not a wider allowance, and the lesson is that a crate-scoped test run does
//! not see a workspace-scoped gate.
//!
//! A crate under `crates/` with no entry at all fails too: the table has to be told what a new
//! crate may do before that crate can pass.
//!
//! It lives here, under `grimoire-forge/tests/`, because reading the source tree is I/O and
//! `grimoire-forge` is the one crate permitted to do it. The scan does not cover this directory,
//! only each crate's `src/`, so this file naming all four namespaces in the allow table below is
//! not itself a violation.
//!
//! This is a text scan over source lines, not a compiler-level guarantee. Two known blind spots:
//! it does not track block-comment state, so a `/* ... */` block spanning lines that mentions a
//! banned namespace is reported as a violation even though it is prose; and it cannot see a macro
//! that assembles a qualified path from fragments (`concat!("std", "::fs")`), which would reach
//! the banned namespace with no literal token this scanner looks for.

use std::fs;
use std::path::{Path, PathBuf};

/// One crate directory under `crates/`, and the banned namespaces it may use.
/// This table is the purity rule, stated as data instead of prose.
const ALLOW_TABLE: &[(&str, &[&str])] = &[
    ("grimoire-core", &[]),
    ("grimoire-parse", &[]),
    ("grimoire-corpus", &[]),
    ("grimoire-wasm", &[]),
    ("grimoire-forge", &["fs", "net", "env", "time"]),
    /* The native application. See the module note for why these three and why not `net`. */
    ("grimoire-desktop", &["fs", "env", "time"]),
];

/// WHERE A NAMESPACE IS ALLOWED IN ONE FILE RATHER THAN ONE CRATE: (crate, file name, namespace).
///
/// A FILE NAME AND NOT A PATH, because the scan already knows which crate it is in and a path
/// would break the first time a module moved into a directory. Two entries, both the same reason:
/// Twitch chat is IRC over TLS and needs a socket.
const FILE_ALLOW: &[(&str, &str, &str)] = &[
    ("grimoire-desktop", "chat.rs", "net"),
    ("grimoire-desktop", "irc.rs", "net"),
];

/// Whether `file` in `crate_name` is one of the named exceptions for `ns`.
fn file_allows(crate_name: &str, file: &std::path::Path, ns: &str) -> bool {
    let Some(base) = file.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    FILE_ALLOW
        .iter()
        .any(|(c, f, n)| *c == crate_name && *f == base && *n == ns)
}

/// The crates that are allowed something but not everything, and what they may not have.
/// `grimoire-forge` is not here because it is allowed all four; this table is the reason the
/// desktop entry above is a statement rather than a rubber stamp.
const PARTIAL_CRATES: &[(&str, &[&str])] = &[("grimoire-desktop", &["net"])];

const NAMESPACES: [&str; 4] = ["fs", "net", "env", "time"];

const PURE_CRATES: &[&str] = &[
    "grimoire-core",
    "grimoire-parse",
    "grimoire-corpus",
    "grimoire-wasm",
];

#[derive(Debug)]
struct Violation {
    crate_name: String,
    file: PathBuf,
    line: usize,
    text: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}:{}: {}",
            self.crate_name,
            self.file.display(),
            self.line,
            self.text
        )
    }
}

fn violations_report(violations: &[Violation]) -> String {
    violations
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `CARGO_MANIFEST_DIR` is `.../crates/grimoire-forge`; two levels up is the workspace root,
/// so the answer does not depend on the directory `cargo test` was invoked from (EDGE-002).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/grimoire-forge has a parent")
        .parent()
        .expect("crates/ has a parent")
        .to_path_buf()
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("purity-fixtures")
}

/// Recursively collects every file under `dir` whose name ends in `suffix`. Silent on a missing
/// or unreadable `dir`; callers treat an empty result as "found nothing to check" (EDGE-007).
fn find_files(dir: &Path, suffix: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            find_files(&path, suffix, out);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(suffix))
        {
            out.push(path);
        }
    }
}

/// Reads a file as text, stripping a leading BOM. `str::lines()` already strips a trailing `\r`
/// from each line, so CRLF endings need no further handling (EDGE-004).
fn read_normalised(path: &Path) -> String {
    let raw = fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let text = String::from_utf8_lossy(&raw);
    match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_string(),
        None => text.into_owned(),
    }
}

/// Checks one line against the banned namespaces, returning the offending line text if any of
/// REQ-005's four shapes matches. A line whose first non-whitespace characters are `//` is never
/// a violation (REQ-006).
fn line_violation(line: &str, banned: &[&str]) -> Option<String> {
    if line.trim_start().starts_with("//") {
        return None;
    }

    // Shapes (a) and (b): a qualified path anywhere on the line, aliased or not, including a
    // leading `::std`, since that is still a substring match on `std::<namespace>`.
    for ns in banned {
        if line.contains(&format!("std::{ns}")) {
            return Some(line.trim().to_string());
        }
    }

    // Shapes (c) and (d): a grouped import, `use std::{fs, io};` or one level deeper,
    // `use std::{io::Write, time::Instant};`, neither of which spells the qualified path.
    let mut search_from = 0;
    while let Some(rel) = line[search_from..].find("std::{") {
        let group_start = search_from + rel + "std::{".len();
        let Some(rel_end) = line[group_start..].find('}') else {
            break;
        };
        let group = &line[group_start..group_start + rel_end];
        for item in group.split(',') {
            let head = item.trim().split("::").next().unwrap_or("").trim();
            if banned.contains(&head) {
                return Some(line.trim().to_string());
            }
        }
        search_from = group_start + rel_end + 1;
    }

    None
}

/// Scans already-read text for banned-namespace lines, returning `(1-indexed line, line text)`.
fn scan_text_for_bans(text: &str, banned: &[&str]) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| line_violation(line, banned).map(|hit| (i + 1, hit)))
        .collect()
}

/// Scans the real workspace tree: every directory under `crates/` must have an `ALLOW_TABLE`
/// entry (REQ-003), every crate with an entry has its `src/` scanned recursively (REQ-004), and
/// no pure crate's manifest may name `grimoire-forge` (REQ-007). Returns the number of `.rs`
/// files examined and every violation found.
fn real_tree_violations() -> (usize, Vec<Violation>) {
    let root = workspace_root();
    let crates_dir = root.join("crates");
    let mut violations = Vec::new();
    let mut total_files = 0usize;

    let mut dir_names: Vec<String> = fs::read_dir(&crates_dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", crates_dir.display()))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    dir_names.sort();

    for name in &dir_names {
        match ALLOW_TABLE
            .iter()
            .find(|(table_name, _)| table_name == name)
        {
            None => violations.push(Violation {
                crate_name: name.clone(),
                file: crates_dir.join(name),
                line: 0,
                text: format!(
                    "directory `{name}` under crates/ has no ALLOW_TABLE entry in purity.rs; \
                     say explicitly what it may do before it can pass"
                ),
            }),
            Some((_, allowed)) => {
                let src_dir = crates_dir.join(name).join("src");
                let mut files = Vec::new();
                find_files(&src_dir, ".rs", &mut files);
                if files.is_empty() {
                    violations.push(Violation {
                        crate_name: name.clone(),
                        file: src_dir,
                        line: 0,
                        text: format!(
                            "crate `{name}` has no `src/` files; the scanner found nothing to check"
                        ),
                    });
                    continue;
                }
                total_files += files.len();
                let banned: Vec<&str> = NAMESPACES
                    .iter()
                    .copied()
                    .filter(|ns| !allowed.contains(ns))
                    .collect();
                for file in files {
                    /* THE BAN LIST IS NARROWED PER FILE, not per crate. See `FILE_ALLOW`: the
                     * two files that carry the IRC socket may name `net`; every other file in
                     * the same crate still fails on it. */
                    let here: Vec<&str> = banned
                        .iter()
                        .copied()
                        .filter(|ns| !file_allows(name, &file, ns))
                        .collect();
                    let text = read_normalised(&file);
                    for (line, text) in scan_text_for_bans(&text, &here) {
                        violations.push(Violation {
                            crate_name: name.clone(),
                            file: file.clone(),
                            line,
                            text,
                        });
                    }
                }
            }
        }
    }

    for name in PURE_CRATES {
        if !dir_names.iter().any(|d| d == name) {
            continue;
        }
        let manifest = crates_dir.join(name).join("Cargo.toml");
        let text = fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("reading {}: {e}", manifest.display()));
        if text.contains("grimoire-forge") {
            violations.push(Violation {
                crate_name: (*name).to_string(),
                file: manifest,
                line: 0,
                text: "pure crate names `grimoire-forge` among its dependencies or \
                       dev-dependencies; every symbol in the I/O crate becomes reachable from it"
                    .to_string(),
            });
        }
    }

    (total_files, violations)
}

#[test]
fn real_tree_scan() {
    let (files, violations) = real_tree_violations();
    println!("files={files} violations={}", violations.len());
    assert!(
        files >= 15,
        "expected at least 15 source files under the five crates, found {files}"
    );
    assert!(
        violations.is_empty(),
        "purity violations found:\n{}",
        violations_report(&violations)
    );
}

#[test]
fn fixture_scan_two() {
    let dir = fixtures_dir();
    let files = [
        dir.join("impure-fs.rs.txt"),
        dir.join("impure-grouped-import.rs.txt"),
    ];
    let banned = ["fs", "net", "env", "time"];

    let mut violations = Vec::new();
    for file in &files {
        let text = read_normalised(file);
        for (line, text) in scan_text_for_bans(&text, &banned) {
            violations.push(Violation {
                crate_name: "grimoire-core".to_string(),
                file: file.clone(),
                line,
                text,
            });
        }
    }

    assert_eq!(
        violations.len(),
        2,
        "expected exactly two violations across the two impure fixtures, got:\n{}",
        violations_report(&violations)
    );
    for expected in ["impure-fs.rs.txt", "impure-grouped-import.rs.txt"] {
        assert!(
            violations
                .iter()
                .any(|v| v.file.file_name().and_then(|n| n.to_str()) == Some(expected)),
            "expected a violation naming {expected}, got:\n{}",
            violations_report(&violations)
        );
    }
}

#[test]
fn fixture_clean_none() {
    let file = fixtures_dir().join("clean.rs.txt");
    let banned = ["fs", "net", "env", "time"];
    let text = read_normalised(&file);
    let hits = scan_text_for_bans(&text, &banned);
    assert!(
        hits.is_empty(),
        "clean fixture should have no violations, found: {hits:?}"
    );
}

#[test]
fn no_pure_crate_names_forge() {
    let root = workspace_root();
    for name in PURE_CRATES {
        let manifest_path = root.join("crates").join(name).join("Cargo.toml");
        let text = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", manifest_path.display()));
        assert!(
            !text.contains("grimoire-forge"),
            "{name}'s manifest at {} names grimoire-forge, the only crate allowed I/O; \
             a pure crate may not depend on it",
            manifest_path.display()
        );
    }
}

/// Every crate that is allowed SOME of the four namespaces still has the rest denied, and the
/// denial is real rather than a comment: the entry exists, it does not list the denied namespace,
/// and a scan of that crate's own `src/` finds no use of it.
///
/// This is the test the desktop crate arrived without. Before its `ALLOW_TABLE` entry existed the
/// whole workspace suite exited 101 on `real_tree_scan`, which said only "no entry"; this says
/// what the entry has to contain. Adding `net` to the desktop allowance, or opening a `std::net`
/// socket in a UI crate, fails here and names which.
#[test]
fn a_partly_allowed_crate_keeps_the_rest_denied() {
    let crates_dir = workspace_root().join("crates");
    for (name, denied) in PARTIAL_CRATES {
        let allowed = ALLOW_TABLE
            .iter()
            .find(|(table_name, _)| table_name == name)
            .map(|(_, allowed)| *allowed)
            .unwrap_or_else(|| {
                panic!("{name} is named in PARTIAL_CRATES but has no ALLOW_TABLE entry")
            });
        for ns in *denied {
            assert!(
                !allowed.contains(ns),
                "{name}'s ALLOW_TABLE entry lists `{ns}`, which this table says it may not have"
            );
        }

        let src_dir = crates_dir.join(name).join("src");
        let mut files = Vec::new();
        find_files(&src_dir, ".rs", &mut files);
        assert!(
            !files.is_empty(),
            "{name} has no src/ files; the scan proved nothing"
        );
        let mut hits = Vec::new();
        for file in files {
            /* A FILE EXCEPTION IS NOT A CRATE ALLOWANCE, and this test is the one that says so:
             * it walks the SAME denied list and the SAME exceptions, so widening `ALLOW_TABLE`
             * still fails here while a named file in `FILE_ALLOW` does not. */
            let here: Vec<&str> = denied
                .iter()
                .copied()
                .filter(|ns| !file_allows(name, &file, ns))
                .collect();
            let text = read_normalised(&file);
            for (line, text) in scan_text_for_bans(&text, &here) {
                hits.push(format!("{}:{line}: {text}", file.display()));
            }
        }
        assert!(
            hits.is_empty(),
            "{name} names a namespace it is denied:\n{}",
            hits.join("\n")
        );
    }
}
