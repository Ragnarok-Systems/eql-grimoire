//! Whether the combine works.
//!
//! This is the number every quote in the Grimoire is built on, so it is the one number
//! that had to be tested rather than assumed.
//!
//! # Provenance
//!
//! Validated against 343 combine attempts pulled from `eqlog_Reviir_{qeynos,freeport,neriak}.txt`
//! at eleven trivials the log pins exactly — a trivial is pinned when
//! `You can no longer advance your skill from making this item.` first appears, because that
//! line fires precisely when skill reaches trivial, and the neighbouring
//! `You have become better at <skill>! (n)` stamps the skill value.
//!
//! | skill − trivial | attempts | observed | this model |
//! |---|---|---|---|
//! | −60 … −40 | 37 | 0.14 | 0.22 |
//! | −40 … −25 | 68 | 0.50 | 0.45 |
//! | −25 … −15 | 56 | 0.59 | 0.58 |
//! | −15 … −5  | 67 | 0.69 | 0.70 |
//! | −5 … +5   | 112 | 0.72 | 0.76 |
//!
//! Log-likelihood −200.4, against −202.5 for a two-parameter logistic fitted to this same
//! data. A formula with no free parameters beat one with two, so it stays.
//!
//! # What is still assumed
//!
//! - **The 95% ceiling.** No observation in the set is above trivial.
//! - **Mastery.** One crafter, so AA rank never varied. [`Mastery`] is carried through the
//!   API and deliberately does nothing yet; see [`Mastery::bonus`].
//! - **Whether failure destroys components.** The log prints no component-loss line, so this
//!   cannot be read out of a log at all. See [`crate::recipe::Disposition`].

/// Lowest and highest the game will let a combine be.
pub const FLOOR: f64 = 0.05;
pub const CEILING: f64 = 0.95;

/// Above this trivial the game switches to the shallower slope.
const KNEE: f64 = 68.0;

/// Chance a single combine succeeds, given the crafter's skill and the recipe's trivial.
///
/// ```
/// use grimoire_core::combine::success_chance;
/// // At trivial a crafter is well short of certain — 146 − 0.75·146 + 51.5 = 88.
/// assert!((success_chance(146, 146) - 0.88).abs() < 1e-9);
/// // Far under it, the floor holds.
/// assert_eq!(success_chance(1, 200), 0.05);
/// ```
pub fn success_chance(skill: u16, trivial: u16) -> f64 {
    let (s, t) = (skill as f64, trivial as f64);
    let raw = if t >= KNEE {
        s - 0.75 * t + 51.5
    } else {
        s - t + 66.0
    };
    (raw / 100.0).clamp(FLOOR, CEILING)
}

/// Expected attempts to land one success. Never less than 1.
#[inline]
pub fn attempts_per_success(p: f64) -> f64 {
    if p <= 0.0 {
        1.0 / FLOOR
    } else {
        1.0 / p
    }
}

/// Alternate Advancement rank in the tradeskill mastery line.
///
/// Carried everywhere a chance is computed so that the day the effect is measured, one
/// function changes and every quote in the app moves with it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mastery(pub u8);

impl Mastery {
    pub const NONE: Mastery = Mastery(0);
    pub const MAX_RANK: u8 = 3;

    /// Percentage points added to the raw chance.
    ///
    /// **Deliberately zero.** The spec has carried this term as UNVALIDATED since it was
    /// written, and the log data cannot settle it — one crafter means AA rank never varied.
    /// Returning a guess here would put an invented number inside every price the app quotes.
    /// It stays at zero until pooled combine logs at differing ranks say otherwise, which is
    /// the whole point of shipping the parser first.
    #[inline]
    pub fn bonus(self) -> f64 {
        0.0
    }

    #[inline]
    pub fn rank(self) -> u8 {
        self.0.min(Self::MAX_RANK)
    }
}

/// [`success_chance`] with the mastery term folded in.
pub fn success_chance_with(skill: u16, trivial: u16, mastery: Mastery) -> f64 {
    (success_chance(skill, trivial) + mastery.bonus() / 100.0).clamp(FLOOR, CEILING)
}

/// The con colour a player expects to see against a difficulty.
///
/// Same ladder the game uses for mob difficulty, reused for craft difficulty because an EQ
/// player reads it without being taught.
///
/// **Grey means trivial — no more skill-ups. It does not mean safe**, and on cheap recipes it
/// is a long way from it. At skill exactly equal to trivial the classic formula gives
/// `0.25·trivial + 51.5` percent, so a trivial-83 potion still fails better than one time in
/// four for a crafter who has maxed it out. That is not a bug in the model; it is what the
/// logs measured (0.72 observed in the ±5 band). Anywhere this colour is shown, show the
/// percentage beside it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Con {
    Red,
    Yellow,
    White,
    Blue,
    LightBlue,
    Green,
    Grey,
}

impl Con {
    pub fn of(skill: u16, trivial: u16) -> Con {
        if skill >= trivial {
            return Con::Grey;
        }
        match success_chance(skill, trivial) {
            p if p < 0.20 => Con::Red,
            p if p < 0.40 => Con::Yellow,
            p if p < 0.60 => Con::White,
            p if p < 0.75 => Con::Blue,
            p if p < 0.88 => Con::LightBlue,
            _ => Con::Green,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Con::Red => "red",
            Con::Yellow => "yellow",
            Con::White => "white",
            Con::Blue => "blue",
            Con::LightBlue => "light blue",
            Con::Green => "green",
            Con::Grey => "grey",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_at_both_ends() {
        assert_eq!(success_chance(0, 300), FLOOR);
        assert_eq!(success_chance(300, 20), CEILING);
    }

    #[test]
    fn the_knee_is_continuous_enough_to_not_jump() {
        // Either side of trivial 68 the two branches must not disagree wildly, or a
        // recipe would get cheaper by being harder.
        let below = success_chance(60, 67);
        let above = success_chance(60, 68);
        assert!((below - above).abs() < 0.10, "{below} vs {above}");
    }

    #[test]
    fn harder_recipes_are_never_more_likely() {
        for skill in (10..=250).step_by(10) {
            let mut last = 1.0;
            for trivial in (10..=300).step_by(5) {
                let p = success_chance(skill, trivial);
                assert!(p <= last + 1e-9, "skill {skill} trivial {trivial}");
                last = p;
            }
        }
    }

    #[test]
    fn more_skill_is_never_worse() {
        for trivial in (10..=300).step_by(10) {
            let mut last = 0.0;
            for skill in (0..=300).step_by(5) {
                let p = success_chance(skill, trivial);
                assert!(p >= last - 1e-9);
                last = p;
            }
        }
    }

    #[test]
    fn mastery_is_inert_and_says_so() {
        for rank in 0..=3 {
            assert_eq!(
                success_chance_with(100, 150, Mastery(rank)),
                success_chance(100, 150)
            );
        }
    }

    #[test]
    fn con_greys_out_at_trivial() {
        assert_eq!(Con::of(150, 146), Con::Grey);
        assert_eq!(Con::of(146, 146), Con::Grey);
        assert_eq!(Con::of(60, 200), Con::Red);
    }

    /// The counter-intuitive one, pinned so nobody "fixes" it. Grey is about skill-ups, not
    /// safety: a maxed hand on a cheap recipe still burns materials one attempt in four.
    #[test]
    fn grey_does_not_mean_safe_on_a_cheap_recipe() {
        let p = success_chance(83, 83); // Potion of Accuracy, trivial pinned by the log
        assert_eq!(Con::of(83, 83), Con::Grey);
        assert!(
            (p - 0.7225).abs() < 1e-9,
            "trivial-83 at trivial should be 72%, got {p}"
        );
        // And the expensive one really is nearly safe, which is why one number cannot do.
        assert!(success_chance(300, 300) >= CEILING - 1e-9);
    }

    #[test]
    fn attempts_never_below_one() {
        assert!(attempts_per_success(1.0) >= 1.0);
        assert!((attempts_per_success(0.5) - 2.0).abs() < 1e-9);
        assert!(attempts_per_success(0.0).is_finite());
    }
}
