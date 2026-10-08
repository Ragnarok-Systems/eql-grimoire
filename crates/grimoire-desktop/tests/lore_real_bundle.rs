//! Loads a real wiki bundle when `GRIMOIRE_LORE` names one, and checks the cross-links the
//! Compendium draws resolve. Skips (passes) when it is not set, so CI needs no bundle.

use grimoire_desktop::lore::{Kind, Lore};

#[test]
fn a_real_bundle_loads_and_its_links_resolve() {
    let Ok(root) = std::env::var("GRIMOIRE_LORE") else {
        eprintln!("GRIMOIRE_LORE not set; skipped");
        return;
    };
    let t = std::time::Instant::now();
    let lore = Lore::load(std::path::Path::new(&root)).expect("the bundle loads");
    eprintln!("loaded in {:?}", t.elapsed());
    for k in Kind::ALL {
        assert!(lore.count(k) > 0, "{k:?} is empty");
        eprintln!("{k:?}: {}", lore.count(k));
    }
    // An item's drop zones and mobs, and a zone's neighbours, mostly name records that exist.
    let (mut seen, mut found) = (0, 0);
    for r in lore.records(Kind::Item) {
        if let Some(d) = r.fields.get("drops") {
            for (key, kind) in [("zones", Kind::Zone), ("mobs", Kind::Npc)] {
                for n in d.get(key).and_then(|v| v.as_array()).into_iter().flatten() {
                    if let Some(n) = n.as_str() {
                        seen += 1;
                        found += lore.find_in(kind, n).is_some() as usize;
                    }
                }
            }
        }
    }
    eprintln!("drop links resolving: {found}/{seen}");
    assert!(
        seen > 0 && found * 10 >= seen * 7,
        "only {found}/{seen} drop links resolve"
    );
    assert!(lore.find("Ail'Vorith").is_some());
}
