//! Reputation, on the faction ladder.
//!
//! Deliberately coarse. An EQ player already reads "Warmly" without being taught, and a word
//! collapses 4.6 and 4.8 into the same answer — which is the point. There are no moderators
//! in this app; regard is the only social pressure, and a 0.2 difference is not evidence.
//!
//! Volume is not in here on purpose. Ranking hands by orders completed goes lopsided fast:
//! whoever starts first stays first, and newcomers never get a first order.

/// Where a hand stands with you, worst to best.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Regard {
    Dubiously,
    Apprehensively,
    Indifferently,
    Amiably,
    Kindly,
    Warmly,
    Ally,
}

/// Floors, best first. A score at or above a floor takes that rung.
const LADDER: [(f64, Regard); 7] = [
    (4.85, Regard::Ally),
    (4.55, Regard::Warmly),
    (4.15, Regard::Kindly),
    (3.70, Regard::Amiably),
    (2.60, Regard::Indifferently),
    (1.60, Regard::Apprehensively),
    (0.00, Regard::Dubiously),
];

impl Regard {
    /// A mean score out of 5 becomes a rung.
    pub fn of(score: f64) -> Regard {
        let s = if score.is_nan() { 0.0 } else { score };
        LADDER
            .iter()
            .find(|(floor, _)| s >= *floor)
            .map(|(_, r)| *r)
            .unwrap_or(Regard::Dubiously)
    }

    /// Where someone with no history starts.
    ///
    /// Indifferent, not Dubious: a new crafter has not done anything wrong, and starting
    /// people at the bottom is how a reputation system becomes a closed shop.
    pub const UNPROVEN: Regard = Regard::Indifferently;

    pub fn as_str(self) -> &'static str {
        match self {
            Regard::Ally => "Ally",
            Regard::Warmly => "Warmly",
            Regard::Kindly => "Kindly",
            Regard::Amiably => "Amiably",
            Regard::Indifferently => "Indifferently",
            Regard::Apprehensively => "Apprehensively",
            Regard::Dubiously => "Dubiously",
        }
    }

    /// Lowest rung that may still commission you, by default.
    pub const DEFAULT_FLOOR: Regard = Regard::Indifferently;

    /// Does this rung clear a workshop's stated floor?
    #[inline]
    pub fn clears(self, floor: Regard) -> bool {
        self >= floor
    }
}

/// A hand's standing: the rung, and how much history is behind it.
///
/// The count is carried so the UI can say "on 3 orders" rather than implying a rung earned
/// over fifty. It is *not* an input to the rung itself.
#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Standing {
    pub score: f64,
    pub ratings: u32,
}

impl Standing {
    pub const UNPROVEN: Standing = Standing {
        score: 3.0,
        ratings: 0,
    };

    pub fn regard(&self) -> Regard {
        if self.ratings == 0 {
            Regard::UNPROVEN
        } else {
            Regard::of(self.score)
        }
    }

    /// Fold in one new rating out of 5.
    pub fn rated(self, stars: f64) -> Standing {
        let stars = stars.clamp(0.0, 5.0);
        let n = self.ratings as f64;
        Standing {
            score: (self.score * n + stars) / (n + 1.0),
            ratings: self.ratings + 1,
        }
    }
}

impl Default for Standing {
    fn default() -> Self {
        Standing::UNPROVEN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_is_monotone() {
        let mut last = Regard::Dubiously;
        let mut s = 0.0;
        while s <= 5.0 {
            let r = Regard::of(s);
            assert!(r >= last, "went backwards at {s}");
            last = r;
            s += 0.01;
        }
        assert_eq!(Regard::of(5.0), Regard::Ally);
        assert_eq!(Regard::of(0.0), Regard::Dubiously);
    }

    #[test]
    fn nonsense_scores_do_not_panic() {
        assert_eq!(Regard::of(f64::NAN), Regard::Dubiously);
        assert_eq!(Regard::of(-3.0), Regard::Dubiously);
        assert_eq!(Regard::of(99.0), Regard::Ally);
    }

    #[test]
    fn a_new_hand_starts_indifferent_not_dubious() {
        assert_eq!(Standing::UNPROVEN.regard(), Regard::Indifferently);
        assert!(Standing::UNPROVEN.regard().clears(Regard::DEFAULT_FLOOR));
    }

    #[test]
    fn one_bad_rating_does_not_bury_a_good_hand() {
        let mut s = Standing::UNPROVEN;
        for _ in 0..20 {
            s = s.rated(5.0);
        }
        let before = s.regard();
        let after = s.rated(1.0).regard();
        assert_eq!(before, Regard::Ally);
        assert!(
            after >= Regard::Warmly,
            "one 1-star dropped them to {after:?}"
        );
    }

    #[test]
    fn the_unproven_seed_does_not_drag_a_real_record() {
        // The 3.0 seed exists so an unrated hand has a sane score to show; it must not
        // count as a rating, or everyone's first order would be scored against a phantom.
        let s = Standing::UNPROVEN.rated(5.0);
        assert_eq!(s.ratings, 1);
        assert!((s.score - 5.0).abs() < 1e-9, "seed leaked into the mean");
    }

    #[test]
    fn a_rung_says_nothing_about_volume() {
        // Two hands at the same rung, wildly different histories. The UI needs the count to
        // say so; the rung itself must not encode it, or volume becomes rank.
        let quiet = Standing {
            score: 4.6,
            ratings: 2,
        };
        let busy = Standing {
            score: 4.6,
            ratings: 400,
        };
        assert_eq!(quiet.regard(), busy.regard());
    }
}
