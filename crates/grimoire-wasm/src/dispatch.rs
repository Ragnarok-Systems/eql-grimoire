//! The whole browser API: one function, JSON in, JSON out.
//!
//! Kept separate from the FFI shell so it can be tested on the host. Everything below runs
//! natively under `cargo test`; the only untested code in this crate is the twenty lines of
//! pointer arithmetic in `lib.rs`, which is the point of splitting them.
//!
//! A deliberately narrow surface. Every op is a pure function of its arguments, which is why
//! the whole app can run on a CDN: there is nothing here a server needs to do.

use grimoire_core::combine::{success_chance_with, Con, Mastery};
use grimoire_core::order::{courtesy_for, may_commission, Event, Party, Phase, Terms};
use grimoire_core::quote::{quote, Hand, Supply};
use grimoire_core::recipe::Recipe;
use grimoire_core::regard::Standing;
use grimoire_corpus::{InMemory, Reader};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    /// Odds and con colour for one recipe against one hand.
    Chance {
        skill: u16,
        trivial: u16,
        #[serde(default)]
        mastery: u8,
    },
    /// Price a job.
    Quote {
        recipe: Recipe,
        #[serde(default = "one")]
        qty: u32,
        hand: Hand,
        #[serde(default)]
        buyer_supplies: bool,
    },
    /// Price the same job for several hands at once — what the "who can make it" list needs.
    Hands {
        recipe: Recipe,
        #[serde(default = "one")]
        qty: u32,
        hands: Vec<NamedHand>,
        #[serde(default)]
        buyer_supplies: bool,
    },
    /// Where a score sits on the faction ladder.
    Regard { score: f64, ratings: u32 },
    /// Advance an order.
    Order {
        phase: Phase,
        actor: Party,
        event: Event,
    },
    /// May this buyer commission this crafter at all?
    MayCommission {
        terms: Terms,
        same_server: bool,
        same_guild: bool,
        buyer: grimoire_core::Regard,
    },
    /// Read a crafting log. The text never leaves the browser; this runs on it in place.
    Harvest { log: String },
    /// Read an `/outputfile inventory` dump.
    Inventory { dump: String },
    /// Every recipe in a corpus, as a light index for search.
    Catalogue { corpus: Vec<u8> },
    /// One recipe out of a corpus by key.
    Recipe { corpus: Vec<u8>, key: String },
}

fn one() -> u32 {
    1
}

#[derive(Deserialize, Serialize, Clone)]
pub struct NamedHand {
    pub name: String,
    #[serde(flatten)]
    pub hand: Hand,
}

#[derive(Serialize)]
struct ChanceOut {
    chance: f64,
    con: &'static str,
    /// Grey means no skill-up, *not* safe. Carried so a caller cannot show the colour alone.
    trivial_to_him: bool,
}

#[derive(Serialize)]
struct HandQuote {
    name: String,
    chance: f64,
    con: &'static str,
    attempts: f64,
    total: grimoire_core::Coin,
}

#[derive(Serialize)]
struct CatalogueEntry {
    key: String,
    id: u32,
    name: String,
    skill: &'static str,
    trivial: u16,
    parts: usize,
    /// True when every component has a known vendor price, so the job can be fully quoted.
    priced: bool,
}

/// Handle one request. Never panics on bad input — a malformed request comes back as
/// `{"error": "..."}`, because a panic in wasm poisons the module for the rest of the session.
pub fn dispatch(input: &str) -> String {
    match handle(input) {
        Ok(s) => s,
        Err(e) => serde_json::json!({ "error": e }).to_string(),
    }
}

fn handle(input: &str) -> Result<String, String> {
    let req: Request = serde_json::from_str(input).map_err(|e| e.to_string())?;
    let out = match req {
        Request::Chance {
            skill,
            trivial,
            mastery,
        } => serde_json::to_string(&ChanceOut {
            chance: success_chance_with(skill, trivial, Mastery(mastery)),
            con: Con::of(skill, trivial).as_str(),
            trivial_to_him: skill >= trivial,
        }),

        Request::Quote {
            recipe,
            qty,
            hand,
            buyer_supplies,
        } => serde_json::to_string(&quote(&recipe, qty, &hand, supply(buyer_supplies))),

        Request::Hands {
            recipe,
            qty,
            hands,
            buyer_supplies,
        } => {
            let mut out: Vec<HandQuote> = hands
                .into_iter()
                .map(|h| {
                    let q = quote(&recipe, qty, &h.hand, supply(buyer_supplies));
                    HandQuote {
                        name: h.name,
                        chance: q.chance,
                        con: Con::of(h.hand.skill, recipe.trivial).as_str(),
                        attempts: q.attempts,
                        total: q.total,
                    }
                })
                .collect();
            // Cheapest first, and ties broken by name so the list does not shuffle between
            // renders — a list that reorders itself under the cursor is unusable.
            out.sort_by(|a, b| a.total.cmp(&b.total).then_with(|| a.name.cmp(&b.name)));
            serde_json::to_string(&out)
        }

        Request::Regard { score, ratings } => {
            let s = Standing { score, ratings };
            serde_json::to_string(&serde_json::json!({
                "rung": s.regard().as_str(),
                "ratings": s.ratings,
            }))
        }

        Request::Order {
            phase,
            actor,
            event,
        } => match phase.apply(actor, event) {
            Ok(next) => serde_json::to_string(&serde_json::json!({
                "phase": next,
                "seal": next.seal(),
                "waiting_on": next.waiting_on(),
                "closed": next.is_closed(),
            })),
            Err(refused) => serde_json::to_string(&serde_json::json!({ "refused": refused })),
        },

        Request::MayCommission {
            terms,
            same_server,
            same_guild,
            buyer,
        } => {
            let barred = may_commission(&terms, same_server, same_guild, buyer).err();
            serde_json::to_string(&serde_json::json!({
                "allowed": barred.is_none(),
                "barred": barred,
                "courtesy": courtesy_for(&terms, same_guild),
            }))
        }

        Request::Harvest { log } => {
            let h = grimoire_parse::harvest(&log);
            let pinned: Vec<_> = h
                .items
                .iter()
                .filter_map(|(k, v)| v.pinned_trivial().map(|t| (k.clone(), t)))
                .collect();
            serde_json::to_string(&serde_json::json!({
                "attempts": h.attempts.len(),
                "successes": h.attempts.iter().filter(|a| a.success).count(),
                "items": h.items.len(),
                "skills": h.skills.iter().map(|(k, v)| (k.as_str(), v)).collect::<Vec<_>>(),
                "pinned_trivials": pinned,
                // The only thing that would ever be uploaded, so the caller can show it.
                "buckets": h.calibration_buckets(),
            }))
        }

        Request::Inventory { dump } => {
            let inv = grimoire_parse::inventory::parse(&dump);
            serde_json::to_string(&serde_json::json!({
                "held": inv.held,
                "collected": inv.collected,
                "ids": inv.ids(),
                "unreadable": inv.unreadable,
            }))
        }

        Request::Catalogue { corpus } => {
            let r = Reader::open(InMemory(corpus)).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for key in r.prefix("recipe/").map_err(|e| e.to_string())? {
                let rec: Recipe = r.get(&key).map_err(|e| e.to_string())?;
                out.push(CatalogueEntry {
                    key,
                    id: rec.product,
                    name: rec.product_name.clone(),
                    skill: rec.skill.as_str(),
                    trivial: rec.trivial,
                    parts: rec.components.len(),
                    priced: rec.components.iter().all(|c| c.source.purchasable()),
                });
            }
            serde_json::to_string(&out)
        }

        Request::Recipe { corpus, key } => {
            let r = Reader::open(InMemory(corpus)).map_err(|e| e.to_string())?;
            let rec: Recipe = r.get(&key).map_err(|e| e.to_string())?;
            serde_json::to_string(&rec)
        }
    };
    out.map_err(|e| e.to_string())
}

fn supply(buyer_supplies: bool) -> Supply {
    if buyer_supplies {
        Supply::Buyer
    } else {
        Supply::Crafter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn call(v: Value) -> Value {
        serde_json::from_str(&dispatch(&v.to_string())).expect("output was not json")
    }

    fn recipe() -> Value {
        serde_json::json!({
            "id": 1, "product": 9001, "product_name": "Gold Malachite Bracelet",
            "skill": "Jewelry Making", "trivial": 146, "yields": 1, "no_fail": false,
            "components": [
                {"item": 101, "name": "Gold Bar", "qty": 1, "disposition": "PerAttempt",
                 "source": "Vendor", "unit_price": 800},
                {"item": 102, "name": "Malachite", "qty": 1, "disposition": "PerAttempt",
                 "source": "Drop", "unit_price": 0}
            ]
        })
    }

    #[test]
    fn chance_carries_the_warning_that_grey_is_not_safe() {
        let v = call(serde_json::json!({"op": "chance", "skill": 83, "trivial": 83}));
        assert_eq!(v["con"], "grey");
        assert_eq!(v["trivial_to_him"], true);
        assert!((v["chance"].as_f64().unwrap() - 0.7225).abs() < 1e-9);
    }

    #[test]
    fn a_quote_round_trips_through_json() {
        let v = call(serde_json::json!({
            "op": "quote", "recipe": recipe(), "qty": 10,
            "hand": {"skill": 146, "mastery": 0, "courtesy": 0.15, "owns_tools": true}
        }));
        assert!(v["total"].as_i64().unwrap() > 0);
        assert!(v["courtesy"].as_i64().unwrap() > 0);
        assert_eq!(v["runs"], 10);
    }

    #[test]
    fn hands_come_back_cheapest_first_and_stably() {
        let hands = serde_json::json!([
            {"name": "Zeke", "skill": 146, "mastery": 0, "courtesy": 0.0, "owns_tools": true},
            {"name": "Abel", "skill": 146, "mastery": 0, "courtesy": 0.0, "owns_tools": true},
            {"name": "Mott", "skill": 90,  "mastery": 0, "courtesy": 0.0, "owns_tools": true}
        ]);
        let v = call(serde_json::json!({
            "op": "hands", "recipe": recipe(), "qty": 5, "hands": hands
        }));
        let names: Vec<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["name"].as_str().unwrap())
            .collect();
        // Two equal hands tie on price, so the tie-break must be by name, not by luck.
        assert_eq!(names, vec!["Abel", "Zeke", "Mott"]);
    }

    #[test]
    fn the_order_machine_refuses_over_the_wire_too() {
        let ok = call(serde_json::json!({
            "op": "order", "phase": "Offered", "actor": "Crafter", "event": "Accept"
        }));
        assert_eq!(ok["phase"], "Accepted");
        assert_eq!(ok["waiting_on"], "Buyer");

        let no = call(serde_json::json!({
            "op": "order", "phase": "Delivered", "actor": "Crafter", "event": "Received"
        }));
        assert_eq!(no["refused"]["NotYours"]["needs"], "Buyer");
    }

    #[test]
    fn may_commission_reports_both_the_bar_and_the_courtesy() {
        let v = call(serde_json::json!({
            "op": "may_commission",
            "terms": {"open": true, "guild_only": true, "least_regard": "Kindly", "courtesy": 0.15},
            "same_server": true, "same_guild": false, "buyer": "Ally"
        }));
        assert_eq!(v["allowed"], false);
        assert_eq!(v["barred"], "GuildOnly");
        assert_eq!(v["courtesy"], 0.0);
    }

    #[test]
    fn harvest_returns_a_summary_and_the_upload_buckets() {
        let log = "[Mon Aug 03 01:12:13 2026] You have fashioned the items together to create something new: Ring.\n\
                   [Mon Aug 03 01:12:13 2026] You have become better at Jewelry Making! (73)\n\
                   [Mon Aug 03 01:12:14 2026] You lacked the skills to fashion Ring.\n\
                   [Mon Aug 03 01:12:15 2026] You have become better at Jewelry Making! (74)\n\
                   [Mon Aug 03 01:12:16 2026] You can no longer advance your skill from making this item.\n\
                   [Mon Aug 03 01:12:16 2026] You have fashioned the items together to create something new: Ring.\n";
        let v = call(serde_json::json!({"op": "harvest", "log": log}));
        assert_eq!(v["attempts"], 3);
        assert_eq!(v["successes"], 2);
        assert_eq!(v["pinned_trivials"][0][1], 74);
        assert!(!v["buckets"].as_array().unwrap().is_empty());
    }

    #[test]
    fn inventory_separates_what_you_hold_from_what_you_have_collected() {
        let dump = "Location\tName\tID\tCount\tSlots\r\n\
                    Ear\tBlack Sapphire Electrum Earring +4\t14701\t1\t10\r\n\
                    KeyRing\tName\tID\t\r\n\
                    Augmentation\tEarthshaker (Exaltation)\t5667\r\n";
        let v = call(serde_json::json!({"op": "inventory", "dump": dump}));
        assert_eq!(v["held"].as_array().unwrap().len(), 1);
        assert_eq!(v["collected"].as_array().unwrap().len(), 1);
        assert_eq!(v["ids"], serde_json::json!([14701]));
        assert_eq!(v["unreadable"], 0);
    }

    /// A malformed request must come back as an error, not a panic. A panic in wasm aborts
    /// the module and every later call in that page fails too.
    #[test]
    fn bad_input_is_an_error_not_a_trap() {
        for bad in [
            "",
            "{}",
            "not json at all",
            r#"{"op":"nope"}"#,
            r#"{"op":"quote"}"#,
            r#"{"op":"chance","skill":"lots","trivial":1}"#,
            r#"{"op":"catalogue","corpus":[1,2,3]}"#,
        ] {
            let out = dispatch(bad);
            let v: Value = serde_json::from_str(&out).expect("output was not json");
            assert!(
                v.get("error").is_some(),
                "{bad:?} did not report an error: {out}"
            );
        }
    }

    #[test]
    fn a_corpus_can_be_read_through_the_same_door() {
        let mut w = grimoire_corpus::Writer::new();
        let r: Recipe = serde_json::from_value(recipe()).unwrap();
        w.put("recipe/Jewelry Making/000001", &r);
        let (bytes, _) = w.finish("test");

        let cat = call(serde_json::json!({"op": "catalogue", "corpus": bytes}));
        assert_eq!(cat[0]["name"], "Gold Malachite Bracelet");
        assert_eq!(cat[0]["trivial"], 146);
        assert_eq!(
            cat[0]["priced"], false,
            "malachite drops, so this is not fully priced"
        );

        let one = call(serde_json::json!({
            "op": "recipe", "corpus": bytes, "key": cat[0]["key"].as_str().unwrap()
        }));
        assert_eq!(one["product_name"], "Gold Malachite Bracelet");
    }
}
