//! `grimoire corpus` / `grimoire check` — cutting and inspecting the artifact.

use crate::{flag, positionals, read};
use grimoire_core::recipe::Recipe;
use grimoire_corpus::{InMemory, Manifest, Reader, Writer};

pub fn build(args: &[String]) -> Result<(), String> {
    let out = positionals(args)
        .first()
        .map(|s| s.to_string())
        .ok_or("where should the artifact go?")?;

    let mut w = Writer::new();
    w.source("EverQuest Legends client files");

    let mut n = 0usize;
    if let Some(dir) = flag(args, "--from") {
        // `recipes-<skill>.json`, and nothing else in the directory. Scanning every .json and
        // hoping meant a stray file failed the whole build with a serde error.
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.starts_with("recipes-") && s.ends_with(".json"))
            })
            .collect();
        paths.sort(); // so the artifact is byte-identical run to run
        if paths.is_empty() {
            return Err(format!(
                "no recipes-*.json in {} — run `grimoire wiki` first",
                dir.display()
            ));
        }
        for path in paths {
            let text = read(&path)?;
            let recipes: Vec<Recipe> =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            for r in &recipes {
                // Recipe ids restart per file, so namespace them by tradeskill or the second
                // file silently overwrites the first.
                w.put(format!("recipe/{}/{:06}", r.skill.as_str(), r.id), r);
                n += 1;
            }
            w.source(format!("{}", path.display()));
        }
    }

    // Item names, keyed by the id the inventory dump prints.
    let mut items = 0usize;
    if let Some(path) = flag(args, "--items") {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct Named {
            id: u32,
            name: String,
        }
        let rows: Vec<Named> =
            serde_json::from_str(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))?;
        for r in &rows {
            w.put(format!("item/{:08}", r.id), r);
            items += 1;
        }
        w.source(format!("{}", path.display()));
    }

    // The trivials the logs pinned are knowledge no wiki has. They ride along, in their own
    // namespace, so a consumer can tell measurement from received wisdom.
    let mut trivials = 0usize;
    if let Some(csv) = flag(args, "--trivials") {
        let text = read(&csv)?;
        for line in text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() >= 2 {
                if let Ok(t) = f[1].trim().parse::<u16>() {
                    w.put(
                        format!("trivial/{}", f[0]),
                        &serde_json::json!({
                            "item": f[0],
                            "trivial": t,
                            "skill": f.get(2).unwrap_or(&"").trim(),
                            "how": "pinned by the can-no-longer-advance line in a combat log",
                        }),
                    );
                    trivials += 1;
                }
            }
        }
        w.source(format!("measured trivials from {}", csv.display()));
    }

    let (bytes, manifest) = w.finish(&today());
    let named = out.replace(".grim", &format!("-{}.grim", &manifest.content_hash[..12]));
    std::fs::write(&named, &bytes).map_err(|e| format!("{named}: {e}"))?;
    let mpath = format!("{named}.manifest.json");
    std::fs::write(&mpath, serde_json::to_vec_pretty(&manifest).unwrap())
        .map_err(|e| format!("{mpath}: {e}"))?;

    println!("{named}");
    println!(
        "  {} records — {n} recipes, {items} items, {trivials} measured trivials — {} bytes",
        manifest.records,
        bytes.len()
    );
    println!("  {}", manifest.content_hash);
    println!("  manifest -> {mpath}");
    Ok(())
}

pub fn check(args: &[String]) -> Result<(), String> {
    let path = positionals(args)
        .first()
        .map(|s| s.to_string())
        .ok_or("which artifact?")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let total = bytes.len();

    let hash = grimoire_corpus::hash::hex(&bytes[..total - grimoire_corpus::FOOTER_LEN]);
    let r = Reader::open(InMemory(bytes)).map_err(|e| e.to_string())?;

    println!("{path}");
    println!(
        "  {total} bytes, {} records in {} index blocks",
        r.len().map_err(|e| e.to_string())?,
        r.index_blocks()
    );
    println!("  content hash {hash}");
    if let Ok(text) = std::fs::read_to_string(format!("{path}.manifest.json")) {
        if let Ok(m) = serde_json::from_str::<Manifest>(&text) {
            let ok = m.content_hash == hash;
            println!(
                "  manifest says {} — {}",
                &m.content_hash[..12],
                if ok { "matches" } else { "DOES NOT MATCH" }
            );
            if !ok {
                return Err("the artifact does not match its manifest".into());
            }
            for p in &m.provenance {
                println!("    from {p}");
            }
        }
    }
    let recipes = r.prefix("recipe/").map_err(|e| e.to_string())?.len();
    println!("  {recipes} recipes");
    Ok(())
}

/// No date crate for a stamp in a manifest.
fn today() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("unix:{}", d.as_secs()))
        .unwrap_or_else(|_| "unknown".into())
}
