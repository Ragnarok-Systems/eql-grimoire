//! `/outputfile inventory`.
//!
//! The dump is a tab-separated table and — this is the part that matters — it carries the
//! **item id**, not just the name:
//!
//! ```text
//! Location	Name	ID	Count	Slots
//! Any Slot	Bladestopper +4	11632	1	10
//! Any Slot-Slot8	Bladestopper (Exaltation)	11632	1	10
//! Ear	Black Sapphire Electrum Earring +4	14701	1	10
//! Head-Slot2	Empty	0	0	0
//! ```
//!
//! The id is the join key to the corpus, so a checklist can tick itself and a quote can know
//! you already hold three of the five parts.
//!
//! Three EQL-specific things the format hides, none of them documented anywhere — they came
//! out of reading a real dump:
//!
//! - `Name +4` is an **upgrade level**, not part of the item's name. `Bladestopper +4` and
//!   `Bladestopper` share id 11632.
//! - `Location-SlotN` is an **exaltation socket** inside the item in `Location`, not a
//!   container slot. Counting sockets as inventory double-counts gear.
//! - **There is a second table.** After the inventory, the dump emits a keyring with a
//!   different header and only three columns:
//!
//!   ```text
//!   KeyRing	Name	ID
//!   Augmentation	Earthshaker (Exaltation)	5667
//!   Activated	Guise of the Deceiver	2469
//!   Equipment	Thorny Vine Bracer +3	4894
//!   ```
//!
//!   These are things you have *collected*, not things you are carrying, so they must not be
//!   counted as stock — but they are exactly what a collection checklist wants.
//!
//! Parsed here and kept here. The dump never leaves the machine.

#![allow(clippy::tabs_in_doc_comments)] // the samples above are a real TSV

/// Where an item sits.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Place {
    /// Worn, or in a top-level inventory slot.
    Worn(String),
    /// Inside a container in that slot.
    Bag { slot: String, index: u16 },
    /// Socketed into the item in that slot.
    Socket { slot: String, index: u16 },
}

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Held {
    pub place: Place,
    /// Name with the upgrade suffix removed.
    pub name: String,
    pub id: u32,
    pub count: u32,
    /// The `+n` upgrade level, when present.
    pub upgrade: u8,
    /// True when the name carried `(Exaltation)`.
    pub exaltation: bool,
}

/// An entry on the keyring — collected, not carried.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Collected {
    /// `Augmentation`, `Activated`, `Equipment`.
    pub category: String,
    pub name: String,
    pub id: u32,
    pub upgrade: u8,
    pub exaltation: bool,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Inventory {
    pub held: Vec<Held>,
    /// The keyring: collected augmentations, clickies and equipment.
    pub collected: Vec<Collected>,
    /// Rows that were not `Empty` and could not be read. Non-zero means the format moved.
    pub unreadable: u32,
}

impl Inventory {
    /// How many of an item you hold, ignoring sockets.
    ///
    /// Sockets are excluded because an exaltation stone fused into a bracer is not a thing
    /// you can hand a crafter.
    pub fn count_of(&self, id: u32) -> u32 {
        self.held
            .iter()
            .filter(|h| h.id == id && !matches!(h.place, Place::Socket { .. }))
            .map(|h| h.count)
            .sum()
    }

    pub fn has(&self, id: u32) -> bool {
        self.count_of(id) > 0
    }

    /// Distinct item ids held, sockets excluded.
    pub fn ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self
            .held
            .iter()
            .filter(|h| !matches!(h.place, Place::Socket { .. }))
            .map(|h| h.id)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Slot prefixes whose `-SlotN` children are exaltation sockets rather than bag contents.
///
/// Equipment slots hold sockets; `General`/bank slots hold bags. Getting this backwards
/// double-counts every piece of worn gear.
const EQUIPMENT: &[&str] = &[
    "Any Slot",
    "Ear",
    "Head",
    "Face",
    "Neck",
    "Shoulders",
    "Arms",
    "Back",
    "Wrist",
    "Range",
    "Hands",
    "Primary",
    "Secondary",
    "Fingers",
    "Chest",
    "Legs",
    "Feet",
    "Waist",
    "Ammo",
    "Charm",
    "Power Source",
];

pub fn parse(dump: &str) -> Inventory {
    let mut inv = Inventory::default();
    let mut keyring = false;

    for raw in dump.lines() {
        let line = raw.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }
        // Either table's header line. The keyring header also switches the parser over.
        if line.starts_with("Location\t") {
            keyring = false;
            continue;
        }
        if line.starts_with("KeyRing\t") {
            keyring = true;
            continue;
        }

        if keyring {
            let mut f = line.split('\t');
            let (Some(category), Some(name), Some(id)) = (f.next(), f.next(), f.next()) else {
                inv.unreadable += 1;
                continue;
            };
            let Ok(id) = id.trim().parse::<u32>() else {
                inv.unreadable += 1;
                continue;
            };
            let (name, upgrade, exaltation) = strip_suffixes(name);
            inv.collected.push(Collected {
                category: category.to_string(),
                name,
                id,
                upgrade,
                exaltation,
            });
            continue;
        }

        let mut f = line.split('\t');
        let (Some(loc), Some(name), Some(id), Some(count)) =
            (f.next(), f.next(), f.next(), f.next())
        else {
            inv.unreadable += 1;
            continue;
        };
        // The game writes a full row of zeroes for an empty slot.
        if name == "Empty" {
            continue;
        }
        let (Ok(id), Ok(count)) = (id.trim().parse::<u32>(), count.trim().parse::<u32>()) else {
            inv.unreadable += 1;
            continue;
        };
        if id == 0 {
            continue;
        }
        let (name, upgrade, exaltation) = strip_suffixes(name);
        inv.held.push(Held {
            place: place_of(loc),
            name,
            id,
            count,
            upgrade,
            exaltation,
        });
    }
    inv
}

fn place_of(loc: &str) -> Place {
    match loc.split_once("-Slot") {
        Some((base, idx)) => {
            let index = idx.parse().unwrap_or(0);
            if EQUIPMENT.contains(&base) {
                Place::Socket {
                    slot: base.to_string(),
                    index,
                }
            } else {
                Place::Bag {
                    slot: base.to_string(),
                    index,
                }
            }
        }
        None => Place::Worn(loc.to_string()),
    }
}

/// `Bladestopper (Exaltation) +4` → `("Bladestopper", 4, true)`.
fn strip_suffixes(raw: &str) -> (String, u8, bool) {
    let mut s = raw.trim();
    let mut upgrade = 0u8;
    // `+n` is always last when present.
    if let Some((head, tail)) = s.rsplit_once(" +") {
        if let Ok(n) = tail.parse::<u8>() {
            upgrade = n;
            s = head;
        }
    }
    let exaltation = s.ends_with(" (Exaltation)");
    if exaltation {
        s = s.trim_end_matches(" (Exaltation)");
    }
    (s.to_string(), upgrade, exaltation)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = "Location\tName\tID\tCount\tSlots\n\
Any Slot\tBladestopper +4\t11632\t1\t10\n\
Any Slot-Slot2\tEmpty\t0\t0\t0\n\
Any Slot-Slot8\tBladestopper (Exaltation)\t11632\t1\t10\n\
Ear\tBlack Sapphire Electrum Earring +4\t14701\t1\t10\n\
Head\tSkull-Shaped Barbute +6\t4301\t1\t10\n\
Face-Slot7\tPolished Mithril Mask (Exaltation)\t4505\t1\t10\n\
General1\tLarge Bag\t17\t1\t10\n\
General1-Slot1\tWater Flask\t13102\t12\t0\n";

    #[test]
    fn reads_the_real_dump() {
        let inv = parse(REAL);
        assert_eq!(inv.unreadable, 0);
        assert_eq!(inv.held.len(), 7, "one Empty row should have been skipped");
    }

    #[test]
    fn upgrade_levels_are_not_part_of_the_name() {
        let inv = parse(REAL);
        let b = inv.held.iter().find(|h| h.id == 11632).unwrap();
        assert_eq!(b.name, "Bladestopper");
        assert_eq!(b.upgrade, 4);
    }

    #[test]
    fn a_socketed_exaltation_is_not_a_second_copy_of_the_item() {
        // Bladestopper appears twice: worn at +4, and as an exaltation in a socket. If the
        // socket counted, the app would tell you that you own two.
        let inv = parse(REAL);
        assert_eq!(inv.count_of(11632), 1);
        assert!(inv
            .held
            .iter()
            .any(|h| matches!(h.place, Place::Socket { .. }) && h.exaltation));
    }

    #[test]
    fn bag_contents_are_bags_not_sockets() {
        let inv = parse(REAL);
        let flask = inv.held.iter().find(|h| h.id == 13102).unwrap();
        assert!(
            matches!(&flask.place, Place::Bag { slot, index } if slot == "General1" && *index == 1),
            "{:?}",
            flask.place
        );
        assert_eq!(inv.count_of(13102), 12, "stack size must be respected");
    }

    #[test]
    fn empty_slots_and_zero_ids_vanish() {
        let inv = parse("Location\tName\tID\tCount\tSlots\nHead-Slot2\tEmpty\t0\t0\t0\n");
        assert!(inv.held.is_empty());
        assert_eq!(inv.unreadable, 0, "an empty slot is not a parse failure");
    }

    #[test]
    fn a_changed_format_is_reported_rather_than_silently_dropped() {
        let inv = parse("Location\tName\tID\tCount\tSlots\nEar\tThing\tnot-a-number\t1\t0\n");
        assert!(inv.held.is_empty());
        assert_eq!(inv.unreadable, 1);
    }

    #[test]
    fn ids_are_unique_and_sorted() {
        assert_eq!(parse(REAL).ids(), vec![17, 4301, 11632, 13102, 14701]);
    }

    /// The second table, exactly as the game writes it — CRLF, trailing tab on the header,
    /// three columns instead of five.
    const WITH_KEYRING: &str = "Location\tName\tID\tCount\tSlots\r\n\
Ear\tBlack Sapphire Electrum Earring +4\t14701\t1\t10\r\n\
\r\n\
KeyRing\tName\tID\t\r\n\
Augmentation\tEarthshaker (Exaltation)\t5667\r\n\
Activated\tGuise of the Deceiver\t2469\r\n\
Equipment\tThorny Vine Bracer +3\t4894\r\n";

    #[test]
    fn the_keyring_table_is_read_not_reported_as_damage() {
        let inv = parse(WITH_KEYRING);
        assert_eq!(
            inv.unreadable, 0,
            "the keyring is a real section, not corruption"
        );
        assert_eq!(inv.held.len(), 1);
        assert_eq!(inv.collected.len(), 3);
    }

    #[test]
    fn collected_things_are_not_stock() {
        // You have "collected" Earthshaker as an augmentation; that does not mean you have
        // one to hand a crafter.
        let inv = parse(WITH_KEYRING);
        assert_eq!(inv.count_of(5667), 0);
        assert!(!inv.ids().contains(&5667));
        assert!(inv.collected.iter().any(|c| c.id == 5667 && c.exaltation));
    }

    #[test]
    fn keyring_names_get_the_same_treatment_as_inventory_names() {
        let inv = parse(WITH_KEYRING);
        let bracer = inv.collected.iter().find(|c| c.id == 4894).unwrap();
        assert_eq!(bracer.name, "Thorny Vine Bracer");
        assert_eq!(bracer.upgrade, 3);
        assert_eq!(bracer.category, "Equipment");
    }

    #[test]
    fn a_plus_in_a_real_name_is_not_eaten() {
        let (name, up, _) = strip_suffixes("Potion of Fire +Resist");
        assert_eq!(name, "Potion of Fire +Resist");
        assert_eq!(up, 0);
    }

    #[test]
    fn empty_input_is_an_empty_inventory() {
        assert_eq!(parse(""), Inventory::default());
        assert_eq!(
            parse("Location\tName\tID\tCount\tSlots\n"),
            Inventory::default()
        );
    }
}
