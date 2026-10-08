//! The rail for the Monsters & Memories build: COMPENDIUM, HERALD, ORACLE, THE TAVERN, STREAMS.
//!
//! WHY THE ROWS ARE BOUND TO AN ENUM AND NOT TO A STRING. Every row carries a `ScreenId`, the body
//! switches on that, and a test proves each variant appears in the table exactly once: a screen
//! that is built but unreachable, or a row that leads nowhere, fails the build.
//!
//! WHY THE LEADING SQUARE IS COMPUTED HERE. `square` is the one place that answers "does this row
//! have what it needs", from a plain `Facts` struct, so the rules can be read side by side and
//! tested without a live context.
//!
//! WHAT CHANGED FROM THE EVERQUEST LEGENDS BUILD. Its rail was built around a combat log and an EQ
//! data snapshot (CHRONICLE, MY LEGEND, the EQ COMPENDIUM, THE BAZAAR). Monsters & Memories writes
//! no log, and its data comes from the community wiki bundle, so those rows are gone. Every row
//! now has a screen.

use crate::chrome::State;
use crate::screens::Cx;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ScreenId {
    /* COMPENDIUM: what the game contains, from the community wiki bundle. */
    Zones,
    Npcs,
    Items,
    Quests,
    Spells,
    /* HERALD: what the developers said. */
    Herald,
    /* ORACLE: ask a question, answered from the wiki and the developers' posts. */
    Oracle,
    /* THE TAVERN: other people. */
    Lfg,
    /* STREAMS: watching and chatting. */
    Watch,
    Videos,
    Chat,
}

impl ScreenId {
    /// Every variant, in declaration order. Kept honest by `ordinal`'s exhaustive match and the
    /// test that compares the two.
    pub const ALL: [ScreenId; 11] = [
        ScreenId::Zones,
        ScreenId::Npcs,
        ScreenId::Items,
        ScreenId::Quests,
        ScreenId::Spells,
        ScreenId::Herald,
        ScreenId::Oracle,
        ScreenId::Lfg,
        ScreenId::Watch,
        ScreenId::Videos,
        ScreenId::Chat,
    ];

    /// A stable position per variant. Exhaustive on purpose: see `ALL`.
    pub fn ordinal(self) -> usize {
        match self {
            ScreenId::Zones => 0,
            ScreenId::Npcs => 1,
            ScreenId::Items => 2,
            ScreenId::Quests => 3,
            ScreenId::Spells => 4,
            ScreenId::Herald => 5,
            ScreenId::Oracle => 6,
            ScreenId::Lfg => 7,
            ScreenId::Watch => 8,
            ScreenId::Videos => 9,
            ScreenId::Chat => 10,
        }
    }
}

/// The rail: headings, each with its rows.
pub const NAV: &[(&str, &[(&str, ScreenId)])] = &[
    (
        "COMPENDIUM",
        &[
            ("Zones", ScreenId::Zones),
            ("NPCs", ScreenId::Npcs),
            ("Items", ScreenId::Items),
            ("Quests", ScreenId::Quests),
            ("Spells & Abilities", ScreenId::Spells),
        ],
    ),
    ("HERALD", &[("Dev posts", ScreenId::Herald)]),
    ("ORACLE", &[("Ask Oracle", ScreenId::Oracle)]),
    ("THE TAVERN", &[("Groups", ScreenId::Lfg)]),
    (
        "STREAMS",
        &[
            ("Watch Live", ScreenId::Watch),
            ("Videos", ScreenId::Videos),
            ("Chat", ScreenId::Chat),
        ],
    ),
];

/* ------------------------------------------------------- tier 3: the sections -- */

/// A section: its label, the screen it is (None when it is a view of its owner), and its tabs.
pub type Section = (&'static str, Option<ScreenId>, &'static [&'static str]);

/// Screens with several views. None yet in this build: each row is one page.
pub const SECTIONS: &[(ScreenId, &[Section])] = &[];

/// The rail label for a row, when the screen is a row.
pub fn name_of(id: ScreenId) -> Option<&'static str> {
    let (head, row) = find(id)?;
    NAV.get(head)
        .and_then(|(_, rows)| rows.get(row))
        .map(|(name, _)| *name)
}

/// The sections a screen owns, or none.
pub fn sections_of(id: ScreenId) -> &'static [Section] {
    SECTIONS
        .iter()
        .find(|(s, _)| *s == id)
        .map(|(_, rows)| *rows)
        .unwrap_or(&[])
}

/// The tabs of one section, or none.
pub fn tabs_of(id: ScreenId, section: usize) -> &'static [&'static str] {
    sections_of(id)
        .get(section)
        .map(|(_, _, tabs)| *tabs)
        .unwrap_or(&[])
}

/// The section a screen opens on. Every section is written, so the first.
pub fn default_section(_id: ScreenId) -> usize {
    0
}

/// The screen that owns `id` as one of its sections, and where.
pub fn parent_of(id: ScreenId) -> Option<(ScreenId, usize)> {
    SECTIONS.iter().find_map(|(owner, rows)| {
        rows.iter()
            .position(|(_, sec, _)| *sec == Some(id))
            .map(|at| (*owner, at))
    })
}

/// The headings open on first launch: the stream tools, which work today.
pub fn default_open() -> Vec<usize> {
    ["COMPENDIUM", "STREAMS"]
        .iter()
        .filter_map(|want| NAV.iter().position(|(head, _)| head == want))
        .collect()
}

/// Where a screen sits in the rail: (heading, row).
pub fn find(id: ScreenId) -> Option<(usize, usize)> {
    NAV.iter()
        .enumerate()
        .find_map(|(si, (_, rows))| rows.iter().position(|(_, r)| *r == id).map(|ri| (si, ri)))
}

/// The label a screen goes by, as a row or as a section.
pub fn label(id: ScreenId) -> &'static str {
    if let Some((si, ri)) = find(id) {
        return NAV[si].1[ri].0;
    }
    parent_of(id)
        .map(|(owner, at)| sections_of(owner)[at].0)
        .unwrap_or("")
}

/// The heading a screen lives under.
pub fn section_of(id: ScreenId) -> &'static str {
    if let Some((si, _)) = find(id) {
        return NAV[si].0;
    }
    parent_of(id)
        .map(|(owner, _)| section_of(owner))
        .unwrap_or("")
}

/* ------------------------------------------------------------- the squares -- */

/// What the rail needs to know to colour its squares, boiled out of `Cx` so the rules are pure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub lfg_entries: bool,
    pub twitch_known: bool,
    pub twitch_error: bool,
    pub youtube_known: bool,
    pub youtube_error: bool,
    pub lore_loaded: bool,
    pub lore_wrong: bool,
}

pub fn facts(cx: &Cx) -> Facts {
    let has_entries = match cx.settings.extra.get("lfg_board") {
        Some(serde_json::Value::Object(o)) => o
            .get("entries")
            .and_then(|e| e.as_array())
            .is_some_and(|a| !a.is_empty()),
        _ => false,
    };
    Facts {
        lfg_entries: has_entries,
        twitch_known: cx.live.twitch.live.is_some(),
        twitch_error: cx.live.twitch.error.is_some(),
        youtube_known: cx.live.youtube.live.is_some(),
        youtube_error: cx.live.youtube.error.is_some(),
        lore_loaded: cx.lore.lore.is_some(),
        lore_wrong: cx.lore.why.is_some(),
    }
}

/// The leading square of a row.
pub fn square(id: ScreenId, f: &Facts) -> State {
    let from_channel = |known: bool, error: bool| -> State {
        if known {
            State::Settled
        } else if error {
            State::Wrong
        } else {
            State::Idle
        }
    };
    match id {
        /* The Compendium follows the wiki bundle: settled once loaded, wrong when it is missing
         * or would not load, idle while it loads. */
        ScreenId::Zones
        | ScreenId::Npcs
        | ScreenId::Items
        | ScreenId::Quests
        | ScreenId::Spells => from_channel(f.lore_loaded, f.lore_wrong),
        /* Herald reads the recorder's file; whether it has posts is the screen's to say. */
        ScreenId::Herald => State::Idle,
        /* Oracle answers from the bundle: it can do nothing without one. */
        ScreenId::Oracle => {
            if f.lore_wrong {
                State::Wrong
            } else {
                State::Idle
            }
        }
        /* The board is local: settled once it holds an entry. */
        ScreenId::Lfg => {
            if f.lfg_entries {
                State::Settled
            } else {
                State::Idle
            }
        }
        /* Watch follows the Twitch channel; Videos the YouTube channel; Chat has no status. */
        ScreenId::Watch => from_channel(f.twitch_known, f.twitch_error),
        ScreenId::Videos => from_channel(f.youtube_known, f.youtube_error),
        ScreenId::Chat => State::Idle,
    }
}

/// The Compendium row that shows a kind.
pub fn screen_of(kind: crate::lore::Kind) -> ScreenId {
    use crate::lore::Kind;
    match kind {
        Kind::Zone => ScreenId::Zones,
        Kind::Npc => ScreenId::Npcs,
        Kind::Item => ScreenId::Items,
        Kind::Quest => ScreenId::Quests,
        Kind::Spell => ScreenId::Spells,
    }
}

/// The kind a Compendium row shows, or None for any other row.
pub fn kind_of(id: ScreenId) -> Option<crate::lore::Kind> {
    crate::lore::Kind::ALL
        .into_iter()
        .find(|k| screen_of(*k) == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_screen_is_in_the_rail_exactly_once() {
        for id in ScreenId::ALL {
            let n = NAV
                .iter()
                .flat_map(|(_, rows)| rows.iter())
                .filter(|(_, r)| *r == id)
                .count();
            assert_eq!(n, 1, "{id:?} appears {n} times in the rail");
        }
    }

    #[test]
    fn all_and_ordinal_agree() {
        for (i, id) in ScreenId::ALL.iter().enumerate() {
            assert_eq!(id.ordinal(), i, "{id:?}");
        }
    }

    #[test]
    fn the_headings_opened_on_first_launch_exist() {
        assert_eq!(default_open().len(), 2);
    }

    #[test]
    fn labels_and_headings_resolve() {
        assert_eq!(label(ScreenId::Watch), "Watch Live");
        assert_eq!(section_of(ScreenId::Watch), "STREAMS");
        assert_eq!(name_of(ScreenId::Oracle), Some("Ask Oracle"));
        assert_eq!(section_of(ScreenId::Zones), "COMPENDIUM");
    }

    #[test]
    fn squares_follow_their_sources() {
        let none = Facts::default();
        assert_eq!(square(ScreenId::Lfg, &none), State::Idle);
        assert_eq!(
            square(
                ScreenId::Lfg,
                &Facts {
                    lfg_entries: true,
                    ..none
                }
            ),
            State::Settled
        );
        assert_eq!(
            square(
                ScreenId::Watch,
                &Facts {
                    twitch_known: true,
                    ..none
                }
            ),
            State::Settled
        );
        assert_eq!(
            square(
                ScreenId::Watch,
                &Facts {
                    twitch_error: true,
                    ..none
                }
            ),
            State::Wrong
        );
        assert_eq!(
            square(
                ScreenId::Videos,
                &Facts {
                    youtube_known: true,
                    ..none
                }
            ),
            State::Settled
        );
        assert_eq!(square(ScreenId::Zones, &none), State::Idle);
    }
}
