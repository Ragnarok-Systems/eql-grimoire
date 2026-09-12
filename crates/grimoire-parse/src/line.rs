//! One log line, recognised.
//!
//! The grammar below was read off 254 MB of real logs, not guessed:
//!
//! ```text
//! [Mon Aug 03 01:12:13 2026] You have fashioned the items together to create something new: Silver Malachite Ring.
//! [Mon Aug 03 01:12:22 2026] You lacked the skills to fashion Silver Malachite Ring.
//! [Mon Aug 03 01:12:13 2026] You can no longer advance your skill from making this item.
//! [Mon Aug 03 01:15:14 2026] You have become better at Jewelry Making! (22)
//! [Mon Aug 03 01:12:45 2026] Requesting matching recipes...
//! ```

const OK: &str = "You have fashioned the items together to create something new: ";
const FAIL: &str = "You lacked the skills to fashion ";
const TRIVIAL: &str = "You can no longer advance your skill from making this item.";
const BETTER: &str = "You have become better at ";
const RECIPES: &str = "Requesting matching recipes...";
const ENTERED: &str = "You have entered ";

/// A log line the Grimoire cares about. Everything else — and it is nearly everything — is
/// dropped without allocating.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Line<'a> {
    /// A combine landed.
    Fashioned { item: &'a str },
    /// A combine failed.
    Lacked { item: &'a str },
    /// Skill has reached this recipe's trivial. Fires *before* the result line.
    Trivial,
    /// Skill is now exactly `value`. Fires on the same timestamp as the combine that caused it.
    SkillUp { skill: &'a str, value: u16 },
    /// The recipe window was opened.
    RecipeSearch,
    /// Zone change.
    Entered { zone: &'a str },
}

/// A parsed line and the raw timestamp it carried.
///
/// The timestamp stays a `&str` — `[Mon Aug 03 01:12:13 2026]`. Two combines belong together
/// when their stamps are equal, which is all the parser needs, and dodges pulling a date
/// library into a wasm build for a string comparison.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stamped<'a> {
    pub at: &'a str,
    pub line: Line<'a>,
}

/// Split `[stamp] body`. Returns `None` for any line that is not stamped.
pub fn split_stamp(raw: &str) -> Option<(&str, &str)> {
    let raw = raw.strip_prefix('[')?;
    let end = raw.find(']')?;
    let stamp = &raw[..end];
    // The game writes exactly `Mon Aug 03 01:12:13 2026`. Anything else is a chat line that
    // happens to start with a bracket.
    if stamp.len() != 24 {
        return None;
    }
    let body = raw[end + 1..].strip_prefix(' ')?;
    Some((stamp, body))
}

/// Recognise one line. Cheap enough to run over every line of a 160 MB file.
pub fn parse(raw: &str) -> Option<Stamped<'_>> {
    let (at, body) = split_stamp(raw.trim_end_matches(['\r', '\n']))?;
    let line = parse_body(body)?;
    Some(Stamped { at, line })
}

fn parse_body(body: &str) -> Option<Line<'_>> {
    // Ordered by how often each fires, and gated on the first byte so the common case —
    // somebody talking in General — costs one comparison.
    if !body.starts_with('Y') {
        if let Some(rest) = body.strip_prefix(ENTERED) {
            return Some(Line::Entered {
                zone: rest.trim_end_matches('.'),
            });
        }
        if body == RECIPES {
            return Some(Line::RecipeSearch);
        }
        return None;
    }
    if let Some(rest) = body.strip_prefix(OK) {
        return Some(Line::Fashioned {
            item: trim_sentence(rest),
        });
    }
    if let Some(rest) = body.strip_prefix(FAIL) {
        return Some(Line::Lacked {
            item: trim_sentence(rest),
        });
    }
    if body == TRIVIAL {
        return Some(Line::Trivial);
    }
    if let Some(rest) = body.strip_prefix(BETTER) {
        // `Jewelry Making! (22)`
        let (skill, tail) = rest.split_once("! (")?;
        let value = tail.strip_suffix(')')?.parse().ok()?;
        return Some(Line::SkillUp { skill, value });
    }
    None
}

#[inline]
fn trim_sentence(s: &str) -> &str {
    s.strip_suffix('.').unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &[&str] = &[
        "[Mon Aug 03 01:12:13 2026] You can no longer advance your skill from making this item.",
        "[Mon Aug 03 01:12:13 2026] You have fashioned the items together to create something new: Silver Malachite Ring.",
        "[Mon Aug 03 01:12:22 2026] You lacked the skills to fashion Silver Malachite Ring.",
        "[Mon Aug 03 01:15:14 2026] You have become better at Jewelry Making! (22)",
        "[Mon Aug 03 01:12:45 2026] Requesting matching recipes...",
    ];

    #[test]
    fn reads_the_real_lines() {
        let got: Vec<Line> = REAL
            .iter()
            .filter_map(|l| parse(l))
            .map(|s| s.line)
            .collect();
        assert_eq!(
            got,
            vec![
                Line::Trivial,
                Line::Fashioned {
                    item: "Silver Malachite Ring"
                },
                Line::Lacked {
                    item: "Silver Malachite Ring"
                },
                Line::SkillUp {
                    skill: "Jewelry Making",
                    value: 22
                },
                Line::RecipeSearch,
            ]
        );
    }

    #[test]
    fn keeps_the_timestamp() {
        let s = parse(REAL[1]).unwrap();
        assert_eq!(s.at, "Mon Aug 03 01:12:13 2026");
        assert_eq!(s.at.len(), 24);
    }

    /// Players type. Anything a player can put in a chat line must not become an event.
    #[test]
    fn chat_cannot_forge_an_event() {
        let hostile = [
            "[Mon Aug 03 01:12:29 2026] Mogging tells General:3, 'You have fashioned the items together to create something new: Free Plat.'",
            "[Mon Aug 03 01:12:29 2026] Dias says, 'You have become better at Jewelry Making! (250)'",
            "You have fashioned the items together to create something new: No Timestamp.",
            "[bogus] You have become better at Jewelry Making! (250)",
        ];
        for h in hostile {
            let got = parse(h);
            assert!(
                !matches!(
                    got.as_ref().map(|s| &s.line),
                    Some(Line::Fashioned { .. }) | Some(Line::SkillUp { .. })
                ),
                "chat line was accepted as an event: {h}\n  -> {got:?}"
            );
        }
    }

    #[test]
    fn ignores_the_ocean_of_other_lines() {
        for junk in [
            "[Mon Aug 03 01:12:44 2026] Convulsions tells General:3, 'Even hairy ones?'",
            "[Mon Aug 03 01:12:41 2026] You have gained experience!",
            "",
            "[]",
            "[Mon Aug 03 01:12:41 2026]",
        ] {
            assert!(parse(junk).is_none(), "accepted junk: {junk:?}");
        }
    }

    #[test]
    fn item_names_keep_their_punctuation_but_lose_the_full_stop() {
        let s = parse(
            "[Mon Aug 03 01:12:13 2026] You have fashioned the items together to create something new: Jeweler's Kit Mk. II.",
        )
        .unwrap();
        assert_eq!(
            s.line,
            Line::Fashioned {
                item: "Jeweler's Kit Mk. II"
            }
        );
    }

    #[test]
    fn a_malformed_skill_up_is_dropped_not_guessed() {
        for bad in [
            "[Mon Aug 03 01:15:14 2026] You have become better at Jewelry Making!",
            "[Mon Aug 03 01:15:14 2026] You have become better at Jewelry Making! (abc)",
            "[Mon Aug 03 01:15:14 2026] You have become better at Jewelry Making! (22",
        ] {
            assert!(parse(bad).is_none(), "guessed at {bad}");
        }
    }

    #[test]
    fn crlf_does_not_leak_into_a_value() {
        let s =
            parse("[Mon Aug 03 01:15:14 2026] You have become better at Baking! (7)\r\n").unwrap();
        assert_eq!(
            s.line,
            Line::SkillUp {
                skill: "Baking",
                value: 7
            }
        );
    }
}
