//! The licence gate: `eql-grimoire/LICENSE` must exist, must be the GNU Affero General Public
//! Licence version 3, must name the same holder as the workspace manifest, and the manifest's
//! `license` field must say the same thing the file does.
//!
//! # THE LICENCE CHANGED AND THIS GATE CHANGED WITH IT
//!
//! This file used to require the MIT licence and to BAN the strings `Affero`, `GNU` and
//! `General Public License` outright. The owner chose AGPL-3.0 on 2026-09-11, so the guard is
//! inverted: what was banned is now required, and MIT is what must not appear. A gate that was
//! left pointing at the old answer would have gone red on the correct licence and green on the
//! wrong one.
//!
//! # WHY THE MANIFEST IS CHECKED AGAINST THE FILE
//!
//! Two places state the licence and a reader believes whichever they happen to look at: `LICENSE`
//! is what a person reads, and `[workspace.package].license` is what the app's own footer prints
//! (`titlebar::licence` reads `CARGO_PKG_LICENSE`) and what crates.io and every tool would
//! publish. Editing one and not the other is a silent lie, so the two are compared here.
//!
//! `env!("CARGO_PKG_AUTHORS")` is not used for the holder comparison: it reflects
//! `grimoire-forge`'s own `[package].authors`, which Cargo leaves empty unless that crate's
//! manifest opts in with `authors.workspace = true` (verified empirically: with only
//! `[workspace.package].authors` set, `CARGO_PKG_AUTHORS` here resolves to `""`). The holder is
//! read from the root manifest's `[workspace.package].authors` directly.

use std::fs;
use std::path::{Path, PathBuf};

/// The SPDX identifier this project ships under. Exactly version 3, not "or later": the owner
/// picked AGPL-3.0 and a future version of the licence is a decision nobody has taken.
const SPDX: &str = "AGPL-3.0-only";

/// `CARGO_MANIFEST_DIR` is `.../crates/grimoire-forge`; two levels up is the workspace root, so
/// the answer does not depend on the directory `cargo test` was invoked from (EDGE-002).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/grimoire-forge has a parent")
        .parent()
        .expect("crates/ has a parent")
        .to_path_buf()
}

/// Reads a file as text, stripping a leading BOM and normalising CRLF to LF, so an editor that
/// saves with either does not fail the comparison (EDGE-002). Asserts existence explicitly
/// rather than treating a read error as an empty file (EDGE-007).
fn read_normalised(path: &Path) -> String {
    assert!(path.is_file(), "expected a file at {}", path.display());
    let raw = fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let text = String::from_utf8_lossy(&raw);
    let text = match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_string(),
        None => text.into_owned(),
    };
    text.replace("\r\n", "\n")
}

fn licence_text() -> String {
    read_normalised(&workspace_root().join("LICENSE"))
}

fn manifest_text() -> String {
    read_normalised(&workspace_root().join("Cargo.toml"))
}

/// The first name in the root manifest's `[workspace.package].authors` (EDGE-001: an authors
/// list may grow a second entry later, and only the first is the holder of record).
fn workspace_holder() -> String {
    let manifest = manifest_text();
    let marker = "authors = [";
    let start = manifest
        .find(marker)
        .unwrap_or_else(|| panic!("Cargo.toml has no `authors = [...]` line:\n{manifest}"))
        + marker.len();
    let rest = &manifest[start..];
    let end = rest
        .find(']')
        .unwrap_or_else(|| panic!("Cargo.toml's authors array is never closed"));
    let first_entry = rest[..end]
        .split(',')
        .next()
        .unwrap_or_else(|| panic!("Cargo.toml's authors array is empty"));
    first_entry.trim().trim_matches('"').to_string()
}

#[test]
fn licence_file_exists_and_is_not_empty() {
    let path = workspace_root().join("LICENSE");
    let text = read_normalised(&path);
    assert!(
        !text.trim().is_empty(),
        "LICENSE at {} exists but is empty",
        path.display()
    );
}

/// THE FILE IS THE AGPL, AND IT IS THE WHOLE LICENCE RATHER THAN A REFERENCE TO ONE.
///
/// The heading alone would pass on a file that merely mentions the licence, so this also looks
/// for the body: the version line, and two clauses that only the real text carries. Section 13 is
/// the one that matters to this project, because it is what makes a hosted, modified Grimoire
/// have to offer its source.
#[test]
fn licence_names_the_agpl_version_3() {
    let text = licence_text();
    for required in [
        "GNU AFFERO GENERAL PUBLIC LICENSE",
        "Version 3, 19 November 2007",
        "Remote Network Interaction",
        "TERMS AND CONDITIONS",
    ] {
        assert!(
            text.contains(required),
            "LICENSE does not contain {required:?}, so it is not the full AGPL version 3 text"
        );
    }
}

/// AND IT IS NOT THE OLD ONE. The repository shipped MIT until 2026-09-11; a stale file would
/// otherwise pass every check above by sitting beside a correct manifest.
#[test]
fn licence_is_not_the_mit_licence_any_more() {
    let text = licence_text();
    assert!(
        !text.contains("MIT License"),
        "LICENSE still names the MIT License, which this project no longer ships under"
    );
}

/// THE MANIFEST AND THE FILE AGREE. `titlebar::licence` prints `CARGO_PKG_LICENSE` in the app's
/// footer, so a manifest that disagreed with the file would put the wrong licence on screen.
#[test]
fn the_manifest_declares_the_same_licence_the_file_carries() {
    let manifest = manifest_text();
    let declared = format!("license = \"{SPDX}\"");
    assert!(
        manifest.contains(&declared),
        "Cargo.toml's [workspace.package] does not declare {declared:?}; the footer would print \
         a licence the LICENSE file does not carry"
    );
}

#[test]
fn licence_holder_matches_workspace_manifest() {
    let text = licence_text();
    let holder = workspace_holder();
    let copyright_line = text
        .lines()
        .find(|line| line.trim_start().starts_with("Copyright (c) "))
        .unwrap_or_else(|| panic!("LICENSE has no `Copyright (c) ` line:\n{text}"));
    assert!(
        copyright_line.contains(&holder),
        "LICENSE copyright line {copyright_line:?} does not name the holder {holder:?} \
         read out of Cargo.toml's [workspace.package].authors"
    );
}
