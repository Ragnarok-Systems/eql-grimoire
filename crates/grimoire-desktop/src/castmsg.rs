//! WHO A SPELL LANDED ON, RECOVERED FROM THE SENTENCE THE GAME PRINTED.
//!
//! # The finding this module exists for
//!
//! The log never names a spell when it lands. It prints flavour text: `Tanefilo has been diseased.`,
//! `A lurking mummy is bathed in fire.` The wiki records that sentence per spell as
//! `msg_cast_on_other`, written with `Someone` where the name goes:
//!
//! ```text
//!   wiki:   "Someone has been charmed."
//!   client: "a dry bone skeleton has been charmed."
//! ```
//!
//! So a landing line **names its target**, and that is the only evidence in the file for three
//! things this project had written off as impossible:
//!
//!   * WHICH MOB IS CHARMED, and therefore which mob is somebody's charm pet.
//!   * WHO A DAMAGE SHIELD WAS PUT ON. `Shield { effect }` credits the WEARER, and the cast line
//!     carries no target, so a shield you put on the tank counted as the tank's. The landing line
//!     names the tank.
//!   * WHEN A BUFF STARTED ON SOMEBODY ELSE. Only the start: there is no wears-off message for
//!     anyone but the reader, so an uptime for another person can be opened and never closed.
//!
//! Measured over the reference capture: 33 distinct lines resolve to a spell and a real name.
//!
//! # It is gated on the grammar, not on a heuristic
//!
//! A naive matcher over every line produces false positives. `Your endurance to magic fades.` is
//! caught by any spell whose message ends in ` fades.`, capturing `Your endurance to magic` as
//! though it were a name. Filtering that with a "does this look like a name" rule would be this app
//! guessing.
//!
//! It does not have to. `grimoire_parse` already classifies these lines, and the classification
//! separates them cleanly: measured in `cast_on_other.rs`, every fade line reads
//! [`Flavour::BuffFaded`] and every landing reads [`Flavour::LandedOnOther`],
//! [`Flavour::CrowdControl`] or [`Flavour::HealOnOther`]. So the resolver runs ONLY on the three
//! landing flavours and the fade lines are unreachable by construction rather than filtered out
//! afterwards. [`is_landing`] is that gate.
//!
//! # A match is a candidate SET, and saying so is the point
//!
//! Spells share sentences. `Someone blinks.` belongs to four different charm spells; 134 fade
//! strings are shared by more than one spell. [`Landed::candidates`] is therefore a list, and a
//! caller that wants one answer has to narrow it with something else it knows, such as which spell
//! the reader was seen casting a second earlier. Collapsing the list here, by picking the first or
//! the most common, would be inventing certainty the file does not carry.
use crate::data::Spell;
use grimoire_parse::combat::Flavour;
use std::collections::HashMap;

/// The placeholder the wiki writes where the game puts a name.
const PLACEHOLDER: &str = "Someone";

/// The shortest tail a pattern may have before it is refused.
///
/// A SPECIFICITY FLOOR, NOT A STYLE RULE. `Someone blinks.` leaves the six characters ` blinks.`,
/// which will match any sentence that happens to end that way; the shorter the tail the more of the
/// log it claims. Twelve is measured against the capture: it keeps every one of the 33 real
/// resolutions and is the point below which the tails stop being sentences.
const MIN_TAIL: usize = 12;

/// One spell's cast-on-other sentence, split at the placeholder.
struct Pattern {
    /// The spell's display name.
    spell: String,
    /// What follows the placeholder, whitespace-normalised.
    tail: String,
}

/// WHAT A LANDING LINE SAID: who it landed on, and which spells say that sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landed {
    /// The name the game substituted for the placeholder, exactly as the line spelled it.
    pub target: String,
    /// Every spell whose cast-on-other message is this sentence, in corpus order.
    ///
    /// A LIST BECAUSE THE LOG IS AMBIGUOUS, not because this is unfinished. Four charm spells share
    /// `Someone blinks.`; a caller narrows with what else it knows or shows the set.
    pub candidates: Vec<String>,
}

/// Every cast-on-other sentence in the corpus, indexed so a line can be resolved without walking
/// all of them.
///
/// INDEXED ON THE LAST WORD. There are about 1,300 patterns and a tail of the running log is tens of
/// millions of lines; comparing every pattern to every line is not a thing that can ship. The last
/// whitespace token of a sentence is cheap to take from a line and narrows the candidates to a
/// handful, which are then confirmed with a real suffix comparison.
pub struct Cast {
    by_last: HashMap<String, Vec<Pattern>>,
}

impl Cast {
    /// Build the index from the spell corpus. Spells with no cast-on-other message, or whose tail is
    /// under [`MIN_TAIL`], are left out.
    pub fn new(spells: &[Spell]) -> Cast {
        let mut by_last: HashMap<String, Vec<Pattern>> = HashMap::new();
        for s in spells {
            let Some(raw) = s.msg.other.as_deref() else {
                continue;
            };
            let Some(tail) = tail_of(raw) else { continue };
            let Some(last) = last_word(&tail) else {
                continue;
            };
            by_last.entry(last).or_default().push(Pattern {
                spell: s.name.clone(),
                tail,
            });
        }
        Cast { by_last }
    }

    /// How many patterns the index holds. For a test to prove it is not empty.
    pub fn len(&self) -> usize {
        self.by_last.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// RESOLVE ONE LINE, or answer nothing.
    ///
    /// `body` is the line with its timestamp already stripped. `flavour` is what the grammar made of
    /// it, and this refuses anything that is not a landing: see the module note.
    pub fn resolve(&self, body: &str, flavour: Flavour) -> Option<Landed> {
        if !is_landing(flavour) {
            return None;
        }
        let body = body.trim().trim_end_matches(char::is_whitespace);
        let last = last_word(body)?;
        let mut target: Option<&str> = None;
        let mut candidates = Vec::new();

        for p in self.by_last.get(&last)? {
            let Some(name) = body.strip_suffix(p.tail.as_str()) else {
                continue;
            };
            let name = name.trim();
            /* AN EMPTY NAME IS NOT A NAME. `Someone` at the very start of a sentence that IS the
             * whole line leaves nothing, which means the line was about the reader and not about a
             * third party at all. */
            if name.is_empty() {
                continue;
            }
            /* EVERY PATTERN THAT MATCHES MUST AGREE ABOUT THE NAME. Two spells whose tails differ in
             * length would carve the same line into two different names, and reporting one of them
             * would be a coin toss. */
            match target {
                None => target = Some(name),
                Some(t) if t == name => {}
                Some(_) => continue,
            }
            candidates.push(p.spell.clone());
        }

        let target = target?;
        if candidates.is_empty() {
            return None;
        }
        Some(Landed {
            target: target.to_owned(),
            candidates,
        })
    }
}

/// IS THIS FLAVOUR A SPELL LANDING ON SOMEBODY?
///
/// THE THREE ARE MEASURED AND NOT GUESSED. Running the capture's matched lines through the parser
/// (`grimoire-parse/tests/cast_on_other.rs`) puts them in exactly these three: `LandedOnOther` for a
/// plain buff or debuff, `CrowdControl` for a stun or snare landing, `HealOnOther` for a heal. Every
/// FADE line reads `BuffFaded`, which is why gating here removes the whole false-positive family
/// rather than filtering it.
///
/// `LandedOnSelf` IS DELIBERATELY OUT. The reader is not a third party and his own landings are
/// matched by `msg.you`, which carries no placeholder and needs no resolver.
pub fn is_landing(f: Flavour) -> bool {
    matches!(
        f,
        Flavour::LandedOnOther | Flavour::CrowdControl | Flavour::HealOnOther
    )
}

/// What follows the placeholder, with the wiki's stray spacing collapsed.
///
/// THE DOUBLE SPACE IS REAL. The wiki template renders the placeholder and the sentence with a gap
/// between them, so a great many records read `Someone  weakens.` A tail taken literally would never
/// match a client line, which puts exactly one space after the name.
fn tail_of(raw: &str) -> Option<String> {
    let at = raw.find(PLACEHOLDER)?;
    /* Only a sentence that STARTS with the placeholder can be split this way. One with the name in
     * the middle would need a two-sided match and none in the corpus does. */
    if raw[..at].trim().is_empty() {
        let rest = raw[at + PLACEHOLDER.len()..].trim_start();
        let tail = format!(" {rest}");
        if tail.len() >= MIN_TAIL {
            return Some(tail);
        }
    }
    None
}

/// The last whitespace-separated token, lowercased, for the index.
fn last_word(s: &str) -> Option<String> {
    s.split_whitespace().next_back().map(str::to_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::spells::Msg;

    fn spell(name: &str, other: &str) -> Spell {
        Spell {
            name: name.to_owned(),
            msg: Msg {
                you: None,
                other: Some(other.to_owned()),
                off: None,
            },
            ..Spell::default()
        }
    }

    /// DEFECT: the resolver running on a fade line and calling it a landing.
    ///
    /// This is the false positive the naive matcher produced: `Your endurance to magic fades.` is
    /// caught by any spell whose sentence ends in ` fades.`, and the captured "name" is
    /// `Your endurance to magic`. Gating on the grammar makes it unreachable.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `is_landing` check from `resolve`.
    #[test]
    fn a_fade_line_is_refused_however_well_it_matches() {
        let c = Cast::new(&[spell("Fade", "Someone fades away slowly.")]);
        let line = "Your endurance to magic fades away slowly.";

        assert_eq!(
            c.resolve(line, Flavour::BuffFaded),
            None,
            "a fade line resolved to a landing"
        );
        assert_eq!(c.resolve(line, Flavour::LandedOnSelf), None);

        /* And the same sentence IS resolved when the grammar calls it a landing, or the gate would
         * be refusing everything and this test would pass for the wrong reason. */
        assert!(c.resolve(line, Flavour::LandedOnOther).is_some());
    }

    /// DEFECT: reporting one spell for a sentence that several spells share.
    ///
    /// `Someone blinks.` belongs to four charm spells in the real corpus. Picking the first would
    /// name a spell the reader may never have cast.
    #[test]
    fn a_shared_sentence_answers_with_every_candidate() {
        let c = Cast::new(&[
            spell("Beguile Animals", "Someone blinks in confusion."),
            spell("Charm Animals", "Someone blinks in confusion."),
            spell("Allure of the Wild", "Someone blinks in confusion."),
        ]);
        let got = c
            .resolve(
                "a dry bone skeleton blinks in confusion.",
                Flavour::LandedOnOther,
            )
            .expect("resolves");
        assert_eq!(got.target, "a dry bone skeleton");
        assert_eq!(got.candidates.len(), 3, "{:?}", got.candidates);
    }

    /// DEFECT: the wiki's double space making every pattern unmatchable.
    ///
    /// A great many records read `Someone  weakens.` with two spaces, because of how the template
    /// renders. The client writes one. A tail taken literally would match nothing at all, and the
    /// whole feature would silently resolve zero lines while every test over synthetic single-spaced
    /// data passed.
    #[test]
    fn the_wikis_stray_spacing_does_not_stop_a_match() {
        let c = Cast::new(&[spell(
            "Abduction of Strength",
            "Someone  weakens noticeably.",
        )]);
        let got = c
            .resolve("Tanefilo weakens noticeably.", Flavour::LandedOnOther)
            .expect("the double space must not defeat the match");
        assert_eq!(got.target, "Tanefilo");
        assert_eq!(got.candidates, vec!["Abduction of Strength".to_owned()]);
    }

    /// DEFECT: a tail so short it claims unrelated sentences.
    ///
    /// The shorter the tail the more of the log a pattern owns. `Someone blinks.` leaves eight
    /// characters and would match anything ending that way.
    #[test]
    fn a_sentence_too_short_to_be_specific_is_not_indexed() {
        let c = Cast::new(&[spell("Blink", "Someone blinks.")]);
        assert!(
            c.is_empty(),
            "an eight character tail was indexed and will claim unrelated lines"
        );

        /* And one that is long enough is kept, so the floor is not simply refusing everything. */
        let ok = Cast::new(&[spell("Blaze", "Someone is bathed in fire.")]);
        assert_eq!(ok.len(), 1);
    }

    /// DEFECT: two patterns carving one line into two different names and one being picked.
    #[test]
    fn patterns_that_disagree_about_the_name_do_not_both_answer() {
        let c = Cast::new(&[
            spell("Long", "Someone the tall is bathed in fire."),
            spell("Short", "Someone is bathed in fire."),
        ]);
        let got = c
            .resolve(
                "Tanefilo the tall is bathed in fire.",
                Flavour::LandedOnOther,
            )
            .expect("resolves");
        /* Both tails match this line, carving it as `Tanefilo` and as `Tanefilo the tall`. The first
         * agreed name wins and the disagreeing pattern is dropped rather than averaged. */
        assert_eq!(got.candidates.len(), 1, "{:?}", got.candidates);
    }

    /// A line about the reader has no third party in it, so there is no name to take.
    #[test]
    fn a_sentence_with_nothing_before_the_tail_resolves_to_nobody() {
        let c = Cast::new(&[spell("Blaze", "Someone is bathed in fire.")]);
        assert_eq!(
            c.resolve("is bathed in fire.", Flavour::LandedOnOther),
            None
        );
    }
}

/* ------------------------------------------------------------------ the book -- */

/// WHAT IS CURRENTLY ON WHOM, kept as the log goes by.
///
/// # Two halves, because the log gives two different amounts of information
///
/// THE READER'S OWN EFFECTS CAN BE OPENED AND CLOSED. `msg.you` announces a landing on him and
/// `msg.off` announces it wearing off, so his uptime is a real interval with two ends.
///
/// SOMEBODY ELSE'S CAN ONLY BE OPENED. There is no wears-off sentence for a third party anywhere in
/// the corpus, so a buff seen landing on the tank has a start and no end. This type says so by
/// giving those entries no close path at all, rather than inventing an expiry from a duration: a
/// spell's listed duration is the MAXIMUM, and a dispel, a death or a zone all end it early.
#[derive(Default)]
pub struct Book {
    /// Effects on the reader, most recent last.
    mine: Vec<Effect>,
    /// Landings seen on other people, most recent last, bounded.
    theirs: Vec<Effect>,
}

/// One effect, on somebody, seen at a moment in the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    /// Who it landed on. Empty for the reader, whose own lines name nobody.
    pub who: String,
    /// Every spell whose sentence this was. See [`Landed::candidates`].
    pub candidates: Vec<String>,
    /* `ended: bool` LIVED HERE AND WAS NEVER TRUE.
     *
     * Its doc read "Whether this reading closed the effect rather than opening it. Only ever true
     * for the reader." Both construction sites wrote `false`; the fade path does not MARK an
     * entry, it removes it; and the only read of the field was a `!e.ended` guard that was
     * therefore always taken. A field, a doc claiming a behaviour, and a branch, all three inert.
     *
     * THE DOC IS THE PART THAT COULD HAVE COST SOMETHING. A reader of this file would have
     * believed closed effects are kept and marked, and written a screen that filters on it. */
}

/// How many of somebody else's landings are kept.
///
/// BOUNDED BECAUSE NOTHING CLOSES THEM. The reader's own list is self-limiting, since a fade removes
/// an entry; a list of other people's landings only ever grows, and a tail that runs for an evening
/// would grow it without limit.
const THEIRS_CAP: usize = 64;

impl Book {
    /// TAKE THE EFFECT THESE SPELL NAMES END OUT OF `mine`, and say whether anything moved.
    ///
    /// THE NEWEST MATCH AND NOT THE FIRST, because a re-applied effect is one row (see the
    /// landing arm) and the newest is the one the reader is under.
    fn close(&mut self, ending: &[String]) -> bool {
        let Some(at) = self
            .mine
            .iter()
            .rposition(|e| e.candidates.iter().any(|c| ending.contains(c)))
        else {
            /* A FADE FOR SOMETHING THIS WINDOW NEVER SAW LAND. Real: the buff was up before the
             * app started reading. Nothing moved, and `read`'s contract is that `true` means it
             * did. */
            return false;
        };
        self.mine.remove(at);
        true
    }

    /// Read one classified line. Returns true when the book changed.
    pub fn read(&mut self, cast: &Cast, body: &str, flavour: Flavour, spells: &[Spell]) -> bool {
        /* THE READER'S OWN, FIRST. His sentences carry no placeholder, so they are matched whole
         * against `msg.you` and `msg.off` rather than through the resolver. */
        match flavour {
            Flavour::LandedOnSelf => {
                let names = whole_match(spells, body, |m| m.you.as_deref());
                if names.is_empty() {
                    /* A SENTENCE THE GRAMMAR CALLS A LANDING AND THE CORPUS CALLS AN ENDING.
                     *
                     * `Strength returns to your legs.` is `Ghoul Root`'s OFF message, and
                     * `combat.rs` classifies it `LandedOnSelf`: it has no corpus, so it cannot
                     * know the difference between a sentence that starts an effect and one that
                     * ends it. The fade arm below therefore never ran for it, and the EFFECTS
                     * panel went on listing Ghoul Root as ON the reader after the log had lifted
                     * it. Measured on the capture: four roots, three lifts, and the panel ended
                     * the file still rooted.
                     *
                     * THIS CANNOT MISFIRE ON A REAL LANDING, and that is why the check sits here
                     * rather than in front of the arm. A landing matches some spell's `you`
                     * message BY DEFINITION, so this branch is only reached by a sentence that
                     * matches none. A sentence that matches no landing message and does match an
                     * ending one is an ending, and there is nothing else it could be.
                     *
                     * THE GRAMMAR IS NOT THE PLACE FOR IT. `grimoire_parse` classifies by shape
                     * and ships without the corpus; teaching it these sentences would mean
                     * carrying two thousand spell messages into a crate that is meant to read the
                     * log alone. */
                    let ending = whole_match(spells, body, |m| m.off.as_deref());
                    if !ending.is_empty() {
                        return self.close(&ending);
                    }
                    return false;
                }
                /* ONE ROW PER EFFECT AND NOT ONE PER SENTENCE, and it used to push regardless.
                 *
                 * A damage shield, a regen or a recast dot prints its landing line every time it
                 * fires, and `screens::live` draws one row per entry, so the `on you` list showed
                 * the same name several times in a column. Measured on the reference capture:
                 * `You feel your skin smolder.` appears seven times inside a 340 line span, well
                 * within the 400 line window the panel reads.
                 *
                 * AND THE TYPE'S OWN DOC CALLS THIS "WHAT IS CURRENTLY ON WHOM", so each row is a
                 * claim that a distinct effect is up. Seven rows for one buff is six invented
                 * effects, and it compounds with the fade: `BuffFaded` removes exactly ONE entry,
                 * so after land, land, fade the panel still shows a row for something the log has
                 * said is gone.
                 *
                 * MATCHED ON THE CANDIDATE SET AND NOT ON THE SENTENCE, because the set IS the
                 * identity here: two spells sharing a message are one ambiguous reading, and the
                 * panel already says so with `or N others`. A second landing of the same reading
                 * tells the file nothing it has not already said. */
                if self.mine.iter().any(|e| e.candidates == names) {
                    return false;
                }
                self.mine.push(Effect {
                    who: String::new(),
                    candidates: names,
                });
                return true;
            }
            Flavour::BuffFaded => {
                let names = whole_match(spells, body, |m| m.off.as_deref());
                if names.is_empty() {
                    return false;
                }
                /* CLOSE THE MATCHING ENTRY IF ONE IS OPEN.
                 *
                 * THE COMMENT USED TO SAY "and record the fade either way", AND NOTHING RECORDED
                 * IT. A fade for something never seen landing is a real state, and the honest
                 * thing would be to keep it; what the code does is remove an entry when there is
                 * one and otherwise do nothing at all. Saying so is the fix here, because there is
                 * nowhere to put a faded effect: `Book::mine` is what is currently up, by its own
                 * doc, and an entry meaning `this went away` in that list would be read as an
                 * effect that is on. */
                /* AND `close` REPORTS WHETHER ANYTHING MOVED. A fade for an effect this window
                 * never saw land is a real state and changes nothing; `read`'s contract is that
                 * `true` means the book moved, so it must not claim one. */
                return self.close(&names);
            }
            _ => {}
        }

        let Some(landed) = cast.resolve(body, flavour) else {
            return false;
        };
        self.theirs.push(Effect {
            who: landed.target,
            candidates: landed.candidates,
        });
        while self.theirs.len() > THEIRS_CAP {
            self.theirs.remove(0);
        }
        true
    }

    /// What is on the reader right now, oldest first.
    pub fn mine(&self) -> &[Effect] {
        &self.mine
    }

    /// Landings seen on other people, oldest first. Never closed: see the type note.
    pub fn theirs(&self) -> &[Effect] {
        &self.theirs
    }

    /* `who()` LIVED HERE AND NOTHING CALLED IT. `screens::live` is the only consumer of this
     * type in the workspace and it calls `mine` and `theirs`; a grep for the name across the tree
     * returned the definition and nothing else, not even a test. It compiled, it was documented,
     * and no route reached it, which is this tree's signature defect. It comes back the day a
     * screen wants a roster of who has been buffed, and not before. */
}

/// Every spell whose chosen message IS this line, whole.
///
/// WHOLE AND NOT A SUFFIX. A message about the reader has no placeholder in it, so there is no name
/// to carve off and nothing to be ambiguous about the boundary of. Comparing the whole sentence is
/// both cheaper and stricter than the resolver's suffix match.
fn whole_match(
    spells: &[Spell],
    body: &str,
    pick: fn(&crate::data::spells::Msg) -> Option<&str>,
) -> Vec<String> {
    let body = body.trim();
    spells
        .iter()
        .filter(|s| pick(&s.msg).is_some_and(|m| m.trim() == body))
        .map(|s| s.name.clone())
        .collect()
}
