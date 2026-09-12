//! Recipes, components and where a component comes from.

use crate::combine::{success_chance_with, Mastery};

/// The tradeskills EQL actually has, as they name themselves in the log line
/// `You have become better at <skill>!`.
///
/// Confirmed present in `eqlog_Reviir_*`: Alchemy, Baking, Blacksmithing, Brewing, Fishing,
/// Fletching, Jewelry Making, Pottery, Tailoring. Tinkering and Poison Making are
/// race/class-locked and never appeared in these logs — they are listed because the game has
/// them, and will read as unknown until a log proves the exact string.
///
/// Serialised as the game's own string — `"Jewelry Making"`, not `"JewelryMaking"`.
///
/// Worth the annotations: the JSON crosses into the browser, into the corpus and into saved
/// recipe files, and a name the game does not use is a name every consumer has to translate.
/// The first thing that went wrong when the UI met the engine was exactly this.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Skill {
    Alchemy,
    Baking,
    Blacksmithing,
    Brewing,
    Fishing,
    Fletching,
    #[cfg_attr(feature = "serde", serde(rename = "Jewelry Making"))]
    JewelryMaking,
    Pottery,
    Tailoring,
    Tinkering,
    #[cfg_attr(feature = "serde", serde(rename = "Poison Making"))]
    PoisonMaking,
}

impl Skill {
    /// Parse the exact string the game logs.
    pub fn from_log(s: &str) -> Option<Skill> {
        Some(match s {
            "Alchemy" => Skill::Alchemy,
            "Baking" => Skill::Baking,
            "Blacksmithing" => Skill::Blacksmithing,
            "Brewing" => Skill::Brewing,
            "Fishing" => Skill::Fishing,
            "Fletching" => Skill::Fletching,
            "Jewelry Making" => Skill::JewelryMaking,
            "Pottery" => Skill::Pottery,
            "Tailoring" => Skill::Tailoring,
            "Tinkering" => Skill::Tinkering,
            "Poison Making" => Skill::PoisonMaking,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Skill::Alchemy => "Alchemy",
            Skill::Baking => "Baking",
            Skill::Blacksmithing => "Blacksmithing",
            Skill::Brewing => "Brewing",
            Skill::Fishing => "Fishing",
            Skill::Fletching => "Fletching",
            Skill::JewelryMaking => "Jewelry Making",
            Skill::Pottery => "Pottery",
            Skill::Tailoring => "Tailoring",
            Skill::Tinkering => "Tinkering",
            Skill::PoisonMaking => "Poison Making",
        }
    }

    /// The tradeskills a combine can happen in. Fishing and Forage produce, but not by combine.
    pub const CRAFTING: [Skill; 8] = [
        Skill::Alchemy,
        Skill::Baking,
        Skill::Blacksmithing,
        Skill::Brewing,
        Skill::Fletching,
        Skill::JewelryMaking,
        Skill::Pottery,
        Skill::Tailoring,
    ];
}

/// How a component is consumed, which decides how many of it a job actually needs.
///
/// The distinction matters more than it looks: a failed combine still eats its materials, so
/// a component consumed per *attempt* scales with the crafter's odds, while one consumed per
/// *run* does not. Quoting them the same way is how a bad crafter looks cheap.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Disposition {
    /// Burned on every attempt, successful or not — the ordinary case.
    ///
    /// **Assumption, and a load-bearing one.** Classic EverQuest destroys components on a
    /// failed combine, and EQL's log prints no component-loss line either way, so this
    /// cannot be confirmed from a log. It needs an inventory diff across a known failure.
    /// If it turns out EQL returns materials on failure, this variant becomes [`PerRun`] and
    /// every quote in the app drops.
    PerAttempt,
    /// Consumed once per successful unit produced, regardless of failed attempts.
    PerRun,
    /// Needed once for the whole job — a container, a mould, a tool.
    Once,
}

/// Where a component comes from, which decides who is expected to find it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Source {
    /// A merchant sells it. Anyone can buy it, so the crafter just buys it.
    Vendor,
    /// It drops. Someone has to go and kill for it.
    Drop,
    /// Foraged.
    Forage,
    /// A quest reward.
    Quest,
    /// Made by another recipe.
    Crafted,
    /// Not known yet. Treated as un-buyable, because promising to source something the app
    /// cannot price is worse than asking.
    Unknown,
}

impl Source {
    /// Can the crafter simply buy this, or does someone have to go and get it?
    ///
    /// This is the whole "you supply the parts" split: the buyer only ever sends the things
    /// no merchant sells.
    #[inline]
    pub fn purchasable(self) -> bool {
        matches!(self, Source::Vendor)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Component {
    /// Item id from the client's own tables — the same id an inventory dump prints.
    pub item: u32,
    pub name: String,
    pub qty: u32,
    pub disposition: Disposition,
    pub source: Source,
    /// Vendor price in copper, when a vendor sells it.
    pub unit_price: crate::Coin,
}

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Recipe {
    pub id: u32,
    pub product: u32,
    pub product_name: String,
    pub skill: Skill,
    /// Skill at which the combine stops granting skill-ups and becomes near-certain.
    pub trivial: u16,
    /// How many of the product one successful combine makes. Never zero.
    pub yields: u32,
    /// Some combines cannot fail — assembly recipes, mostly.
    pub no_fail: bool,
    /// What the thing does, in the wiki's words. `None` for anything that just is what it is.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub effect: Option<String>,
    pub components: Vec<Component>,
}

impl Recipe {
    /// Chance one attempt at this recipe succeeds for a crafter of the given skill.
    pub fn chance(&self, skill: u16, mastery: Mastery) -> f64 {
        if self.no_fail {
            1.0
        } else {
            success_chance_with(skill, self.trivial, mastery)
        }
    }

    /// Components the buyer would have to find, because no merchant sells them.
    pub fn unbuyable(&self) -> impl Iterator<Item = &Component> {
        self.components.iter().filter(|c| !c.source.purchasable())
    }

    /// Guard against a corpus row that would divide by zero downstream.
    pub fn yields_or_one(&self) -> u32 {
        self.yields.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_logged_skill_string_parses() {
        // Exactly the tradeskill strings seen in eqlog_Reviir_*.
        for s in [
            "Alchemy",
            "Baking",
            "Blacksmithing",
            "Brewing",
            "Fishing",
            "Fletching",
            "Jewelry Making",
            "Pottery",
            "Tailoring",
        ] {
            let parsed = Skill::from_log(s).unwrap_or_else(|| panic!("{s} did not parse"));
            assert_eq!(parsed.as_str(), s, "round trip failed for {s}");
        }
    }

    #[test]
    fn non_tradeskills_do_not_parse() {
        // The same log line is used for combat skills; those must not become recipes.
        for s in ["Double Attack", "Meditate", "Orcish", "Forage"] {
            assert!(Skill::from_log(s).is_none(), "{s} parsed as a tradeskill");
        }
    }

    /// The serialised name must be the game's, because it is what the browser, the corpus
    /// and every saved recipe file carry.
    #[cfg(feature = "serde")]
    #[test]
    fn skills_serialise_as_the_game_names_them() {
        for s in Skill::CRAFTING {
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(
                json,
                format!("\"{}\"", s.as_str()),
                "{s:?} serialised wrong"
            );
            let back: Skill = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn only_vendor_goods_are_purchasable() {
        assert!(Source::Vendor.purchasable());
        for s in [
            Source::Drop,
            Source::Forage,
            Source::Quest,
            Source::Crafted,
            Source::Unknown,
        ] {
            assert!(!s.purchasable(), "{s:?} should not be purchasable");
        }
    }
}
