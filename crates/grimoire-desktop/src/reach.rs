//! The reachability floor that rustc cannot run for us. Test only.
//!
//! WHAT RUSTC MISSES, MEASURED. `-D warnings` on a bin-only copy of this crate reports every
//! function, struct, constant and method the binary never reaches, and every field it never
//! reads, EXCEPT fields on a struct that derives `PartialEq`, `Eq`, `Hash`, `PartialOrd` or `Ord`.
//! Those derives expand to an impl that reads every field, so rustc's dead-code pass counts every
//! field as read and says nothing. Only `Clone` and `Debug` are ignored by that pass. An
//! adversarial review of the round-one tree found fourteen fields hiding behind exactly that:
//! computed rules (`Fit::narrows_cls`, `Landing::toward`, `Crit::goal`, `AuditSummary::swaps`)
//! that a test read and no screen ever drew.
//!
//! WHAT THIS TEST DOES ABOUT IT. It walks every `.rs` file under `src/`, cuts out every
//! `#[cfg(test)]` item (a `mod tests { .. }` block, or a single test fn), finds every struct whose
//! derive list carries one of the five masking traits, and demands that every field of that struct
//! is READ somewhere in the remaining production text: `.field` (a field access), or `field` as a
//! bare name in a destructuring pattern (`let Foo { field, .. } = ..`, `Foo { field, .. } =>`).
//! A struct literal `Foo { field: value }` is a write and does not count; neither does the field's
//! own declaration.
//!
//! WHAT IT STILL CANNOT SEE, SAID PLAINLY. It is a text floor, not a type check. A field that
//! shares its name with a read field on some other struct passes (the round-one
//! `Changed::always_on_top` hid behind `Settings::always_on_top` exactly this way; it is gone). A
//! read inside a `#[cfg(test)]` block that this cutter fails to recognise would count. The next
//! adversarial pass starts from this floor, not from zero.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const MASKING: [&str; 5] = ["PartialEq", "Eq", "Hash", "PartialOrd", "Ord"];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The index of the `}` that closes the `{` at `open`, counting braces and skipping string and
/// char literals and comments well enough for this crate's source.
fn matching_brace(s: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < s.len() {
        match s[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'"' => {
                /* a string: skip to its unescaped close, raw strings included by their hashes */
                let mut j = i + 1;
                while j < s.len() {
                    if s[j] == b'\\' {
                        j += 2;
                        continue;
                    }
                    if s[j] == b'"' {
                        break;
                    }
                    j += 1;
                }
                i = j;
            }
            b'\'' => {
                /* a char literal such as '{' or '\''; a lifetime ('a) has no closing quote */
                if i + 2 < s.len() && s[i + 2] == b'\'' {
                    i += 2;
                } else if i + 3 < s.len() && s[i + 1] == b'\\' && s[i + 3] == b'\'' {
                    i += 3;
                }
            }
            b'/' if i + 1 < s.len() && s[i + 1] == b'/' => {
                while i < s.len() && s[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < s.len() && s[i + 1] == b'*' => {
                let mut j = i + 2;
                while j + 1 < s.len() && !(s[j] == b'*' && s[j + 1] == b'/') {
                    j += 1;
                }
                i = j + 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The source with every `#[cfg(test)]` item removed: the attribute, and the item after it up to
/// its closing brace (a module, an impl, a fn) or its semicolon (a `mod x;` declaration).
fn strip_test_items(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let marker = b"#[cfg(test)]";
    while i < b.len() {
        if b[i..].starts_with(marker) {
            let mut j = i + marker.len();
            /* the item's body: the first `{` before a `;` closes the search */
            let mut open: Option<usize> = None;
            while j < b.len() {
                if b[j] == b'{' {
                    open = Some(j);
                    break;
                }
                if b[j] == b';' {
                    break;
                }
                j += 1;
            }
            let end = match open {
                Some(o) => matching_brace(b, o).unwrap_or(b.len() - 1),
                None => j,
            };
            i = end + 1;
            continue;
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

/// Every struct whose derive list carries a masking trait: (struct name, field names).
fn masked_structs(prod: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let lines: Vec<&str> = prod.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let l = lines[i].trim();
        if l.starts_with("#[derive(") {
            let masked = MASKING.iter().any(|t| {
                l[8..]
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w == *t)
            });
            /* the struct line follows the attributes */
            let mut k = i + 1;
            while k < lines.len() && lines[k].trim().starts_with("#[") {
                k += 1;
            }
            let head = lines.get(k).map(|s| s.trim()).unwrap_or("");
            let is_struct = head.starts_with("pub struct ")
                || head.starts_with("struct ")
                || head.starts_with("pub(crate) struct ");
            if masked && is_struct && head.ends_with('{') {
                let name = head
                    .trim_start_matches("pub(crate) ")
                    .trim_start_matches("pub ")
                    .trim_start_matches("struct ");
                let name: String = name
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let mut fields = Vec::new();
                let mut m = k + 1;
                while m < lines.len() {
                    let f = lines[m].trim();
                    if f == "}" {
                        break;
                    }
                    if !f.starts_with("//")
                        && !f.starts_with("#[")
                        && !f.starts_with("/*")
                        && !f.starts_with('*')
                    {
                        let f = f
                            .trim_start_matches("pub(crate) ")
                            .trim_start_matches("pub ");
                        if let Some((fname, _)) = f.split_once(':') {
                            let fname = fname.trim();
                            if !fname.is_empty()
                                && fname.chars().all(|c| c.is_alphanumeric() || c == '_')
                            {
                                fields.push(fname.to_owned());
                            }
                        }
                    }
                    m += 1;
                }
                out.push((name, fields));
                i = m;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// A `.field` access anywhere in the production text, not followed by `(` (that is a method).
fn has_field_access(prod: &str, field: &str) -> bool {
    let b = prod.as_bytes();
    let needle = format!(".{field}");
    let mut from = 0usize;
    while let Some(pos) = prod[from..].find(&needle) {
        let at = from + pos;
        let end = at + needle.len();
        let after_ok = end >= b.len() || (!is_ident(b[end]) && b[end] != b'(');
        /* `..field` is a range, `x.0.field` is fine, `1.field` cannot happen with an ident */
        let before_ok = at == 0 || b[at - 1] != b'.';
        if after_ok && before_ok {
            return true;
        }
        from = end;
    }
    false
}

/// A bare `field` inside a destructuring pattern: preceded by `{` or `,` (with spaces), followed
/// by `,` or `}` (with spaces), on a line that is a pattern (`let`, `match` arm, `if let`,
/// `for`, `Some(`, or a closure parameter) rather than a struct literal.
fn has_destructure_read(prod: &str, field: &str) -> bool {
    for line in prod.lines() {
        let t = line.trim();
        let pattern_line = t.starts_with("let ")
            || t.starts_with("if let ")
            || t.starts_with("while let ")
            || t.starts_with("for ")
            || t.contains("=> ")
            || t.contains(") =>")
            || t.contains("} =>");
        if !pattern_line {
            continue;
        }
        let b = t.as_bytes();
        let mut from = 0usize;
        while let Some(pos) = t[from..].find(field) {
            let at = from + pos;
            let end = at + field.len();
            let before = t[..at].trim_end();
            let after = t[end..].trim_start();
            let before_ok = before.ends_with('{') || before.ends_with(',');
            let after_ok = after.starts_with(',') || after.starts_with('}');
            let whole = (at == 0 || !is_ident(b[at - 1])) && (end >= b.len() || !is_ident(b[end]));
            if whole && before_ok && after_ok {
                return true;
            }
            from = end;
        }
    }
    false
}

#[test]
fn every_field_behind_a_masking_derive_is_read_in_production() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&root, &mut files);
    assert!(
        files.len() > 20,
        "walked {} files under {}",
        files.len(),
        root.display()
    );

    let mut prod = String::new();
    let mut structs: BTreeMap<String, (PathBuf, Vec<String>)> = BTreeMap::new();
    for f in &files {
        let src = fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let stripped = strip_test_items(&src);
        for (name, fields) in masked_structs(&stripped) {
            /* two structs of one name in two files (screens::inventory::Section and
             * ingest::Section) are checked as one union of fields; a miss on either is a miss */
            let e = structs
                .entry(name)
                .or_insert_with(|| (f.clone(), Vec::new()));
            for fld in fields {
                if !e.1.contains(&fld) {
                    e.1.push(fld);
                }
            }
        }
        prod.push_str(&stripped);
        prod.push('\n');
    }
    assert!(
        structs.len() >= 60,
        "found only {} masked structs; the parser broke, not the crate",
        structs.len()
    );

    let mut unread: Vec<String> = Vec::new();
    for (name, (file, fields)) in &structs {
        for fld in fields {
            if !has_field_access(&prod, fld) && !has_destructure_read(&prod, fld) {
                unread.push(format!(
                    "{}::{} ({})",
                    name,
                    fld,
                    file.strip_prefix(&root).unwrap_or(file).display()
                ));
            }
        }
    }
    assert!(
        unread.is_empty(),
        "{} field(s) on structs that derive one of {:?} have no production read (rustc cannot \
         see these; wire each into a screen or remove it):\n  {}",
        unread.len(),
        MASKING,
        unread.join("\n  ")
    );
}

#[test]
fn the_cutter_removes_a_test_module_and_keeps_the_rest() {
    let src = "pub struct A { pub x: u32 }\n#[cfg(test)]\nmod tests { fn t() { let s = \"}\"; a.x; } }\nfn keep() {}\n#[cfg(test)]\nmod other;\nfn also() {}\n";
    let out = strip_test_items(src);
    assert!(out.contains("pub struct A"));
    assert!(out.contains("fn keep()"));
    assert!(out.contains("fn also()"));
    assert!(!out.contains("a.x"), "{out}");
    assert!(!out.contains("mod other"), "{out}");
}

#[test]
fn the_reader_tells_a_read_from_a_write() {
    let prod =
        "let l = Landing { exp: 1, tier: 2 };\nlet t = l.tier;\nlet Landing { exp, .. } = l;\n";
    assert!(has_field_access(prod, "tier"));
    assert!(!has_field_access(prod, "exp"));
    assert!(has_destructure_read(prod, "exp"));
    assert!(!has_destructure_read(prod, "tier"));
    /* a method call is not a field read */
    assert!(!has_field_access("x.len()", "len"));
    /* the struct literal alone is a write */
    let w = "Fit { ok: true, narrows_cls, narrows_sl: false }";
    assert!(!has_field_access(w, "narrows_cls"));
    assert!(!has_destructure_read(w, "narrows_cls"));
}

/* ------------------------------------------------------- the module floor --
 *
 * THE BLIND SPOT THIS CLOSES, AND WHAT IT COST. Both of the crate's declared reachability floors
 * were structurally incapable of seeing a module with no callers, and one was present: `seal.rs`,
 * 1,211 lines and 33 passing tests, a complete wax seal drawing system, named by `lib.rs` and by
 * nothing else in the tree. The linker dropped it from the binary and no reader of the app ever
 * saw a seal.
 *
 * Neither floor could have said so. Rustc's dead-code lint runs on the BIN target, and `seal` was
 * `pub mod` in a LIBRARY, where a `pub` item is the crate's API by definition. The test above
 * walks FIELDS on structs behind a masking derive, and has no notion of a function or a module
 * having no caller at all. So the check below is a third floor, aimed at the granularity the other
 * two skip: the module.
 *
 * WHAT IT DOES. Every `pub mod` in `lib.rs` must be NAMED, as `name::`, by production code in some
 * other file: a `use`, a call, a type path. Its own file does not count and neither does `lib.rs`'s
 * declaration of it, which is the whole point.
 *
 * WHAT IT CANNOT SEE, said as plainly as the rest of this file. It skips whole-line comments so a
 * doc comment cannot vouch for a module (round one's `persona.rs` described `seal::draw` at
 * length in prose, which would have passed a naive substring scan while nothing called it), but a
 * trailing comment on a line of code is not stripped and would. It proves a module is REACHED, not
 * that every item in it is; a module with one live caller and forty dead functions passes. And it
 * reads `lib.rs`, so a module declared only inside `main.rs` is outside its scope. */

/// Whether `prod` names the module `name` as a path (`name::`), on a line that is not a comment.
fn names_module(prod: &str, name: &str) -> bool {
    let needle = format!("{name}::");
    for line in prod.lines() {
        let t = line.trim_start();
        /* Whole line comments only: `//`, `///`, `//!` and the `*` continuations of the block
         * comments this crate writes its long notes in. */
        if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            continue;
        }
        let b = line.as_bytes();
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(&needle) {
            let at = from + rel;
            /* A whole word: `data::` must not match `testdata::`. */
            if at == 0 || !is_ident(b[at - 1]) {
                /* AND A NESTED PATH IS NOT A REACH OF THE TOP LEVEL MODULE.
                 *
                 * `screens::chat::ChatScreen` names `screens`'s child. `crate::chat` is a
                 * different module that happens to share a name, and this rule used to let the
                 * first vouch for the second: it asked only that the byte before the needle not
                 * be an ident byte, and `:` is not one, so EVERY `a::b::` path in the tree
                 * counted as a reach of a top level `b`.
                 *
                 * IT WAS LIVE WHEN THIS WAS WRITTEN. `crate::chat`, the Twitch reader thread, was
                 * called by nothing at all, and this floor stayed green because `main.rs` holds a
                 * field typed `screens::chat::ChatScreen`. That is the module level form of the
                 * `Changed::always_on_top` hiding behind `Settings::always_on_top` defect the note
                 * at the top of this file describes, which is the same file admitting the same
                 * blind spot twice, at two scales.
                 *
                 * A `::` before the needle is a reach only when the segment before THAT is a root:
                 * `crate`, `self`, `super`, or the crate's own name the way an external path
                 * spells it. A single `:` is not a path separator at all (`let s: chat::Event`),
                 * and neither is a `{`, a comma or a space, so all of those still count. */
                let nested = at >= 2 && b[at - 1] == b':' && b[at - 2] == b':' && {
                    let end = at - 2;
                    let mut start = end;
                    while start > 0 && is_ident(b[start - 1]) {
                        start -= 1;
                    }
                    !matches!(
                        &b[start..end],
                        b"crate" | b"self" | b"super" | b"grimoire_desktop"
                    )
                };
                if !nested {
                    return true;
                }
            }
            from = at + 1;
        }
    }
    false
}

/// The modules `lib.rs` declares, in the order it declares them.
fn declared_modules(lib: &str) -> Vec<String> {
    lib.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("pub mod "))
        .filter_map(|rest| rest.strip_suffix(';'))
        .map(|n| n.trim().to_owned())
        .collect()
}

#[test]
fn every_public_module_is_reached_from_outside_itself() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let lib = fs::read_to_string(root.join("lib.rs")).expect("read lib.rs");
    let modules = declared_modules(&lib);
    assert!(
        modules.len() >= 10,
        "found only {} `pub mod` lines in lib.rs; the parser broke, not the crate",
        modules.len()
    );

    let mut files = Vec::new();
    rs_files(&root, &mut files);

    let mut unreached: Vec<String> = Vec::new();
    for m in &modules {
        let own_file = root.join(format!("{m}.rs"));
        let own_dir = root.join(m);
        let mut prod = String::new();
        for f in &files {
            /* Its own file cannot vouch for it, and neither can `lib.rs`, which only declares it. */
            if *f == own_file || f.starts_with(&own_dir) || *f == root.join("lib.rs") {
                continue;
            }
            let src = fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            prod.push_str(&strip_test_items(&src));
            prod.push('\n');
        }
        if !names_module(&prod, m) {
            unreached.push(m.clone());
        }
    }

    assert!(
        unreached.is_empty(),
        "{} module(s) declared `pub` in lib.rs are named by no production code outside \
         themselves, so the binary cannot reach them and the linker drops them (wire each into a \
         screen or remove it):\n  {}",
        unreached.len(),
        unreached.join("\n  ")
    );
}

#[test]
fn the_module_floor_can_tell_a_caller_from_a_comment_and_a_prefix() {
    /* The exact shape that shipped: a module named only by prose. */
    assert!(!names_module(
        "/// see `seal::draw` for the tilt rule\n//! and `seal::Struck`\nfn f() {}\n",
        "seal"
    ));
    assert!(!names_module(
        " * the block comment continuation: seal::draw\n",
        "seal"
    ));
    /* A real call, a real use, and a type path. */
    assert!(names_module("    seal::draw(ui, at);\n", "seal"));
    assert!(names_module("use crate::seal::Seal;\n", "seal"));
    assert!(names_module(
        "use grimoire_desktop::{self, seal::Seal};\n",
        "seal"
    ));
    /* Whole words only: `data::` is not `testdata::`. */
    assert!(!names_module("testdata::snapshot();\n", "data"));
    assert!(names_module("let s: data::Snapshot = x;\n", "data"));

    /* A NESTED MODULE THAT SHARES THE NAME IS NOT THIS MODULE.
     *
     * The first line here is the exact text out of `main.rs` that held this floor green over a
     * `crate::chat` nothing called. Every assertion below it fails on the rule that shipped, which
     * accepted any `::` before the needle. */
    assert!(!names_module(
        "    chat: screens::chat::ChatScreen,\n",
        "chat"
    ));
    assert!(!names_module(
        "use crate::screens::chat::ChatScreen;\n",
        "chat"
    ));
    /* And the four shapes that ARE a reach of the top level module. */
    assert!(names_module("use crate::chat::Event;\n", "chat"));
    assert!(names_module("    chat::start(&cx);\n", "chat"));
    assert!(names_module("let e: chat::Event = x;\n", "chat"));
    assert!(names_module("use grimoire_desktop::chat::Event;\n", "chat"));
    /* A module named nowhere at all. */
    assert!(!names_module("fn f() {}\n", "seal"));
}

/// DEFECT: A DEFAULT WINDOWS CLONE WAS RED BEFORE ANYONE TOUCHED IT.
///
/// Git for Windows ships `core.autocrlf=true`, so a checkout writes CRLF. A dozen tests in this
/// crate read their own source with `include_str!` and match on `"\n"`; with a CR in front of every
/// newline `screens::dashboards::tests::a_release_that_moves_nothing_writes_nothing` could not find
/// the end of `fn grid(` and panicked. Measured on this machine: one `git stash` rewrote 84 tracked
/// files as CRLF and the gate went from green to red with no code change.
///
/// `.gitattributes` pins `eol=lf`, which beats any user or system autocrlf. This names the cause
/// directly instead of leaving the next contributor to decode a panic about "its end".
///
/// WHAT MUTATION MAKES THIS RED: deleting `.gitattributes` and re-checking out on Windows.
#[test]
fn the_source_this_suite_reads_is_checked_out_with_lf_endings() {
    let mut files = Vec::new();
    rs_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    assert!(files.len() > 20, "the walk found only {} files", files.len());
    let crlf: Vec<String> = files
        .iter()
        .filter(|p| fs::read(p).is_ok_and(|b| b.contains(&b'\r')))
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        crlf.is_empty(),
        "{} source files carry CR bytes, so every include_str! test matching on \"\\n\" is \
         measuring the checkout and not the code. Check .gitattributes still says eol=lf, then \
         `git rm -r --cached . && git reset --hard`. First few: {:?}",
        crlf.len(),
        &crlf[..crlf.len().min(5)]
    );

    let attrs = include_str!("../../../.gitattributes");
    assert!(
        attrs.lines().any(|l| l.trim() == "* text=auto eol=lf"),
        ".gitattributes no longer pins LF for every text file"
    );
}

#[test]
fn the_module_list_is_read_off_lib_rs() {
    let src = "//! doc\npub mod chrome;\npub mod data;\n#[cfg(test)]\nmod reach;\npub fn f() {}\n";
    assert_eq!(declared_modules(src), vec!["chrome", "data"]);
}
