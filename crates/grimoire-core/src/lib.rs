//! EQL Grimoire — the domain, and the maths, in one place.
//!
//! Everything the app has to agree with itself about lives here: whether a combine works,
//! what a job costs, where a hand stands, and whose turn it is. The Worker, the browser and
//! the corpus builder all call this crate, so a price can only be computed one way.
//!
//! Nothing in here does I/O, and nothing knows about Discord, HTTP or the wiki.
//!
//! ```
//! use grimoire_core::{combine::Con, quote::{quote, Hand, Supply}, regard::Regard};
//! # use grimoire_core::recipe::*;
//! # use grimoire_core::Coin;
//! # let recipe = Recipe { id: 1, product: 2, product_name: "Iron Ration".into(),
//! #   skill: Skill::Baking, trivial: 21, yields: 1, no_fail: false, effect: None,
//! #   components: vec![Component { item: 3, name: "Flour".into(), qty: 1,
//! #     disposition: Disposition::PerAttempt, source: Source::Vendor,
//! #     unit_price: Coin::copper(120) }] };
//! let hand = Hand { skill: 40, ..Default::default() };
//! let q = quote(&recipe, 10, &hand, Supply::Crafter);
//! assert_eq!(Con::of(40, recipe.trivial), Con::Grey);   // trivial to him
//! assert!(q.total > grimoire_core::Coin::ZERO);
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod coin;
pub mod combine;
pub mod derive;
pub mod order;
pub mod quote;
pub mod recipe;
pub mod regard;

pub use coin::Coin;
pub use combine::{Con, Mastery};
pub use derive::{
    Basis, Competing, Derivation, Rate, Refusal, ScoreKey, SettledBasis, Subject, Term, TermNote,
    Unit, Verdict,
};
pub use order::{Party, Phase};
pub use quote::{Quote, Supply};
pub use recipe::{Recipe, Skill};
pub use regard::{Regard, Standing};
