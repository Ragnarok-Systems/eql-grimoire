//! WHAT A MOB CAN ABSORB, MEASURED FROM KILLS THAT ACTUALLY HAPPENED.
//!
//! # THE LOG NEVER PRINTS HIT POINTS, AND A COMPLETED KILL DOES NOT NEED IT TO
//!
//! EverQuest states no mob health, which is why the Live header carries no bar: the damage is real
//! and the denominator does not exist. But a mob that was engaged whole and then died absorbed
//! exactly what it could take, and that number IS in the fold already: [`Fighter::taken`] on the
//! victim.
//!
//! So the design mock's `TARGET HEALTH 30.7%` is not buildable and the thing it was reaching for
//! is: after N kills of a named mob there is a measured distribution to compare against.
//!
//! # IT IS A DISTRIBUTION AND NEVER AN AVERAGE, AND THE OWNER'S LOG IS WHY
//!
//! A first pass over the live log summed damage between slay lines and produced this for twenty
//! three `a thunder spirit princess` kills:
//!
//! ```text
//! 14161 14353 14590 14801 14999 15513 17304 17836 18555 18706 19059 19071 19707 19895
//! 30805 31635 35017 39911 40340 40359 44391 54784 58893
//! ```
//!
//! Two populations, spread 167% of the mean. An average of 26,725 describes neither and would be
//! wrong on every fight. So this module reports SAMPLES and lets a caller see the shape; a single
//! figure is only offered when the samples agree, and [`Reading::settled`] is where that is decided.
//!
//! # ONE DEATH PER FIGHT OR THE SAMPLE IS THROWN AWAY
//!
//! The engine folds by NAME and says so in its own module note: nothing in the log distinguishes
//! two `a thunder spirit princess` alive at once. So in a fight where that name died TWICE,
//! `taken` is two mobs' worth and using it would record a mob with double the health. That is very
//! probably where the high cluster above came from, since the crude first pass could not see it.
//!
//! A fight is a usable sample only when the name died exactly once in it. Everything else is
//! dropped, and [`Reading::skipped`] counts what went, so a screen can say the reading is partial.
//!
//! # LEVEL IS CARRIED WHERE THE LOG GAVE IT, WHICH IS RARELY
//!
//! `/consider` prints `(Lvl: 20)`, and mob health plainly varies with it. Measured on the owner's
//! live log: 169 consider lines against 4,577 kills, 3.7% coverage, and not one for the mob he has
//! killed twenty-three times. So level cannot be the key; it is a note on a sample when it is
//! known, and a caller may split by it once the coverage is there.
use crate::fights::{FightRow, Mark};
use std::collections::BTreeMap;

/// HOW CLOSE SAMPLES MUST SIT BEFORE ONE FIGURE MAY STAND FOR THEM, as a percent of the median.
///
/// TEN, AND IT WAS TWENTY-FIVE UNTIL THE OWNER'S OWN NIGHT SETTLED IT.
///
/// The princess samples hold two exact values, ~20,015 and ~23,016, which are FIFTEEN PERCENT
/// apart. At twenty-five they fell in one window and `modal` answered 20,025 for a group of
/// sixteen that was really two groups; a bar drawn on that is fifteen percent wrong against every
/// kill of the larger variant, which is visibly wrong on screen.
///
/// AT TEN THEY SEPARATE and the answer is the eleven kills that agree to a tenth of a percent.
/// The number is a judgement, it is written down rather than buried, and the log is what moved it.
pub const TIGHT_PERCENT: u64 = 10;

/// HOW MANY KILLS BEFORE ANY FIGURE IS OFFERED AT ALL.
///
/// THREE, because two samples cannot show a spread: any two numbers are consistent with each other
/// and with a distribution that would embarrass this app on the third kill.
pub const ENOUGH: usize = 3;

/// WHAT ONE NAMED MOB HAS BEEN SEEN TO ABSORB.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    /// Damage absorbed in each usable kill, ascending.
    pub samples: Vec<u64>,
    /// Levels this mob was conned at, ascending. Usually empty: see the module note.
    pub levels: Vec<u32>,
    /// Kills this book SAW and could not measure, so a screen can say the reading is partial.
    ///
    /// THIS ONCE COUNTED ONE THING AND NOW COUNTS THREE, and the doc said only the first: "kills
    /// dropped because the name died more than once in that fight, so `taken` was shared". The
    /// other two were always counted here and were never written down. A kill lands here when the
    /// name died more than once in the fight (the fold shares one row between two mobs of one
    /// name), when `taken` came back zero (something the grammar does not attribute did the
    /// killing, and a zero is not a health pool), and now when the fight itself was CLIPPED (see
    /// [`fold_into`]: a clipped fight's damage is a floor and a floor is not a measurement).
    ///
    /// WHAT IT IS FOR IS UNCHANGED: `samples` is what was measured and this is what was not, and a
    /// reading that shows one without the other is a measurement with its denominator hidden.
    pub skipped: usize,
    /// The spelling to show, which is the first one seen. See [`read`] on why the KEY is folded.
    pub shown: String,
}

impl Reading {
    pub fn n(&self) -> usize {
        self.samples.len()
    }

    pub fn median(&self) -> Option<u64> {
        self.samples.get(self.samples.len() / 2).copied()
    }

    pub fn low(&self) -> Option<u64> {
        self.samples.first().copied()
    }

    pub fn high(&self) -> Option<u64> {
        self.samples.last().copied()
    }

    /// HOW FAR APART THE SAMPLES SIT, as a percent of the median. `None` with nothing to compare.
    pub fn spread(&self) -> Option<u64> {
        let (lo, hi, mid) = (self.low()?, self.high()?, self.median()?);
        if mid == 0 {
            return None;
        }
        Some((hi - lo).saturating_mul(100) / mid)
    }

    /// MAY ONE FIGURE STAND FOR THIS MOB?
    ///
    /// Enough kills, and they agree. When this is false a caller must show the spread rather than
    /// a number, because a number here would be an average of two different things.
    pub fn settled(&self) -> bool {
        self.n() >= ENOUGH && self.spread().is_some_and(|s| s <= TIGHT_PERCENT)
    }

    /// THE LARGEST GROUP OF KILLS THAT AGREE WITH EACH OTHER, and how many are in it.
    ///
    /// # THE SAMPLES ARE NOT NOISY, THEY ARE SEVERAL EXACT ANSWERS MIXED TOGETHER
    ///
    /// Twenty-six princess kills off the owner's own night, through the real fold:
    ///
    /// ```text
    /// 16706 18199
    /// 20011 20012 20012 20014 20014 20016 20018 20019 20025 20025 20140   <- eleven
    /// 23014 23015 23017 23018 23018                                       <- five
    /// 34595 39035 39195 40919 41243 42792 43555 48066                     <- eight
    /// ```
    ///
    /// ELEVEN OF THEM SIT INSIDE 129 POINTS, which is a tenth of a percent. That is not a noisy
    /// measurement of one number, it is an EXACT number measured eleven times, beside a second
    /// exact number measured five times. Almost certainly two level variants of one mob, which is
    /// what the owner predicted before any of this was built.
    ///
    /// AND THE HIGH TAIL IS CONTAMINATION THAT CANNOT BE FILTERED OUT UPSTREAM. Those eight are
    /// roughly twice the tight clusters: fights holding two princesses where only ONE died. The
    /// one-death-per-fight guard cannot see them, because there genuinely was one death; the
    /// second mob's damage is in the same folded row and nothing in the log separates them.
    ///
    /// SO THE ANSWER IS THE DENSEST AGREEING GROUP RATHER THAN THE MIDDLE OF EVERYTHING. Eight
    /// contaminated samples cannot outvote eleven that agree to a tenth of a percent, and a median
    /// across the lot lands between two real values and describes neither.
    ///
    /// THE COUNT COMES BACK WITH IT AND A CALLER MUST SHOW IT. `20,015 from 11 of 26 kills` is a
    /// measurement with its own denominator attached; `20,015` alone is this app inventing
    /// certainty it has not got.
    pub fn modal(&self) -> Option<(u64, usize)> {
        if self.samples.len() < ENOUGH {
            return None;
        }
        /* SORTED ALREADY, so every candidate group is a contiguous window and the widest one
         * starting at each sample is found by walking forward. O(n^2) on a list that is a few
         * dozen long at most. */
        let mut best: Option<(usize, usize)> = None;
        for i in 0..self.samples.len() {
            let lo = self.samples[i];
            let mut j = i;
            while j + 1 < self.samples.len() {
                let hi = self.samples[j + 1];
                /* AGAINST THE GROUP'S OWN LOW, so the window is a true spread and not a chain of
                 * small steps that drifts arbitrarily far. */
                if lo == 0 || (hi - lo).saturating_mul(100) / lo > TIGHT_PERCENT {
                    break;
                }
                j += 1;
            }
            let size = j - i + 1;
            /* THE BIGGEST GROUP, and on a tie the LOWER one: a double pull inflates a sample and
             * never deflates it, so where two groups are equally supported the smaller value is
             * the one less likely to be two mobs. */
            /* `is_none_or` IS 1.82 AND THIS CRATE'S MSRV IS 1.80, which clippy pins. */
            let better = match best {
                None => true,
                Some((bi, bs)) => size > bs || (size == bs && self.samples[i] < self.samples[bi]),
            };
            if better {
                best = Some((i, size));
            }
        }
        let (i, size) = best?;
        if size < ENOUGH {
            return None;
        }
        /* THE MIDDLE OF THE GROUP, not of everything. */
        Some((self.samples[i + size / 2], size))
    }

    /// WHAT A BAR WOULD BE DRAWN AGAINST AND HOW MANY KILLS STAND BEHIND IT, from ONE branch.
    ///
    /// # THE FIGURE AND ITS COUNT WERE TWO CALLS AND THEY ANSWERED TWO DIFFERENT QUESTIONS
    ///
    /// A caller wanting both took the figure from [`Reading::expect`] and the count from
    /// [`Reading::modal`], and those are not the same group whenever the samples are SETTLED:
    /// `expect` is then the median of every sample, while `modal` is the densest agreeing window
    /// inside them, which can be a subset. Six kills that agree inside ten percent of the median
    /// printed `5 kills of 6 agreed on ~111`, where the 111 was measured off all six and the 5
    /// belongs to a group the reader is not being shown. Two statistics wearing one sentence, on
    /// the one line whose entire job is to say how much a reader should trust the figure beside it.
    ///
    /// BOTH CALL SITES IN THE APP HAD IT WRONG BEFORE THIS AUDIT, which is the argument for the
    /// pair being one call rather than a rule each page reimplements. A screen that spells out
    /// `expect`'s branch again is a screen that agrees with it until somebody edits one of them.
    ///
    /// THE COUNT IS NOT `n()`. It is how many kills the figure was measured FROM: the whole set on
    /// the settled branch, the modal group's own size on the other. `20,015 from 11 of 26 kills`
    /// is a measurement with its denominator attached; `20,015` alone is this app inventing
    /// certainty it has not got.
    ///
    /// `None` when neither branch can answer, which is a first meeting or a mob nobody has killed
    /// three times.
    pub fn expect_with_count(&self) -> Option<(u64, usize)> {
        if self.settled() {
            return self.median().map(|v| (v, self.n()));
        }
        self.modal()
    }

    /// WHAT A BAR WOULD BE DRAWN AGAINST, or `None` when nothing has been measured enough.
    ///
    /// The whole set when it agrees, otherwise the densest group that does. `None` when neither,
    /// which is a first meeting or a mob nobody has killed three times.
    ///
    /// THE BRANCH IS [`Reading::expect_with_count`]'S AND IS NOT WRITTEN OUT AGAIN HERE. It was,
    /// and that is what let a caller pair this figure with a count taken from `modal`: two
    /// spellings of one rule, one of them a subset of the other, with nothing to make them drift
    /// visibly. A caller that shows the count beside the figure must ask for both at once.
    pub fn expect(&self) -> Option<u64> {
        self.expect_with_count().map(|(v, _)| v)
    }
}

/// EVERY NAMED MOB THESE FIGHTS KILLED, AND WHAT EACH ABSORBED.
///
/// TAKES FIGHTS RATHER THAN READING A STORE, so this is testable against a handful of rows and so
/// the caller decides the scope: tonight, this month, everything. `store::Store::all` is the usual
/// source.
///
/// A ROW PER MOB SEEN, WHICH IS NOT A ROW PER MOB MEASURED. A name that only ever died in fights
/// this book had to throw away is in here with an empty `samples` and its `skipped` count, because
/// "three kills, none of them usable" is a thing a screen has to be able to say. So the LENGTH of
/// this map is the number of mobs SEEN. Counting it as mobs measured is the defect
/// `Ingest::hp_known` was carrying: see that function.
pub fn read(fights: &[FightRow]) -> BTreeMap<String, Reading> {
    let mut out: BTreeMap<String, Reading> = BTreeMap::new();
    fold_into(&mut out, fights);
    out
}

/// FOLD MORE KILLS INTO A BOOK THAT ALREADY EXISTS.
///
/// # WHY THIS IS SPLIT OUT OF [`read`] AT ALL
///
/// The book behind the Live header's `of ~N` was built once, at bootstrap, off every fight in the
/// store. Kills made while the app was RUNNING therefore never improved it: a reader could kill the
/// same named mob ten times in an evening and the hover would still print the count it had at
/// launch. `Ingest` now folds each fight into the book as that fight finishes, and it needs a way
/// to add kills to a book rather than rebuild one from the whole of history sixty times a night.
///
/// SO [`read`] IS THIS FUNCTION AGAINST AN EMPTY BOOK, deliberately and not by coincidence. Two
/// implementations of "what did this kill measure" would be two chances to disagree, and the one
/// the bootstrap uses is not the one a live kill would go through, which is exactly the pair of
/// paths nobody would compare.
///
/// THE CALLER OWNS THE DOUBLE-COUNT RULE AND THIS FUNCTION CANNOT HELP IT. A fight has no identity
/// (only a start stamp, which is the fight store's dedupe key), so folding the same fight in twice
/// records one kill as two and inflates the count printed beside the figure. `Ingest` folds a fight
/// here only when the store reports it was NEWLY written, which is the one place that question has
/// an answer.
pub fn fold_into(book: &mut BTreeMap<String, Reading>, fights: &[FightRow]) {
    let mut touched: std::collections::BTreeSet<String> = Default::default();

    for f in fights {
        /* HOW MANY TIMES EACH SLOT DIED IN THIS FIGHT. Counted first, because a slot that died
         * twice makes the whole fight unusable for that name and there is no way to know that
         * from the first death alone. */
        let mut deaths: BTreeMap<usize, usize> = BTreeMap::new();
        for m in &f.moments {
            if let Mark::Death { victim, .. } = m.what {
                *deaths.entry(victim).or_default() += 1;
            }
        }

        for (slot, times) in deaths {
            let Some(who) = f.fighters.get(slot) else {
                continue;
            };
            /* PLAYERS ARE NOT MOBS. A raid death is a real event and belongs in the deaths column;
             * a player's `taken` is not a health pool anybody is fighting through. */
            if f.player(&who.who) {
                continue;
            }
            /* THE KEY IS CASE FOLDED, AS THE ENGINE'S OWN `same` IS.
             *
             * `fights::same` compares names with `eq_ignore_ascii_case` and its doc says why: the
             * game sentence-capitalises inconsistently, and `a dry bone skeleton` and `A dry bone
             * skeleton` are one mob. Keying this book on the DISPLAY string did not fold them, so
             * a mob split into two readings depending on which spelling each fight happened to
             * enrol first.
             *
             * MEASURED ON THE OWNER'S OWN NIGHT: twenty-two princess kills came back as
             * `A thunder spirit princess` with eighteen samples and `a thunder spirit princess`
             * with eight, two readings of one mob, each too thin to settle on its own.
             *
             * THE FIRST SPELLING SEEN IS WHAT IS SHOWN, because the log has no canonical form and
             * inventing one would put a name on screen the file never printed. */
            let key = who.who.text().to_ascii_lowercase();
            touched.insert(key.clone());
            let entry = book.entry(key).or_default();
            if entry.shown.is_empty() {
                entry.shown = who.who.text().to_owned();
            }
            /* A CLIPPED FIGHT MEASURES NOTHING, IT ONLY PUTS A FLOOR UNDER SOMETHING.
             *
             * `FightRow::cut` means the text this fight was folded from began part way through it:
             * the 40MB tail cap bit, or the live window's ceiling threw lines away. Its `taken` is
             * therefore whatever the reader did AFTER the window opened and not what the mob
             * absorbed, and it is low by an unknown amount, which is the worst shape an error can
             * have here. `modal` answers with the densest agreeing group, so a handful of low
             * floors do not merely widen the spread: enough of them outvote the true cluster and
             * this app prints a mob's health as a number no kill ever measured.
             *
             * IT IS COUNTED RATHER THAN IGNORED, because a mob whose kills were all clipped must
             * read as "seen, not measured" and not as a mob nobody ever killed. */
            if f.cut {
                entry.skipped += 1;
                continue;
            }
            if times != 1 {
                /* THE FOLD SHARES ONE ROW BETWEEN TWO MOBS OF ONE NAME. See the module note. */
                entry.skipped += 1;
                continue;
            }
            if who.taken == 0 {
                /* KILLED BY SOMETHING THIS LOG DID NOT ATTRIBUTE, or by a source the grammar
                 * leaves unowned. A zero is not a health pool. */
                entry.skipped += 1;
                continue;
            }
            entry.samples.push(who.taken);
        }
    }

    /* ONLY WHAT THIS CALL TOUCHED. Every other reading in the book was sorted when it was written
     * and a fold that re-sorted the whole book would cost the length of a character's history on
     * every kill, which is the shape of thing this split exists to avoid. */
    for k in touched {
        if let Some(r) = book.get_mut(&k) {
            r.samples.sort_unstable();
            r.levels.sort_unstable();
            r.levels.dedup();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fights::{Fighter, Moment, Who};

    fn mob(name: &str, taken: u64) -> Fighter {
        Fighter {
            who: Who::Named(name.into()),
            taken,
            ..Fighter::default()
        }
    }

    fn died(victim: usize, at: u32) -> Moment {
        Moment {
            at,
            what: Mark::Death { killer: 0, victim },
        }
    }

    /// A kill of one mob, absorbing `taken`.
    fn kill(name: &str, taken: u64) -> FightRow {
        FightRow {
            start: "Wed Jul 15 23:16:50 2026".into(),
            fighters: vec![
                Fighter {
                    who: Who::You,
                    dealt: taken,
                    ..Fighter::default()
                },
                mob(name, taken),
            ],
            moments: vec![died(1, 10)],
            ..FightRow::default()
        }
    }

    /// THE HEADLINE CLAIM: a completed kill measures what the mob absorbed.
    #[test]
    fn a_completed_kill_measures_what_the_mob_absorbed() {
        let got = read(&[kill("a thunder spirit princess", 4_743)]);
        let r = &got["a thunder spirit princess"];
        assert_eq!(r.samples, vec![4_743]);
        assert_eq!(r.skipped, 0);
        assert_eq!(
            r.expect(),
            None,
            "one sample is not enough to stand a figure on"
        );
    }

    /// DEFECT: TWO MOBS OF ONE NAME RECORDED AS ONE MOB WITH DOUBLE THE HEALTH.
    ///
    /// The engine folds by NAME and its own module note says nothing in the log distinguishes two
    /// `a lurking mummy` alive at once, so in a fight where the name died twice `taken` is both of
    /// them. This is very probably where the owner's high cluster came from: a first pass that
    /// summed damage between slay lines could not see the double pull at all.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `times != 1` guard.
    #[test]
    fn a_name_that_died_twice_in_one_fight_is_not_a_sample() {
        let mut f = kill("a lurking mummy", 9_000);
        f.moments.push(died(1, 40));

        let got = read(&[f]);
        let r = &got["a lurking mummy"];
        assert!(
            r.samples.is_empty(),
            "two mummies were recorded as one with 9,000 health: {r:?}"
        );
        assert_eq!(r.skipped, 1, "the drop was not counted, so it is invisible");
    }

    /// SAMPLES THAT AGREE MAY STAND A FIGURE. Samples that do not, may not.
    ///
    /// This is the owner's own case, with his own numbers: the princess is bimodal and an average
    /// of 26,725 describes neither cluster.
    ///
    /// WHAT MUTATION MAKES THIS RED: `settled` ignoring the spread.
    #[test]
    fn a_figure_is_offered_only_when_the_kills_agree() {
        let tight = read(&[
            kill("a dry bone skeleton", 1_000),
            kill("a dry bone skeleton", 1_050),
            kill("a dry bone skeleton", 1_100),
        ]);
        let t = &tight["a dry bone skeleton"];
        assert!(t.settled(), "{t:?}");
        assert_eq!(t.expect(), Some(1_050));

        /* THE PRINCESS, ABRIDGED: one sample from each cluster and one between. */
        let split = read(&[
            kill("a thunder spirit princess", 14_161),
            kill("a thunder spirit princess", 19_071),
            kill("a thunder spirit princess", 58_893),
        ]);
        let s = &split["a thunder spirit princess"];
        assert_eq!(s.n(), 3, "enough kills");
        assert!(
            s.spread().is_some_and(|v| v > TIGHT_PERCENT),
            "spread {:?} should be wide",
            s.spread()
        );
        assert!(!s.settled());
        assert_eq!(
            s.expect(),
            None,
            "a single figure was offered for a mob whose kills disagree by hundreds of percent"
        );
        /* AND THE RANGE IS STILL THERE TO SHOW, which is the honest thing to draw instead. */
        assert_eq!((s.low(), s.high()), (Some(14_161), Some(58_893)));
    }

    /// DEFECT: `5 kills of 6 agreed on ~111`, WHERE THE 111 WAS MEASURED OFF ALL SIX.
    ///
    /// A caller wanting the figure and the count it rests on took the figure from `expect` and the
    /// count from `modal`, and those are two different groups on the SETTLED branch: `expect` is
    /// the median of every sample and `modal` is the densest agreeing window inside them. Both call
    /// sites in the app had it that way before this audit. The count is on that line for exactly
    /// one reason, to say how much of the history stands behind the figure, so a count belonging to
    /// a different group than the figure is worse than no count at all.
    ///
    /// SO ONE CALL ANSWERS BOTH AND `expect` IS DEFINED IN TERMS OF IT, which is the version that
    /// cannot drift: a screen spelling the branch out again agrees with `expect` until somebody
    /// edits one of the two.
    ///
    /// WHAT MUTATION MAKES THIS RED: `expect_with_count` returning `self.modal()` unconditionally,
    /// or `expect` going back to its own copy of the branch (the second assertion in each half
    /// pins the pair together, so a copy that drifts is caught even when both halves are
    /// individually plausible).
    #[test]
    fn the_count_comes_out_of_the_same_branch_as_the_figure() {
        /* SETTLED, BECAUSE `spread` DIVIDES BY THE MEDIAN AND `modal`'s WINDOW DIVIDES BY ITS OWN
         * LOW. 100 to 111 is 9 percent of the median 111 and 11 percent of the low 100, so the
         * whole set agrees while the densest window holds only five of the six. */
        let settled = Reading {
            samples: vec![100, 110, 110, 111, 111, 111],
            ..Reading::default()
        };
        assert!(settled.settled(), "the fixture is not in the branch tested");
        assert_eq!(
            settled.modal(),
            Some((111, 5)),
            "the fixture no longer tells the two statistics apart"
        );
        let (v, of) = settled
            .expect_with_count()
            .expect("a settled reading answers");
        assert_eq!(
            of, 6,
            "the count came from the modal group while the figure came from the median of \
             everything, so the sentence pairs 5 with a number measured off 6"
        );
        assert_eq!(
            Some(v),
            settled.expect(),
            "the figure drifted from `expect`, so a page and the book disagree about one mob"
        );

        /* AND THE OTHER BRANCH IS `modal`'s OWN COUNT AND NOT THE WHOLE SET. Three kills agree,
         * two more sit fifteen percent above them, one is a double pull. */
        let split = Reading {
            samples: vec![20_011, 20_014, 20_016, 23_014, 23_015, 40_000],
            ..Reading::default()
        };
        assert!(!split.settled());
        let (v, of) = split.expect_with_count().expect("three kills agree");
        assert_eq!(
            of, 3,
            "the count is the whole history rather than the group"
        );
        assert_ne!(of, split.n());
        assert_eq!(Some(v), split.expect());

        /* NOTHING TO SAY IS SAID THE SAME WAY BY BOTH. */
        let thin = Reading::default();
        assert_eq!(thin.expect_with_count(), None);
        assert_eq!(thin.expect(), None);
    }

    /// TWO KILLS ARE NOT ENOUGH, whatever they say.
    ///
    /// Any two numbers agree with each other. The third is the first one that can disagree.
    #[test]
    fn two_kills_never_settle_however_close_they_are() {
        let got = read(&[kill("a fire beetle", 500), kill("a fire beetle", 500)]);
        let r = &got["a fire beetle"];
        assert_eq!(r.spread(), Some(0), "identical samples");
        assert!(!r.settled(), "two samples settled a reading");
        assert_eq!(r.expect(), None);
    }

    /// DEFECT: A MOB NOBODY MEASURED IS STILL A ROW IN THIS BOOK, AND THE ROW LOOKS LIKE A READING.
    ///
    /// This is not a bug in `read`, it is the contract of it, and it is written down here because a
    /// caller got it wrong on screen. `Ingest::hp_known` returned this map's LENGTH and the
    /// Dashboards tile printed it as "N mobs measured" under a tooltip promising the kills had
    /// agreed on a figure. Every name that ever died is in here, including one whose every kill was
    /// thrown away, so that number was mobs SEEN.
    ///
    /// THE ROW EARNS ITS PLACE: `skipped` is how a screen says "killed three times, none of them
    /// usable", which is a different sentence from "never killed". The count is what had to move,
    /// and it did: `Ingest::hp_known` now counts readings that can answer.
    ///
    /// WHAT MUTATION MAKES THIS RED: `read` dropping the entry when nothing survived, which would
    /// take the honest "seen but not measured" state away from every screen with it.
    #[test]
    fn a_mob_whose_kills_were_all_thrown_away_is_a_row_with_no_samples() {
        let mut twice = kill("a lurking mummy", 9_000);
        twice.moments.push(died(1, 40));
        let got = read(&[twice, kill("a dry bone skeleton", 1_000)]);

        assert_eq!(got.len(), 2, "both names are in the book: {got:?}");
        let m = &got["a lurking mummy"];
        assert_eq!((m.n(), m.skipped), (0, 1));
        assert_eq!(
            m.expect(),
            None,
            "nothing was measured, so nothing may be shown"
        );
    }

    /// DEFECT: A CLIPPED FIGHT RECORDED AS A MEASUREMENT, WHICH IS A FLOOR WEARING A FIGURE'S NAME.
    ///
    /// `FightRow::cut` says the text began part way through this fight, so `taken` is what the
    /// reader did after the window opened. Left in, it is a low sample of unknown depth, and
    /// `modal` answers with the DENSEST group rather than the middle of everything: three clipped
    /// kills that happen to agree with each other outvote two whole ones and this app prints a
    /// number no kill ever measured.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `f.cut` guard in `fold_into`.
    #[test]
    fn a_fight_that_was_clipped_is_not_a_measurement() {
        let mut clipped = kill("a thunder spirit princess", 400);
        clipped.cut = true;
        let mut also = kill("a thunder spirit princess", 402);
        also.cut = true;
        let mut third = kill("a thunder spirit princess", 404);
        third.cut = true;

        let got = read(&[
            clipped,
            also,
            third,
            kill("a thunder spirit princess", 20_011),
            kill("a thunder spirit princess", 20_014),
        ]);
        let r = &got["a thunder spirit princess"];
        assert_eq!(
            r.samples,
            vec![20_011, 20_014],
            "a clipped fight's damage was taken for a health pool: {r:?}"
        );
        assert_eq!(
            r.skipped, 3,
            "the clipped kills went unrecorded, so they are invisible"
        );
        assert_eq!(
            r.expect(),
            None,
            "three floors agreeing with each other stood up a figure for a mob measured twice"
        );
    }

    /// THE LIVE PATH: kills fold into a book that already exists, and it stays sorted.
    ///
    /// The book behind the header's `of ~N` used to be built once at bootstrap, so a mob killed ten
    /// times during a session never improved. `fold_into` is what `Ingest` calls as each fight
    /// finishes, and a book it has added to has to be indistinguishable from one built from the
    /// same kills in one pass, or `median`, `low`, `high` and `modal` (all of which assume the
    /// samples are ascending) quietly answer nonsense.
    ///
    /// WHAT MUTATION MAKES THIS RED: `fold_into` pushing samples without re-sorting the readings it
    /// touched.
    #[test]
    fn folding_a_later_kill_in_matches_reading_them_all_at_once() {
        let first = kill("a dry bone skeleton", 1_100);
        let later = kill("a dry bone skeleton", 1_000);
        let last = kill("a dry bone skeleton", 1_050);

        let mut grown = read(std::slice::from_ref(&first));
        fold_into(&mut grown, std::slice::from_ref(&later));
        fold_into(&mut grown, std::slice::from_ref(&last));

        let at_once = read(&[first, later, last]);
        assert_eq!(
            grown, at_once,
            "a book grown a kill at a time drifted from one read whole"
        );
        let r = &grown["a dry bone skeleton"];
        assert_eq!(
            r.samples,
            vec![1_000, 1_050, 1_100],
            "the samples are not ascending"
        );
        assert_eq!(r.expect(), Some(1_050));
    }

    /// A PLAYER DEATH IS NOT A HEALTH POOL.
    #[test]
    fn a_player_who_died_is_not_recorded_as_a_mob() {
        let f = FightRow {
            start: "Wed Jul 15 23:16:50 2026".into(),
            fighters: vec![Fighter {
                who: Who::Named("Poguhy".into()),
                taken: 1_700,
                ..Fighter::default()
            }],
            moments: vec![died(0, 5)],
            ..FightRow::default()
        };
        assert!(read(&[f]).is_empty(), "a player was recorded as a mob");
    }
}
