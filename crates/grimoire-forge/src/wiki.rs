//! Reading eqlwiki.
//!
//! The wiki runs a live MediaWiki API — `api.php?action=parse&prop=wikitext&section=N` returns
//! a page's source verbatim — and the tradeskill pages carry the thing the Grimoire actually
//! needs: **product, trivial, and the components that make it**.
//!
//! There are two table dialects, and pages use whichever the author felt like:
//!
//! *HTML*, on Jewelcrafting's Crafters Item Table:
//!
//! ```text
//!    <tr>
//!       <td>{{:Silver Malachite Ring}}</td><td>17</td><td>0.553</td>
//!       … eighteen stat columns …
//!       <td>Finger</td><td>[[Silver Bar| Silver]] </td><td>{{:Malachite}}</td>
//!    </tr>
//! ```
//!
//! *Wiki pipe syntax*, on Alchemy's recipe list:
//!
//! ```text
//!   |-
//!   | 83 || [[Potion of Accuracy]] || Buff || Stat || Agility, Dexterity || [[Birthwort]] || [[Fenugreek]] || [[Blue Vervain Bulb]] ||  ||
//! ```
//!
//! Fetching is deliberately *not* done here. Wikitext is saved to a file and parsed offline,
//! so an ingest is reproducible, reviewable in a diff, and does not depend on the wiki being
//! up when a corpus is cut.

use grimoire_core::recipe::{Component, Disposition, Recipe, Skill, Source};
use grimoire_core::Coin;
use std::collections::BTreeMap;

/// A component and how many of it the recipe wants.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Part {
    pub name: String,
    pub qty: u32,
}

/// One row of a recipe table.
#[derive(Clone, PartialEq, Debug)]
pub struct WikiRecipe {
    pub product: String,
    /// `None` when the wiki leaves the trivial blank, which it does for unreleased items.
    pub trivial: Option<u16>,
    /// The wiki's "Cost\*" column, in platinum. Materials only.
    pub cost_pp: Option<f64>,
    pub slot: Option<String>,
    pub effect: Option<String>,
    pub parts: Vec<Part>,
    pub deity: Option<String>,
}

/// Which cells mean what. Column *order* is stable across the wiki even where the headers
/// are not, so this is indexed rather than matched by name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Columns {
    pub product: usize,
    pub trivial: Option<usize>,
    pub cost: Option<usize>,
    pub slot: Option<usize>,
    pub effect: Option<usize>,
    pub deity: Option<usize>,
    /// Component columns, in order.
    pub parts: &'static [usize],
}

/// `Item Triv Cost HP…CR Slot Metal Gem`
pub const JEWELCRAFTING: Columns = Columns {
    product: 0,
    trivial: Some(1),
    cost: Some(2),
    slot: Some(19),
    effect: None,
    deity: None,
    parts: &[20, 21],
};

/// The deity table adds a trailing deity column.
pub const JEWELCRAFTING_DEITY: Columns = Columns {
    deity: Some(22),
    ..JEWELCRAFTING
};

/// `Trivial Recipe Func. Type Effect Ing.1 … Ing.8`
pub const ALCHEMY: Columns = Columns {
    product: 1,
    trivial: Some(0),
    cost: None,
    slot: None,
    effect: Some(4),
    deity: None,
    parts: &[5, 6, 7, 8, 9, 10, 11, 12],
};

/// Split a saved page into rows of cells, whichever dialect it is written in.
pub fn rows(wikitext: &str) -> Vec<Vec<String>> {
    if wikitext.contains("<tr") {
        html_rows(wikitext)
    } else {
        pipe_rows(wikitext)
    }
}

fn html_rows(wikitext: &str) -> Vec<Vec<String>> {
    wikitext
        .split("<tr")
        .skip(1)
        .filter_map(|row| {
            let row = &row[row.find('>')? + 1..];
            let row = row.split("</tr").next().unwrap_or(row);
            if row.contains("<th") {
                return None;
            }
            Some(
                row.split("<td")
                    .skip(1)
                    .map(|c| {
                        let c = c.find('>').map_or(c, |i| &c[i + 1..]);
                        clean(c.split("</td").next().unwrap_or(c))
                    })
                    .collect(),
            )
        })
        .collect()
}

/// MediaWiki pipe tables.
///
/// A row starts at `|-` and its cells are separated by `||`. Header rows begin with `!` and
/// are dropped. A cell may carry `[[Small Vial]] x 5`, which is five of them, not one.
fn pipe_rows(wikitext: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<String>> = None;

    for line in wikitext.lines() {
        let t = line.trim();
        if t.starts_with("|-") {
            if let Some(r) = cur.take() {
                out.push(r);
            }
            cur = Some(Vec::new());
            continue;
        }
        if t.starts_with('!') || t.starts_with("{|") {
            cur = None; // header row, or the table opener
            continue;
        }
        if t.starts_with("|}") {
            if let Some(r) = cur.take() {
                out.push(r);
            }
            continue;
        }
        if let (Some(row), Some(body)) = (cur.as_mut(), t.strip_prefix('|')) {
            row.extend(body.split("||").map(clean));
        }
    }
    if let Some(r) = cur {
        out.push(r);
    }
    out.retain(|r| !r.is_empty());
    out
}

/// Read a table into recipes.
pub fn parse_table(wikitext: &str, cols: Columns) -> Vec<WikiRecipe> {
    let get = |cells: &[String], i: Option<usize>| -> Option<String> {
        i.and_then(|i| cells.get(i))
            .filter(|s| !s.is_empty())
            .cloned()
    };

    rows(wikitext)
        .into_iter()
        .filter_map(|cells| {
            let product = cells.get(cols.product).filter(|s| !s.is_empty())?.clone();

            // Repeats are quantities: five `Small Vial` cells mean five vials.
            let mut tally: BTreeMap<String, u32> = BTreeMap::new();
            let mut order: Vec<String> = Vec::new();
            for &i in cols.parts {
                let Some(cell) = cells.get(i).filter(|s| !s.is_empty()) else {
                    continue;
                };
                let (name, qty) = split_quantity(cell);
                if !tally.contains_key(&name) {
                    order.push(name.clone());
                }
                *tally.entry(name).or_default() += qty;
            }
            if order.is_empty() {
                return None;
            }

            Some(WikiRecipe {
                product,
                trivial: get(&cells, cols.trivial).and_then(|s| s.parse().ok()),
                cost_pp: get(&cells, cols.cost).and_then(|s| s.parse().ok()),
                slot: get(&cells, cols.slot),
                effect: get(&cells, cols.effect),
                deity: get(&cells, cols.deity),
                parts: order
                    .into_iter()
                    .map(|name| {
                        let qty = tally[&name];
                        Part { name, qty }
                    })
                    .collect(),
            })
        })
        .collect()
}

/// `Small Vial x 5` → `("Small Vial", 5)`.
fn split_quantity(cell: &str) -> (String, u32) {
    if let Some((name, n)) = cell.rsplit_once(" x ") {
        if let Ok(q) = n.trim().parse::<u32>() {
            return (name.trim().to_string(), q.max(1));
        }
    }
    (cell.to_string(), 1)
}

/// Strip wiki markup down to the name a player would recognise.
///
/// `{{:Silver Malachite Ring}}` → `Silver Malachite Ring`
/// `[[Silver Bar| Silver]]` → `Silver Bar` — the **target**, not the display text, because the
/// target is the item and the display text is whatever read nicely in that column.
fn clean(cell: &str) -> String {
    let mut s = cell.trim().to_string();
    let mut suffix = String::new();

    for (open, close) in [("{{:", "}}"), ("{{", "}}"), ("[[", "]]")] {
        if let Some(start) = s.find(open) {
            let rest = &s[start + open.len()..];
            if let Some(end) = rest.find(close) {
                // Keep anything trailing, so `[[Small Vial]] x 5` does not lose its count.
                suffix = rest[end + close.len()..].to_string();
                s = rest[..end].to_string();
                break;
            }
        }
    }
    if let Some((target, _display)) = s.split_once('|') {
        s = target.to_string();
    }
    s.push_str(&suffix);

    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ").trim().to_string()
}

/// Prices from a `… || Material Cost (pp)` table, where the last column is decimal platinum.
///
/// This is how the Jewelcrafting page publishes metal bars: `[[Gold Bar]] … || 10.079`.
pub fn parse_pp_prices(wikitext: &str) -> BTreeMap<String, Coin> {
    let mut out = BTreeMap::new();
    for cells in rows(wikitext) {
        let (Some(name), Some(last)) = (cells.first(), cells.last()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        if let Ok(pp) = last.trim().parse::<f64>() {
            if pp > 0.0 {
                out.insert(name.clone(), Coin::from_f64(pp * 1000.0));
            }
        }
    }
    out
}

/// Fill in what the wiki does not price directly.
///
/// Jewelcrafting publishes a per-recipe `Cost*` and a per-bar price, but never a gem price.
/// Since a piece is exactly one bar plus one gem, the gem is the difference — and every
/// recipe using that gem then agrees on it. Derived rather than transcribed, so it is worth
/// saying out loud: a gem price here is **inferred**, and a negative or absurd difference is
/// discarded rather than clamped, because a bad price is worse than no price.
pub fn derive_prices(
    rows: &[WikiRecipe],
    known: &BTreeMap<String, Coin>,
) -> BTreeMap<String, Coin> {
    let mut votes: BTreeMap<String, Vec<i64>> = BTreeMap::new();

    for r in rows {
        let Some(total) = r.cost_pp.map(|pp| Coin::from_f64(pp * 1000.0)) else {
            continue;
        };
        // Exactly one unpriced component, or the difference cannot be attributed.
        let unknown: Vec<&Part> = r
            .parts
            .iter()
            .filter(|p| !known.contains_key(&p.name))
            .collect();
        if unknown.len() != 1 {
            continue;
        }
        let accounted: i64 = r
            .parts
            .iter()
            .filter_map(|p| known.get(&p.name).map(|c| c.0 * p.qty as i64))
            .sum();
        let each = (total.0 - accounted) / unknown[0].qty.max(1) as i64;
        if each > 0 {
            votes.entry(unknown[0].name.clone()).or_default().push(each);
        }
    }

    // Median across every recipe that mentions the gem — one mistyped Cost* should not move it.
    votes
        .into_iter()
        .map(|(name, mut v)| {
            v.sort_unstable();
            (name, Coin::copper(v[v.len() / 2]))
        })
        .collect()
}

/// Vendor prices from a `Reagent || Plat || Gold || Silver || Copper` table.
pub fn parse_prices(wikitext: &str) -> BTreeMap<String, Coin> {
    let mut out = BTreeMap::new();
    for cells in rows(wikitext) {
        if cells.len() < 5 || cells[0].is_empty() {
            continue;
        }
        let n = |i: usize| cells[i].trim().parse::<i64>().unwrap_or(0);
        // A row of all zeroes is a blank the wiki has not filled in, not a free reagent.
        let total = n(1) * 1000 + n(2) * 100 + n(3) * 10 + n(4);
        if total > 0 {
            out.insert(cells[0].clone(), Coin::copper(total));
        }
    }
    out
}

/// Turn wiki rows into recipes the quote engine can price.
///
/// - `ids` joins names to client item ids where one is known.
/// - `measured_trivial` overrides the wiki, because a measurement beats a transcription.
/// - `price` gives a vendor price and, by implication, that the component is buyable at all.
pub fn to_recipes(
    rows: &[WikiRecipe],
    skill: Skill,
    ids: &dyn Fn(&str) -> u32,
    measured_trivial: &dyn Fn(&str) -> Option<u16>,
    price: &dyn Fn(&str) -> Option<Coin>,
) -> Vec<Recipe> {
    rows.iter()
        .enumerate()
        .filter_map(|(n, r)| {
            // No trivial anywhere means no quote. Skip rather than invent one.
            let trivial = measured_trivial(&r.product).or(r.trivial)?;
            let components = r
                .parts
                .iter()
                .map(|p| {
                    let unit_price = price(&p.name);
                    Component {
                        item: ids(&p.name),
                        name: p.name.clone(),
                        qty: p.qty,
                        disposition: Disposition::PerAttempt,
                        // A known vendor price is what "you can buy this" means here. No
                        // price is not evidence it drops, so it stays Unknown, and Unknown
                        // is treated as un-buyable — the buyer gets asked rather than
                        // silently charged for something the crafter cannot find.
                        source: if unit_price.is_some() {
                            Source::Vendor
                        } else {
                            Source::Unknown
                        },
                        unit_price: unit_price.unwrap_or(Coin::ZERO),
                    }
                })
                .collect();
            Some(Recipe {
                id: n as u32 + 1,
                product: ids(&r.product),
                product_name: r.product.clone(),
                skill,
                trivial,
                yields: 1,
                no_fail: false,
                effect: None,
                components,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Copied byte for byte out of the wiki's Jewelcrafting page.
    const JC: &str = r#"
<table class="eoTable sortable">
   <tr style="font-size: 87%;">
      <th>Item</th><th>Triv</th><th>Cost*</th><th>HP</th>
      <th>MP</th><th>END</th><th>AC</th>
      <th>STR</th><th>STA</th><th>DEX</th><th>AGI</th><th>INT</th><th>WIS</th><th>CHA</th>
      <th>MR</th><th>DR</th><th>PR</th><th>FR</th><th>CR</th>
      <th>Slot</th><th>Metal</th><th>Gem</th>
   </tr>
   <tr>
      <td>{{:Silver Malachite Ring}}</td><td>17</td><td>0.553</td><td></td>
      <td></td><td></td><td></td>
      <td></td><td></td><td></td><td></td><td></td><td></td><td></td>
      <td></td><td></td><td>2</td><td></td><td></td>
      <td>Finger</td><td>[[Silver Bar| Silver]] </td><td>{{:Malachite}}</td>
   </tr>
</table>"#;

    /// Copied byte for byte out of the wiki's Alchemy page.
    const AL: &str = r#"{| class="wikitable sortable"
|-
! Trivial !! Recipe !! Func. !! Type !! Effect !! Ing. 1 !! Ing. 2 !! Ing. 3 !! Ing. 4
|-
| 83 || [[Potion of Accuracy]] || Buff || Stat || Agility, Dexterity || [[Birthwort]] || [[Fenugreek]] || [[Blue Vervain Bulb]] ||
|-
| 31 || [[Distillate of Clarity I]] || Utility || Regen || Mana || [[Small Vial]] || [[Small Vial]] || [[Small Vial]] || [[Katuka Bark]]
|-
| 302 || [[Distillate of Alacrity IX]] || Buff || Combat || Haste || [[Comfrey]] || [[Deepwater Ink]] || [[Arnworth]]|| [[Small Vial]] x 5
|}"#;

    const PRICES: &str = r#"{| class="eoTable2 sortable"
! Reagent || Plat || Gold || Silver || Copper
|-
| [[Agrimony]]||132||2||9||9
|-
| [[Lucerne]]||2||6||4||5
|-
| [[Balm Leaves]] || 66 || 8 || 4 || 0
|-
| [[Not Priced Yet]]||0||0||0||0
|}"#;

    #[test]
    fn reads_the_html_dialect() {
        let rows = parse_table(JC, JEWELCRAFTING);
        assert_eq!(rows.len(), 1, "header row should not become a recipe");
        assert_eq!(rows[0].product, "Silver Malachite Ring");
        assert_eq!(rows[0].trivial, Some(17));
        assert_eq!(rows[0].cost_pp, Some(0.553));
        assert_eq!(rows[0].slot.as_deref(), Some("Finger"));
        assert_eq!(
            rows[0].parts,
            vec![
                Part {
                    name: "Silver Bar".into(),
                    qty: 1
                },
                Part {
                    name: "Malachite".into(),
                    qty: 1
                },
            ]
        );
    }

    #[test]
    fn reads_the_pipe_dialect() {
        let rows = parse_table(AL, ALCHEMY);
        assert_eq!(rows.len(), 3, "header row should not become a recipe");
        assert_eq!(rows[0].product, "Potion of Accuracy");
        assert_eq!(rows[0].trivial, Some(83));
        assert_eq!(rows[0].effect.as_deref(), Some("Agility, Dexterity"));
        assert_eq!(rows[0].parts.len(), 3);
    }

    /// Five separate `Small Vial` cells are five vials, not one. Quoting them as one is how a
    /// distillate comes out five times too cheap.
    #[test]
    fn repeated_ingredients_become_a_quantity() {
        let rows = parse_table(AL, ALCHEMY);
        let clarity = rows
            .iter()
            .find(|r| r.product.starts_with("Distillate of Clarity"))
            .unwrap();
        assert_eq!(
            clarity.parts,
            vec![
                Part {
                    name: "Small Vial".into(),
                    qty: 3
                },
                Part {
                    name: "Katuka Bark".into(),
                    qty: 1
                },
            ]
        );
    }

    /// The wiki also writes the count inline on some rows.
    #[test]
    fn an_inline_multiplier_is_read_as_a_quantity() {
        let rows = parse_table(AL, ALCHEMY);
        let alac = rows
            .iter()
            .find(|r| r.product.starts_with("Distillate of Alacrity"))
            .unwrap();
        let vial = alac.parts.iter().find(|p| p.name == "Small Vial").unwrap();
        assert_eq!(vial.qty, 5);
    }

    #[test]
    fn a_link_yields_its_target_not_its_label() {
        assert_eq!(clean("[[Silver Bar| Silver]] "), "Silver Bar");
        assert_eq!(clean("{{:Platinum Bar| Platinum}}"), "Platinum Bar");
        assert_eq!(
            clean("{{:Imbued Plains Pebble | Plains Pebble}}"),
            "Imbued Plains Pebble"
        );
        assert_eq!(clean("{{:Malachite}}"), "Malachite");
        assert_eq!(clean("[[Small Vial]] x 5"), "Small Vial x 5");
        assert_eq!(clean(" Neck"), "Neck");
        assert_eq!(clean(""), "");
    }

    #[test]
    fn prices_are_read_in_all_four_denominations() {
        let p = parse_prices(PRICES);
        assert_eq!(p["Lucerne"], Coin::copper(2 * 1000 + 6 * 100 + 4 * 10 + 5));
        assert_eq!(
            p["Agrimony"],
            Coin::copper(132 * 1000 + 2 * 100 + 9 * 10 + 9)
        );
        assert_eq!(p["Balm Leaves"], Coin::copper(66 * 1000 + 8 * 100 + 4 * 10));
    }

    /// A row of zeroes is a gap in the wiki, not a free reagent. Treating it as free would
    /// quote a job at nothing.
    #[test]
    fn an_unpriced_reagent_is_absent_rather_than_free() {
        let p = parse_prices(PRICES);
        assert!(!p.contains_key("Not Priced Yet"));
    }

    const METALS: &str = r#"{| class="eoTable"
|-
! Unenchanted Bar !! Enchanted Bar !! Spell !! Enc Level !! Mana Cost !! Material Cost (pp)
|-
| [[Silver Bar]]   || [[Enchanted Silver Bar]]   || [[Enchant Silver]]   || 8  || 60 || 0.503
|-
| [[Gold Bar]]     || [[Enchanted Gold Bar]]     || [[Enchant Gold]]     || 24 || 150 || 10.079
|}"#;

    #[test]
    fn decimal_platinum_prices_are_read() {
        let p = parse_pp_prices(METALS);
        assert_eq!(p["Silver Bar"], Coin::copper(503));
        assert_eq!(p["Gold Bar"], Coin::copper(10079));
        assert_eq!(p.len(), 2, "the header row is not a metal");
    }

    /// Silver Malachite Ring costs 0.553pp and a Silver Bar is 0.503pp, so a Malachite is
    /// 0.050pp. Nowhere on the wiki does it say so.
    #[test]
    fn a_gem_price_falls_out_of_the_difference() {
        let rows = parse_table(JC, JEWELCRAFTING);
        let derived = derive_prices(&rows, &parse_pp_prices(METALS));
        assert_eq!(derived["Malachite"], Coin::copper(50));
        assert!(
            !derived.contains_key("Silver Bar"),
            "already known, must not be re-derived"
        );
    }

    #[test]
    fn nothing_is_derived_when_two_components_are_unknown() {
        let rows = parse_table(JC, JEWELCRAFTING);
        let derived = derive_prices(&rows, &BTreeMap::new());
        assert!(
            derived.is_empty(),
            "with no metal price the split is unattributable"
        );
    }

    /// A row whose Cost* is below the metal alone is a wiki error. Discard it — a negative or
    /// zero gem price would quote a job at less than its materials.
    #[test]
    fn an_impossible_difference_is_discarded_not_clamped() {
        let t = JC.replace("<td>0.553</td>", "<td>0.100</td>");
        let rows = parse_table(&t, JEWELCRAFTING);
        let derived = derive_prices(&rows, &parse_pp_prices(METALS));
        assert!(!derived.contains_key("Malachite"));
    }

    #[test]
    fn a_blank_trivial_is_none_rather_than_zero() {
        let t = JC.replace("<td>17</td>", "<td>  </td>");
        assert_eq!(parse_table(&t, JEWELCRAFTING)[0].trivial, None);
    }

    #[test]
    fn a_row_without_components_is_skipped() {
        let t = JC
            .replace("<td>{{:Malachite}}</td>", "<td></td>")
            .replace("<td>[[Silver Bar| Silver]] </td>", "<td></td>");
        assert!(parse_table(&t, JEWELCRAFTING).is_empty());
    }

    #[test]
    fn recipes_without_a_trivial_anywhere_are_dropped_not_invented() {
        let rows = vec![WikiRecipe {
            product: "Mystery Ring".into(),
            trivial: None,
            cost_pp: None,
            slot: None,
            effect: None,
            deity: None,
            parts: vec![Part {
                name: "Gold Bar".into(),
                qty: 1,
            }],
        }];
        let out = to_recipes(&rows, Skill::JewelryMaking, &|_| 0, &|_| None, &|_| None);
        assert!(out.is_empty());
    }

    #[test]
    fn a_measured_trivial_beats_the_wikis() {
        let rows = vec![WikiRecipe {
            product: "Gold Malachite Bracelet".into(),
            trivial: Some(140),
            cost_pp: None,
            slot: None,
            effect: None,
            deity: None,
            parts: vec![Part {
                name: "Gold Bar".into(),
                qty: 1,
            }],
        }];
        let out = to_recipes(
            &rows,
            Skill::JewelryMaking,
            &|_| 0,
            &|n| (n == "Gold Malachite Bracelet").then_some(146),
            &|_| None,
        );
        assert_eq!(out[0].trivial, 146);
    }

    /// Without a price we do not know a component can be bought, so the buyer is asked for it
    /// rather than quietly billed for something the crafter may not be able to get.
    #[test]
    fn an_unpriced_component_is_unknown_not_vendor() {
        let rows = parse_table(AL, ALCHEMY);
        let out = to_recipes(&rows, Skill::Alchemy, &|_| 0, &|_| None, &|n| {
            (n == "Birthwort").then(|| Coin::copper(2645))
        });
        let acc = out
            .iter()
            .find(|r| r.product_name == "Potion of Accuracy")
            .unwrap();
        let birthwort = acc
            .components
            .iter()
            .find(|c| c.name == "Birthwort")
            .unwrap();
        let vervain = acc
            .components
            .iter()
            .find(|c| c.name == "Blue Vervain Bulb")
            .unwrap();
        assert_eq!(birthwort.source, Source::Vendor);
        assert_eq!(vervain.source, Source::Unknown);
        assert!(!vervain.source.purchasable());
    }

    #[test]
    fn item_ids_are_joined_when_known() {
        let rows = parse_table(JC, JEWELCRAFTING);
        let out = to_recipes(
            &rows,
            Skill::JewelryMaking,
            &|n| if n == "Silver Bar" { 1234 } else { 0 },
            &|_| None,
            &|_| None,
        );
        let bar = out[0]
            .components
            .iter()
            .find(|c| c.name == "Silver Bar")
            .unwrap();
        assert_eq!(bar.item, 1234);
    }
}
