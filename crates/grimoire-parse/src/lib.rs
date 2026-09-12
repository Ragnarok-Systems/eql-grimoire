//! Reading what the game wrote.
//!
//! Two inputs, both on the player's own disk: the chat log and the `/outputfile inventory`
//! dump. Neither is uploaded — this crate compiles to `wasm32` so the browser can do it, and
//! a 160 MB log has no business crossing a wire with a 10 ms CPU budget on the far side.
//!
//! `tail` exists because logs here are genuinely that big; the newest 8 MB answers almost
//! every question and costs nothing to read.

#![forbid(unsafe_code)]

/// The combat parser. Deliberately not re-exported at the crate root: the names it exports are
/// numerous and shape-specific, and a root alias nothing calls is exactly the unreachable surface
/// this repository keeps having to delete. Callers say `grimoire_parse::combat::`.
pub mod combat;
pub mod combines;
pub mod fights;
/// Who is in the reader's group, and when the log actually knows. Not re-exported at the root, for
/// the same reason as `combat`: callers say `grimoire_parse::group::`.
pub mod group;
pub mod inventory;
pub mod line;

pub use combines::{harvest, Attempt, Harvest, ItemStats};
pub use inventory::{Inventory, Place};
pub use line::{parse as parse_line, Line, Stamped};

/// The last `bytes` of a log, resynchronised to a line boundary.
///
/// A blind byte offset lands mid-line and mid-UTF-8. This walks forward to the first newline,
/// then to the first line that actually starts with a timestamp, so the caller can never be
/// handed a fragment that parses into something untrue.
pub fn tail(log: &str, bytes: usize) -> &str {
    if log.len() <= bytes {
        return log;
    }
    let mut start = log.len() - bytes;
    while start < log.len() && !log.is_char_boundary(start) {
        start += 1;
    }
    let rest = &log[start..];
    let rest = match rest.find('\n') {
        Some(i) => &rest[i + 1..],
        None => return "",
    };
    // Skip anything before the first properly stamped line.
    let mut off = 0usize;
    loop {
        let end = rest[off..].find('\n').map_or(rest.len(), |i| off + i);
        if line::split_stamp(rest[off..end].trim_end_matches('\r')).is_some() {
            return &rest[off..];
        }
        if end >= rest.len() {
            return "";
        }
        off = end + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_returns_everything_when_the_log_is_small() {
        let log = "[Mon Aug 03 01:12:13 2026] You have become better at Baking! (7)\n";
        assert_eq!(tail(log, 1 << 20), log);
    }

    #[test]
    fn tail_never_yields_a_partial_line() {
        let log = "[Mon Aug 03 01:12:13 2026] You have become better at Baking! (7)\n\
                   [Mon Aug 03 01:12:14 2026] You have become better at Baking! (8)\n\
                   [Mon Aug 03 01:12:15 2026] You have become better at Baking! (9)\n";
        for n in 1..log.len() {
            let t = tail(log, n);
            assert!(
                t.is_empty() || t.starts_with('['),
                "tail({n}) began mid-line: {:?}",
                &t[..t.len().min(30)]
            );
            // Whatever survived must parse cleanly, with nothing invented.
            for line in t.lines() {
                assert!(
                    parse_line(line).is_some(),
                    "tail({n}) produced junk: {line:?}"
                );
            }
        }
    }

    #[test]
    fn tail_is_safe_across_multibyte_characters() {
        let log = "[Mon Aug 03 01:12:13 2026] Ünïcödé says, 'hello'\n\
                   [Mon Aug 03 01:12:14 2026] You have become better at Baking! (8)\n";
        for n in 1..log.len() {
            let _ = tail(log, n); // must not panic on a char boundary
        }
    }

    #[test]
    fn a_log_with_no_stamped_lines_yields_nothing() {
        assert_eq!(tail("garbage\nmore garbage\nand more\n", 10), "");
    }

    #[test]
    fn harvesting_a_tail_matches_harvesting_the_end_of_the_whole_log() {
        let mut log = String::new();
        for i in 1..=40 {
            log.push_str(&format!(
                "[Mon Aug 03 01:12:13 2026] You have fashioned the items together to create something new: Ring.\n\
                 [Mon Aug 03 01:12:13 2026] You have become better at Jewelry Making! ({i})\n"
            ));
        }
        let whole = harvest(&log);
        let part = harvest(tail(&log, 400));
        assert!(part.attempts.len() < whole.attempts.len());
        assert_eq!(part.skills[&grimoire_core::Skill::JewelryMaking], 40);
    }
}
