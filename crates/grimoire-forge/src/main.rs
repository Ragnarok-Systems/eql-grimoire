//! `grimoire` — the workbench.
//!
//! Everything here runs on the player's own machine against the player's own files. Nothing
//! in this binary uploads anything.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod combat;
mod combines;
mod corpus;
mod fights;
mod quote;
mod serve;
mod wiki;

const USAGE: &str = "\
grimoire — EQL Grimoire workbench

  grimoire combines <eqlog.txt>...      read crafting out of a log
    --csv <path>                        write the calibration fixture
    --json <path>                       write the full harvest
  grimoire combat <eqlog.txt>...        read fighting out of a log
    --top N                             how many entities to list (default 12)
    --unknown                           print the lines the parser did not recognise
  grimoire fights <eqlog.txt>...        cut a log into fights, with DPS per participant
    --me NAME                           your character, so `you` and your own name are one
                                        person (taken from an eqlog_<char>_<server>.txt
                                        filename when you do not say)
    --quiet N                           seconds of no combat that end a fight (default 30,
                                        measured: every value from 29 to 167 cuts the
                                        reference capture the same way)
    --fights N                          how many fights to list (default 20)
    --top N                             how many participants per fight (default 10)
  grimoire inventory <dump.txt>         read an /outputfile inventory dump
  grimoire wiki <page.wikitext>...      turn a Crafters Item Table into recipes
    --skill <name>                      which tradeskill the page is (default Jewelry Making)
    --items <items.json>                join names to client item ids
    --trivials <trivials.csv>           measured trivials, which beat the wiki's
    --prices <reagents.wikitext>        a p/g/s/c vendor price table
    --metals <metals.wikitext>          a decimal-platinum price table
    --out <recipes.json>                where the recipes go
  grimoire corpus <out.grim>            cut a corpus artifact
    --from <dir>                        directory of recipe json
    --items <items.json>                [{id,name}] from an inventory dump
    --trivials <trivials.csv>           item,trivial,skill measured from a log
  grimoire check <corpus.grim>          open an artifact and report on it
  grimoire quote <corpus.grim> <item>   price a job
    --qty N --skill N --courtesy 0.15   the hand and the job
    --my-parts                          you post the un-buyable components
  grimoire quotes <corpus.grim> <item>  the same job across a band of skills
  grimoire serve [--port 8787]          run the app: engine on POST /engine, files from web/
    --root web --corpus web/corpus.grim
  grimoire dispatch [--corpus <f>]      one JSON request per line on stdin, replies on
                                        stdout — the same door the browser goes through,
                                        so a page can be tested without a wasm build

Logs are usually in  <EverQuest Legends>\\Logs\\eqlog_<char>_<server>.txt
Inventory dumps land next to the client as  <Char>_<server>-Inventory.txt
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        eprint!("{USAGE}");
        return ExitCode::FAILURE;
    };

    let result = match cmd {
        "combines" => combines::run(&args[1..]),
        "combat" => combat::run(&args[1..]),
        "fights" => fights::run(&args[1..]),
        "inventory" => inventory(&args[1..]),
        "wiki" => ingest_wiki(&args[1..]),
        "corpus" => corpus::build(&args[1..]),
        "check" => corpus::check(&args[1..]),
        "quote" => quote::run(&args[1..]),
        "quotes" => quote::table(&args[1..]),
        "dispatch" => dispatch(&args[1..]),
        "serve" => serve::run(&args[1..]),
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("grimoire: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Pull `--name value` out of an argument list, leaving the positionals.
pub fn flag(args: &[String], name: &str) -> Option<PathBuf> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}

pub fn positionals(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if a.starts_with("--") {
            skip = true;
            continue;
        }
        out.push(a.as_str());
    }
    out
}

pub fn read(path: impl AsRef<Path>) -> Result<String, String> {
    let p = path.as_ref();
    // Logs are latin-1 in practice; a stray byte must not sink a 160 MB read.
    let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    })
}

/// `[{id, name}]` — the shape the item list travels in.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Named {
    pub id: u32,
    pub name: String,
}

/// Load `--items` into a name → id lookup. Missing file means no ids, not an error.
pub fn item_ids(args: &[String]) -> Result<std::collections::HashMap<String, u32>, String> {
    let Some(p) = flag(args, "--items") else {
        return Ok(Default::default());
    };
    let rows: Vec<Named> =
        serde_json::from_str(&read(&p)?).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(rows.into_iter().map(|r| (r.name, r.id)).collect())
}

/// Load `--trivials` into a name → trivial lookup.
pub fn measured_trivials(
    args: &[String],
) -> Result<std::collections::HashMap<String, u16>, String> {
    let Some(p) = flag(args, "--trivials") else {
        return Ok(Default::default());
    };
    let text = read(&p)?;
    let mut out = std::collections::HashMap::new();
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let f: Vec<&str> = line.split(',').collect();
        if let (Some(name), Some(Ok(t))) = (f.first(), f.get(1).map(|s| s.trim().parse::<u16>())) {
            out.insert(name.to_string(), t);
        }
    }
    Ok(out)
}

fn ingest_wiki(args: &[String]) -> Result<(), String> {
    use grimoire_core::recipe::Skill;

    let files = positionals(args);
    if files.is_empty() {
        return Err("give me a saved Crafters Item Table".into());
    }
    let skill_name = args
        .iter()
        .position(|a| a == "--skill")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or("Jewelry Making");
    let skill = Skill::from_log(skill_name)
        .ok_or_else(|| format!("`{skill_name}` is not a tradeskill this game has"))?;

    let ids = item_ids(args)?;
    let measured = measured_trivials(args)?;

    let mut prices: std::collections::BTreeMap<String, grimoire_core::Coin> = Default::default();
    if let Some(p) = flag(args, "--prices") {
        prices = wiki::parse_prices(&read(&p)?);
        println!("{} vendor prices from {}", prices.len(), p.display());
    }
    if let Some(p) = flag(args, "--metals") {
        let m = wiki::parse_pp_prices(&read(&p)?);
        println!("{} metal prices from {}", m.len(), p.display());
        prices.extend(m);
    }

    let cols = match skill {
        Skill::Alchemy => wiki::ALCHEMY,
        _ => wiki::JEWELCRAFTING,
    };

    let mut all = Vec::new();
    for f in &files {
        let text = read(f)?;
        // The deity table has one extra column; it always follows a `=== Deity ===` heading.
        let (basic, deity) = match text.find("=== Deity ===") {
            Some(i) if cols == wiki::JEWELCRAFTING => (&text[..i], Some(&text[i..])),
            _ => (text.as_str(), None),
        };
        let mut rows = wiki::parse_table(basic, cols);
        let basic_n = rows.len();
        let mut deity_n = 0;
        if let Some(d) = deity {
            let dr = wiki::parse_table(d, wiki::JEWELCRAFTING_DEITY);
            deity_n = dr.len();
            rows.extend(dr);
        }
        if deity_n > 0 {
            println!("{f}\n  {basic_n} rows, {deity_n} deity rows");
        } else {
            println!("{f}\n  {basic_n} rows");
        }
        all.extend(rows);
    }

    // Whatever the wiki prices directly, plus whatever falls out of the per-recipe totals.
    let derived = wiki::derive_prices(&all, &prices);
    if !derived.is_empty() {
        println!(
            "  {} component prices derived from per-recipe totals",
            derived.len()
        );
        for (k, v) in &derived {
            prices.entry(k.clone()).or_insert(*v);
        }
    }

    let recipes = wiki::to_recipes(
        &all,
        skill,
        &|n| ids.get(n).copied().unwrap_or(0),
        &|n| measured.get(n).copied(),
        &|n| prices.get(n).copied(),
    );

    let with_id = recipes.iter().filter(|r| r.product != 0).count();
    let agreed = all
        .iter()
        .filter(|r| {
            measured
                .get(&r.product)
                .is_some_and(|m| Some(*m) == r.trivial)
        })
        .count();
    let corrected = all
        .iter()
        .filter(|r| {
            measured
                .get(&r.product)
                .is_some_and(|m| r.trivial.is_some_and(|w| w != *m))
        })
        .count();
    let priced: usize = recipes
        .iter()
        .filter(|r| r.components.iter().all(|c| c.source.purchasable()))
        .count();

    println!(
        "\n  {} rows -> {} recipes ({} dropped for having no trivial)",
        all.len(),
        recipes.len(),
        all.len() - recipes.len()
    );
    println!("  {with_id} joined to a client item id");
    println!("  {priced} fully priced from vendor goods");
    if agreed + corrected > 0 {
        println!("  {agreed} trivials confirmed against a log, {corrected} corrected by it");
    }

    if let Some(p) = flag(args, "--out") {
        std::fs::write(&p, serde_json::to_vec_pretty(&recipes).unwrap())
            .map_err(|e| format!("{}: {e}", p.display()))?;
        println!("  -> {}", p.display());
    }
    Ok(())
}

/// The browser's engine API over stdin/stdout.
///
/// Exactly the same [`grimoire_wasm::dispatch`] the wasm module wraps. It exists so the web
/// page can be driven by the real engine in a test harness on a machine where the wasm target
/// is not installed — the transport differs, the code answering does not.
fn dispatch(args: &[String]) -> Result<(), String> {
    use std::io::{BufRead, Write};

    // With `--corpus`, a request may omit the corpus bytes and have them spliced in. Without
    // it, every catalogue/recipe request has to carry a 139 KB byte array, and a few hundred
    // of them is hundreds of megabytes down a pipe for no reason.
    let corpus: Option<serde_json::Value> = match flag(args, "--corpus") {
        Some(p) => {
            let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            Some(serde_json::Value::from(bytes))
        }
        None => None,
    };

    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let request = match &corpus {
            Some(c) => match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(mut v) if v.get("corpus").map_or(true, |x| x.is_null()) => {
                    if let Some(o) = v.as_object_mut() {
                        o.insert("corpus".into(), c.clone());
                    }
                    v.to_string()
                }
                _ => line,
            },
            None => line,
        };
        writeln!(out, "{}", grimoire_wasm::dispatch::dispatch(&request))
            .map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn inventory(args: &[String]) -> Result<(), String> {
    let files = positionals(args);
    if files.is_empty() {
        return Err("give me an inventory dump".into());
    }
    for f in files {
        let inv = grimoire_parse::inventory::parse(&read(f)?);
        let sockets = inv
            .held
            .iter()
            .filter(|h| matches!(h.place, grimoire_parse::Place::Socket { .. }))
            .count();
        println!("{f}");
        println!(
            "  {} entries, {} of them exaltation sockets",
            inv.held.len(),
            sockets
        );
        println!("  {} distinct items you actually hold", inv.ids().len());
        if !inv.collected.is_empty() {
            let mut cats: Vec<(&str, usize)> = Vec::new();
            for c in &inv.collected {
                match cats.iter_mut().find(|(k, _)| *k == c.category) {
                    Some((_, n)) => *n += 1,
                    None => cats.push((&c.category, 1)),
                }
            }
            let summary: Vec<String> = cats.iter().map(|(k, n)| format!("{n} {k}")).collect();
            println!("  keyring: {}", summary.join(", "));
        }
        if inv.unreadable > 0 {
            println!(
                "  {} rows unreadable — the dump format may have moved",
                inv.unreadable
            );
        }
        let mut upgraded: Vec<_> = inv.held.iter().filter(|h| h.upgrade > 0).collect();
        upgraded.sort_by_key(|h| std::cmp::Reverse(h.upgrade));
        for h in upgraded.iter().take(5) {
            println!("  +{:<2} {} ({})", h.upgrade, h.name, h.id);
        }
    }
    Ok(())
}
