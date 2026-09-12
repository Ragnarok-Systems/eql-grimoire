//! WHAT CLASSES A CHARACTER HAS BEEN SEEN USING, read out of the spells they cast.
//!
//! # THE LOG NAMES OTHER PEOPLE'S SPELLS, WHICH IS THE WHOLE REASON THIS IS POSSIBLE
//!
//! `Poguhy begins casting Shield of Barbs.` The client prints the caster and the spell by name for
//! everybody, not just the reader, and `combat::Event::CastStart` has carried both since the
//! grammar was written. Nothing read it. The corpus supplies the other half: `Spell::classes` is
//! the wiki's `(class, level)` table, present on 1,460 of 2,001 records.
//!
//! # IT IS A COVERING PROBLEM AND NOT AN INTERSECTION, BECAUSE A CHARACTER IS A TRIO
//!
//! EverQuest Legends characters are three classes at once, which is what `screens::exalt`'s
//! `usable_by(cls, trio)` has always been about. The first version of this reasoning intersected
//! the class lists of everything a person cast, and on the reference capture two of the six casters
//! came out EMPTY:
//!
//! ```text
//! Poguhy   Cascade of Hail (Druid)   Rain of Blades (Magician)   =>  no class casts both
//! Rykabe   Flame Bolt (Magician)     Light Healing (six classes) =>  no class casts both
//! ```
//!
//! That is not a contradiction and it is not bad data. Poguhy is a Druid AND a Magician, and the
//! empty answer was the model being wrong rather than the log. So the question this module asks is
//! not "which one class explains everything" but "which classes has this person been PROVED to
//! have", and the answer has room for three.
//!
//! # WHAT COUNTS AS PROOF
//!
//! A spell whose class list has exactly ONE class in it proves that class, full stop: nobody else
//! can cast it. That is [`Seen::certain`], and on the capture it resolves Wizard for Fylasem,
//! Enchanter for Losumyda, and Druid AND Magician for Poguhy.
//!
//! A spell with several classes on it proves nothing ON ITS OWN. If one of the certain classes can
//! already cast it, it is explained and adds nothing. If NOT, then this person has at least one
//! more class out of that list, which is a real narrowing and is [`Seen::narrowed`]: on the capture
//! it says Rykabe is a Magician plus at least one of six.
//!
//! NOTHING HERE GUESSES THE THIRD SLOT. A trio has three places and a quiet night fills one; a
//! character seen casting nothing at all gets no answer, which is the honest one.
//!
//! # WHY THE CORPUS'S GAPS DO NOT BECOME WRONG ANSWERS
//!
//! 541 of 2,001 records carry no class list, and pet attacks (`Fire Elemental Attack`) and some
//! abilities (`Lay on Hands III`) are among them. A spell this module cannot look up is SKIPPED
//! rather than counted as evidence of nothing, and [`Seen::unlisted`] keeps the names so a screen
//! can say the reading is partial instead of implying it is complete.
use crate::data::Spell;
use std::collections::{BTreeMap, BTreeSet};

/// WHAT ONE CHARACTER HAS BEEN SEEN TO BE.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Seen {
    /// Classes proved outright, by a spell only that class can cast. Sorted, deduplicated.
    pub certain: Vec<String>,
    /// Each entry is a class list, none of whose classes is in [`Seen::certain`], from a spell this
    /// character cast. It says "at least one more class, out of these". Sorted, deduplicated.
    pub narrowed: Vec<Vec<String>>,
    /// Spells cast that the corpus has no class list for, so the reading is partial and says so.
    pub unlisted: Vec<String>,
}

impl Seen {
    /// Nothing was proved and nothing was narrowed: the log has not shown this person casting.
    pub fn is_empty(&self) -> bool {
        self.certain.is_empty() && self.narrowed.is_empty()
    }

    /// WHAT A ROW PUTS BESIDE A NAME, or `None` when there is nothing to say.
    ///
    /// A NAME AND A COUNT, never a guess. `Druid/Magician` when two are proved; `Magician +1` when
    /// one is proved and something else is narrowed but not settled.
    pub fn tag(&self) -> Option<String> {
        if self.certain.is_empty() {
            /* NOTHING PROVED. A narrowing alone is not a class and must not be printed as one:
             * "one of six" beside a name reads as a class nobody has. */
            return None;
        }
        let mut out = self.certain.join("/");
        if !self.narrowed.is_empty() {
            out.push_str(" +1");
        }
        Some(out)
    }
}

/// EVERY CASTER THE LOG HAS NAMED, AND WHAT EACH ONE'S SPELLS PROVE.
///
/// A `BTreeMap` rather than a `HashMap`, so two runs over one log answer in one order. This is read
/// by a renderer and an order that moved between frames would move the rows with it.
#[derive(Default)]
pub struct Book {
    /// Caster to the spell names seen from them, in the log's own spelling.
    casts: BTreeMap<String, BTreeSet<String>>,
}

impl Book {
    /// Record `who begins casting what`. Cheap and idempotent: a spell cast fifty times is one fact.
    pub fn saw(&mut self, who: &str, spell: &str) {
        if who.is_empty() || spell.is_empty() {
            return;
        }
        self.casts
            .entry(who.to_owned())
            .or_default()
            .insert(spell.to_owned());
    }

    /// How many casters have been seen at all.
    pub fn len(&self) -> usize {
        self.casts.len()
    }

    /// Whether anything has been seen.
    pub fn is_empty(&self) -> bool {
        self.casts.is_empty()
    }

    /// WHAT THIS CHARACTER'S SPELLS PROVE, against a corpus.
    ///
    /// `None` when the log has not seen them cast. An empty [`Seen`] is a different answer and means
    /// they cast only spells the corpus cannot place.
    pub fn of(&self, who: &str, spells: &[Spell]) -> Option<Seen> {
        let seen = self.casts.get(who)?;
        Some(read(seen.iter().map(String::as_str), spells))
    }

    /// Every caster, in the log's own spelling, in a stable order.
    pub fn casters(&self) -> impl Iterator<Item = &str> {
        self.casts.keys().map(String::as_str)
    }

    /// HOW MANY DISTINCT SPELLS WERE CAST THAT THE CORPUS CANNOT PLACE.
    ///
    /// THE DENOMINATOR OF THE WHOLE READING, and the Logs page is where it belongs: that page's
    /// subject is what was read and what was skipped, and a class tag missing from a table is
    /// otherwise indistinguishable from a person who cast nothing.
    ///
    /// DISTINCT SPELLS AND NOT CASTS. A pet attack fired two hundred times is one gap in the
    /// corpus, not two hundred.
    pub fn unplaceable(&self, spells: &[Spell]) -> usize {
        let mut names: BTreeSet<&str> = BTreeSet::new();
        for who in self.casts.keys() {
            let Some(seen) = self.of(who, spells) else {
                continue;
            };
            for gap in &seen.unlisted {
                /* Borrowed off the book rather than the reading, which is dropped here. */
                if let Some(kept) = self.casts[who].get(gap.as_str()) {
                    names.insert(kept.as_str());
                }
            }
        }
        names.len()
    }
}

/// THE COLOUR A CLASS IS DRAWN IN, or `None` for a name this table does not know.
///
/// # WHY A NAME MAY BE COLOURED WHEN A RANK MUST NOT BE HASHED
///
/// `dps::RAMP` colours a row BY RANK and its doc gives the reason: "hashing a character name
/// to a hue would be this app inventing a fact about a person". That argument still stands and
/// this does not break it. A class is not a hash of a name; it is READ from the spells that
/// character was seen casting, and this table is a LEGEND for it, the way a chart's key is not
/// a claim about the data.
///
/// THE OWNER'S COMPLAINT IS WHAT THIS ANSWERS. Four coloured names and a grey tail, because
/// the rank ramp is four colours deep and everything past it shares one tint. Rank is also the
/// wrong thing to colour by on a live meter: it moves under the reader mid-pull, so a person
/// he has learned to find by colour changes colour by overtaking somebody.
///
/// EACH CLASS GETS ONE COLOUR AND NO TWO SHARE ONE, which is the only property that matters
/// for reading a table at a glance and is what `no_two_classes_share_a_colour` holds. The hues
/// follow the archetypes so a reader guesses right before he has learned the key: melee warm,
/// priests gold and green, casters blue and violet.
pub fn colour(class: &str) -> Option<egui::Color32> {
    let c = |r: u8, g: u8, b: u8| Some(egui::Color32::from_rgb(r, g, b));
    match class {
        /* MELEE, WARM. */
        "Warrior" => c(0xC7, 0x4A, 0x3C),
        "Berserker" => c(0xA8, 0x3A, 0x2E),
        "Monk" => c(0xD4, 0x7A, 0x3C),
        "Rogue" => c(0x8C, 0x6E, 0x4A),
        /* HYBRIDS, BETWEEN THE TWO. */
        "Paladin" => c(0xE0, 0xC0, 0x70),
        "Shadowknight" | "Shadow Knight" => c(0x7A, 0x4A, 0x7A),
        "Ranger" => c(0x6E, 0x9E, 0x54),
        "Bard" => c(0xC8, 0x8A, 0xC8),
        "Beastlord" => c(0x8A, 0xA8, 0x6E),
        /* PRIESTS. */
        "Cleric" => c(0xD4, 0xA3, 0x3C),
        "Druid" => c(0x5A, 0xA9, 0x6E),
        "Shaman" => c(0x4A, 0x9E, 0x8E),
        /* CASTERS, COOL. */
        "Wizard" => c(0x4A, 0x9E, 0xD8),
        "Magician" => c(0xD8, 0x5A, 0x4A),
        "Enchanter" => c(0x9E, 0x8A, 0xD8),
        "Necromancer" => c(0x6E, 0x8A, 0x5A),
        _ => None,
    }
}

/// THE COLOUR FOR A ROW'S TAG, which is the FIRST class proved and not a blend.
///
/// A TRIO IS SEVERAL CLASSES AND A ROW IS ONE COLOUR, so something has to choose. The first in
/// the sorted list is the choice, for the reason every tie in this app is broken on the name:
/// it is stable, it does not move as more evidence arrives for the OTHER slots, and it does not
/// depend on which spell the reader happened to cast first.
pub fn tag_colour(tag: &str) -> Option<egui::Color32> {
    colour(tag.split(['/', ' ']).next()?)
}

/// THE READING ITSELF, over a set of spell names.
///
/// SEPARATE FROM [`Book`] SO A TEST CAN DRIVE IT with a handful of names and no log at all, which
/// is the only reason the intersection mistake was caught: the shape of the answer is what was
/// wrong, and that is visible in six spell names.
pub fn read<'a>(cast: impl Iterator<Item = &'a str>, spells: &[Spell]) -> Seen {
    let mut certain: BTreeSet<String> = BTreeSet::new();
    let mut multi: Vec<Vec<String>> = Vec::new();
    let mut unlisted: BTreeSet<String> = BTreeSet::new();

    for name in cast {
        let Some(rec) = find(name, spells) else {
            unlisted.insert(name.to_owned());
            continue;
        };
        let classes: Vec<String> = rec.classes.iter().map(|(c, _)| c.clone()).collect();
        match classes.len() {
            /* A SPELL THE CORPUS HAS BUT LISTS NO CLASS FOR is the same evidence as one it does
             * not have: none. `Spell::note` is where the page said "cast by NPCs only". */
            0 => {
                unlisted.insert(name.to_owned());
            }
            /* ONE CLASS IS A PROOF. Nobody else can cast it. */
            1 => {
                certain.insert(classes[0].clone());
            }
            _ => multi.push(classes),
        }
    }

    /* A MULTI-CLASS SPELL EXPLAINED BY A CLASS ALREADY PROVED ADDS NOTHING, and this pass has to
     * happen AFTER every single-class spell has been read: a list is only redundant once the
     * proofs are all in, and doing it inline would keep or drop it depending on the order the log
     * happened to print the casts in. */
    let mut narrowed: Vec<Vec<String>> = Vec::new();
    for list in multi {
        if list.iter().any(|c| certain.contains(c)) {
            continue;
        }
        let mut sorted = list;
        sorted.sort();
        sorted.dedup();
        if !narrowed.contains(&sorted) {
            narrowed.push(sorted);
        }
    }
    narrowed.sort();

    Seen {
        certain: certain.into_iter().collect(),
        narrowed,
        unlisted: unlisted.into_iter().collect(),
    }
}

/// The corpus record for a spell the log named, matched the way the log writes it.
///
/// CASE FOLDED AND NOTHING ELSE. The client and the wiki agree on spelling and punctuation; what
/// they do not always agree on is capitalisation, and a looser match would start pairing
/// `Shield of Barbs` with `Shield of Brambles`.
fn find<'a>(name: &str, spells: &'a [Spell]) -> Option<&'a Spell> {
    spells.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spell(name: &str, classes: &[&str]) -> Spell {
        Spell {
            name: name.to_owned(),
            classes: classes
                .iter()
                .map(|c| ((*c).to_owned(), Some(1.0)))
                .collect(),
            ..Spell::default()
        }
    }

    fn corpus() -> Vec<Spell> {
        vec![
            spell("Cascade of Hail", &["Druid"]),
            spell("Rain of Blades", &["Magician"]),
            spell("Flame Bolt", &["Magician"]),
            spell(
                "Light Healing",
                &[
                    "Cleric",
                    "Druid",
                    "Shaman",
                    "Paladin",
                    "Beastlord",
                    "Ranger",
                ],
            ),
            spell("Fire Elemental Attack", &[]),
        ]
    }

    /// DEFECT: READING A TRIO AS ONE CLASS, WHICH ANSWERS NOTHING AT ALL.
    ///
    /// The first version of this intersected the class lists. On the reference capture that made
    /// two of six casters come out EMPTY: Poguhy casts `Cascade of Hail` (Druid only) and `Rain of
    /// Blades` (Magician only), and no class casts both. The empty answer was the model being
    /// wrong, not the log: an EverQuest Legends character is three classes at once, which is what
    /// `exalt::usable_by(cls, trio)` has always been about.
    ///
    /// WHAT MUTATION MAKES THIS RED: intersecting instead of covering.
    #[test]
    fn a_character_who_casts_two_classes_worth_of_spells_is_read_as_both() {
        let got = read(["Cascade of Hail", "Rain of Blades"].into_iter(), &corpus());
        assert_eq!(got.certain, vec!["Druid", "Magician"]);
        assert!(
            got.narrowed.is_empty(),
            "both spells are single-class, so nothing is left to narrow"
        );
        assert_eq!(got.tag().as_deref(), Some("Druid/Magician"));
    }

    /// A SPELL SEVERAL CLASSES CAN CAST PROVES NOTHING ON ITS OWN.
    ///
    /// Rykabe's case from the capture: `Flame Bolt` proves Magician, and `Light Healing` is not
    /// something a Magician can cast, so he has at least one more class out of the six. The tag
    /// says `Magician +1` and never picks one of the six.
    ///
    /// WHAT MUTATION MAKES THIS RED: promoting a multi-class list into `certain`.
    #[test]
    fn a_spell_many_classes_share_narrows_and_never_decides() {
        let got = read(["Flame Bolt", "Light Healing"].into_iter(), &corpus());
        assert_eq!(got.certain, vec!["Magician"]);
        assert_eq!(got.narrowed.len(), 1, "{got:?}");
        assert_eq!(got.narrowed[0].len(), 6);
        assert_eq!(got.tag().as_deref(), Some("Magician +1"));
    }

    /// AND A NARROWING A PROVED CLASS ALREADY EXPLAINS IS NOT A NARROWING.
    ///
    /// Tanefilo's shape: a Druid casting `Light Healing` has told us nothing new, because Druid is
    /// on that spell's own list. Printing `+1` there would invent a fourth class.
    ///
    /// AND THE ORDER OF THE CASTS MUST NOT DECIDE IT, which is why the redundancy pass runs after
    /// every single-class spell has been read rather than inline.
    ///
    /// WHAT MUTATION MAKES THIS RED: doing the `certain.contains` check inside the first loop.
    #[test]
    fn a_narrowing_an_already_proved_class_explains_is_dropped_whatever_the_order() {
        for order in [
            ["Cascade of Hail", "Light Healing"],
            ["Light Healing", "Cascade of Hail"],
        ] {
            let got = read(order.into_iter(), &corpus());
            assert_eq!(got.certain, vec!["Druid"], "{order:?}");
            assert!(
                got.narrowed.is_empty(),
                "a Druid casting a Druid spell was read as a second class: {got:?} from {order:?}"
            );
            assert_eq!(got.tag().as_deref(), Some("Druid"));
        }
    }

    /// A SPELL THE CORPUS CANNOT PLACE IS NOT EVIDENCE OF ANYTHING.
    ///
    /// 541 of 2,001 records carry no class list, and pet attacks are among them. Counting one as a
    /// class of its own, or letting it empty an answer, would both be inventions.
    #[test]
    fn a_spell_with_no_class_list_is_kept_as_a_gap_and_not_read_as_a_class() {
        let got = read(
            ["Fire Elemental Attack", "Cascade of Hail"].into_iter(),
            &corpus(),
        );
        assert_eq!(got.certain, vec!["Druid"]);
        assert_eq!(got.unlisted, vec!["Fire Elemental Attack"]);
        assert!(got.narrowed.is_empty());

        /* AND ONE ON ITS OWN LEAVES NOTHING TO SAY. */
        let alone = read(["Fire Elemental Attack"].into_iter(), &corpus());
        assert!(alone.is_empty(), "{alone:?}");
        assert_eq!(alone.tag(), None, "a gap was printed beside a name");
    }

    /// A NARROWING ALONE IS NOT A CLASS AND IS NEVER PRINTED AS ONE.
    ///
    /// Tanefilo casts `Light Healing` and nothing else the corpus places. Six classes could have
    /// done it and this app does not pick. `+1` with nothing in front of it would read as a class.
    #[test]
    fn a_row_with_nothing_proved_carries_no_tag() {
        let got = read(["Light Healing"].into_iter(), &corpus());
        assert!(got.certain.is_empty());
        assert_eq!(got.narrowed.len(), 1);
        assert_eq!(got.tag(), None, "a narrowing was printed as a class");
        assert!(!got.is_empty(), "it did narrow, which is worth keeping");
    }

    /// THE BOOK ANSWERS IN ONE ORDER, whatever order the log printed the casts in.
    #[test]
    fn two_runs_over_one_log_answer_the_same_way_round() {
        let c = corpus();
        let mut a = Book::default();
        a.saw("Poguhy", "Rain of Blades");
        a.saw("Poguhy", "Cascade of Hail");
        let mut b = Book::default();
        b.saw("Poguhy", "Cascade of Hail");
        b.saw("Poguhy", "Rain of Blades");
        assert_eq!(a.of("Poguhy", &c), b.of("Poguhy", &c));
        assert_eq!(a.of("Nobody", &c), None, "an unseen name has no reading");
    }

    /// DEFECT: TWO CLASSES DRAWN IN ONE COLOUR, which makes the key worse than no key.
    ///
    /// The whole value of colouring a name by class is that a reader learns the key once and then
    /// finds people without reading. Two classes sharing a hue means he learns something false and
    /// keeps using it.
    ///
    /// AND NONE OF THEM MAY BE THE READER'S GOLD. `Who::You` keeps `GOLD_HI` because his own row
    /// is the one a person looks for before reading any number; a class that came out the same
    /// colour would make somebody else look like him at a glance.
    ///
    /// WHAT MUTATION MAKES THIS RED: giving two classes the same triple.
    #[test]
    fn no_two_classes_share_a_colour() {
        /* EVERY CLASS THIS APP CAN READ, which is every class the corpus names. */
        const ALL: &[&str] = &[
            "Warrior",
            "Berserker",
            "Monk",
            "Rogue",
            "Paladin",
            "Shadowknight",
            "Ranger",
            "Bard",
            "Beastlord",
            "Cleric",
            "Druid",
            "Shaman",
            "Wizard",
            "Magician",
            "Enchanter",
            "Necromancer",
        ];

        let mut seen: Vec<(&str, [u8; 4])> = Vec::new();
        for c in ALL {
            let got = colour(c).unwrap_or_else(|| panic!("{c} has no colour"));
            if let Some((other, _)) = seen.iter().find(|(_, v)| *v == got.to_array()) {
                panic!("{c} and {other} are drawn in the same colour");
            }
            seen.push((c, got.to_array()));
        }

        /* AND NOT THE READER'S OWN GOLD. */
        for (c, v) in &seen {
            assert_ne!(
                *v,
                crate::theme::GOLD_HI.to_array(),
                "{c} is drawn in the colour reserved for the reader's own row"
            );
        }

        /* A NAME THE TABLE DOES NOT KNOW GETS NOTHING RATHER THAN A DEFAULT, or an unrecognised
         * class would silently join whichever colour the fallback picked. */
        assert_eq!(colour("Bartender"), None);
    }

    /// A TRIO IS SEVERAL CLASSES AND A ROW IS ONE COLOUR, so the tag's first class decides.
    ///
    /// WHAT MUTATION MAKES THIS RED: splitting on something other than the tag's own separators,
    /// which would make `Druid/Magician` and `Magician +1` fall through to no colour at all.
    #[test]
    fn a_trio_takes_the_colour_of_the_first_class_it_proved() {
        assert_eq!(tag_colour("Druid/Magician"), colour("Druid"));
        assert_eq!(tag_colour("Magician +1"), colour("Magician"));
        assert_eq!(tag_colour("Wizard"), colour("Wizard"));
        assert_eq!(tag_colour(""), None);

        /* AND THE THREE SHAPES `Seen::tag` CAN PRODUCE ALL RESOLVE, which is the thing that would
         * break silently: a tag whose colour is None draws in the plain text tint and looks
         * exactly like a row whose class was never read. */
        let c = vec![
            spell("Cascade of Hail", &["Druid"]),
            spell("Rain of Blades", &["Magician"]),
            spell(
                "Light Healing",
                &[
                    "Cleric",
                    "Druid",
                    "Shaman",
                    "Paladin",
                    "Beastlord",
                    "Ranger",
                ],
            ),
        ];
        for cast in [
            vec!["Cascade of Hail"],
            vec!["Cascade of Hail", "Rain of Blades"],
            vec!["Rain of Blades", "Light Healing"],
        ] {
            let tag = read(cast.iter().copied(), &c)
                .tag()
                .unwrap_or_else(|| panic!("{cast:?} proved nothing"));
            assert!(
                tag_colour(&tag).is_some(),
                "the tag {tag:?} draws in no colour, so it reads as a row with no class"
            );
        }
    }
}
