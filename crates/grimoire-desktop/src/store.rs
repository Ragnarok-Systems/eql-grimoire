//! FIGHTS KEPT BETWEEN RUNS, so a scope can be wider than one launch.
//!
//! # WHAT THIS UNBLOCKS
//!
//! Everything on the dashboard that says a range. `Ingest::fights` is folded fresh at every launch
//! from the tail of one file, so before this existed the widest question the app could answer was
//! "what has this launch read". A raid night, a week, a mob's hit points averaged over twenty-three
//! kills: all of them are the same missing piece, which is a fight that outlives the process.
//!
//! # THE KEY IS THE FIGHT'S OWN START STAMP, NOT A FRESH ID
//!
//! A generated id would make the same fight a new row every time the log was re-read, and this app
//! re-reads constantly: a rescan on character switch, a fresh bootstrap on every launch, a
//! re-fold of the live window on every poll. The natural key is `(character, server, start)` and it
//! is DERIVED from the bytes, so folding the same fight twice produces the same key twice and the
//! second write is a no-op. That is what makes [`Store::append`] safe to call on every poll.
//!
//! THE SECOND IS FINE AS A GRAIN because the log stamps to the second and one character cannot
//! start two fights inside one. Two characters can, which is why the character and server are in
//! the key and in the path.
//!
//! # ONE FILE PER CHARACTER PER MONTH
//!
//! `fights/<character>_<server>/YYYY-MM.jsonl`, one JSON object per line, appended.
//!
//! SHARDED BY MONTH SO A RANGE QUERY READS ONLY THE RANGE. "Last 30 days" touches two files
//! whatever the history behind them; a single file would grow without bound and be read whole to
//! answer any question at all.
//!
//! JSON LINES BECAUSE APPEND IS THE EVERYDAY WRITE. A fight's numbers never change once folded,
//! and a torn line at the end of a crashed write costs one fight rather than the file:
//! [`Store::read_month`] skips lines it cannot parse and says how many.
//!
//! THE ONE REWRITE IS [`Store::refill`], and it only ever ADDS what an older build could not know
//! (which group a fight was fought in, and the reader's pets). It never touches a number, it keeps
//! every line it does not change byte for byte, and it keeps a copy of the month as it was.
//!
//! # IT WRITES UNDER `%APPDATA%`, WHICH IS THE OWNER'S OWN DATA
//!
//! Beside `settings.json`, in `eql-grimoire`. TESTS MUST NEVER GO THERE: every test in this module
//! takes an explicit root under a temp dir, and [`Store::at`] is what makes that possible. The
//! no-argument [`Store::app_data`] is the production path and is not reachable from a test.
use crate::fights::FightRow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Which character's fights these are. The log is per character and server, so the store is too.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Owner {
    pub character: String,
    pub server: String,
}

impl Owner {
    /// The folder name, with anything that is not plainly safe in a path replaced.
    ///
    /// NOT A HASH, because a person opening the folder should recognise it. EverQuest names are
    /// letters, but the server part comes off a file name and this app has already met a log
    /// called `eqlog_Reviir_qeynos 1.txt`.
    fn dir(&self) -> String {
        let keep = |s: &str| -> String {
            s.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect()
        };
        format!("{}_{}", keep(&self.character), keep(&self.server))
    }
}

/// How many fights a write added, and how many it skipped as already stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wrote {
    pub added: usize,
    pub already: usize,
}

/// What a refill changed. See [`Store::refill`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refilled {
    /// Stored fights that had no group and were given the one the whole log proves.
    pub grouped: usize,
    /// Stored fights that had no pets and were given the reader's.
    pub petted: usize,
    /// Stored fights whose start stamp matched an offered fight but whose span did not. Left
    /// exactly as they were: see [`same_fight`].
    pub differs: usize,
}

/// THE MARKER TAG FOR THE GROUP REFILL. A log named after this tag in [`Store::mark_refilled`] has
/// been read whole for its groups and is not read whole again.
///
/// A TAG AND NOT A BOOL, so a later field an older build could not know gets a pass of its own
/// instead of being skipped because a different refill already ran.
pub const GROUP_REFILL: &str = "group-v1";

/// Where the refill markers live, one per owner, beside the month files and not matching them.
const REFILLED: &str = "refilled.txt";

/// IS THIS STORED ROW THE FIGHT THAT WAS OFFERED, and not merely a fight that started on the same
/// second?
///
/// THE STAMP IS THE STORE'S KEY AND IT IS NOT ENOUGH HERE. A refill attaches a group to a row, and
/// attaching it to a DIFFERENT span of combat is this app inventing a fact. The same start with a
/// different end, length, damage or line count happens when a row was folded under a different
/// quiet window by an older build, and that row is left as it was rather than half-updated.
fn same_fight(stored: &FightRow, offered: &FightRow) -> bool {
    stored.start == offered.start
        && stored.end == offered.end
        && stored.secs == offered.secs
        && stored.damage == offered.damage
        && stored.lines == offered.lines
}

/// ONE MONTH FILE WITH ITS KNOWN GAPS FILLED, and what changed.
///
/// PURE, so every rule is testable without a disk. The rules are the whole of what a refill may do:
///
///   * ONLY A GAP IS FILLED. A stored `group` of `None` takes an offered `Some`; stored empty `pets`
///     take offered ones. A row that already says something keeps saying it, because a second
///     opinion from a later read is not more true than the first.
///   * ONLY THE SAME FIGHT. See [`same_fight`].
///   * NOTHING IS ADDED. An offered fight the month does not hold is not written: adding fights is
///     [`Store::append`]'s job and it has its own rules about which ones may be kept.
///   * EVERY OTHER LINE IS KEPT BYTE FOR BYTE, torn lines included. A line this build cannot read
///     may be one a later build can.
fn refill_text(text: &str, offered: &BTreeMap<&str, &FightRow>) -> (String, Refilled) {
    let mut did = Refilled::default();
    let mut body = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        let Ok(stored) = serde_json::from_str::<FightRow>(bare) else {
            body.push_str(line);
            continue;
        };
        let Some(new) = offered.get(stored.start.as_str()) else {
            body.push_str(line);
            continue;
        };
        if !same_fight(&stored, new) {
            did.differs += 1;
            body.push_str(line);
            continue;
        }
        let grouped = stored.group.is_none() && new.group.is_some();
        let petted = stored.pets.is_empty() && !new.pets.is_empty();
        if !grouped && !petted {
            body.push_str(line);
            continue;
        }
        let mut up = stored;
        if grouped {
            up.group = new.group.clone();
        }
        if petted {
            up.pets = new.pets.clone();
        }
        /* COUNTED ONLY ONCE THE LINE IS WRITTEN, so a row that would not serialise is reported as
         * unchanged, which is what the file then says. */
        match serde_json::to_string(&up) {
            Ok(json) => {
                body.push_str(&json);
                body.push('\n');
                did.grouped += usize::from(grouped);
                did.petted += usize::from(petted);
            }
            Err(_) => body.push_str(line),
        }
    }
    (body, did)
}

/// FIGHTS ON DISK.
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The production store, beside `settings.json`.
    ///
    /// `None` when the platform has no config dir, which is the same answer `Settings::path` gives
    /// and the same reason: an app that guessed a location would write somebody's fights somewhere
    /// they will never find them.
    pub fn app_data() -> Option<Store> {
        dirs::config_dir().map(|d| Store {
            root: d.join(crate::settings::APP_DIR).join("fights"),
        })
    }

    /// A store at an explicit root. THE ONLY CONSTRUCTOR A TEST MAY USE.
    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    /// Where this store keeps its files.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// WHICH MONTH A FIGHT BELONGS TO, from the stamp the log printed.
    ///
    /// PARSED OFF THE ENGINE'S OWN STAMP rather than from a clock. `FightRow::start` is the log's
    /// text, `Wed Jul 15 23:16:50 2026`, and the fight belongs to the month the LOG says, not the
    /// month the app happens to be running in. A raid that crosses midnight into a new month files
    /// each fight where its own stamp puts it.
    fn month_of(start: &str) -> Option<String> {
        /* `Www Mmm DD HH:MM:SS YYYY`, split on whitespace. Two tokens are wanted and they are not
         * adjacent, so this is an index rather than a `split_once`. */
        let mut it = start.trim_start_matches('[').split_whitespace();
        let _dow = it.next()?;
        let mon = it.next()?;
        let _day = it.next()?;
        let _time = it.next()?;
        let year = it.next()?.trim_end_matches(']');
        let n = match mon {
            "Jan" => "01",
            "Feb" => "02",
            "Mar" => "03",
            "Apr" => "04",
            "May" => "05",
            "Jun" => "06",
            "Jul" => "07",
            "Aug" => "08",
            "Sep" => "09",
            "Oct" => "10",
            "Nov" => "11",
            "Dec" => "12",
            _ => return None,
        };
        if year.len() != 4 || !year.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some(format!("{year}-{n}"))
    }

    fn month_file(&self, who: &Owner, month: &str) -> PathBuf {
        self.root.join(who.dir()).join(format!("{month}.jsonl"))
    }

    /// EVERY START STAMP ALREADY STORED FOR THIS MONTH.
    ///
    /// THE DEDUPE KEY AND NOTHING ELSE IS READ, because this runs on the write path and the whole
    /// row is not needed to know it is there.
    fn stamps_in(&self, who: &Owner, month: &str) -> BTreeSet<String> {
        let Ok(text) = fs::read_to_string(self.month_file(who, month)) else {
            return BTreeSet::new();
        };
        text.lines()
            .filter_map(|l| serde_json::from_str::<FightRow>(l).ok())
            .map(|r| r.start)
            .collect()
    }

    /// STORE THESE FIGHTS, SKIPPING ANY ALREADY HELD.
    ///
    /// SAFE TO CALL WITH THE SAME FIGHTS REPEATEDLY, which is the point: the caller re-folds the
    /// same window on every poll and cannot easily know which rows are new.
    ///
    /// A FIGHT STILL RUNNING IS NOT STORED. `Ended::EndOfLog` means the text ran out, which from
    /// the end of a file being appended to is what an OPEN fight looks like; storing it would
    /// write a half-fight whose totals grow, and the dedupe key would then keep the half and
    /// reject the whole. The caller passes only fights it knows are closed.
    pub fn append(&self, who: &Owner, rows: &[FightRow]) -> Result<Wrote, String> {
        let mut out = Wrote::default();
        if rows.is_empty() {
            return Ok(out);
        }
        let dir = self.root.join(who.dir());
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

        /* GROUPED BY MONTH SO EACH FILE IS OPENED ONCE, and so the existing stamps for that month
         * are read once rather than per row. */
        let mut by_month: std::collections::BTreeMap<String, Vec<&FightRow>> = Default::default();
        for r in rows {
            let Some(m) = Self::month_of(&r.start) else {
                /* A STAMP THIS BUILD CANNOT PLACE IS NOT FILED UNDER A GUESS. The engine already
                 * counts unreadable stamps; a fight whose own start will not parse has no month
                 * and is dropped here rather than landing in whatever month is current. */
                continue;
            };
            by_month.entry(m).or_default().push(r);
        }

        for (month, rows) in by_month {
            let have = self.stamps_in(who, &month);
            let path = self.month_file(who, &month);
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            for r in rows {
                if have.contains(&r.start) {
                    out.already += 1;
                    continue;
                }
                let line = serde_json::to_string(r).map_err(|e| e.to_string())?;
                writeln!(f, "{line}").map_err(|e| format!("{}: {e}", path.display()))?;
                out.added += 1;
            }
        }
        Ok(out)
    }

    /// FILL IN WHAT STORED FIGHTS COULD NOT KNOW WHEN THEY WERE WRITTEN, from a fresh fold of the log.
    ///
    /// # WHY THE STORE NEEDS THIS AT ALL
    ///
    /// [`Store::append`] refuses a fight it already holds, by start stamp, and that is what makes it
    /// safe to call on every poll. It also means a row written by a build that did not read the
    /// group keeps saying `group not known` for ever, however many times a newer build folds the
    /// same fight and knows better. On the owner's machine that was every one of the 327 fights
    /// stored before the group reader existed.
    ///
    /// # WHAT IT MAY CHANGE
    ///
    /// See [`refill_text`]: gaps only, in the same fight only, nothing added, every other byte kept.
    ///
    /// # HOW IT WRITES, BECAUSE THIS IS THE OWNER'S DATA
    ///
    ///   * A MONTH WITH NOTHING TO FILL IS NOT WRITTEN AT ALL.
    ///   * THE MONTH AS IT WAS IS COPIED to `YYYY-MM.jsonl.before-refill` the first time it is
    ///     rewritten, and never overwritten after, so the original survives any number of refills.
    ///   * THE NEW TEXT GOES TO A TEMP FILE AND IS RENAMED OVER THE MONTH, so a crash leaves the old
    ///     file or the new one and never half of each.
    ///   * A MONTH THAT GREW WHILE IT WAS BEING REFILLED IS LEFT ALONE and this returns an error. A
    ///     second ingest (the pop-out has its own) may append a fight between the read and the
    ///     rename, and renaming over it would delete that fight. The next launch tries again.
    pub fn refill(&self, who: &Owner, rows: &[FightRow]) -> Result<Refilled, String> {
        let mut by_month: BTreeMap<String, BTreeMap<&str, &FightRow>> = BTreeMap::new();
        for r in rows {
            /* AN OFFER WITH NOTHING TO GIVE IS NOT AN OFFER, and skipping it here means a month
             * whose every fight is still not known is never even read. */
            if r.group.is_none() && r.pets.is_empty() {
                continue;
            }
            let Some(m) = Self::month_of(&r.start) else {
                continue;
            };
            by_month.entry(m).or_default().insert(r.start.as_str(), r);
        }

        let mut total = Refilled::default();
        for (month, offered) in by_month {
            let path = self.month_file(who, &month);
            let text = match fs::read_to_string(&path) {
                Ok(t) => t,
                /* NOTHING STORED FOR THAT MONTH, so there is nothing to fill. */
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("{}: {e}", path.display())),
            };
            let (body, did) = refill_text(&text, &offered);
            total.differs += did.differs;
            if did.grouped + did.petted == 0 {
                continue;
            }
            Self::replace(&path, text.len() as u64, &body)?;
            total.grouped += did.grouped;
            total.petted += did.petted;
        }
        Ok(total)
    }

    /// PUT `body` WHERE `path` WAS, keeping the original once and refusing if the file moved.
    ///
    /// `read` IS THE LENGTH THE CALLER READ. Appends only ever grow a month file, so a different
    /// length now means somebody wrote to it after that read, and the rename would lose what they
    /// wrote. See [`Store::refill`].
    fn replace(path: &Path, read: u64, body: &str) -> Result<(), String> {
        let backup = path.with_extension("jsonl.before-refill");
        if !backup.exists() {
            fs::copy(path, &backup).map_err(|e| format!("{}: {e}", backup.display()))?;
        }
        /* ONE TEMP NAME PER CALL, so two ingests refilling the same month cannot write into one
         * file. The name does not end in `.jsonl`, so [`Store::months`] never lists it. */
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let tmp = path.with_extension(format!(
            "jsonl.refill-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&tmp, body).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let now = fs::metadata(path).map(|m| m.len());
        if now.as_ref().ok() != Some(&read) {
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "{} was written to while it was being refilled, so it was left as it was",
                path.display()
            ));
        }
        fs::rename(&tmp, path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("{}: {e}", path.display())
        })
    }

    /// HAS THIS LOG ALREADY BEEN READ WHOLE FOR THIS REFILL? See [`GROUP_REFILL`].
    pub fn refilled(&self, who: &Owner, tag: &str, log: &str) -> bool {
        let want = format!("{tag} {log}");
        fs::read_to_string(self.root.join(who.dir()).join(REFILLED))
            .is_ok_and(|t| t.lines().any(|l| l == want))
    }

    /// SAY THAT THIS LOG HAS BEEN READ WHOLE FOR THIS REFILL, so the next launch does not do it
    /// again. Idempotent: a log already marked is not marked twice.
    pub fn mark_refilled(&self, who: &Owner, tag: &str, log: &str) -> Result<(), String> {
        if self.refilled(who, tag, log) {
            return Ok(());
        }
        let dir = self.root.join(who.dir());
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(REFILLED);
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        writeln!(f, "{tag} {log}").map_err(|e| format!("{}: {e}", path.display()))
    }

    /// EVERY FIGHT IN ONE MONTH, and how many lines could not be read.
    ///
    /// A TORN LINE COSTS ONE FIGHT AND NOT THE FILE. An append interrupted by a crash or a full
    /// disk leaves a partial last line; skipping it and SAYING SO is the honest reading, and the
    /// count is what lets a screen tell "no fights" apart from "the file is damaged".
    pub fn read_month(&self, who: &Owner, month: &str) -> (Vec<FightRow>, usize) {
        let Ok(text) = fs::read_to_string(self.month_file(who, month)) else {
            return (Vec::new(), 0);
        };
        let mut rows = Vec::new();
        let mut torn = 0;
        for l in text.lines() {
            if l.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<FightRow>(l) {
                Ok(r) => rows.push(r),
                Err(_) => torn += 1,
            }
        }
        (rows, torn)
    }

    /// EVERY MONTH THIS CHARACTER HAS FIGHTS IN, oldest first.
    pub fn months(&self, who: &Owner) -> Vec<String> {
        let Ok(rd) = fs::read_dir(self.root.join(who.dir())) else {
            return Vec::new();
        };
        let mut out: Vec<String> = rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.strip_suffix(".jsonl").map(str::to_owned)
            })
            .collect();
        out.sort();
        out
    }

    /// EVERY CHARACTER THIS STORE HOLDS, by folder name.
    pub fn characters(&self) -> Vec<String> {
        let Ok(rd) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut out: Vec<String> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }

    /// EVERY FIGHT THIS CHARACTER HAS, oldest first, and the torn-line count across the lot.
    ///
    /// NO DATE FILTER HERE ON PURPOSE. The months are the coarse filter and a caller that wants a
    /// day works on stamps, which are the log's own text; a range argument here would need this
    /// module to own a calendar, and `screens::reports` already owns one.
    pub fn all(&self, who: &Owner) -> (Vec<FightRow>, usize) {
        let mut rows = Vec::new();
        let mut torn = 0;
        for m in self.months(who) {
            let (r, t) = self.read_month(who, &m);
            rows.extend(r);
            torn += t;
        }
        /* IN TIME ORDER AND NOT IN THE ORDER THE WEEKDAYS SPELL. This compared the stamp
         * STRINGS, so `Thu Aug 06` sorted before `Wed Jul 15` and a look back page fed
         * this list had its nights in alphabetical order. */
        crate::screens::night::sort_chronologically(&mut rows);
        (rows, torn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fights::{Fighter, Who};

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("grimoire-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    fn who() -> Owner {
        Owner {
            character: "Reviir".into(),
            server: "neriak".into(),
        }
    }

    fn fight(start: &str, damage: u64) -> FightRow {
        FightRow {
            start: start.to_owned(),
            end: start.to_owned(),
            secs: 30,
            damage,
            headline: Some("a thunder spirit princess".into()),
            fighters: vec![Fighter {
                who: Who::You,
                dealt: damage,
                ..Fighter::default()
            }],
            ..FightRow::default()
        }
    }

    /// DEFECT: THE SAME FIGHT STORED AGAIN ON EVERY POLL.
    ///
    /// This app re-reads constantly: a fresh bootstrap each launch, a rescan on character switch, a
    /// re-fold of the live window on every poll that brought lines. A generated id would make each
    /// of those a new row, and a night's history would be the same twenty fights written hundreds
    /// of times. The key is the fight's own start stamp, which the bytes decide.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `have.contains` check.
    #[test]
    fn storing_the_same_fight_twice_stores_it_once() {
        let s = Store::at(tmp("dupe"));
        let rows = vec![
            fight("Wed Jul 15 23:16:50 2026", 100),
            fight("Wed Jul 15 23:20:00 2026", 200),
        ];

        let first = s.append(&who(), &rows).expect("write");
        assert_eq!(
            first,
            Wrote {
                added: 2,
                already: 0
            }
        );

        let again = s.append(&who(), &rows).expect("write");
        assert_eq!(
            again,
            Wrote {
                added: 0,
                already: 2
            },
            "a re-fold of the same window wrote the same fights a second time"
        );

        let (back, torn) = s.all(&who());
        assert_eq!(back.len(), 2);
        assert_eq!(torn, 0);
    }

    /// A FIGHT IS FILED UNDER THE MONTH ITS OWN STAMP NAMES, not the month the app is running in.
    ///
    /// A raid that crosses midnight into a new month files each fight where the log puts it, which
    /// is the only reading that survives somebody importing an old log next year.
    #[test]
    fn a_fight_is_filed_under_the_month_the_log_printed() {
        let s = Store::at(tmp("months"));
        s.append(
            &who(),
            &[
                fight("Tue Dec 31 23:59:00 2024", 1),
                fight("Wed Jan 01 00:01:00 2025", 2),
            ],
        )
        .expect("write");

        assert_eq!(s.months(&who()), vec!["2024-12", "2025-01"]);
        assert_eq!(s.read_month(&who(), "2024-12").0.len(), 1);
        assert_eq!(s.read_month(&who(), "2025-01").0.len(), 1);
    }

    /// A STAMP THIS BUILD CANNOT PLACE IS DROPPED, NOT FILED UNDER A GUESS.
    ///
    /// Filing it under the current month would put a fight in a range it does not belong to, and
    /// every total over that range would then be wrong by however much it held.
    #[test]
    fn a_fight_whose_stamp_will_not_parse_is_not_filed_under_a_guess() {
        let s = Store::at(tmp("badstamp"));
        let out = s
            .append(&who(), &[fight("not a stamp at all", 500)])
            .expect("write");
        assert_eq!(out, Wrote::default(), "{out:?}");
        assert!(s.months(&who()).is_empty());
    }

    /// DEFECT: A TORN LAST LINE COSTING THE WHOLE MONTH.
    ///
    /// An append interrupted by a crash or a full disk leaves a partial line. Parsing the file as
    /// one document would lose every fight in it; skipping the line and REPORTING it keeps the rest
    /// and lets a screen tell "no fights" apart from "this file is damaged".
    ///
    /// WHAT MUTATION MAKES THIS RED: propagating the parse error instead of counting it.
    #[test]
    fn a_torn_line_costs_one_fight_and_is_reported() {
        let s = Store::at(tmp("torn"));
        s.append(&who(), &[fight("Wed Jul 15 23:16:50 2026", 100)])
            .expect("write");

        /* Simulate the crash: append a partial record. */
        let path = s.month_file(&who(), "2026-07");
        let mut f = OpenOptions::new().append(true).open(&path).expect("open");
        writeln!(f, "{{\"start\":\"Wed Jul 15 23:20").expect("tear");
        drop(f);

        let (rows, torn) = s.read_month(&who(), "2026-07");
        assert_eq!(rows.len(), 1, "the whole month was lost to one bad line");
        assert_eq!(torn, 1, "the damage was not reported");
    }

    /// EVERY FIELD SURVIVES THE ROUND TRIP, or a stored fight is a different fight from a folded
    /// one and the dashboard would disagree with the Live page about the same night.
    #[test]
    fn a_fight_comes_back_exactly_as_it_went_in() {
        let s = Store::at(tmp("roundtrip"));
        let mut f = fight("Wed Jul 15 23:16:50 2026", 12_976);
        f.fighters.push(Fighter {
            who: Who::Named("a dry bone skeleton".into()),
            taken: 4_743,
            first_taken_at: Some(2),
            last_taken_at: Some(30),
            class: None,
            ..Fighter::default()
        });
        f.zone = Some("Nektulos Forest".into());
        f.cut = true;
        f.group = Some(vec!["Zarmin".into()]);
        f.pets = vec!["Gabtik".into()];
        /* AND SOLO IS NOT NOT KNOWN ON THE WAY BACK. An empty list and `None` are different answers
         * about the same fight, and a store that wrote one and read the other would put everybody
         * back on a solo fight's meter. */
        let solo = FightRow {
            group: Some(Vec::new()),
            ..fight("Wed Jul 15 23:20:00 2026", 40)
        };

        s.append(&who(), &[f.clone(), solo.clone()]).expect("write");
        let (back, _) = s.all(&who());
        assert_eq!(
            back.iter().map(|r| r.group.clone()).collect::<Vec<_>>(),
            vec![f.group.clone(), solo.group.clone()],
            "the group did not survive the store, so a stored fight names a different group, or \
             none, from the one the fold found"
        );
        assert_eq!(back, vec![f, solo]);
    }

    /// DEFECT: A HISTORY STORED BEFORE `FightRow::group` EXISTED STOPS LOADING, OR LOADS AS SOLO.
    ///
    /// The line below is a row as the build before that field wrote it: every field it had, and
    /// no `group`. There are months of these on the owner's disk. Two ways to get it wrong:
    ///
    ///   * A REQUIRED FIELD. `read_month` counts every old row as torn, and the dashboard's nights
    ///     go empty with a damage count beside them.
    ///   * A DEFAULT THAT MEANS SOLO. The whole history loads as `Some(vec![])`, and a group filter
    ///     then hides every player in every stored fight, on a claim the older build never made.
    ///
    /// WHAT MUTATION MAKES THIS RED: `#[serde(default = ..)]` naming a function that returns
    /// `Some(Vec::new())`.
    ///
    /// WHAT DOES NOT, SAID OUT LOUD: deleting `#[serde(default)]` on its own. serde's derive already
    /// reads a missing `Option` field as `None`, so the attribute states what serde does anyway;
    /// it stays because a new field here says its default where a reader can see it. This test
    /// guards the value, not the attribute.
    #[test]
    fn a_row_stored_before_the_group_field_existed_loads_as_not_known() {
        let s = Store::at(tmp("pre-group"));
        let old = concat!(
            r#"{"start":"Wed Jul 15 23:16:50 2026","end":"Wed Jul 15 23:17:20 2026","secs":30,"#,
            r#""damage":100,"deaths":1,"lines":3,"ended":"quiet","#,
            r#""headline":"a thunder spirit princess","fighters":[],"moments":[],"#,
            r#""zone":"Nektulos Forest","zone_gap":null,"cut":false}"#
        );
        assert!(
            !old.contains("group"),
            "the planted row names the new field, so it is not an old row"
        );
        let path = s.month_file(&who(), "2026-07");
        fs::create_dir_all(path.parent().expect("a month file has a folder")).expect("folder");
        fs::write(&path, format!("{old}\n")).expect("plant the old row");

        let (rows, torn) = s.all(&who());
        assert_eq!(
            torn, 0,
            "a row written before `group` existed no longer parses, so every fight an older build \
             stored is now counted as damage"
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].group, None,
            "a row from a build that never looked at the group loaded as a claim about it"
        );
        assert!(
            rows[0].pets.is_empty(),
            "a row from a build that never read a pet's answer loaded with a pet in it"
        );
        assert_eq!(rows[0].damage, 100, "and the rest of the row is the row");
    }

    /// A FIGHT OFFERED TO A REFILL WITH A GROUP AND A PET, and otherwise the stored row itself.
    fn known(mut r: FightRow, members: &[&str], pets: &[&str]) -> FightRow {
        r.group = Some(members.iter().map(|m| (*m).to_owned()).collect());
        r.pets = pets.iter().map(|p| (*p).to_owned()).collect();
        r
    }

    /// DEFECT: A STORED FIGHT SAID `GROUP NOT KNOW` FOR EVER, HOWEVER WELL A NEWER BUILD KNEW.
    ///
    /// [`Store::append`] refuses a start stamp it holds, so every fight stored before the group
    /// reader existed kept its empty group whatever later folds of the same bytes proved. That was
    /// all 327 of the owner's stored fights.
    ///
    /// # WHAT IS ASSERTED, AND EACH HALF IS A WAY TO DAMAGE THE OWNER'S DATA
    ///
    ///   * The same fight is given its group and its pet.
    ///   * A fight on the same second with a different span is NOT: the group of one span of combat
    ///     attached to another is an invented fact.
    ///   * A fight the month does not hold is not added.
    ///   * Every other line, the torn one included, comes back byte for byte.
    ///   * The month as it was is kept, and a LATER refill does not overwrite that copy with its own
    ///     starting point.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the [`same_fight`] check; a refill that appends the
    /// offered fights it does not hold; dropping the torn line; a backup written on every
    /// refill rather than the first.
    #[test]
    fn a_refill_gives_a_stored_fight_its_group_and_leaves_every_other_byte_alone() {
        let s = Store::at(tmp("refill"));
        let a = fight("Wed Jul 15 23:16:50 2026", 100);
        let b = fight("Wed Jul 15 23:20:00 2026", 200);
        let c = fight("Wed Jul 15 23:30:00 2026", 300);
        s.append(&who(), &[a.clone(), b.clone(), c.clone()])
            .expect("store the old rows");
        let path = s.month_file(&who(), "2026-07");
        {
            let mut f = OpenOptions::new().append(true).open(&path).expect("open");
            writeln!(f, "{{\"start\":\"Wed Jul 15 23:4").expect("plant a torn line");
        }
        let before = fs::read_to_string(&path).expect("read");

        /* B IS THE SAME SECOND AND A DIFFERENT FIGHT; D IS A FIGHT THE MONTH NEVER HELD. */
        let mut b_other = known(b.clone(), &["Zarmin"], &[]);
        b_other.damage = 999;
        let d = known(fight("Wed Jul 15 23:50:00 2026", 400), &["Zarmin"], &[]);
        let did = s
            .refill(
                &who(),
                &[known(a.clone(), &["Zarmin"], &["Jebobab"]), b_other, d],
            )
            .expect("refill");
        assert_eq!(
            did,
            Refilled {
                grouped: 1,
                petted: 1,
                differs: 1
            },
            "the refill did not do exactly one fill of each and refuse the different fight"
        );

        let (rows, torn) = s.all(&who());
        assert_eq!(
            rows.len(),
            3,
            "the refill added a fight the store did not hold"
        );
        assert_eq!(torn, 1, "the torn line was dropped or mended");
        let got = |start: &str| rows.iter().find(|r| r.start == start).expect("a row");
        assert_eq!(got(&a.start).group, Some(vec!["Zarmin".to_owned()]));
        assert_eq!(got(&a.start).pets, vec!["Jebobab".to_owned()]);
        assert_eq!(got(&a.start).damage, 100, "the refill restated a number");
        assert_eq!(
            got(&b.start).group,
            None,
            "a group proved for one span of combat was written onto a different fight that \
             started on the same second"
        );

        let after = fs::read_to_string(&path).expect("read");
        let (old, new): (Vec<&str>, Vec<&str>) =
            (before.lines().collect(), after.lines().collect());
        assert_eq!(old.len(), new.len());
        assert_ne!(old[0], new[0], "the filled row was not rewritten");
        assert_eq!(
            old[1..],
            new[1..],
            "lines the refill had no business changing came back different"
        );
        let backup = path.with_extension("jsonl.before-refill");
        assert_eq!(
            fs::read_to_string(&backup).expect("the month as it was is kept"),
            before
        );

        /* A LATER REFILL KEEPS THE FIRST COPY, which is the only one that is the original. */
        let did = s
            .refill(&who(), &[known(c.clone(), &[], &[])])
            .expect("refill again");
        assert_eq!(
            did.grouped, 1,
            "a solo fight, which is a known group, was not filled"
        );
        assert_eq!(
            fs::read_to_string(&backup).expect("backup"),
            before,
            "the second refill overwrote the original month with its own starting point"
        );
        assert_eq!(
            s.months(&who()),
            vec!["2026-07".to_owned()],
            "the backup or a temp file is being read as a month of fights"
        );
    }

    /// A ROW THAT ALREADY SAYS SOMETHING KEEPS SAYING IT, and a month with nothing to fill is not
    /// written at all.
    ///
    /// WHAT MUTATION MAKES THIS RED: filling a group that is already `Some`; filling pets that are
    /// already there; writing a month (or its backup) when nothing changed.
    #[test]
    fn a_refill_never_overwrites_what_a_stored_row_already_says() {
        let s = Store::at(tmp("refill-keeps"));
        let a = known(
            fight("Wed Jul 15 23:16:50 2026", 100),
            &["Hert"],
            &["Bzzazzt"],
        );
        s.append(&who(), std::slice::from_ref(&a)).expect("store");
        let path = s.month_file(&who(), "2026-07");
        let before = fs::read_to_string(&path).expect("read");

        let did = s
            .refill(&who(), &[known(a.clone(), &["Zarmin"], &["Jebobab"])])
            .expect("refill");
        assert_eq!(
            did,
            Refilled::default(),
            "a row that already knew was changed"
        );
        assert_eq!(fs::read_to_string(&path).expect("read"), before);
        assert!(
            !path.with_extension("jsonl.before-refill").exists(),
            "a month nothing was filled in was copied as if it had been rewritten"
        );
    }

    /// A MONTH THAT MOVED UNDER THE REFILL IS NOT RENAMED OVER.
    ///
    /// The pop-out has its own ingest on the same store, and a fight it appends between the
    /// refill's read and its rename would be deleted by the rename.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the length check in [`Store::replace`], or leaving
    /// its temp file behind when it refuses.
    #[test]
    fn a_refill_does_not_rename_over_a_month_that_grew_while_it_read() {
        let s = Store::at(tmp("refill-race"));
        s.append(&who(), &[fight("Wed Jul 15 23:16:50 2026", 100)])
            .expect("store");
        let path = s.month_file(&who(), "2026-07");
        let before = fs::read_to_string(&path).expect("read");

        /* THE CALLER READ ONE BYTE LESS THAN IS THERE NOW, which is an append that landed. */
        let err = Store::replace(&path, before.len() as u64 - 1, "replacement")
            .expect_err("a month that grew under the refill was renamed over");
        assert!(err.contains("written to"), "{err}");
        assert_eq!(fs::read_to_string(&path).expect("read"), before);
        let left: Vec<String> = fs::read_dir(path.parent().expect("folder"))
            .expect("list")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("refill-"))
            .collect();
        assert!(
            left.is_empty(),
            "the refused refill left its temp file: {left:?}"
        );
    }

    /// A LOG IS READ WHOLE ONCE PER REFILL, and the marker cannot be mistaken for anything else.
    ///
    /// WHAT MUTATION MAKES THIS RED: a marker that ignores the tag or the log's name, or one
    /// written again every time it is set.
    #[test]
    fn a_refill_marker_is_per_log_and_per_refill() {
        let s = Store::at(tmp("refill-marker"));
        let log = "eqlog_Reviir_neriak.txt";
        assert!(!s.refilled(&who(), GROUP_REFILL, log));
        s.mark_refilled(&who(), GROUP_REFILL, log).expect("mark");
        s.mark_refilled(&who(), GROUP_REFILL, log)
            .expect("mark again");
        assert!(s.refilled(&who(), GROUP_REFILL, log));
        assert!(
            !s.refilled(&who(), GROUP_REFILL, "eqlog_Reviir_neriak 1.txt"),
            "marking one log marked another"
        );
        assert!(
            !s.refilled(&who(), "pets-v2", log),
            "one refill's marker skipped a different refill"
        );
        let text = fs::read_to_string(tmp_marker(&s)).expect("marker");
        assert_eq!(
            text.lines().count(),
            1,
            "the same log was marked twice: {text:?}"
        );
        assert!(
            s.months(&who()).is_empty(),
            "the marker is being read as a month"
        );
    }

    fn tmp_marker(s: &Store) -> PathBuf {
        s.root.join(who().dir()).join(REFILLED)
    }

    /// TWO CHARACTERS DO NOT SHARE A FILE, and the same stamp from each is two fights.
    ///
    /// Two people can start a fight in the same second, and the dedupe key is the stamp; without
    /// the character in the path the second one would be swallowed as a duplicate.
    #[test]
    fn two_characters_keep_their_own_fights() {
        let s = Store::at(tmp("two"));
        let a = who();
        let b = Owner {
            character: "Poguhy".into(),
            server: "neriak".into(),
        };
        let stamp = "Wed Jul 15 23:16:50 2026";
        s.append(&a, &[fight(stamp, 100)]).expect("write");
        let out = s.append(&b, &[fight(stamp, 999)]).expect("write");

        assert_eq!(out.added, 1, "the second character's fight was swallowed");
        assert_eq!(s.all(&a).0.len(), 1);
        assert_eq!(s.all(&b).0.len(), 1);
        assert_eq!(s.all(&b).0[0].damage, 999);
        assert_eq!(s.characters().len(), 2);
    }

    /// A LOG NAME WITH A SPACE IN IT DOES NOT BECOME TWO PATH SEGMENTS.
    ///
    /// This is not hypothetical: the owner's own folder holds `eqlog_Reviir_qeynos 1.txt`.
    #[test]
    fn a_server_name_with_a_space_stays_one_folder() {
        let s = Store::at(tmp("space"));
        let odd = Owner {
            character: "Reviir".into(),
            server: "qeynos 1".into(),
        };
        s.append(&odd, &[fight("Wed Jul 15 23:16:50 2026", 5)])
            .expect("write");
        let dirs = s.characters();
        assert_eq!(dirs, vec!["Reviir_qeynos_1"]);
        assert_eq!(s.all(&odd).0.len(), 1);
    }
}
