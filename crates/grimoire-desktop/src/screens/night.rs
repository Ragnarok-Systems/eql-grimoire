//! WHAT A DASHBOARD LOOKS BACK AT: a night, a week, or everything this character has stored.
//!
//! # THE DASHBOARD IS NOT LIVE, AND THAT IS A DECISION AND NOT A GAP
//!
//! The owner's words: "people that are using the dashboard are NOT using it live, they are looking
//! back at things such as their progression or raid night. Leave live alone." So the page reads
//! `Ingest::history`, which is every finished fight this character has had written to disk, and
//! never `Ingest::current_fight`. A [`Filter`] is which slice of that history is on the page.
//!
//! # A NIGHT ENDS AT SIX IN THE MORNING, NOT AT MIDNIGHT
//!
//! A raid that starts at nine and finishes at one is one night to everybody in it, and a calendar
//! day would cut it in two at midnight and file the kill on the wrong day. So a fight stamped
//! before 06:00 belongs to the previous date. Six is a convention and not a measurement, which is
//! why it is one constant with its reason on it rather than a rule spread through the code.
//!
//! # THE STAMP IS THE LOG'S OWN TEXT AND IT IS PARSED HERE, ONCE
//!
//! `FightRow::start` is `Wed Jul 15 23:16:50 2026` as the log printed it, kept as text so that no
//! timezone is ever claimed (see `FightRow::start`). Grouping into nights needs it as a date, and
//! `Store::all` needs it to sort by, and both were doing string comparison before this: a store
//! sorted by the STRING of that stamp puts `Thu Aug` before `Wed Jul`, which is a look back
//! dashboard with its nights in alphabetical order.
use crate::fights::FightRow;
use chrono::{NaiveDate, NaiveDateTime, Timelike};

/// The hour a night rolls over. See the module note.
pub const NIGHT_ROLLS_AT: u32 = 6;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// THE INSTANT A LOG STAMP NAMES, as the log's own wall clock with no zone attached.
///
/// `Www Mmm DD HH:MM:SS YYYY`, with or without the log's square brackets. `None` for anything
/// else, which is the honest answer for a row whose stamp this build cannot read: it is dropped
/// from every scope rather than filed under a guessed date.
pub fn started_at(stamp: &str) -> Option<NaiveDateTime> {
    let mut it = stamp
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split_whitespace();
    let _dow = it.next()?;
    let mon = it.next()?;
    let day: u32 = it.next()?.parse().ok()?;
    let time = it.next()?;
    let year: i32 = it.next()?.parse().ok()?;
    let month = MONTHS.iter().position(|m| *m == mon)? as u32 + 1;
    let mut hms = time.split(':');
    let h: u32 = hms.next()?.parse().ok()?;
    let mi: u32 = hms.next()?.parse().ok()?;
    let s: u32 = hms.next()?.parse().ok()?;
    NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(h, mi, s)
}

/// WHICH NIGHT AN INSTANT BELONGS TO. Before [`NIGHT_ROLLS_AT`] it is still the night before.
pub fn night_of(t: NaiveDateTime) -> NaiveDate {
    if t.hour() < NIGHT_ROLLS_AT {
        t.date().pred_opt().unwrap_or(t.date())
    } else {
        t.date()
    }
}

/// The night a fight belongs to, or `None` for a stamp this build cannot read.
pub fn night_of_row(f: &FightRow) -> Option<NaiveDate> {
    started_at(&f.start).map(night_of)
}

/// WHICH STRETCH OF NIGHTS THE PAGE IS LOOKING BACK AT.
///
/// NOT `Copy` SINCE [`When::Nights`] ARRIVED, because a set of dates is a `Vec`. Everything
/// that matched this by value now matches it by reference, which is the whole cost.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum When {
    /// One night, by the date it started on. See [`night_of`].
    Night(NaiveDate),
    /// THESE PARTICULAR NIGHTS, which need not be next to each other.
    ///
    /// # WHY A SET AND NOT ANOTHER RANGE
    ///
    /// The owner asked to pick one or many days off a calendar, and the questions he asks are
    /// not intervals: `the last two Nagafen fights` is two nights a fortnight apart, and
    /// `compare these two raids` is two nights with a month of grinding between them. A range
    /// can only say that by dragging in everything in the middle.
    ///
    /// SORTED AND DEDUPED BY [`When::picked`], so two filters that mean the same nights compare
    /// equal and the cached fold behind them is not thrown away by a reordering.
    Nights(Vec<NaiveDate>),
    /// Every night from the first date to the second, both ends included.
    Range(NaiveDate, NaiveDate),
    /// The N most recent nights THAT HAVE FIGHTS IN THEM, which is not the same as the last N
    /// calendar days: a week with two raids in it is two nights, and a reader asking for his last
    /// three nights means the last three he played.
    LastNights(u32),
    /// Everything this character has stored.
    #[default]
    All,
}

impl When {
    /// A SET OF NIGHTS, NORMALISED: sorted, deduped, and collapsed when it can be said plainer.
    ///
    /// ONE NIGHT IS [`When::Night`] AND NOT A SET OF ONE, so the label reads `Sep 7` rather than
    /// `1 night` and the quick buttons in the sheet light up when a calendar click lands on the
    /// same night they mean.
    ///
    /// NONE IS [`When::All`], and that is not a shrug: no night picked is no narrowing BY night,
    /// which is exactly what `All` means. The alternative is a filter that matches nothing and
    /// reads as a broken page.
    pub fn picked(mut days: Vec<NaiveDate>) -> When {
        days.sort_unstable();
        days.dedup();
        match days.len() {
            0 => When::All,
            1 => When::Night(days[0]),
            _ => When::Nights(days),
        }
    }

    /// THIS STRETCH WITH ONE NIGHT ADDED OR TAKEN AWAY, which is what a calendar click means.
    ///
    /// # A TOGGLE, SO MANY DAYS COST NO MORE THAN ONE
    ///
    /// Clicking an unpicked night adds it and clicking a picked one takes it away, which needs
    /// no modifier key and no drag and no second control. The owner asked to select one or many
    /// days; this is the whole of the difference between them.
    ///
    /// A STRETCH THAT IS NOT A SET OF DAYS BECOMES ONE. [`When::days`] answers empty for a range
    /// and for `last N nights`, so clicking a day out of those starts a fresh pick of that day
    /// rather than silently editing a stretch the reader cannot see the ends of.
    ///
    /// NORMALISED THROUGH [`When::picked`], so taking the last night away is `All` and not a
    /// filter that matches nothing.
    pub fn toggled(&self, day: NaiveDate) -> When {
        let mut days = self.days();
        match days.iter().position(|d| *d == day) {
            Some(at) => {
                days.remove(at);
            }
            None => days.push(day),
        }
        When::picked(days)
    }

    /// THE NIGHTS THIS PICKS ONE BY ONE, or empty for a stretch that is not a set of days.
    ///
    /// A range and `last N nights` are deliberately absent: they are answers to a different
    /// question and a calendar that lit up their days would invite a click that silently threw
    /// the stretch away.
    pub fn days(&self) -> Vec<NaiveDate> {
        match self {
            When::Night(d) => vec![*d],
            When::Nights(ds) => ds.clone(),
            _ => Vec::new(),
        }
    }
}

/// WHAT THE PAGE IS LOOKING BACK AT: when, where, against what, and how much of it.
///
/// # THE OWNER ASKED FOR THIS AS A FILTER SHEET AND HE IS RIGHT ABOUT WHY
///
/// It began as a row of chips in the encounter head, one per night, and that does not scale past
/// a week and cannot express the questions he actually asks: "I was doing this dps in Plane of
/// Sky but now I am doing this", "let us look over the logs from last night's raid", "let us look
/// over the last two Nagafen fights". Those are three dimensions and a count, so this is three
/// dimensions and a count.
///
/// # THERE IS NO RAID HERE, AND THAT IS NOT AN OVERSIGHT
///
/// Nothing in an EverQuest Legends log line says a fight was part of a raid: there is no raid id,
/// no roster and no instance. What a raid night IS in this data is a NIGHT in a ZONE, and those
/// are both real, so picking `Sep 7` and `Plane of Sky` is picking that raid night exactly. The
/// sheet says so in as many words rather than inventing an entity and letting a reader believe
/// the app knows something it does not.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Filter {
    pub when: When,
    /// The zone, as the log spells it. `None` is every zone.
    pub zone: Option<String>,
    /// The fight's headline, which is the named thing that took the most damage. `None` is
    /// anything.
    pub mob: Option<String>,
    /// Keep only the most recent N of whatever the rest of the filter left. `None` is all of them.
    pub last: Option<usize>,
}

impl Filter {
    /// A fresh page's filter: the most recent night with a fight in it, and no other narrowing.
    pub fn opening(rows: &[FightRow]) -> Filter {
        Filter {
            when: latest(rows).map_or(When::All, When::Night),
            ..Filter::default()
        }
    }

    /// Is anything narrowed beyond the stretch of nights?
    pub fn narrowed(&self) -> bool {
        self.zone.is_some() || self.mob.is_some() || self.last.is_some()
    }

    /// THE WHOLE FILTER IN WORDS, for the head. Every part that is set, in the order a person
    /// would say it: when, where, against what, how many.
    pub fn label(&self) -> String {
        let mut out = vec![self.when_label()];
        if let Some(z) = &self.zone {
            out.push(z.clone());
        }
        if let Some(m) = &self.mob {
            out.push(m.clone());
        }
        if let Some(n) = self.last {
            out.push(format!("last {n}"));
        }
        out.join(" \u{b7} ")
    }

    /// Just the stretch of nights, in words.
    pub fn when_label(&self) -> String {
        match &self.when {
            When::Night(d) => d.format("%b %-d, %Y").to_string(),
            /* NAMED WHILE THEY STILL FIT, COUNTED AFTER THAT. Three dates is about what reads
             * in a head; past that the count is the useful fact and the sheet has the list. */
            When::Nights(ds) if ds.len() <= 3 => ds
                .iter()
                .map(|d| d.format("%b %-d").to_string())
                .collect::<Vec<_>>()
                .join(", "),
            When::Nights(ds) => format!("{} nights picked", ds.len()),
            When::Range(a, b) => format!("{} to {}", a.format("%b %-d"), b.format("%b %-d, %Y")),
            When::LastNights(n) => format!("Last {n} nights"),
            When::All => String::from("All time"),
        }
    }

    /// A short form for a strip cell: `Sep 7`, `7 nights`, `All`.
    pub fn short(&self) -> String {
        match &self.when {
            When::Night(d) => d.format("%b %-d").to_string(),
            When::Nights(ds) => format!("{} nights", ds.len()),
            When::Range(a, b) => format!("{}-{}", a.format("%b %-d"), b.format("%b %-d")),
            When::LastNights(n) => format!("{n} nights"),
            When::All => String::from("All"),
        }
    }
}

/// EVERY NIGHT WITH A FIGHT IN IT, NEWEST FIRST, with how many fights each has.
///
/// Newest first because that is the order a reader looks back in: last night, the night before,
/// then the raid a week ago.
pub fn nights(rows: &[FightRow]) -> Vec<(NaiveDate, usize)> {
    let mut out: Vec<(NaiveDate, usize)> = Vec::new();
    for f in rows {
        let Some(n) = night_of_row(f) else {
            continue;
        };
        match out.iter_mut().find(|(d, _)| *d == n) {
            Some((_, c)) => *c += 1,
            None => out.push((n, 1)),
        }
    }
    out.sort_by_key(|(d, _)| std::cmp::Reverse(*d));
    out
}

/// The most recent night with a fight in it, which is the night a fresh page opens on.
pub fn latest(rows: &[FightRow]) -> Option<NaiveDate> {
    rows.iter().filter_map(night_of_row).max()
}

/// WHICH ZONE A NIGHT WAS MOSTLY SPENT IN, for the label beside it in the sheet.
///
/// THE ONE WITH THE MOST FIGHTS IN IT, and `None` when no fight that night named a zone. This is
/// what makes a night row read as a raid night without the app claiming there is such a thing:
/// `Sep 7 \u{b7} 14 fights \u{b7} Plane of Sky` is three facts, and the reader supplies the word raid.
pub fn zone_of_night(rows: &[FightRow], night: NaiveDate) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for f in rows.iter().filter(|f| night_of_row(f) == Some(night)) {
        let Some(z) = f.zone.as_deref() else {
            continue;
        };
        match counts.iter_mut().find(|(n, _)| n == z) {
            Some((_, c)) => *c += 1,
            None => counts.push((z.to_owned(), 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counts.into_iter().next().map(|(z, _)| z)
}

/// DOES THIS FIGHT PASS EVERY PART OF THE FILTER EXCEPT `last`, which is a count and not a test?
///
/// `order` is the night list newest first, which [`When::LastNights`] needs and which the caller
/// already has.
fn passes(f: &FightRow, filter: &Filter, order: &[(NaiveDate, usize)]) -> bool {
    let Some(n) = night_of_row(f) else {
        /* A STAMP THIS BUILD CANNOT READ IS IN NO NIGHT, so it is in no scope. Dropping it is the
         * honest answer: filing it under a guessed date would put it in a reading it does not
         * belong to. */
        return false;
    };
    let when = match &filter.when {
        When::Night(d) => n == *d,
        When::Nights(ds) => ds.contains(&n),
        When::Range(a, b) => {
            let (lo, hi) = if a <= b { (*a, *b) } else { (*b, *a) };
            lo <= n && n <= hi
        }
        When::LastNights(k) => order.iter().take(*k as usize).any(|(d, _)| *d == n),
        When::All => true,
    };
    if !when {
        return false;
    }
    if let Some(z) = &filter.zone {
        if !f.zone.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(z)) {
            return false;
        }
    }
    if let Some(m) = &filter.mob {
        /* CASE INSENSITIVE, because the engine folds names that way (`Fights::same`) and a log
         * prints `A dry bone skeleton` and `a dry bone skeleton` for one mob. */
        if !f
            .headline
            .as_deref()
            .is_some_and(|x| x.eq_ignore_ascii_case(m))
        {
            return false;
        }
    }
    true
}

/// THE FIGHTS A FILTER KEEPS, oldest first, borrowed out of the history.
///
/// `last` IS APPLIED AT THE END AND TAKES THE MOST RECENT, which is what `the last two Nagafen
/// fights` means: filter to Nagafen, then keep the newest two, in time order.
pub fn in_filter<'a>(rows: &'a [FightRow], filter: &Filter) -> Vec<&'a FightRow> {
    let order = nights(rows);
    let mut out: Vec<&FightRow> = rows.iter().filter(|f| passes(f, filter, &order)).collect();
    if let Some(n) = filter.last {
        let drop = out.len().saturating_sub(n);
        out.drain(..drop);
    }
    out
}

/// THE NIGHTS THIS FILTER COULD PICK, ignoring its own `when`, newest first with a fight count.
///
/// # THE OPTIONS NARROW WITH THE OTHER DIMENSIONS AND THAT IS THE POINT
///
/// With Plane of Sky chosen, the night list is the nights that HAVE a Plane of Sky fight in them.
/// A sheet that offered every night regardless would send the reader to a night that comes back
/// empty, which reads as a broken filter rather than as a night he did not raid.
pub fn nights_for(rows: &[FightRow], filter: &Filter) -> Vec<(NaiveDate, usize)> {
    let wide = Filter {
        when: When::All,
        last: None,
        ..filter.clone()
    };
    let kept: Vec<FightRow> = in_filter(rows, &wide).into_iter().cloned().collect();
    nights(&kept)
}

/// THE ZONES THIS FILTER COULD PICK, ignoring its own zone, most fights first.
pub fn zones_for(rows: &[FightRow], filter: &Filter) -> Vec<(String, usize)> {
    let wide = Filter {
        zone: None,
        last: None,
        ..filter.clone()
    };
    let mut out: Vec<(String, usize)> = Vec::new();
    for f in in_filter(rows, &wide) {
        let Some(z) = f.zone.as_deref() else {
            continue;
        };
        match out.iter_mut().find(|(n, _)| n == z) {
            Some((_, c)) => *c += 1,
            None => out.push((z.to_owned(), 1)),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// THE NAMED THINGS THIS FILTER COULD PICK, ignoring its own mob, most fights first.
///
/// FOLDED CASE INSENSITIVELY under the first spelling seen, because the log prints both `A dry
/// bone skeleton` and `a dry bone skeleton` for one mob and two rows for one thing is a picker
/// that looks broken.
pub fn mobs_for(rows: &[FightRow], filter: &Filter) -> Vec<(String, usize)> {
    let wide = Filter {
        mob: None,
        last: None,
        ..filter.clone()
    };
    let mut out: Vec<(String, usize)> = Vec::new();
    for f in in_filter(rows, &wide) {
        let Some(m) = f.headline.as_deref() else {
            continue;
        };
        match out.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(m)) {
            Some((_, c)) => *c += 1,
            None => out.push((m.to_owned(), 1)),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// Chronological order for a list of rows/// Chronological order for a list of rows, by the parsed stamp; rows this build cannot date keep
/// their relative order at the end. `Store::all` uses this so the history is in time order and
/// not in alphabetical order of weekday names.
pub fn sort_chronologically(rows: &mut [FightRow]) {
    rows.sort_by_cached_key(|r| (started_at(&r.start).is_none(), started_at(&r.start)));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A SET OF NIGHTS PICKS EXACTLY THOSE NIGHTS, AND SAYS ITSELF PLAINLY.
    ///
    /// # WHY THE NORMALISING IS PART OF THE MEANING
    ///
    /// A calendar produces days in the order they were clicked, so the same two nights can
    /// arrive as `[8, 5]` or `[5, 8]`, and a double click can put one in twice. Left alone
    /// those are three different [`When`] values that select the same fights, and the page keys
    /// its cached fold on the filter: a reordering would throw the fold away and re-roll the
    /// whole scope for nothing.
    ///
    /// ONE NIGHT COLLAPSES TO [`When::Night`] so the label reads `Sep 7` rather than `1 night`
    /// and the sheet's own night rows light up. NONE COLLAPSES TO [`When::All`], because no
    /// night picked is no narrowing by night, which is what `All` means.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the sort or the dedup, collapsing an empty pick
    /// to a set rather than to `All`, or a `passes` that treats a set as a range.
    #[test]
    fn a_set_of_nights_is_normalised_and_selects_exactly_those() {
        let d = |day: u32| NaiveDate::from_ymd_opt(2026, 9, day).expect("a real date");

        assert_eq!(
            When::picked(Vec::new()),
            When::All,
            "no night picked is no narrowing"
        );
        assert_eq!(
            When::picked(vec![d(7)]),
            When::Night(d(7)),
            "one night is one night"
        );
        assert_eq!(
            When::picked(vec![d(8), d(5), d(8)]),
            When::Nights(vec![d(5), d(8)]),
            "a set is sorted and deduped, or two filters meaning one thing compare unequal"
        );
        assert_eq!(
            When::picked(vec![d(8), d(5)]),
            When::picked(vec![d(5), d(8)]),
            "the order they were clicked in is not part of what was picked"
        );

        /* AND IT SELECTS THE PICKED NIGHTS AND NOTHING BETWEEN THEM, which is the whole reason
         * this is not a range: the 6th is skipped. */
        let rows: Vec<FightRow> = [5u32, 6, 8]
            .iter()
            .map(|day| FightRow {
                /* THE LOG ORDER: day of week, month, day, time, year. See started_at. */
                start: format!("Mon Sep {day:02} 21:00:00 2026"),
                ..FightRow::default()
            })
            .collect();
        let night_of_each: Vec<Option<NaiveDate>> = rows.iter().map(night_of_row).collect();
        assert!(
            night_of_each.iter().all(Option::is_some),
            "the fixture's stamps do not parse, so this test proves nothing: {night_of_each:?}"
        );

        let filter = Filter {
            when: When::picked(vec![d(8), d(5)]),
            ..Filter::default()
        };
        let kept: Vec<String> = in_filter(&rows, &filter)
            .into_iter()
            .map(|f| f.start.clone())
            .collect();
        assert_eq!(kept.len(), 2, "picked two nights and got {kept:?}");
        assert!(
            kept.iter().all(|s| !s.contains("Sep 06")),
            "the night between the two picked ones was dragged in: {kept:?}"
        );

        /* AND A CALENDAR CLICK IS A TOGGLE, which is the whole of one day versus many.
         *
         * DRIVEN HERE AND NOT THROUGH A POINTER, because the cell that carries it is one of
         * forty two in a modal over a scrolled page, and a test that found it by coordinate
         * would be testing the coordinate. */
        let one = When::Night(d(7));
        let two = one.toggled(d(9));
        assert_eq!(
            two,
            When::Nights(vec![d(7), d(9)]),
            "a second night joins the first"
        );
        assert_eq!(
            two.toggled(d(7)),
            When::Night(d(9)),
            "taking one of two away leaves the other, named as one night"
        );
        assert_eq!(
            When::Night(d(9)).toggled(d(9)),
            When::All,
            "taking the last night away is no narrowing, not a filter that matches nothing"
        );
        /* A STRETCH THAT IS NOT A SET OF DAYS STARTS A FRESH PICK RATHER THAN EDITING ITSELF. */
        assert_eq!(
            When::LastNights(3).toggled(d(8)),
            When::Night(d(8)),
            "clicking a day out of `last 3 nights` quietly edited a stretch with no visible ends"
        );

        /* AND A RANGE OVER THE SAME ENDS DOES DRAG IT IN, which is the difference stated. */
        let spread = Filter {
            when: When::Range(d(5), d(8)),
            ..Filter::default()
        };
        assert_eq!(
            in_filter(&rows, &spread).len(),
            3,
            "a range is supposed to include what is between its ends"
        );
    }

    use chrono::Datelike;

    fn row(stamp: &str) -> FightRow {
        FightRow {
            start: stamp.to_owned(),
            end: stamp.to_owned(),
            secs: 30,
            ..FightRow::default()
        }
    }

    /// DEFECT: A HISTORY SORTED BY THE SPELLING OF THE WEEKDAY.
    ///
    /// `Store::all` sorted rows by the stamp STRING, and `Thu Aug 06` sorts before `Wed Jul 15`.
    /// A look back page fed that order has August before July.
    ///
    /// WHAT MUTATION MAKES THIS RED: sorting by the string again, or `started_at` reading the
    /// month as text.
    #[test]
    fn stamps_are_parsed_and_ordered_in_time_and_not_alphabetically() {
        let a = started_at("Wed Jul 15 23:16:50 2026").expect("parses");
        let b = started_at("[Thu Aug 06 01:02:03 2026]").expect("brackets are tolerated");
        assert!(a < b, "July is before August");
        assert_eq!(a.year(), 2026);
        assert_eq!(a.month(), 7);
        assert_eq!(a.day(), 15);
        assert_eq!(a.hour(), 23);
        assert!(started_at("not a stamp").is_none());
        assert!(started_at("Wed Zzz 15 23:16:50 2026").is_none());

        let mut rows = vec![
            row("Thu Aug 06 01:02:03 2026"),
            row("garbage"),
            row("Wed Jul 15 23:16:50 2026"),
        ];
        sort_chronologically(&mut rows);
        assert!(rows[0].start.starts_with("Wed Jul"), "July first");
        assert!(rows[1].start.starts_with("Thu Aug"));
        assert_eq!(rows[2].start, "garbage", "the unreadable stamp goes last");
    }

    /// A NIGHT ROLLS OVER AT SIX, so a kill at one in the morning is still last night's.
    #[test]
    fn a_fight_before_six_belongs_to_the_night_before() {
        let late = started_at("Wed Jul 15 23:16:50 2026").unwrap();
        let small_hours = started_at("Thu Jul 16 01:30:00 2026").unwrap();
        let morning = started_at("Thu Jul 16 09:00:00 2026").unwrap();
        assert_eq!(
            night_of(late),
            NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
        );
        assert_eq!(
            night_of(small_hours),
            NaiveDate::from_ymd_opt(2026, 7, 15).unwrap(),
            "01:30 is still the night of the 15th"
        );
        assert_eq!(
            night_of(morning),
            NaiveDate::from_ymd_opt(2026, 7, 16).unwrap()
        );
    }

    #[test]
    fn scopes_pick_the_right_fights_and_the_week_is_anchored_on_the_last_night() {
        let rows = vec![
            row("Mon Aug 24 21:00:00 2026"),
            row("Mon Aug 24 23:30:00 2026"),
            row("Tue Aug 25 00:40:00 2026"),
            row("Sat Sep 05 20:00:00 2026"),
            row("Sun Sep 06 21:00:00 2026"),
            row("Mon Sep 07 22:00:00 2026"),
        ];
        let sep7 = NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let aug24 = NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();

        let n = nights(&rows);
        assert_eq!(n[0], (sep7, 1), "newest first");
        assert_eq!(
            n.last().copied(),
            Some((aug24, 3)),
            "the 00:40 fight counts for the 24th"
        );
        assert_eq!(latest(&rows), Some(sep7));

        let night = |d| Filter {
            when: When::Night(d),
            ..Filter::default()
        };
        assert_eq!(in_filter(&rows, &night(aug24)).len(), 3);
        assert_eq!(
            in_filter(
                &rows,
                &Filter {
                    when: When::All,
                    ..Filter::default()
                }
            )
            .len(),
            6
        );
        /* THE LAST N NIGHTS ARE NIGHTS HE PLAYED, not calendar days: Sep 5, 6 and 7 are three
         * nights spanning three days, and Aug 24 is the fourth night however far back it is. */
        assert_eq!(
            in_filter(
                &rows,
                &Filter {
                    when: When::LastNights(3),
                    ..Filter::default()
                }
            )
            .len(),
            3
        );
        assert_eq!(
            in_filter(
                &rows,
                &Filter {
                    when: When::LastNights(4),
                    ..Filter::default()
                }
            )
            .len(),
            6,
            "the fourth night back is the three fights of Aug 24"
        );
        /* A RANGE, BOTH ENDS INCLUDED, and the ends given the wrong way round still work. */
        let sep5 = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        assert_eq!(
            in_filter(
                &rows,
                &Filter {
                    when: When::Range(sep5, sep7),
                    ..Filter::default()
                }
            )
            .len(),
            3
        );
        assert_eq!(
            in_filter(
                &rows,
                &Filter {
                    when: When::Range(sep7, sep5),
                    ..Filter::default()
                }
            )
            .len(),
            3,
            "a range given backwards is the same range"
        );

        assert_eq!(night(sep7).label(), "Sep 7, 2026");
        assert_eq!(night(sep7).short(), "Sep 7");
        assert!(in_filter(&[], &night(sep7)).is_empty());
    }

    /// THE OTHER THREE DIMENSIONS, AND THE COUNT.
    ///
    /// The owner's three questions: "I was doing this in Plane of Sky", "last night's raid", and
    /// "the last two Nagafen fights". The first is a zone, the second is a night and a zone, the
    /// third is a mob and a count.
    ///
    /// WHAT MUTATION MAKES THIS RED: a `last` that takes the OLDEST N, a zone or mob match that is
    /// case sensitive, or an options list that ignores the other dimensions.
    #[test]
    fn a_filter_narrows_by_zone_and_mob_and_keeps_the_most_recent_few() {
        let at = |stamp: &str, zone: &str, mob: &str| FightRow {
            start: stamp.to_owned(),
            end: stamp.to_owned(),
            secs: 30,
            zone: Some(zone.to_owned()),
            headline: Some(mob.to_owned()),
            ..FightRow::default()
        };
        let rows = vec![
            at("Mon Sep 07 21:00:00 2026", "Plane of Sky", "Lord Nagafen"),
            at("Mon Sep 07 21:30:00 2026", "Plane of Sky", "a cloud giant"),
            at("Mon Sep 07 22:00:00 2026", "Plane of Sky", "Lord Nagafen"),
            at("Tue Sep 08 21:00:00 2026", "Nagafen's Lair", "lord nagafen"),
            at(
                "Tue Sep 08 22:00:00 2026",
                "Nagafen's Lair",
                "a fire beetle",
            ),
        ];
        let all = Filter {
            when: When::All,
            ..Filter::default()
        };

        /* A ZONE. */
        let sky = Filter {
            zone: Some(String::from("Plane of Sky")),
            ..all.clone()
        };
        assert_eq!(in_filter(&rows, &sky).len(), 3);

        /* A MOB, FOLDED CASE INSENSITIVELY: three Nagafen fights across two zones. */
        let naggy = Filter {
            mob: Some(String::from("Lord Nagafen")),
            ..all.clone()
        };
        assert_eq!(in_filter(&rows, &naggy).len(), 3);
        assert_eq!(
            in_filter(
                &rows,
                &Filter {
                    mob: Some(String::from("LORD NAGAFEN")),
                    ..all.clone()
                }
            )
            .len(),
            3,
            "the mob match is case sensitive, so one mob is two rows"
        );

        /* THE LAST TWO NAGAFEN FIGHTS: the NEWEST two, in time order. */
        let last_two = Filter {
            last: Some(2),
            ..naggy.clone()
        };
        let got = in_filter(&rows, &last_two);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].start, "Mon Sep 07 22:00:00 2026");
        assert_eq!(
            got[1].start, "Tue Sep 08 21:00:00 2026",
            "`last` took the oldest two, so `the last two Nagafen fights` is the first two"
        );
        /* AND ASKING FOR MORE THAN THERE ARE IS EVERYTHING, not an empty page. */
        assert_eq!(
            in_filter(
                &rows,
                &Filter {
                    last: Some(99),
                    ..naggy.clone()
                }
            )
            .len(),
            3
        );

        /* THE OPTIONS NARROW WITH THE OTHER DIMENSIONS. In Plane of Sky there are two mobs, not
         * three, and Nagafen was fought on two nights, not one. */
        assert_eq!(
            mobs_for(&rows, &sky)
                .iter()
                .map(|(m, _)| m.as_str())
                .collect::<Vec<_>>(),
            vec!["Lord Nagafen", "a cloud giant"],
            "the mob list is not narrowed by the chosen zone"
        );
        assert_eq!(
            zones_for(&rows, &naggy).len(),
            2,
            "Nagafen was fought in two zones and the zone list does not say so"
        );
        assert_eq!(nights_for(&rows, &naggy).len(), 2);
        assert_eq!(
            nights_for(&rows, &sky).len(),
            1,
            "Plane of Sky was one night and the night list offers more"
        );
        /* AND A NIGHT'S ZONE IS THE ONE IT WAS MOSTLY SPENT IN, which is what makes a night row
         * read as a raid night without the app inventing a raid. */
        assert_eq!(
            zone_of_night(&rows, NaiveDate::from_ymd_opt(2026, 9, 7).unwrap()),
            Some(String::from("Plane of Sky"))
        );

        /* THE LABEL SAYS EVERY PART THAT IS SET, in the order a person would say it. */
        assert_eq!(
            Filter {
                when: When::Night(NaiveDate::from_ymd_opt(2026, 9, 7).unwrap()),
                zone: Some(String::from("Plane of Sky")),
                mob: Some(String::from("Lord Nagafen")),
                last: Some(2),
            }
            .label(),
            "Sep 7, 2026 \u{b7} Plane of Sky \u{b7} Lord Nagafen \u{b7} last 2"
        );
        assert!(!all.narrowed());
        assert!(sky.narrowed());
    }
}
