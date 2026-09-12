//! `grimoire quote` — price a job out of the corpus.
//!
//! The end of the chain: a log measured the trivial, the wiki supplied the recipe and the
//! reagent prices, the artifact carries both, and the same [`grimoire_core`] code that will
//! run in the browser and in the Worker prices the job. If this command is right, all three
//! are right, because there is only one implementation.

use crate::{flag, positionals};
use grimoire_core::combine::{Con, Mastery};
use grimoire_core::quote::{quote, Hand, Supply};
use grimoire_core::recipe::Recipe;
use grimoire_corpus::{InMemory, Reader};

pub fn run(args: &[String]) -> Result<(), String> {
    let pos = positionals(args);
    let corpus = pos.first().ok_or("which corpus?")?;
    let wanted = pos
        .get(1)
        .ok_or("what do you want made? e.g. grimoire quote eql.grim \"Potion of Accuracy\"")?;

    let qty: u32 = flag(args, "--qty")
        .and_then(|p| p.to_string_lossy().parse().ok())
        .unwrap_or(1);
    let skill: u16 = flag(args, "--skill")
        .and_then(|p| p.to_string_lossy().parse().ok())
        .unwrap_or(100);
    let courtesy: f64 = flag(args, "--courtesy")
        .and_then(|p| p.to_string_lossy().parse().ok())
        .unwrap_or(0.0);
    let buyer_supplies = args.iter().any(|a| a == "--my-parts");

    let bytes = std::fs::read(corpus).map_err(|e| format!("{corpus}: {e}"))?;
    let reader = Reader::open(InMemory(bytes)).map_err(|e| e.to_string())?;

    // Recipes are keyed by id, so finding one by name means a walk. Fine here — the browser
    // holds a name index built once from the same artifact.
    let mut found: Option<Recipe> = None;
    for key in reader.prefix("recipe/").map_err(|e| e.to_string())? {
        let r: Recipe = reader.get(&key).map_err(|e| e.to_string())?;
        if r.product_name.eq_ignore_ascii_case(wanted) {
            found = Some(r);
            break;
        }
    }
    let recipe = found.ok_or_else(|| format!("nothing called `{wanted}` in this corpus"))?;

    let hand = Hand {
        skill,
        mastery: Mastery::NONE,
        courtesy,
        owns_tools: true,
    };
    let supply = if buyer_supplies {
        Supply::Buyer
    } else {
        Supply::Crafter
    };
    let q = quote(&recipe, qty, &hand, supply);

    println!("{} x{qty}", recipe.product_name);
    println!(
        "  {} · trivial {} · a hand of skill {skill} cons it {} and lands {:.0}% of the time",
        recipe.skill.as_str(),
        recipe.trivial,
        Con::of(skill, recipe.trivial).as_str(),
        q.chance * 100.0
    );
    println!(
        "  {} combines wanted, {:.1} attempts expected",
        q.runs, q.attempts
    );

    println!("\n  materials");
    for m in &q.materials {
        let note = if m.buyer_supplies {
            "  (you post it)"
        } else if m.crafter_owns {
            "  (he has one)"
        } else {
            ""
        };
        println!(
            "    {:>4} x {:<28} {:>12}{note}",
            m.units,
            m.name,
            m.cost.to_string()
        );
    }

    if !q.to_post.is_empty() {
        println!("\n  you post him");
        for p in &q.to_post {
            println!("    {:>4} x {:<28} {:?}", p.units, p.name, p.source);
        }
    }

    println!("\n  materials        {:>14}", q.material_cost.to_string());
    println!("  his work         {:>14}", q.labour.to_string());
    println!("  risk             {:>14}", q.risk.to_string());
    println!("  ------------------------------");
    println!("  subtotal         {:>14}", q.subtotal.to_string());
    if !q.courtesy.is_zero() {
        println!(
            "  guild courtesy   {:>14}   −{:.0}%",
            format!("−{}", q.courtesy),
            courtesy * 100.0
        );
    }
    println!("  total            {:>14}", q.total.to_string());
    println!(
        "  with a 20% tip   {:>14}",
        q.with_gratuity(0.20).to_string()
    );
    Ok(())
}

/// `grimoire quotes` — the same thing across a band of skills, to see the curve bite.
pub fn table(args: &[String]) -> Result<(), String> {
    let pos = positionals(args);
    let corpus = pos.first().ok_or("which corpus?")?;
    let wanted = pos.get(1).ok_or("what do you want made?")?;
    let qty: u32 = flag(args, "--qty")
        .and_then(|p| p.to_string_lossy().parse().ok())
        .unwrap_or(10);

    let bytes = std::fs::read(corpus).map_err(|e| format!("{corpus}: {e}"))?;
    let reader = Reader::open(InMemory(bytes)).map_err(|e| e.to_string())?;
    let mut found: Option<Recipe> = None;
    for key in reader.prefix("recipe/").map_err(|e| e.to_string())? {
        let r: Recipe = reader.get(&key).map_err(|e| e.to_string())?;
        if r.product_name.eq_ignore_ascii_case(wanted) {
            found = Some(r);
            break;
        }
    }
    let recipe = found.ok_or_else(|| format!("nothing called `{wanted}` in this corpus"))?;

    println!(
        "{} x{qty} — trivial {}",
        recipe.product_name, recipe.trivial
    );
    println!("  skill   con          lands   attempts        total");
    let t = recipe.trivial;
    for skill in [
        t.saturating_sub(60),
        t.saturating_sub(40),
        t.saturating_sub(20),
        t,
        t + 20,
    ] {
        let hand = Hand {
            skill,
            ..Default::default()
        };
        let q = quote(&recipe, qty, &hand, Supply::Crafter);
        println!(
            "  {:>5}   {:<11}  {:>4.0}%   {:>6.1}   {:>12}",
            skill,
            Con::of(skill, t).as_str(),
            q.chance * 100.0,
            q.attempts,
            q.total.to_string()
        );
    }
    Ok(())
}
