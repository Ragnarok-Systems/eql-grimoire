//! What the job costs.
//!
//! One implementation, called by the Worker, the browser and the corpus builder alike. If the
//! quote a buyer sees and the quote a crafter sees are computed by two different pieces of
//! code, they will disagree, and the first argument about it ends the app's usefulness.
//!
//! The shape of a quote:
//!
//! ```text
//!   materials   what the crafter has to buy, scaled by how often he fails
//! + labour      time on the job, priced by difficulty
//! + risk        a hedge against a bad run, zero on a no-fail recipe
//! = subtotal
//! − courtesy    guildmate discount, taken per crafter, not on the running total
//! + gratuity    the buyer's choice, on top
//! ```

use crate::combine::{attempts_per_success, Mastery};
use crate::recipe::{Component, Disposition, Recipe};
use crate::Coin;

/// What the crafter adds to what the merchant charged him.
///
/// **1.0 — he passes materials through at cost.** It was 2.5 while the prices in the mockup
/// were invented; with real vendor prices out of the wiki, a 2.5× markup on top of a 2.0×
/// sourcing surcharge priced twenty Potions of Accuracy at 1,926p against a true reagent cost
/// of 385p. That is not a broker, it is a scalper, and nobody would use it twice.
///
/// The crafter is paid for his work and his risk, on their own lines, where the buyer can see
/// them. Burying a fee in the material cost hides the one number the buyer can check.
pub const MATERIAL_MARKUP: f64 = 1.0;

/// Extra on materials when the crafter does the shopping too.
///
/// Modest on purpose: it buys his walk to the merchant, not the goods again.
pub const SUPPLY_SURCHARGE: f64 = 1.15;

/// Copper of risk premium per expected wasted attempt.
///
/// Deliberately small, because the *expected* cost of failure is already in the material
/// line — a hand who fails half the time buys twice the reagents, and [`quote`] scales the
/// units by his odds. This is only the hedge against a bad run being worse than the average
/// one. Charging failure again here would bill the buyer twice for it.
pub const RISK_PER_WASTED_ATTEMPT: f64 = 5.0;

/// Labour per attempt, by how hard the recipe is.
///
/// Silver rather than copper. In EverQuest the materials are the cost and the crafter's fee
/// is modest by comparison — that is a real feature of the economy, not an error — but at the
/// old copper rates a job's whole labour bill came to less than one reagent, which read as a
/// rounding artefact rather than a price.
pub fn labour_rate(trivial: u16) -> Coin {
    Coin::copper(match trivial {
        0..=99 => 50,
        100..=149 => 100,
        150..=199 => 200,
        _ => 350,
    })
}

/// Who is finding the parts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Supply {
    /// The crafter buys everything he can and charges for it.
    Crafter,
    /// The buyer posts the un-buyable parts; the crafter still buys vendor goods.
    Buyer,
}

/// The crafter, as far as pricing is concerned.
#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Hand {
    pub skill: u16,
    pub mastery: Mastery,
    /// Fraction off for a guildmate, 0.0 … 1.0.
    pub courtesy: f64,
    /// Items he already owns and will not charge for — containers, moulds.
    pub owns_tools: bool,
}

impl Default for Hand {
    fn default() -> Self {
        Hand {
            skill: 0,
            mastery: Mastery::NONE,
            courtesy: 0.0,
            owns_tools: true,
        }
    }
}

/// One component, priced.
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MaterialLine {
    pub item: u32,
    pub name: String,
    /// Expected units consumed across the whole job, rounded up.
    pub units: u32,
    pub disposition: Disposition,
    pub cost: Coin,
    /// True when the buyer is posting this one rather than paying for it.
    pub buyer_supplies: bool,
    /// True when the crafter already has it and is not charging.
    pub crafter_owns: bool,
}

/// What the buyer has to go and find before the crafter can start.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ToPost {
    pub item: u32,
    pub name: String,
    pub units: u32,
    pub source: crate::recipe::Source,
}

#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Quote {
    /// Chance of one attempt landing.
    pub chance: f64,
    /// Successful combines needed to reach the requested quantity.
    pub runs: u32,
    /// Expected attempts including failures.
    pub attempts: f64,
    pub materials: Vec<MaterialLine>,
    pub to_post: Vec<ToPost>,
    pub material_cost: Coin,
    pub labour: Coin,
    pub risk: Coin,
    /// materials + labour + risk, before courtesy and gratuity.
    pub subtotal: Coin,
    /// Amount taken off for guild courtesy — a positive number that gets subtracted.
    pub courtesy: Coin,
    /// subtotal − courtesy.
    pub total: Coin,
}

impl Quote {
    /// Total with a gratuity applied. Kept out of [`Quote::total`] because a tip is the
    /// buyer's decision, not part of the price the crafter asked for.
    pub fn with_gratuity(&self, fraction: f64) -> Coin {
        self.total + self.total.scale(fraction.max(0.0))
    }
}

/// Price a job.
///
/// `quantity` is how many of the product the buyer wants, not how many combines.
pub fn quote(recipe: &Recipe, quantity: u32, hand: &Hand, supply: Supply) -> Quote {
    let quantity = quantity.max(1);
    let chance = recipe.chance(hand.skill, hand.mastery);
    let runs = quantity.div_ceil(recipe.yields_or_one());
    let attempts = runs as f64 * attempts_per_success(chance);

    let mut materials = Vec::with_capacity(recipe.components.len());
    let mut to_post = Vec::new();
    let mut material_cost = Coin::ZERO;

    for c in &recipe.components {
        let units = expected_units(c, runs, attempts);
        let crafter_owns = c.disposition == Disposition::Once && hand.owns_tools;
        let buyer_supplies = supply == Supply::Buyer && !crafter_owns && !c.source.purchasable();

        let cost = if crafter_owns || buyer_supplies {
            Coin::ZERO
        } else {
            let surcharge = if supply == Supply::Crafter {
                SUPPLY_SURCHARGE
            } else {
                1.0
            };
            c.unit_price
                .scale(units as f64 * MATERIAL_MARKUP * surcharge)
        };

        if buyer_supplies {
            to_post.push(ToPost {
                item: c.item,
                name: c.name.clone(),
                units,
                source: c.source,
            });
        }
        material_cost = material_cost + cost;
        materials.push(MaterialLine {
            item: c.item,
            name: c.name.clone(),
            units,
            disposition: c.disposition,
            cost,
            buyer_supplies,
            crafter_owns,
        });
    }

    let labour = labour_rate(recipe.trivial).scale(attempts);
    let risk = if recipe.no_fail {
        Coin::ZERO
    } else {
        let wasted = attempts - runs as f64;
        Coin::from_f64(wasted.max(0.0) * RISK_PER_WASTED_ATTEMPT)
    };

    let subtotal = material_cost + labour + risk;
    let courtesy = subtotal.scale(hand.courtesy.clamp(0.0, 1.0));

    Quote {
        chance,
        runs,
        attempts,
        materials,
        to_post,
        material_cost,
        labour,
        risk,
        subtotal,
        courtesy,
        total: subtotal - courtesy,
    }
}

/// How many of a component the job eats.
///
/// Rounded up, because half a bar of ore is not a thing you can post someone.
fn expected_units(c: &Component, runs: u32, attempts: f64) -> u32 {
    let n = match c.disposition {
        Disposition::Once => c.qty as f64,
        Disposition::PerRun => c.qty as f64 * runs as f64,
        Disposition::PerAttempt => c.qty as f64 * attempts,
    };
    n.ceil().max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{Skill, Source};

    fn comp(item: u32, name: &str, qty: u32, d: Disposition, s: Source, price: i64) -> Component {
        Component {
            item,
            name: name.into(),
            qty,
            disposition: d,
            source: s,
            unit_price: Coin::copper(price),
        }
    }

    /// A jewelry recipe with a trivial the log pinned exactly.
    fn gold_malachite_bracelet() -> Recipe {
        Recipe {
            id: 1,
            product: 9001,
            product_name: "Gold Malachite Bracelet".into(),
            skill: Skill::JewelryMaking,
            trivial: 146,
            yields: 1,
            no_fail: false,
            effect: None,
            components: vec![
                comp(
                    101,
                    "Gold Bar",
                    1,
                    Disposition::PerAttempt,
                    Source::Vendor,
                    800,
                ),
                comp(
                    102,
                    "Malachite",
                    1,
                    Disposition::PerAttempt,
                    Source::Drop,
                    0,
                ),
                comp(
                    103,
                    "Jeweler's Kit",
                    1,
                    Disposition::Once,
                    Source::Vendor,
                    5000,
                ),
            ],
        }
    }

    #[test]
    fn a_worse_crafter_burns_more_and_costs_more() {
        let r = gold_malachite_bracelet();
        let good = Hand {
            skill: 146,
            ..Default::default()
        };
        let poor = Hand {
            skill: 90,
            ..Default::default()
        };
        let a = quote(&r, 10, &good, Supply::Crafter);
        let b = quote(&r, 10, &poor, Supply::Crafter);

        assert!(
            b.attempts > a.attempts,
            "poor hand should attempt more often"
        );
        assert!(b.material_cost > a.material_cost, "and burn more materials");
        assert!(b.total > a.total);
    }

    #[test]
    fn no_fail_recipes_carry_no_risk_and_exactly_one_attempt_per_run() {
        let mut r = gold_malachite_bracelet();
        r.no_fail = true;
        let q = quote(
            &r,
            4,
            &Hand {
                skill: 1,
                ..Default::default()
            },
            Supply::Crafter,
        );
        assert_eq!(q.risk, Coin::ZERO);
        assert!((q.attempts - 4.0).abs() < 1e-9);
        assert!((q.chance - 1.0).abs() < 1e-9);
    }

    #[test]
    fn buyer_supplied_parts_are_the_unbuyable_ones_only() {
        let r = gold_malachite_bracelet();
        let hand = Hand {
            skill: 146,
            ..Default::default()
        };
        let q = quote(&r, 5, &hand, Supply::Buyer);

        // Malachite drops, so the buyer posts it. The gold bar is on a merchant, so it is
        // not the buyer's problem even though he is "supplying materials".
        let posted: Vec<&str> = q.to_post.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(posted, vec!["Malachite"]);

        let gold = q.materials.iter().find(|m| m.name == "Gold Bar").unwrap();
        assert!(!gold.buyer_supplies);
        assert!(gold.cost > Coin::ZERO);
    }

    #[test]
    fn tools_the_crafter_already_owns_are_not_charged() {
        let r = gold_malachite_bracelet();
        let has = Hand {
            skill: 146,
            owns_tools: true,
            ..Default::default()
        };
        let hasnt = Hand {
            skill: 146,
            owns_tools: false,
            ..Default::default()
        };
        let kit = |q: &Quote| {
            q.materials
                .iter()
                .find(|m| m.name == "Jeweler's Kit")
                .unwrap()
                .cost
        };
        assert_eq!(kit(&quote(&r, 3, &has, Supply::Crafter)), Coin::ZERO);
        assert!(kit(&quote(&r, 3, &hasnt, Supply::Crafter)) > Coin::ZERO);
    }

    #[test]
    fn a_one_off_component_does_not_scale_with_quantity() {
        let r = gold_malachite_bracelet();
        let hand = Hand {
            skill: 146,
            owns_tools: false,
            ..Default::default()
        };
        let units = |qty| {
            quote(&r, qty, &hand, Supply::Crafter)
                .materials
                .iter()
                .find(|m| m.name == "Jeweler's Kit")
                .unwrap()
                .units
        };
        assert_eq!(units(1), 1);
        assert_eq!(units(50), 1);
    }

    #[test]
    fn courtesy_comes_off_this_crafters_subtotal_not_the_running_total() {
        let r = gold_malachite_bracelet();
        let full = Hand {
            skill: 146,
            ..Default::default()
        };
        let mate = Hand {
            skill: 146,
            courtesy: 0.15,
            ..Default::default()
        };
        let a = quote(&r, 10, &full, Supply::Crafter);
        let b = quote(&r, 10, &mate, Supply::Crafter);

        assert_eq!(a.subtotal, b.subtotal, "courtesy must not change the gross");
        assert_eq!(b.courtesy, b.subtotal.scale(0.15));
        assert_eq!(b.total, b.subtotal - b.courtesy);
        assert!(b.total < a.total);
    }

    /// The anchor. A buyer can walk to the same merchant and check this number, so it has to
    /// be the merchant's price for the units actually burned — not a markup dressed up as one.
    #[test]
    fn materials_cost_what_the_merchant_charges() {
        let r = gold_malachite_bracelet();
        let hand = Hand {
            skill: 146,
            ..Default::default()
        };
        let q = quote(&r, 10, &hand, Supply::Buyer); // buyer posts the drop, so gold bar only

        let gold = q.materials.iter().find(|m| m.name == "Gold Bar").unwrap();
        assert_eq!(
            gold.cost,
            Coin::copper(800).scale(gold.units as f64),
            "a gold bar costs what a gold bar costs"
        );
    }

    #[test]
    fn the_shopping_surcharge_is_a_walk_not_a_second_purchase() {
        let r = gold_malachite_bracelet();
        let hand = Hand {
            skill: 146,
            ..Default::default()
        };
        let his = quote(&r, 10, &hand, Supply::Crafter).material_cost;
        let mine = quote(&r, 10, &hand, Supply::Buyer).material_cost;
        assert!(his > mine, "sourcing everything should cost the buyer more");
        assert!(
            his.as_f64() < mine.as_f64() * 2.0,
            "sourcing surcharge doubled the bill: {mine} -> {his}"
        );
    }

    /// Failure is paid for once, in the material line, because a hand who fails buys more
    /// reagents. The risk premium must stay a rounding-scale hedge, not a second charge.
    #[test]
    fn failure_is_not_billed_twice() {
        let r = gold_malachite_bracelet();
        let poor = Hand {
            skill: 70,
            ..Default::default()
        };
        let q = quote(&r, 10, &poor, Supply::Crafter);
        assert!(q.attempts > 15.0, "test needs a hand who really does fail");
        assert!(
            q.risk.as_f64() < q.material_cost.as_f64() * 0.05,
            "risk {} is a second failure charge on top of materials {}",
            q.risk,
            q.material_cost
        );
    }

    #[test]
    fn gratuity_sits_on_top_and_never_inside() {
        let r = gold_malachite_bracelet();
        let q = quote(
            &r,
            5,
            &Hand {
                skill: 146,
                ..Default::default()
            },
            Supply::Crafter,
        );
        assert_eq!(q.with_gratuity(0.0), q.total);
        assert!(q.with_gratuity(0.20) > q.total);
        // A negative tip is not a discount.
        assert_eq!(q.with_gratuity(-1.0), q.total);
    }

    #[test]
    fn yield_reduces_the_number_of_combines() {
        let mut r = gold_malachite_bracelet();
        r.yields = 4;
        let q = quote(
            &r,
            10,
            &Hand {
                skill: 200,
                ..Default::default()
            },
            Supply::Crafter,
        );
        assert_eq!(q.runs, 3, "10 wanted at 4 a combine is 3 combines");
    }

    #[test]
    fn zero_quantity_is_treated_as_one_rather_than_dividing_by_nothing() {
        let r = gold_malachite_bracelet();
        let q = quote(
            &r,
            0,
            &Hand {
                skill: 146,
                ..Default::default()
            },
            Supply::Crafter,
        );
        assert_eq!(q.runs, 1);
        assert!(q.total > Coin::ZERO);
    }

    #[test]
    fn a_broken_corpus_row_with_zero_yield_does_not_hang() {
        let mut r = gold_malachite_bracelet();
        r.yields = 0;
        let q = quote(
            &r,
            5,
            &Hand {
                skill: 146,
                ..Default::default()
            },
            Supply::Crafter,
        );
        assert_eq!(q.runs, 5);
    }
}
