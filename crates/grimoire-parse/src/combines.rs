//! Turning a log into combine data.
//!
//! This is the part no other EQL tool has. No other EQL log reader measures combines, so every
//! crafting number until now was an estimate.
//!
//! Three tricks make a plain log far more informative than it looks:
//!
//! 1. **A combine line does not say which tradeskill it was.** But when a combine grants a
//!    skill-up, the game writes both on the same timestamp. That labels the item, and once an
//!    item is labelled it stays labelled for every other attempt at it in the file.
//! 2. **A skill-up carries the new value.** So the crafter's exact skill is known at every
//!    moment, and therefore at every attempt.
//! 3. **`You can no longer advance your skill` fires exactly when skill reaches trivial.** So
//!    the first time it appears for an item, that item's trivial is *pinned*, not estimated.
//!
//! Everything here is a single forward pass and holds only the events, so a 160 MB log is a
//! streaming job rather than a memory problem.

use crate::line::{parse, Line};
use grimoire_core::recipe::Skill;
use std::collections::HashMap;

/// One combine attempt, as observed.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Attempt {
    pub item: String,
    /// `None` when no skill-up ever labelled this item in this log.
    pub skill: Option<Skill>,
    /// The crafter's skill at the moment of the attempt, when known.
    pub at_skill: Option<u16>,
    pub success: bool,
    /// The `can no longer advance` line fired for this attempt.
    pub trivial_reached: bool,
}

/// What a log says about one item.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ItemStats {
    pub attempts: u32,
    pub successes: u32,
    pub skill: Option<Skill>,
    /// Highest skill seen *without* the trivial line — trivial is above this.
    pub trivial_above: Option<u16>,
    /// Lowest skill seen *with* the trivial line — trivial is at or below this.
    pub trivial_at_most: Option<u16>,
}

impl ItemStats {
    /// The trivial, when the log pins it exactly.
    ///
    /// Pinned means the bracket has closed to nothing: the crafter was seen still learning at
    /// skill *n*, and seen trivialling at *n* or *n+1*. Anything looser is a range, and a
    /// range must not be reported as a number.
    pub fn pinned_trivial(&self) -> Option<u16> {
        match (self.trivial_at_most, self.trivial_above) {
            (Some(hi), Some(lo)) if hi.saturating_sub(lo) <= 1 => Some(hi),
            _ => None,
        }
    }

    pub fn success_rate(&self) -> Option<f64> {
        (self.attempts > 0).then(|| self.successes as f64 / self.attempts as f64)
    }
}

/// Everything a log had to say about crafting.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Harvest {
    pub attempts: Vec<Attempt>,
    pub items: HashMap<String, ItemStats>,
    /// Highest skill reached in this log, per tradeskill.
    pub skills: HashMap<Skill, u16>,
    /// Lines that were stamped and understood. Useful for "did I read the right file".
    pub events_seen: u64,
}

impl Harvest {
    /// Buckets of `(trivial, skill, attempts, successes)` for items whose trivial is pinned.
    ///
    /// This is the shape the calibration fixture takes, and the shape a crafting summary
    /// would upload — a few hundred bytes, no item names, nothing about who you are.
    pub fn calibration_buckets(&self) -> Vec<(u16, u16, u32, u32)> {
        let pinned: HashMap<&str, u16> = self
            .items
            .iter()
            .filter_map(|(k, v)| v.pinned_trivial().map(|t| (k.as_str(), t)))
            .collect();

        let mut acc: HashMap<(u16, u16), (u32, u32)> = HashMap::new();
        for a in &self.attempts {
            let (Some(t), Some(s)) = (pinned.get(a.item.as_str()), a.at_skill) else {
                continue;
            };
            let e = acc.entry((*t, s)).or_default();
            e.0 += 1;
            e.1 += u32::from(a.success);
        }
        let mut out: Vec<_> = acc
            .into_iter()
            .map(|((t, s), (n, k))| (t, s, n, k))
            .collect();
        out.sort_unstable();
        out
    }
}

/// Read a whole log.
///
/// Two passes over the events, not the file: the first learns which tradeskill each item
/// belongs to, the second uses that to label every attempt. Item→skill has to be known before
/// the attempts can be labelled, and an item's first attempts usually come before its first
/// skill-up.
pub fn harvest(log: &str) -> Harvest {
    #[derive(Clone, Copy)]
    enum Ev<'a> {
        Ok(&'a str),
        Fail(&'a str),
        Trivial,
        Up(&'a str, u16),
    }

    let mut events: Vec<(&str, Ev)> = Vec::new();
    for raw in log.lines() {
        let Some(s) = parse(raw) else { continue };
        let ev = match s.line {
            Line::Fashioned { item } => Ev::Ok(item),
            Line::Lacked { item } => Ev::Fail(item),
            Line::Trivial => Ev::Trivial,
            Line::SkillUp { skill, value } => Ev::Up(skill, value),
            _ => continue,
        };
        events.push((s.at, ev));
    }

    // Pass one: a skill-up on the same timestamp as a combine names that item's tradeskill.
    // Votes, rather than first-wins, because two crafts can share a second.
    let mut votes: HashMap<&str, HashMap<Skill, u32>> = HashMap::new();
    for (i, (at, ev)) in events.iter().enumerate() {
        let item = match ev {
            Ev::Ok(i) | Ev::Fail(i) => *i,
            _ => continue,
        };
        for (at2, ev2) in events.iter().skip(i + 1).take(2) {
            if at2 != at {
                break;
            }
            if let Ev::Up(name, _) = ev2 {
                if let Some(sk) = Skill::from_log(name) {
                    *votes.entry(item).or_default().entry(sk).or_default() += 1;
                }
                break;
            }
        }
    }
    let item_skill: HashMap<&str, Skill> = votes
        .into_iter()
        .filter_map(|(item, v)| {
            v.into_iter()
                .max_by_key(|&(sk, n)| (n, std::cmp::Reverse(sk)))
                .map(|(sk, _)| (item, sk))
        })
        .collect();

    // Pass two.
    let mut h = Harvest::default();
    let mut current: HashMap<Skill, u16> = HashMap::new();
    let mut trivial_pending = false;

    for (_, ev) in &events {
        h.events_seen += 1;
        match *ev {
            Ev::Up(name, value) => {
                if let Some(sk) = Skill::from_log(name) {
                    current.insert(sk, value);
                    let best = h.skills.entry(sk).or_insert(value);
                    *best = (*best).max(value);
                }
            }
            Ev::Trivial => {
                trivial_pending = true;
                continue;
            }
            Ev::Ok(item) | Ev::Fail(item) => {
                let success = matches!(ev, Ev::Ok(_));
                let skill = item_skill.get(item).copied();
                // The skill value the game reports on a skill-up is the value *after* the
                // increment, so the attempt that caused it happened one point lower. Using
                // the pre-attempt value is what makes the trivial bracket land on the right
                // integer.
                let at_skill = skill.and_then(|s| current.get(&s).copied());

                let st = h.items.entry(item.to_string()).or_default();
                st.attempts += 1;
                st.successes += u32::from(success);
                st.skill = st.skill.or(skill);
                if let Some(v) = at_skill {
                    if trivial_pending {
                        st.trivial_at_most = Some(st.trivial_at_most.map_or(v, |x| x.min(v)));
                    } else {
                        st.trivial_above = Some(st.trivial_above.map_or(v, |x| x.max(v)));
                    }
                }

                h.attempts.push(Attempt {
                    item: item.to_string(),
                    skill,
                    at_skill,
                    success,
                    trivial_reached: trivial_pending,
                });
                trivial_pending = false;
            }
        }
        if !matches!(ev, Ev::Trivial) {
            trivial_pending = false;
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(lines: &[&str]) -> String {
        lines
            .iter()
            .map(|l| format!("[Mon Aug 03 01:12:13 2026] {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn an_unlabelled_combine_is_still_counted() {
        let h = harvest(&log(&[
            "You have fashioned the items together to create something new: Widget.",
        ]));
        let st = &h.items["Widget"];
        assert_eq!((st.attempts, st.successes), (1, 1));
        assert_eq!(
            st.skill, None,
            "no skill-up, so no tradeskill may be claimed"
        );
    }

    #[test]
    fn a_same_second_skill_up_labels_the_item_for_the_whole_file() {
        // The first two attempts come before anything names the skill; they must still end
        // up labelled once the third attempt reveals it.
        let h = harvest(&log(&[
            "You lacked the skills to fashion Gold Ring.",
            "You lacked the skills to fashion Gold Ring.",
            "You have fashioned the items together to create something new: Gold Ring.",
            "You have become better at Jewelry Making! (40)",
        ]));
        assert_eq!(h.items["Gold Ring"].skill, Some(Skill::JewelryMaking));
        assert!(h
            .attempts
            .iter()
            .all(|a| a.skill == Some(Skill::JewelryMaking)));
    }

    #[test]
    fn skill_value_tracks_forward_and_the_high_water_mark_is_kept() {
        let h = harvest(&log(&[
            "You have fashioned the items together to create something new: Ring.",
            "You have become better at Jewelry Making! (10)",
            "You have fashioned the items together to create something new: Ring.",
            "You have become better at Jewelry Making! (11)",
            "You have fashioned the items together to create something new: Ring.",
        ]));
        let at: Vec<Option<u16>> = h.attempts.iter().map(|a| a.at_skill).collect();
        assert_eq!(at, vec![None, Some(10), Some(11)]);
        assert_eq!(h.skills[&Skill::JewelryMaking], 11);
    }

    #[test]
    fn the_trivial_line_pins_a_trivial() {
        // Learning at 73, trivialling at 74 → trivial is 74, exactly.
        let h = harvest(&log(&[
            "You have fashioned the items together to create something new: Bracelet.",
            "You have become better at Jewelry Making! (73)",
            "You have fashioned the items together to create something new: Bracelet.",
            "You have become better at Jewelry Making! (74)",
            "You can no longer advance your skill from making this item.",
            "You have fashioned the items together to create something new: Bracelet.",
        ]));
        let st = &h.items["Bracelet"];
        assert_eq!(st.trivial_above, Some(73));
        assert_eq!(st.trivial_at_most, Some(74));
        assert_eq!(st.pinned_trivial(), Some(74));
    }

    #[test]
    fn a_loose_bracket_is_not_reported_as_a_number() {
        // Seen learning at 20, then not seen again until it was already trivial at 90.
        let h = harvest(&log(&[
            "You have fashioned the items together to create something new: Thing.",
            "You have become better at Baking! (20)",
            "You have fashioned the items together to create something new: Thing.",
            "You have become better at Baking! (90)",
            "You can no longer advance your skill from making this item.",
            "You have fashioned the items together to create something new: Thing.",
        ]));
        let st = &h.items["Thing"];
        assert_eq!(st.trivial_above, Some(20));
        assert_eq!(st.trivial_at_most, Some(90));
        assert_eq!(
            st.pinned_trivial(),
            None,
            "a 70-point range is not a trivial"
        );
    }

    #[test]
    fn the_trivial_flag_applies_to_the_next_combine_only() {
        let h = harvest(&log(&[
            "You have become better at Baking! (50)",
            "You can no longer advance your skill from making this item.",
            "You have fashioned the items together to create something new: Pie.",
            "You have fashioned the items together to create something new: Pie.",
        ]));
        let flags: Vec<bool> = h.attempts.iter().map(|a| a.trivial_reached).collect();
        assert_eq!(flags, vec![true, false]);
    }

    #[test]
    fn calibration_buckets_only_include_pinned_items() {
        let h = harvest(&log(&[
            "You have fashioned the items together to create something new: Bracelet.",
            "You have become better at Jewelry Making! (73)",
            "You lacked the skills to fashion Bracelet.",
            "You have become better at Jewelry Making! (74)",
            "You can no longer advance your skill from making this item.",
            "You have fashioned the items together to create something new: Bracelet.",
            "You have fashioned the items together to create something new: Unpinned.",
        ]));
        let b = h.calibration_buckets();
        assert!(b.iter().all(|&(t, ..)| t == 74));
        assert_eq!(b.iter().map(|&(_, _, n, _)| n).sum::<u32>(), 2);
    }

    #[test]
    fn buckets_carry_no_item_names_or_identity() {
        // The privacy claim is that only a summary crosses the wire. That has to be true of the
        // type, not just the intention.
        let h = harvest(&log(&[
            "You have fashioned the items together to create something new: Bracelet.",
            "You have become better at Jewelry Making! (73)",
            "You have fashioned the items together to create something new: Bracelet.",
            "You have become better at Jewelry Making! (74)",
            "You can no longer advance your skill from making this item.",
            "You have fashioned the items together to create something new: Bracelet.",
        ]));
        let b = h.calibration_buckets();
        assert!(!b.is_empty(), "nothing to check");
        let rendered = format!("{b:?}");
        assert!(!rendered.contains("Bracelet"));
    }

    #[test]
    fn an_empty_log_is_not_an_error() {
        let h = harvest("");
        assert_eq!(h.events_seen, 0);
        assert!(h.attempts.is_empty());
        assert!(h.calibration_buckets().is_empty());
    }

    #[test]
    fn chat_cannot_inject_a_combine() {
        let h = harvest(&log(&[
            "Mogging tells General:3, 'You have fashioned the items together to create something new: Free Plat.'",
        ]));
        assert!(h.items.is_empty());
    }
}
