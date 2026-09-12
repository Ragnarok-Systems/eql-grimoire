//! Norrathian money.
//!
//! Everything is held in copper as an integer, because the quote engine multiplies
//! prices by expected-attempt counts and float money drifts. Rendering drops empty
//! denominations the way the game does: `4p 2g 5c`, never `4p 0g 0s 5c`.

use core::fmt;

/// 1 platinum = 10 gold = 100 silver = 1000 copper.
pub const COPPER_PER_SILVER: i64 = 10;
pub const COPPER_PER_GOLD: i64 = 100;
pub const COPPER_PER_PLAT: i64 = 1000;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Coin(pub i64);

impl Coin {
    pub const ZERO: Coin = Coin(0);

    #[inline]
    pub const fn copper(c: i64) -> Coin {
        Coin(c)
    }
    #[inline]
    pub const fn plat(p: i64) -> Coin {
        Coin(p * COPPER_PER_PLAT)
    }

    /// Round a fractional copper amount to the nearest whole copper.
    ///
    /// The quote engine works in expected values — 3.7 attempts at 12c each — so it
    /// lands here exactly once, at the end, rather than rounding at every step.
    #[inline]
    pub fn from_f64(c: f64) -> Coin {
        if !c.is_finite() {
            return Coin::ZERO;
        }
        Coin(c.round() as i64)
    }

    #[inline]
    pub fn as_f64(self) -> f64 {
        self.0 as f64
    }
    #[inline]
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Split into (plat, gold, silver, copper). Negative amounts split by magnitude.
    pub fn split(self) -> (i64, i64, i64, i64) {
        let n = self.0.abs();
        (
            n / COPPER_PER_PLAT,
            (n % COPPER_PER_PLAT) / COPPER_PER_GOLD,
            (n % COPPER_PER_GOLD) / COPPER_PER_SILVER,
            n % COPPER_PER_SILVER,
        )
    }

    /// Scale by a ratio, rounding once. Used for markups and courtesy discounts.
    #[inline]
    pub fn scale(self, f: f64) -> Coin {
        Coin::from_f64(self.as_f64() * f)
    }
}

impl core::ops::Add for Coin {
    type Output = Coin;
    #[inline]
    fn add(self, o: Coin) -> Coin {
        Coin(self.0 + o.0)
    }
}
impl core::ops::Sub for Coin {
    type Output = Coin;
    #[inline]
    fn sub(self, o: Coin) -> Coin {
        Coin(self.0 - o.0)
    }
}
impl core::iter::Sum for Coin {
    fn sum<I: Iterator<Item = Coin>>(it: I) -> Coin {
        Coin(it.map(|c| c.0).sum())
    }
}

impl fmt::Display for Coin {
    /// `0` renders as `0c`, not as the empty string — an empty price reads as a bug.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            return f.write_str("0c");
        }
        if self.0 < 0 {
            f.write_str("−")?;
        }
        let (p, g, s, c) = self.split();
        let mut first = true;
        for (n, suffix) in [(p, 'p'), (g, 'g'), (s, 's'), (c, 'c')] {
            if n == 0 {
                continue;
            }
            if !first {
                f.write_str(" ")?;
            }
            write!(f, "{n}{suffix}")?;
            first = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_empty_denominations() {
        assert_eq!(Coin(4005).to_string(), "4p 5c");
        assert_eq!(Coin(1234).to_string(), "1p 2g 3s 4c");
        assert_eq!(Coin(1000).to_string(), "1p");
        assert_eq!(Coin(7).to_string(), "7c");
        assert_eq!(Coin(0).to_string(), "0c");
    }

    #[test]
    fn negatives_render_with_a_minus_and_no_double_sign() {
        assert_eq!(Coin(-1234).to_string(), "−1p 2g 3s 4c");
    }

    #[test]
    fn scaling_rounds_once() {
        // 15% courtesy off 216p, the worked example from the mockup.
        assert_eq!(Coin::plat(216).scale(0.85), Coin::plat(183) + Coin(600));
    }
}
