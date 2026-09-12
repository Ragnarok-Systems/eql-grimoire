//! Screen: LOGS. The raw material, and what the app did and did not do with it.
//!
//! # The page that has the most to say when every other page is blank
//!
//! Every other reader in this app is downstream of one decision the ingest makes in silence: which
//! `eqlog_*.txt` in the Logs folder it is going to tail. That decision has no control anywhere, it
//! is never printed in words, and it is re-made on every poll. When it goes the way the reader did
//! not expect, the consequence is not an error. It is a Fights table with nothing in it, a live
//! meter that never moves, and a kill tracker at zero, on a machine where the game is plainly
//! writing a log. Nothing on screen anywhere says "the file being read belongs to your other
//! character", because until this page nothing on screen said which file was being read at all.
//!
//! So this page is inverted with respect to the rest of the app: it is at its most useful exactly
//! when the ingest has found the least. It never bails out to an empty state of its own. A folder
//! that does not exist, a folder with nothing in it, a file that will not open, are all things this
//! page has content ABOUT, and each of them is the reason somebody opened it.
//!
//! # Three facts that exist in the ingest and appear nowhere in the app
//!
//! `Ingest::tail_start` is the byte the first pass began at. Non zero means the file was longer
//! than the bootstrap cap and the read started partway in, which makes the oldest fight in the
//! history a FLOOR rather than a total. `crate::fights::FightRow::cut` carries that consequence
//! forward and the Fights table prints it on the one row it touches, but the cause, a number of
//! bytes on a file, is printed nowhere.
//!
//! `Ingest::fights_unreadable` counts lines whose stamp the fold could not turn into a time, which
//! `grimoire_parse::fights::Fights::unreadable` describes as folded into no fight at all. A month
//! name this build does not know would delete combat with nothing on screen to show for it. The
//! count exists precisely so that cannot happen quietly, and it was being carried across a channel
//! and then dropped on the floor by every screen that read the fights beside it.
//!
//! And the candidate list. `Ingest::sources` has always carried one row per `eqlog_*.txt` with the
//! ingest's own note on the ones it does not read; the Settings ledger prints that table as part of
//! a ledger about sources in general. What was missing is the ledger read as an ANSWER: which one
//! won, why that one, and what it costs you that the others are not read.
//!
//! # What this page refuses to draw, and why the refusals are on the page
//!
//! THERE ARE NO LOG LINES HERE, AND THE REASON GIVEN HERE WAS FALSE FOR A BUILD. The brief asked
//! for the lines themselves with a filter box, if the ingest exposes them, and to say so plainly if
//! it does not. This note used to answer that `Ingest` publishes no accessor returning a line of
//! text. It does. `Ingest::recent_tail` is exactly that, it is `pub`, and `screens::live` reads the
//! EFFECTS panel out of it, which is why [`NO_LINES_WHY`], a hundred lines below this paragraph and
//! in this same file, had already stopped saying it. Two claims in one file, one of them wrong, and
//! the wrong one was the one a reader met first.
//!
//! WHAT IS TRUE IS SMALLER AND IS STILL A REFUSAL. `recent_tail` hands back a BOUNDED TAIL of the
//! active file, sized for re-folding the live fight rather than for reading: `ActiveLog` is a
//! private struct, its `recent` buffer is a private field behind that accessor, and
//! `KillReader::lines`, the count of lines fed, is a public field on a stream that lives inside
//! that same private struct, so the total is out of reach too. A viewer over that buffer would show
//! the last few minutes and call itself the log, and a filter box over it would be a control over
//! an absence, which is the same defect as an invented number wearing a different hat. So the
//! section says what is missing, which is a reader over the FILE rather than an accessor, and says
//! where it would come from, and draws no box.
//!
//! NO SHARE, NO PERCENTAGE, NO COVERAGE BAR. The unreadable count has no denominator on this side
//! of the ingest: nothing publishes how many lines that tail held. "12 unreadable" is a fact.
//! "12 unreadable of about 400,000" is a figure this app cannot support, and it is the more
//! convincing of the two, which is exactly what makes it the dangerous one.
//!
//! NO BYTES READ. `tail_start` was measured when the bootstrap ran and `LogFile::size` is measured
//! on the current poll. Subtracting one from the other produces a number that looks like "how much
//! of the file was read" and is actually two moments of a growing file put through a minus sign.
//! Both are printed, each labelled with when it was taken, and neither is combined with the other.
//!
//! NO CLAIM THAT THE TOP ROW IS THE ONE BEING READ, EXCEPT WHEN IT IS. The ingest's rule is that
//! the newest file wins, but `logs` and `active` are assigned at different moments: `tail` takes
//! the fresh listing and only then starts the rescan that adopts the new file, and `scan` lists
//! the folder before it tries to open anything in it. So "the top row is the one being read" is
//! false for the whole of a camp to another character, and false outright when the newest log will
//! not open. That sentence is now read off the rows rather than asserted over them, and so is the
//! ingest's own `not tailed` note, which names the newest file as the one being read and is true
//! of the ingest's steady state only. See [`ranking_words`].
//!
//! NO KILL COUNT FOR A FILE NOBODY READ. `Source::records` is zero for every log except the tailed
//! one, because nothing counted them, not because there are none. A `0` in that column would be
//! this app stating a measurement it never made about a file it never opened. Those cells say
//! `not read`.
//!
//! # ONE line here is computed, and it is cached because this page never stops repainting
//!
//! This section used to open "Nothing here computes anything. Every value is a field of a struct
//! the ingest owns, printed." That stopped being true the day the class coverage line was added to
//! [`lines`], and nobody came back to this paragraph. `class::Book::unplaceable` walks every caster
//! the log has named, and for each of their spells does a case folded linear `find` over the whole
//! 2,001 record corpus. This page asks egui for a repaint on every frame while a log is being
//! tailed, so that whole-corpus walk was running sixty times a second to produce a figure that
//! moves only when somebody casts a spell nobody has cast before.
//!
//! SO IT IS TAKEN AT MOST ONCE A SECOND AND KEPT. [`Coverage`] holds the last reading, the two
//! sizes it was taken against and the instant it was taken at. A book that has grown a caster, or a
//! corpus of a different size, invalidates it on the spot; a caster casting a spell nobody had cast
//! before moves neither size, so the reading is retaken once a second as well, which is the cadence
//! this page already asks for so its ages tick. The figure is never older than the tick beside it.
//!
//! EVERYTHING ELSE IS STILL A FIELD OF A STRUCT THE INGEST OWNS, PRINTED. The only other arithmetic
//! is thousands separators, a megabyte division shown beside the exact byte count it came from, and
//! an age in seconds handed to `crate::settings::age_text`, which is the app's one implementation of
//! "12s ago". The classification of why nothing is being read is `screens::parser::why_no_fights`,
//! reused rather than re-derived, and the words for an empty fight history are that module's
//! `no_fights_words`, reused verbatim in the one place on this page that is about fights.
use crate::chrome::State;
use crate::ingest::{tail_cap_text, Ingest, Source, SourceKind};
use crate::screens::items::{Col, ROW_H};
use crate::screens::parser::{no_fights_words, why_no_fights, NoFights};
use crate::screens::Cx;
use crate::theme::*;
use chrono::{DateTime, Utc};
use egui::{FontId, RichText, Stroke, StrokeKind, Ui, Vec2};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

/// Width of the label column in the file section, so every value starts on one vertical line.
/// Wide enough for `zone the stream believes`, which is the longest label on the page. (It used to
/// name `folder last scanned` as the longest, which was wrong twice over: that label was two
/// characters shorter than the zone one beneath it, and it has since been retired for saying the
/// folder listing when the value under it is the bootstrap's. See [`active`].)
const FIELD_W: f32 = 164.0;

/// WHAT THIS PAGE IS, ON THE HOVER OF ITS FIRST HEADING AND NOT IN A PARAGRAPH ACROSS THE TOP.
///
/// It was painted on every frame above the first fact on the page. A person who has clicked LOG
/// PARSER / Logs has already been told what he is looking at by the two words he clicked.
const INTRO: &str = "What the app is reading, why that file and not another one, and what it did \
                     not read. Every line is a fact the ingest already holds; nothing here is \
                     derived from the log a second time.";

/// The sort rule, in words, at the point a reader is looking at its result.
///
/// IT IS THE ANSWER TO THE QUESTION THAT BRINGS PEOPLE HERE. `Ingest::tail` lists the folder on
/// every poll, compares the newest file to the one it is on, and starts a whole rescan when they
/// differ, so this is not a launch time decision that a reader could work around by restarting.
const WHY_THIS_FILE: &str =
    "The app lists every eqlog_*.txt in the folder, orders them by the time each file was last \
     written, and reads the top one. There is no picker and this is not a setting: play a \
     different character and that character's file becomes the most recently written, and the next \
     poll notices and starts again on it. A parse that looks empty on a machine that is plainly \
     logging is nearly always this, another character's log winning the sort.";

/// What a name that does not carry the part looks like. `character_of_log` wants `eqlog_<name>_`
/// and the server pattern wants a suffix after it; a file can satisfy the listing rule
/// (`eqlog_.+\.txt`) and neither of those, and the honest cell then says so rather than guessing.
const NO_NAME_PART: &str = "not in the file name";

/// Said while the bootstrap is still on its worker thread.
/// Said while the bootstrap is still on its worker thread. THE LINE; the rest is on the hover
/// of whatever draws it, for the reason every other warning on this page moved there.
const READING: &str = "The folder is still being read.";

/// Said when there is no folder at all. A You and not a Wrong: nothing has failed, the app has
/// simply not been told where to look.
const NO_FOLDER: &str =
    "There is no Logs folder, so no file is being read. The section above lists every place the \
     app looked; set the Logs folder in Settings to the folder the game writes \
     eqlog_<character>_<server>.txt into.";

/// The backstop for a folder with no readable log and no word from the ingest about why. It should
/// be unreachable (`scan` sets `active_problem` on both an empty folder and a file that will not
/// open) and it is written as a real sentence anyway, because a page whose backstop is an empty
/// string reports a hole in the ingest as a hole in itself.
/// # IT USED TO SAY THE INGEST GAVE NO REASON, ONE SECTION ABOVE THE INGEST'S REASON
///
/// `Ingest::active_problem` is printed by `folder` and by `active`, both on this page and both
/// above this line. So on the machine this sentence exists for -- a folder that resolved and a
/// newest file that would not open -- the page said what stopped it and then said nothing said
/// what stopped it, and then blamed `/log` for an OS error.
///
/// SO IT NAMES THE ONE CAUSE IT CAN SUPPORT AND STOPS. A folder with logs in it and no file
/// being tailed, with no problem reported anywhere, really is nearly always logging turned off.
/// What it must not do is claim nothing was reported, because this page is where a report would
/// be printed if there were one.
const NO_LOG: &str =
    "Nothing is being tailed out of the folder above. If no fault is reported above, the usual \
     cause is that the game is not logging (/log on).";

/// Why this page has no line viewer. THE LINE, and [`NO_LINES_WHY`] is the reason behind it.
const NO_LINES: &str = "No line viewer.";

/// What would have to exist for there to be one.
/// # THE OLD VERSION RESTED ON A CLAIM THAT IS FALSE
///
/// It said "there is no accessor on Ingest that returns a line of text". `Ingest::recent_tail`
/// is exactly that, it is `pub`, and `screens::live` already reads the last four hundred lines
/// through it to build the EFFECTS panel. A refusal resting on a false premise is worse than no
/// refusal: it tells a reader the app CANNOT do a thing it does elsewhere on the same fold.
///
/// WHAT IS ACTUALLY TRUE IS SMALLER AND STILL ENOUGH. The buffer is the tail, not the file, and
/// its size is a live-fold budget rather than a reader's scrollback, so a viewer over it would
/// be a window on the last few minutes presented as a log. That is a real reason to withhold a
/// RAW LOG page and it is not a reason to claim there is nothing to read.
const NO_LINES_WHY: &str =
    "The ingest keeps only a bounded tail of the active file, sized for re-folding the live \
     fight rather than for reading: `recent_tail` is what `screens::live` reads effects out \
     of. A viewer over it would show the last few minutes and call itself the log, so what is \
     missing is a reader over the FILE, not an accessor.";

/// The same refusal for the coverage split, which is the thing the unbuilt page for this section
/// promised and is the one part of that promise this build cannot keep.
const NO_SPLIT: &str = "No line by line split.";

/// The whole of it, for the hover.
const NO_SPLIT_WHY: &str =
    "The combat engine does classify every line it is offered as an event, ignored, spell \
     flavour or unrecognised, but the desktop keeps only the count of lines whose stamp would \
     not read, which is the number in this section. The split and the residue both need an \
     accessor that does not exist yet; neither is estimated here.";

/// WHEN THE LOG FOLDER WAS LAST LISTED, which is not when the log was last READ.
///
/// The folder is re-listed on every poll and the bootstrap runs once, so on a normal session
/// these two are seconds and hours apart. A page whose whole subject is which file is being read
/// and how old its numbers are owes both, and until now it could only print one of them.
const FOLDER_LISTED: &str = "folder last listed";

/// Why the two stamps are not one, for the hover on [`FOLDER_LISTED`].
const FOLDER_LISTED_WHY: &str =
    "The app re-lists this folder on every poll to notice a new log file, and that is what this \
     stamp is. It is not when the log was last read: that is the line under it, and it is \
     written once by the bootstrap. Seconds here under hours there is a healthy session.";

/// THE LABEL ON `Ingest::scanned_at`, WHICH NAMES THE BOOTSTRAP AND NOT THE FOLDER LISTING.
///
/// It read `folder last scanned` for a build. See [`active`] for what that cost a reader; the
/// short of it is that the folder is listed on every poll and this stamp is hours old by lunchtime.
/// A constant rather than a literal because the same words are what the fight and stamp counts in
/// [`skipped`] attribute themselves to, and two spellings of one read is how a page comes to look
/// like it is describing two.
const LAST_FULL_READ: &str = "last full read";

/// What that field does and does not cover, on its hover.
const LAST_FULL_READ_WHY: &str =
    "When the worker last read the whole tail: the fights and the unreadable stamp count under \
     WHAT WAS SKIPPED were measured then and are not touched again. It is NOT when the folder was \
     listed. The folder is listed on every poll, about a second apart, and that is what the \
     candidate table below is drawn from; nothing on the ingest stamps that listing, so its age is \
     not printed rather than guessed at.";

/// Why the unreadable count is not shown as a percentage. A COUNT IS ALREADY A NUMBER, so this
/// is a hover and never a line: the figure beside it says the same thing by being a figure.
const NO_DENOMINATOR: &str =
    "A count and not a share. Nothing published by the ingest says how many lines that tail \
     held, so there is no denominator to divide it by and none is guessed at.";

/// THE CLASS COVERAGE READING, AND THE TWO THINGS THAT SAY WHETHER IT IS STILL GOOD.
///
/// See the module note: `class::Book::unplaceable` is a whole-corpus walk and this page repaints
/// continuously, so the reading is taken at most once a second and kept.
///
/// # IT LIVES IN EGUI'S FRAME MEMORY AND NOT ON `LogsScreen`, WHICH IS NOT A PREFERENCE
///
/// `LogsScreen` is a unit struct, and `windows.rs` builds the pop-out tool window's copy with the
/// unit-struct expression `LogsScreen` rather than `LogsScreen::default()`. Giving the screen a
/// field would break that line, and that file belongs to another lane. `Context::data_mut` is
/// egui's own per-context store for exactly this: a value keyed by an `Id`, kept across frames,
/// and private to the context, so the main window and the tool window keep their own readings
/// instead of racing over one.
///
/// THE KEY IS TWO SIZES AND AN INSTANT, AND EACH COVERS WHAT THE OTHER CANNOT. `Book::len` is
/// casters, so a NEW caster invalidates at once; the corpus length catches a snapshot swapped
/// under the app. Neither moves when a caster the book already knows casts something new, which is
/// the ordinary way this figure changes, so the instant is what picks that up.
#[derive(Clone, Default)]
struct Coverage {
    /// `(casters in the book, records in the corpus)` the figures below were read against.
    key: (usize, usize),
    /// When they were read. `None` means never, and never is not a reading.
    at: Option<Instant>,
    casters: usize,
    gaps: usize,
}

/// How stale the coverage figure is allowed to get: the page's own tick.
///
/// The same second `LogsScreen::ui` hands `request_repaint_after`, on purpose. A figure that is
/// refreshed on the same cadence as the ages beside it cannot be older than the oldest thing on
/// the page, so there is nothing extra for a reader to be told.
const COVER_TTL: Duration = Duration::from_secs(1);

impl Coverage {
    /// The reading, taken again only if the book moved or the second is up.
    ///
    /// `now` IS AN ARGUMENT AND NOT `Instant::now()`, so a test can hold the clock still and prove
    /// the cache is a cache: the same instant twice over a book that grew a spell under a caster
    /// it already knew must hand back the FIRST answer, and a second later must not.
    fn read(
        &mut self,
        book: &crate::class::Book,
        spells: &[crate::data::Spell],
        now: Instant,
    ) -> (usize, usize) {
        let key = (book.len(), spells.len());
        let fresh = self.at.is_some_and(|t| now.duration_since(t) < COVER_TTL);
        if key != self.key || !fresh {
            self.key = key;
            self.at = Some(now);
            self.casters = book.len();
            self.gaps = book.unplaceable(spells);
        }
        (self.casters, self.gaps)
    }
}

#[derive(Default)]
pub struct LogsScreen;

impl LogsScreen {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        /* THE PUMP, as `live::LiveScreen::ui` and `parser::ui` do it. In the main window the App
         * has usually pumped already and this returns 0 at once. The repaint request is what makes
         * a poll happen while nobody is touching the app, and on this page it matters more than
         * most: the whole point of the candidate table is that it follows the game, and a table
         * that only updated when the mouse moved would show a stale winner to somebody who had
         * just camped to another character. */
        if cx.ingest.tail() > 0 {
            ui.ctx().request_repaint();
        }
        /* The ages on this page tick. Without this they freeze at whatever the last input event
         * left them at, and "read 4s ago" on a dead ingest is a lie a clock would have caught. */
        ui.ctx().request_repaint_after(Duration::from_millis(1000));

        let now = Utc::now();
        let ig = &*cx.ingest;

        egui::ScrollArea::vertical()
            .id_salt("logs_page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                folder(ui, ig);
                active(ui, ig, now);
                candidates(ui, ig, now);
                skipped(ui, ig, now);
                /* THE SNAPSHOT ITSELF AND NOT ITS SPELL LIST. It used to be
                 * `cx.data.map_or(&[], |d| &d.spells)`, which folded "there is no snapshot on this
                 * machine" and "the snapshot carries no spells.json" into one empty slice, and
                 * [`lines`] then drew nothing at all for either. Two different absences, both
                 * silent. See [`coverage_words`]. */
                lines(ui, ig, cx.data);
                ui.add_space(12.0);
            });
    }
}

/* ------------------------------------------------------------------- the folder -- */

/// WHERE THE RAW MATERIAL COMES FROM, and every place that was tried to find it.
///
/// THE TRIED LIST IS PRINTED WITHOUT A VERDICT PER ROW. `resolve_log_dir` takes the first entry
/// that is a directory, so the ones above the winner did not exist AT SCAN TIME. Printing "does
/// not exist" beside them now would be this page making a claim about the disk that it has not
/// been to the disk to check, on a folder somebody may have created in the meantime. The rule is
/// stated once underneath instead, and only the winner is marked.
fn folder(ui: &mut Ui, ig: &Ingest) {
    heading_why(ui, "THE FOLDER", INTRO);
    let report = ig.log_dir();
    /* THE SCAN IS ASKED FIRST, AND THAT ORDER IS THE WHOLE FIX.
     *
     * `Ingest::new` starts with `LogDirReport::default()` and `self.dir` is written only by
     * `adopt`, when the worker's scan lands. So for the whole of the first bootstrap `dir` is
     * `None` on a PERFECTLY CONFIGURED machine, and this arm could not tell that apart from a
     * folder nobody has pointed at.
     *
     * WHAT IT PAINTED: `State::You` beside "no Logs folder", on a folder it was at that moment
     * reading. `You` is this app's one signal for "something is waiting on a person", so the
     * page's headline fact at launch was an attention flag demanding the reader go set a
     * setting that was already right, one line above the next section saying "The folder is
     * still being read."
     *
     * `file_state` HAS ALWAYS HAD THIS RIGHT because it delegates to `parser::why_no_fights`,
     * whose first arm is the scan. This function reimplemented the same ladder in the wrong
     * order, which is exactly the drift a second copy invites.
     *
     * AND A RESOLVED FOLDER IS NOT A CHECKED ONE. `Settled` is green for "this reads"; nothing
     * here has been to the disk, and `list_logs` failing sets `log_dir_problem` a few lines
     * down. So a path that resolved is Working until the listing has actually come back.
     */
    match &report.dir {
        _ if ig.scanning() => {
            mark(ui, State::Working, READING);
        }
        Some(d) => {
            let ok = ig.log_dir_problem().is_none() && !ig.log_dir().tried.is_empty();
            mark(
                ui,
                if ok { State::Settled } else { State::Working },
                &d.display().to_string(),
            );
        }
        None => {
            mark(ui, State::You, "no Logs folder");
        }
    }
    /* A folder can resolve AND carry a problem: `list_logs` failing on a folder that exists sets
     * it. So this is not an else. */
    if let Some(p) = ig.log_dir_problem() {
        mark(ui, State::Wrong, p);
    }
    if !report.tried.is_empty() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(RichText::new("places looked in, in order:").color(TEXT_3))
                .on_hover_text(
                    "The first of these that exists on disk is the one used. A folder named in \
                     Settings is taken as given whether or not it exists, which is why a wrong \
                     setting reads here as unreadable rather than quietly falling back.",
                );
        });
        for p in &report.tried {
            let chosen = report.dir.as_ref() == Some(p);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(
                    RichText::new(p.display().to_string())
                        .font(FontId::monospace(11.5))
                        .color(if chosen { GOLD_HI } else { TEXT_3 }),
                );
                if chosen {
                    ui.label(RichText::new("in use").color(TEXT_3));
                }
            });
        }
    }
}

/* --------------------------------------------------------------------- the file -- */

/// WHICH FILE, AND WHY THAT ONE.
///
/// The state square and its words come from [`file_state`], which is the parser screen's own
/// classifier asked about this page's question. Everything under the square is a field of
/// `LogFile` or a stamp off the `Ingest`, printed, each labelled with WHEN it was taken: a size
/// read on this poll and a start byte measured at the last bootstrap are two different moments and
/// a reader has to be able to see that before he does arithmetic on them in his head.
fn active(ui: &mut Ui, ig: &Ingest, now: DateTime<Utc>) {
    heading_why(ui, "THE FILE BEING READ", WHY_THIS_FILE);
    let (state, words) = file_state(ig);
    match ig.active_log() {
        Some(f) => {
            mark(ui, state, &f.name());
            field(ui, "path", &f.path.display().to_string(), true);
            field(
                ui,
                "character",
                f.character.as_deref().unwrap_or(NO_NAME_PART),
                false,
            );
            field(
                ui,
                "server",
                f.server.as_deref().unwrap_or(NO_NAME_PART),
                false,
            );
            /* A SIZE, OR THE FACT THAT THERE ISN'T ONE. `Ingest::tail` writes `size = 0` in
             * the ERROR arm of `std::fs::metadata` before it sets the problem, so a zero here
             * is a sentinel and not a reading, and this label says in as many words that it is
             * a reading. A reader who scrolled past the red line below read a 0 byte log file,
             * which is a different fault with a different cause. */
            let size = if ig.active_problem().is_some() && f.size == 0 {
                String::from("not read on this poll")
            } else {
                bytes_text(f.size)
            };
            field(ui, "size on this poll", &size, true);
            field(
                ui,
                "file last written",
                &modified_text(f.modified, now),
                false,
            );
            field(
                ui,
                "lines last taken",
                &since_text(ig.last_read(), now),
                false,
            );
            /* THE LABEL SAID `folder last scanned` AND THE VALUE IS NOTHING OF THE KIND.
             *
             * `Ingest::scanned_at` is stamped in `adopt`, when a BOOTSTRAP lands: the worker's
             * whole-tail read, the thing that fills `fights` and `fights_unreadable`. The folder
             * LISTING is a different act at a different cadence: `Ingest::tail` calls `list_logs`
             * on every poll, a second apart, and assigns the result to `self.logs`, which is what
             * the candidate table below is drawn from.
             *
             * SO ON AN ORDINARY SESSION THIS FIELD SAID "6h ago" ABOUT A FOLDER THE APP HAD
             * LISTED ONE SECOND EARLIER, directly above the table it had listed it for. The
             * reader's conclusion is that the app stopped looking, which is the one thing this
             * page exists to answer and it was answering it wrongly.
             *
             * THE FIX IS THE LABEL AND NOT THE VALUE, because the value is worth printing: it is
             * when the numbers in WHAT WAS SKIPPED were measured, and those are the oldest facts
             * on the page.
             *
             * AND THE LISTING'S OWN AGE IS PRINTED BESIDE IT NOW. This block used to end "the
             * listing's own age cannot be printed at all: nothing on `Ingest` stamps it", and the
             * hover carried the same apology. That was true and is not: `Ingest::listed_at` stamps
             * the folder listing, which happens on every poll, where `scanned_at` stamps the
             * bootstrap, which happens once.
             *
             * THE TWO STAMPS ARE THE WHOLE POINT OF PRINTING EITHER. A reader looking at a page he
             * thinks has stalled is asking one question, is this app still looking, and the two
             * numbers answer it between them: a listing seconds old under a read hours old is a
             * healthy session, and both old together is the thing worth worrying about. One of
             * them alone was what made the old label a lie in the first place. */
            field(ui, FOLDER_LISTED, &since_text(ig.listed_at(), now), false)
                .on_hover_text(FOLDER_LISTED_WHY);
            field(ui, LAST_FULL_READ, &since_text(ig.scanned_at(), now), false)
                .on_hover_text(LAST_FULL_READ_WHY);
            /* `?` IS THE STREAM'S SENTINEL AND NOT A PLACE. `KillReader::new` initialises the
             * zone to the literal "?" and `feed` writes it back whenever a zone line resolves
             * against no roster key, so printing it raw put a question mark on the page under a
             * label saying it is what the stream believes. It believes nothing; that is the
             * point, and it is what the row says now. */
            if let Some(z) = ig.current_zone() {
                let known = z != "?";
                field(
                    ui,
                    "zone the stream believes",
                    if known { z } else { "not resolved" },
                    known,
                );
            }
            /* THE SQUARE HERE IS THE COMPUTED STATE AND NEVER A LITERAL. This mark used to be
             * written `State::Wrong` on the assumption that a file being read plus words about it
             * could only ever mean a read fault. It cannot: a rescan started because the reader
             * camped to another character leaves the OLD file installed as the active one while
             * the worker bootstraps the new one, so `file_state` returns Working with the READING
             * sentence, and a hard `Wrong` painted that ordinary camp red. A red square on a
             * stream overlay is read as "something is broken" from across a room, and this page is
             * the one a reader opens to find out whether something is. */
            if let Some(w) = words {
                mark(ui, state, &w);
            }
        }
        None => {
            mark(ui, state, words.as_deref().unwrap_or(NO_LOG));
        }
    }
}

/// Why the app is or is not reading a file, as a square and words.
///
/// THE CLASSIFIER IS `parser::why_no_fights` AND IS NOT REWRITTEN HERE. That function answers
/// exactly this page's question in exactly this page's order (still scanning, then no folder, then
/// nothing tailed), and a second copy of that ladder is how two screens come to disagree about
/// whether a machine is mid scan.
///
/// ITS WORDS ARE NOT REUSED IN THREE OF THE FOUR ARMS, AND THAT IS DELIBERATE. `no_fights_words`
/// is written about FIGHTS, and every sentence in it ends up somewhere near "so there is no fight
/// to show". On a page about files that is an answer to a question nobody asked, and the reader
/// who is here because his parse is empty would be told again that his parse is empty. The one
/// place those words are exactly right is the fight history line in [`skipped`], and that is where
/// they are used.
///
/// `NoCombat` MEANS A FILE IS BEING READ, which is this page's success state: whether anything in
/// it attacked anything is the Fights page's question and not this one's. The only thing left to
/// report on that arm is a read problem on the file itself.
fn file_state(ig: &Ingest) -> (State, Option<String>) {
    let why = why_no_fights(
        ig.scanning(),
        ig.log_dir().dir.is_some(),
        ig.active_log().is_some(),
    );
    match why {
        NoFights::Reading => (State::Working, Some(READING.to_owned())),
        NoFights::NoFolder => (State::You, Some(NO_FOLDER.to_owned())),
        /* The ingest's own sentence first: it names the folder and the /log command. */
        NoFights::NoLog => (
            State::Wrong,
            Some(ig.active_problem().unwrap_or(NO_LOG).to_owned()),
        ),
        NoFights::NoCombat => match ig.active_problem() {
            Some(p) => (State::Wrong, Some(p.to_owned())),
            None => (State::Settled, None),
        },
    }
}

/* --------------------------------------------------------------- the candidates -- */

/// EVERY LOG IN THE FOLDER, IN THE ORDER THE APP RANKS THEM, with the winner marked.
///
/// THE ROWS ARE `Ingest::sources` AND NOT A SECOND DIRECTORY LISTING. The ingest lists the folder
/// with its own name rule and its own sort on every poll; a `read_dir` here would be a second
/// answer to "what is in this folder", able to show a file the ingest is not considering or to put
/// them in a different order, and the whole value of this table is that it is the ingest's own
/// ranking made visible. `nav::looks_like_eq_log` is what separates the file rows from the folder
/// placeholder `sources` pushes when there is nothing to list, which is the same rule
/// `nav::summarise` uses on the same rows.
///
/// THE `not tailed` NOTE IS PRINTED ONCE AND NOT PER ROW. It is the same sentence on every losing
/// row, differing only in naming the winner, and eight copies of it is a table that has to be read
/// past rather than read. It is lifted verbatim off the first row that carries it, so it is still
/// the ingest's words and not a restatement. A row carrying any OTHER problem is a real fault and
/// gets its own line underneath.
///
/// THE GOLD EDGE ON THE TOP ROW IS NOT A SELECTION AND NOTHING HERE IS CLICKABLE. The gold edge
/// is the nav's selection vocabulary, and what it carries here is "this is the file being read",
/// which is a fact about the app rather than a choice the reader made. This page is READING ONLY:
/// there is no picker, because there is nothing behind one. The ingest re-decides the winner on
/// every poll from the file times, so a control that overrode it would be overridden back a second
/// later, and a control that only appeared to work would be worse than no control.
///
/// # THE TABLE WAS ORDERED ON A NUMBER IT NEVER PRINTED
///
/// `list_logs` sorts the candidates by the time each file was last written, newest first, and that
/// order is the entire reason one of these files is tailed and the others are not. For the life of
/// this page the columns were the file name, the character, a kill count and when lines were last
/// taken out of it, and the sort key was nowhere among them: the rule was stated in prose on two
/// hovers and the READINGS the rule was applied to were not on the page at all. So a reader who
/// came here with the question this page exists for, why THAT file and not my other character's,
/// was handed a ranking he had to take on faith, and the one comparison he wanted to make was the
/// one the table would not let him make.
///
/// `last written` IS THE INGEST'S OWN READING AND NOT A SECOND STAT OF THE FOLDER. It comes off
/// [`Ingest::log_modified`], which searches the same private `logs` listing the sort itself ran
/// over. A `fs::metadata` call per row here would be this page listing the folder a second time,
/// at a different moment from the sort, on every frame; the two readings disagree exactly while a
/// file is being written, which is the whole time this app is worth watching, and the column would
/// then contradict the order it is drawn to explain. That is the same refusal the first paragraph
/// of this doc makes about the ROWS, applied to the values in them.
///
/// IT SITS LEFT OF `last read` BECAUSE THAT IS THE ORDER THE TWO EVENTS HAPPEN IN. The client
/// writes and then the app reads, and the pair side by side is the diagnosis: written seconds ago
/// and read seconds ago is a healthy tail, written seconds ago and `never` is the file the game is
/// filling while the app looks somewhere else, which is the fault this whole page was built for.
///
/// # AND FOR A BUILD THE ROWS SAID OTHERWISE IN THE ONLY LANGUAGE A POINTER SPEAKS
///
/// The paragraph above was written while every row, and the column header with them, was drawn by
/// `items::list_row`. That function is built for the FIND screens, where a row really is a choice:
/// it allocates its rect with `Sense::click()`, paints a `PANEL` fill under the cursor, and hands
/// back the `Response` so the caller can act on the click. This page threw that `Response` away.
///
/// So the table lit up row by row under the pointer exactly like a selectable list, and clicking
/// did nothing, on the header as well. A dead control is worse than a missing one: a reader who
/// clicks a row that lights and gets nothing does not conclude the app has no picker, he concludes
/// the app is broken, and this is the page he opened to find out whether it was.
///
/// SO THE ROWS ARE PAINTED HERE, BY [`table_row`], AND SENSE HOVER AND NOTHING ELSE. The geometry
/// is `list_row`'s to the pixel so the table still reads as one of this app's tables. The hover
/// fill is kept and is no longer a promise: it tracks the row under the cursor across five columns,
/// and every row carries `on_hover_text` saying what the ingest thinks of that file, so hovering
/// now pays out in words instead of in an offer the page cannot honour. The Fights table answers
/// the same question the same way (`parser`: a row is a row, and clicking it does nothing).
fn candidates(ui: &mut Ui, ig: &Ingest, now: DateTime<Utc>) {
    heading(ui, "THE CANDIDATES");
    let all = ig.sources();
    let rows: Vec<&Source> = all.iter().filter(|s| is_log_file(s)).collect();
    if rows.is_empty() {
        note(ui, &no_candidates_words(ig));
        return;
    }

    let active_path = ig.active_log().map(|f| f.path.as_path());
    let (line, why) = ranking_words(&rows, active_path, ig.scanning());
    note(ui, &line).on_hover_text(why);
    if let Some(n) = shared_not_tailed_note(&rows, active_path) {
        note(ui, n);
    }
    ui.add_space(4.0);

    table_row(
        ui,
        false,
        false,
        &[
            head_col("log file", false, 0.0),
            head_col("character", true, 150.0),
            head_col("kills read", true, 90.0),
            head_col("last written", true, WRITTEN_W),
            head_col("last read", true, 90.0),
        ],
    )
    .on_hover_text(HEAD_WHY);
    for s in rows {
        let is_active = active_path == Some(s.path.as_path());
        let name = file_name(&s.path);
        let who = crate::ingest::character_of_log(&name).unwrap_or_else(|| NO_NAME_PART.to_owned());
        let kills = kills_cell(is_active, s.records);
        let written = written_cell(ig.log_modified(&s.path), now);
        let read = since_text(s.last_read, now);
        table_row(
            ui,
            is_active,
            true,
            &[
                Col {
                    text: &name,
                    mono: true,
                    color: if is_active { GOLD_HI } else { TEXT_2 },
                    right: false,
                    width: 0.0,
                },
                Col {
                    text: &who,
                    mono: false,
                    color: TEXT_2,
                    right: true,
                    width: 150.0,
                },
                Col {
                    text: &kills,
                    mono: true,
                    color: if is_active { TEXT } else { TEXT_3 },
                    right: true,
                    width: 90.0,
                },
                /* THE ONE COLUMN THAT DOES NOT DIM ON A LOSING ROW. `kills read` and `last read`
                 * are absent on every row but the tailed one, so dimming them says "there is
                 * nothing here". This is a real reading taken of every file in the folder, and
                 * the losing rows' values are half of what the reader came to compare: dimming
                 * them would hide the side of the comparison that explains the ranking. */
                Col {
                    text: &written,
                    mono: true,
                    color: TEXT_2,
                    right: true,
                    width: WRITTEN_W,
                },
                Col {
                    text: &read,
                    mono: true,
                    color: TEXT_3,
                    right: true,
                    width: 90.0,
                },
            ],
        )
        .on_hover_text(row_words(s, is_active));
        /* A fault, as opposed to the note that says this file is simply not the newest one. */
        if let Some(p) = s.problem.as_deref() {
            if !p.starts_with(crate::nav::NOT_TAILED_PREFIX) {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    mark(ui, State::Wrong, p);
                });
            }
        }
    }
}

/// THE SENTENCE OVER THE TABLE, READ OFF THE TABLE RATHER THAN ASSUMED ABOUT IT.
///
/// THE DEFECT THIS EXISTS TO KILL: this note used to end "The top row is the one being read", as a
/// constant, over rows where that was not true. It is the single most load bearing sentence on the
/// page, because a reader who came here to find out which of his four characters' logs the app has
/// hold of will believe it over the gold edge, and there are two ordinary states in which it lies.
///
/// FIRST, NOTHING IS BEING READ AT ALL. `scan` lists the folder and THEN opens the newest file, so
/// a log that will not open (locked by the client, on a share that dropped, permissions) leaves
/// `logs` full and `active` None. Every row is then unmarked, and the old sentence pointed at a
/// top row the app had never opened. The reader's next move would have been to go and look at that
/// character's log for numbers that were never read out of it.
///
/// SECOND, THE MARK IS NOT ON THE TOP ROW, WHICH IS THE CAMP. `tail` lists the folder, assigns the
/// fresh listing to `self.logs`, and only THEN calls `rescan`; `self.active` keeps pointing at the
/// old file until that worker lands, which on a 40MB tail is seconds. So for the whole of the
/// window that follows camping to another character, the top row is a file the app has not opened
/// and the gold edge is further down. That window is not an edge case here. It is the exact
/// moment this page was built for, and the old sentence was wrong throughout it.
///
/// THE CATCH UP CLAUSE IS GATED ON `scanning` AND NOT INFERRED. A mark below the top row almost
/// always means a bootstrap is in flight, but `rescan` can fail to spawn its thread, and then
/// nothing is coming. Saying "the mark moves when that read lands" over a machine where no read is
/// running would be this page inventing a future instead of a number.
/// # IT RETURNS A LINE AND ITS REASON, AND IT USED TO RETURN A PARAGRAPH
///
/// The sort rule led every one of these three answers: "N eqlog files in the folder, in the order
/// the app ranks them, which is by the time each file was last written, newest first." True, and
/// the same twenty two words above the file list on every frame forever. It is the hover on THE
/// FILE BEING READ heading now, said once, where somebody asking the question is looking.
///
/// THE ABNORMAL ARM STILL SAYS THE ALARMING PART ON THE PAGE. When the marked row is not the top
/// row, that is a state the app should not rest in and a reader must see it without hovering
/// anything, so the line names both files; what moved to the hover is the explanation of what it
/// means for the numbers.
fn ranking_words(rows: &[&Source], active: Option<&Path>, scanning: bool) -> (String, String) {
    let head = count_text(rows.len());
    let top = rows.first().map(|s| s.path.as_path());
    match active {
        None => (
            format!("{head}, none marked"),
            String::from(
                "None of them is being read, so no row is marked: the app listed this folder and \
                 then could not open the newest file in it, and the line above says what stopped \
                 it.",
            ),
        ),
        Some(a) if top == Some(a) => (
            format!("{head}, top row is the one read"),
            String::from(
                "The rows are in the order the app ranks them, which is by the time each file was \
                 last written, newest first.",
            ),
        ),
        Some(a) => {
            let reading = file_name(a);
            let newest = top.map(file_name).unwrap_or_else(|| String::from("it"));
            let tail = if scanning {
                "The app has just noticed the newer file and is reading it now; the mark moves to \
                 it when that read lands, and until then every number in this app is still the \
                 marked file's."
            } else {
                "The app has not taken up the newer file and no read is running, which is not a \
                 state it should rest in: every number in this app is still the marked file's."
            };
            (
                format!("reading {reading}, not the top row ({newest})"),
                tail.to_owned(),
            )
        }
    }
}

/// WHY THERE IS NOTHING TO LIST, in the ingest's own words where it has any.
///
/// The folder case is answered by the section above this one, so this sends the reader up rather
/// than repeating the tried list.
/// # IT CLAIMED AN EMPTY FOLDER IN THE ONE STATE WHERE THE FOLDER WAS NEVER LISTED
///
/// This checked the folder, the scan and `active_problem`, and never `log_dir_problem`. When
/// `list_logs` errors -- an unreadable folder, a path that has gone, a dropped share -- `scan`
/// sets only the DIRECTORY's problem: `dir` is still `Some`, the scan is finished and there is no
/// active problem, so this fell to its last arm and told the reader there are no logs in a folder
/// nothing had managed to read. Two different faults, one sentence, and the wrong one.
fn no_candidates_words(ig: &Ingest) -> String {
    /* THE SCAN FIRST. See `folder`: `dir` is `None` for the whole first bootstrap on a machine
     * that is configured correctly, so testing it first made NO_FOLDER win that race every
     * launch. It told the reader to go set a Logs folder, and its second clause -- "the section
     * above lists every place the app looked" -- was false too, because `tried` is empty until
     * the scan lands and that section listed nothing. */
    if ig.scanning() {
        return READING.to_owned();
    }
    if ig.log_dir().dir.is_none() {
        return NO_FOLDER.to_owned();
    }
    /* THE FOLDER'S OWN FAULT FIRST: it is the reason there is no list at all. */
    if let Some(p) = ig.log_dir_problem() {
        return p.to_owned();
    }
    match ig.active_problem() {
        Some(p) => p.to_owned(),
        None => "No eqlog_*.txt in the folder above. The game writes one only while logging is on \
                 (/log on)."
            .to_owned(),
    }
}

/// The ingest's `not tailed` sentence, taken off the first losing row that carries it.
///
/// `None` when there is nothing to explain, which is the one log case and the case where the
/// losing rows carry real faults instead.
///
/// AND `None` WHENEVER THE MARKED ROW IS NOT THE TOP ROW, WHICH IS A CORRECTNESS GATE AND NOT A
/// TIDINESS ONE. `Ingest::sources` builds that sentence as "only the most recently written log is
/// read, and that is {newest}", where `newest` is `logs.first()`. It is the ingest speaking about
/// its own rule, and it is true of the ingest's steady state. It is NOT true in the two states
/// [`ranking_words`] documents: with nothing being read it names a file the app never opened, and
/// mid camp it names the newest file, which is the one the app has NOT yet adopted, while the gold
/// edge sits on a different row. Lifted to the top of the table and printed as a summary it would
/// then contradict the table underneath it in the app's own voice, which is worse than an
/// unexplained table. The per row fault line still prints anything that is a real fault, so
/// nothing is being hidden here: only a sentence whose subject has moved.
fn shared_not_tailed_note<'a>(rows: &[&'a Source], active: Option<&Path>) -> Option<&'a str> {
    let top = rows.first().map(|s| s.path.as_path());
    if active.is_none() || top != active {
        return None;
    }
    rows.iter()
        .filter(|s| active != Some(s.path.as_path()))
        .filter_map(|s| s.problem.as_deref())
        .find(|p| p.starts_with(crate::nav::NOT_TAILED_PREFIX))
}

/// `Source::records` for the file that was read, and words for every file that was not.
///
/// THIS IS THE SMALLEST INVENTED NUMBER ON THE PAGE AND IT WOULD HAVE BEEN THE EASIEST TO SHIP.
/// `records` is zero on every row but the tailed one because nothing opened those files, not
/// because they hold no kills. A column of zeroes beside three character names reads as a
/// measurement of those characters, and it would be wrong about every one of them.
fn kills_cell(is_active: bool, records: usize) -> String {
    if is_active {
        thousands(records as u64)
    } else {
        "not read".to_owned()
    }
}

/// The `last written` column, wide enough for the ages [`crate::settings::age_text`] produces
/// (`59s`, `59m`, `23h`, `9,999d`, each with ` ago` after it) and for [`NO_MTIME`], and no wider,
/// because every pixel of it comes off the file name column beside it.
const WRITTEN_W: f32 = 90.0;

/// What the `last written` cell says when the filesystem does not keep a modified time.
///
/// IT IS NOT THE SENTENCE [`modified_text`] GIVES THE FIELD ABOVE, AND THE DIFFERENCE IS THE
/// COLUMN AND NOT THE MEANING. See [`written_cell`].
const NO_MTIME: &str = "not reported";

/// WHEN THE CLIENT LAST WROTE THIS CANDIDATE: the number the rows are ordered by, in a cell.
///
/// THE AGE IS `modified_text`'s AND NOT A SECOND IMPLEMENTATION, so the time under `last written`
/// in this table and the time beside `file last written` in the section above are the same
/// arithmetic on the same reading, and cannot drift apart into two answers about one file.
///
/// THE `None` ARM IS THIS FUNCTION'S OWN, AND THAT IS THE WHOLE REASON IT EXISTS. `modified_text`
/// answers a filesystem that reports no time with "the filesystem did not report one", which is
/// the right sentence in a `field`, where the value runs to the edge of the page. This is a
/// [`WRITTEN_W`] pixel column and [`table_row`] paints a right anchored cell inside a clip rect of
/// exactly that width, so a string that does not fit is not shrunk or wrapped or elided: its LEFT
/// end is cut off and what reaches the reader is the tail of a sentence, mid word, in the column
/// that is supposed to be carrying the figure this table is sorted on. Printing the same words in
/// a narrower place is not the same as saying the same thing.
///
/// SO THE CELL SAYS THE SHORT TRUE THING AND THE HEADER'S HOVER CARRIES THE REST ([`HEAD_WHY`]),
/// which is where this page has always put what it will not paint. What it must not do, and what
/// the whole column exists to avoid, is fall back to a blank or to a `0` or to `last_read` wearing
/// the sort key's label: the absence of a modified time is a fact about the filesystem, the sort
/// still ran without it, and none of those three say so.
fn written_cell(t: Option<SystemTime>, now: DateTime<Utc>) -> String {
    match t {
        Some(t) => modified_text(Some(t), now),
        None => NO_MTIME.to_owned(),
    }
}

/// WHAT THE INGEST THINKS OF ONE CANDIDATE, on the hover of that candidate's row.
///
/// THE INGEST'S OWN SENTENCE WHERE IT HAS ONE, which on a losing row is the `not tailed` note
/// naming the winner, and on a row with a real fault is the fault. [`candidates`] prints that fault
/// on a line of its own as well, and this repeating it is deliberate: the line is under the row and
/// the pointer is on the row, and a reader chasing a red square should not have to work out which
/// of eight rows it belongs to.
///
/// AND WORDS FOR THE TWO ROWS THE INGEST SAYS NOTHING ABOUT: the file being read, which is the
/// whole answer this page exists to give, and the single-log folder where there is no ranking to
/// explain. Neither states a measurement, so neither can invent one.
fn row_words(s: &Source, is_active: bool) -> String {
    match s.problem.as_deref() {
        Some(p) => p.to_owned(),
        None if is_active => String::from(
            "This is the file the app is reading. Every fight, kill and meter in this app was \
             folded out of this file and out of no other.",
        ),
        None => String::from(
            "The ingest reports nothing about this file: it is not the one being read, and it has \
             not been opened, so nothing in this app was read out of it.",
        ),
    }
}

/// What the five columns are, on the header's hover.
///
/// THE `kills read` COLUMN IS THE ONE THAT NEEDS SAYING. Every cell in it but one says `not read`,
/// and a reader who does not know why will read that as a fault on his other characters' logs
/// rather than as the app declining to state a figure it never measured. See [`kills_cell`].
///
/// AND `last written` NEEDS SAYING FOR THE OPPOSITE REASON. It is the only column that is a
/// reading of every file rather than a report on the one file that was opened, it is the value the
/// order rests on, and its `not reported` cell is short enough to be mistaken for a fault when it
/// is a fact about the filesystem. See [`written_cell`], which cannot fit that explanation in
/// [`WRITTEN_W`] pixels and does not try.
const HEAD_WHY: &str =
    "log file: the name as it sits in the folder. character: the name part of it, which is all \
     the app knows about who was playing. kills read: how many kills came out of the file, and \
     `not read` on every file the app never opened, because a 0 there would be a measurement \
     nobody took. last written: when the game last wrote to the file, which is the reading the \
     rows are ordered by, newest first, and `not reported` where the filesystem keeps no modified \
     time. last read: when lines were last taken out of it.";

/// Is this `sources` row an actual log file rather than the folder placeholder?
///
/// BOTH TESTS, ON PURPOSE. The kind is the ingest's own label for the row and the name rule is
/// the client's file name, which is what `nav::summarise` asks of the same rows; the placeholder
/// carries the Log kind and a DIRECTORY path, so the name rule is the half that excludes it.
fn is_log_file(s: &Source) -> bool {
    s.kind == SourceKind::Log && crate::nav::looks_like_eq_log(&s.path)
}

/* ------------------------------------------------------------------ the skipped -- */

/// WHAT WAS NOT READ, AND WHAT THAT COSTS.
///
/// Two facts the ingest has always held and no screen has ever printed, plus the one visible
/// consequence of the first of them, so the chain from a byte offset to a number on the Fights
/// page can be followed in one place.
fn skipped(ui: &mut Ui, ig: &Ingest, now: DateTime<Utc>) {
    heading(ui, "WHAT WAS SKIPPED");

    /* THREE LINES, TWO MOMENTS, AND EVERY LINE NOW SAYS WHICH OF THE TWO IT IS.
     *
     * `tail_start` is the READ CURSOR'S ORIGIN and it is current: `Ingest::tail` writes it at
     * bootstrap and writes it again, to 0, the moment it finds the file has shrunk under the
     * cursor, which is what the client rolling or resetting a log looks like from here.
     *
     * `fights`, `fights_unreadable` and `scanned_at` are the LAST FULL READ and are not current:
     * the live path folds appended lines into `live` and never touches them, so they stay as the
     * worker left them until the next bootstrap.
     *
     * ON A ROLLED LOG THOSE TWO COME APART AND THE PAGE READ AS ONE STATEMENT. The cursor goes
     * back to 0 and the cap line turned green saying the whole file was taken, directly above a
     * fight count and a stamp count folded out of a file that no longer exists, with `that tail`
     * as their subject: the tail the green line above had just described. A reader took the lot as
     * one read of one file. So the cap line dates itself to THIS POLL, the other two name THE LAST
     * FULL READ and carry its age, and neither borrows the other's subject any more.
     *
     * WHAT THIS DOES NOT DO IS SAY THE LOG WAS ROLLED, because nothing published by the ingest
     * says so: `tail_start` is 0 both for a rotation and for a file that was always short of the
     * cap, and there is no stamp on the reset. The hover on the cap line says the counts below can
     * outlive the file they came from, which is the honest half of that, and a ticket for the
     * ingest side is in the report this fix shipped with. */
    let (state, words) = cap_words(ig.tail_start());
    mark(ui, state, &words).on_hover_text(CAP_WHY);

    /* ONE AGE FOR BOTH BOOTSTRAP FIGURES, read once so the two lines cannot print ages a
     * millisecond apart and look like two different reads. */
    let since = since_text(ig.scanned_at(), now);

    ui.add_space(4.0);
    let (state, words) =
        unreadable_words(ig.fights_unreadable(), ig.tail_start().is_some(), &since);
    /* THE REASON RIDES THE COUNT. It was a line under it saying a count is not a share, which
     * the count already says by being a count. */
    mark(ui, state, &words).on_hover_text(NO_DENOMINATOR);

    ui.add_space(4.0);
    let fights = ig.fights();
    if fights.is_empty() {
        /* THE ONE PLACE ON THIS PAGE WHERE THE PARSER SCREEN'S WORDS FIT EXACTLY, because the
         * question here really is why the fight history is empty. Reused rather than restated: two
         * screens answering that with two sentences is how they come to disagree about it. */
        let why = why_no_fights(
            ig.scanning(),
            ig.log_dir().dir.is_some(),
            ig.active_log().is_some(),
        );
        note(ui, no_fights_words(why));
    } else {
        let (state, words) =
            folded_words(fights.len(), fights.first().is_some_and(|f| f.cut), &since);
        mark(ui, state, &words);
    }
}

/// The fight count out of the last bootstrap, as a state and a sentence.
///
/// # ITS SUBJECT USED TO BE `that tail`, AND `that tail` WAS THE LINE ABOVE IT
///
/// The sentence read "4 fights folded out of that tail, as of 6h ago", under a cap line describing
/// the read cursor as it stands on this poll. Two facts of two different ages, the second pointing
/// at the first with a demonstrative. On the ordinary machine they really are one read and nobody
/// was misled. On a machine whose client had rolled the log they are not: the cursor is back at
/// byte 0 of a new file and these fights came out of the old one, and the page said the new file
/// was read whole and then counted the old file's fights as its contents.
///
/// SO IT NAMES THE READ IT CAME FROM, in the same words the file section's stamp uses
/// ([`LAST_FULL_READ`]), and keeps the age. Two lines that name the same read are read as one fact;
/// two lines that name two reads are read as two, which is what they are.
fn folded_words(n: usize, clipped: bool, since: &str) -> (State, String) {
    let state = if clipped {
        State::Wrong
    } else {
        State::Settled
    };
    (
        state,
        format!(
            "{} folded out of the {LAST_FULL_READ}, {since}.{}",
            fights_text(n),
            if clipped {
                " The oldest of them is marked as a floor: it opened before the byte that read \
                 started at, so its totals are missing whatever happened first."
            } else {
                ""
            }
        ),
    )
}

/// The bootstrap cap, as a state and a sentence.
///
/// `None` IS NOT ZERO. No file is being tailed, so there is no start byte, and printing a 0 there
/// would say the whole of a file was read when no file was opened at all.
///
/// NOTHING IS DIVIDED BY THE FILE SIZE HERE. See the module note: the start byte was measured at
/// the last bootstrap and the size is measured on this poll, and a percentage built out of the two
/// would be the most quotable number on the page and would be made of two different moments.
/// IT IS DATED TO THIS POLL AND THE TWO LINES UNDER IT ARE NOT, which is the whole of why the
/// wording says so. `Ingest::tail` rewrites `tail_start` to 0 when the file shrinks under the
/// cursor, so this is a live figure sitting on top of two that were measured at the last
/// bootstrap. See [`skipped`] and [`CAP_WHY`].
fn cap_words(tail_start: Option<u64>) -> (State, String) {
    match tail_start {
        None => (
            State::Idle,
            "No file is being tailed, so nothing was read and nothing was skipped.".to_owned(),
        ),
        Some(0) => (
            State::Settled,
            format!(
                "from byte 0 on this poll: whole file, under the {} cap",
                tail_cap_text()
            ),
        ),
        Some(n) => (
            State::Wrong,
            format!(
                "On this poll the read starts at byte {}: everything before that byte was not \
                 read. The bootstrap takes at most {} from the END of the file, and the partial \
                 line at the cut is dropped, so the history begins at the first whole line after \
                 that byte and its oldest fight is a floor rather than a total.",
                thousands(n),
                tail_cap_text()
            ),
        ),
    }
}

/// What the cap line covers, and the one thing it cannot tell a reader.
const CAP_WHY: &str =
    "Where the read cursor stands as of this poll. The two lines under it were measured at the \
     last full read and are not retaken until the next one, so they can outlive the file they came \
     from: when the client rolls or resets the log the cursor starts again at byte 0 and those \
     counts still describe the file that is gone. Nothing published by the ingest stamps that \
     reset, so this page dates each line and does not claim to have spotted one.";

/// The unreadable stamp count, as a state and a sentence.
///
/// THE ENGINE'S OWN DESCRIPTION OF WHAT THE NUMBER IS. `Fights::unreadable` counts lines whose
/// stamp could not be turned into a time and which were therefore folded into no fight at all.
/// Saying anything narrower here (that they are "bad lines", or that they were ignored) would be
/// this page deciding what the engine meant.
/// # A ZERO HERE MEANT TWO DIFFERENT THINGS AND ONLY SAID ONE OF THEM
///
/// `n == 0` returned a SETTLED, green "No line in that tail carried a stamp this build could
/// not read." unconditionally. `Ingest::fights_unreadable` starts at 0 and is only ever
/// assigned inside the `Ok` arm of `read_tail`, so on an empty log folder, during the very
/// first bootstrap, and whenever the newest log will not open, the count is 0 because NOTHING
/// WAS EVER READ.
///
/// SO THE PAGE PAINTED A GREEN ALL-CLEAR ABOUT A TAIL THAT DOES NOT EXIST, directly under its
/// own idle square saying "No file is being tailed, so nothing was read and nothing was
/// skipped." Two squares, two colours, one line apart, disagreeing about whether there was a
/// read. It is the immune-slice mistake exactly: a drawn zero claims the app looked.
///
/// THE CALLER PASSES WHETHER THERE IS A TAIL AT ALL, because this function cannot tell from a
/// number that has one spelling for both answers.
/// # AND THE GREEN ONE AGED INTO A CLAIM ABOUT A READ THAT HAD FINISHED HOURS EARLIER
///
/// `fights_unreadable` is written once per bootstrap, in the `Ok` arm of `read_tail`, and the live
/// path never touches it: `Ingest::tail` folds appended lines and drops the unreadable count that
/// fold produces on the floor. So the figure is exactly as old as `scanned_at`, and it was printed
/// with no age beside it at all, as "No line in that tail carried a stamp this build could not
/// read." On a session left open since morning that green all-clear was about a read that finished
/// at breakfast, and `that tail` named a tail the line above it was no longer describing.
///
/// SO THE CALLER PASSES THE AGE AND THE SENTENCE CARRIES IT, in [`LAST_FULL_READ`]'s words, the
/// same ones the fight count beside it and the stamp in the file section use. The `read` arm is
/// the one arm with no age on it, and it needs none: it says nothing was read, and there is no
/// moment at which nothing was read.
fn unreadable_words(n: u32, read: bool, since: &str) -> (State, String) {
    if !read {
        return (
            State::Idle,
            "Nothing has been read, so nothing was skipped.".to_owned(),
        );
    }
    if n == 0 {
        return (
            State::Settled,
            format!("No unreadable stamp in the {LAST_FULL_READ}, {since}."),
        );
    }
    (
        State::Wrong,
        format!(
            "{} in the {LAST_FULL_READ}, {since}, carried a stamp this build could not turn into a \
             time, and were folded into no fight at all. A month name this build does not know \
             looks exactly like this, and it takes the combat on those lines with it.",
            lines_text(n as u64)
        ),
    )
}

/* -------------------------------------------------------------------- the lines -- */

/// THE SECTION THAT SAYS WHY IT IS NOT A TEXT VIEWER.
///
/// An empty state that names what is missing and where it would come from, which is the rule this
/// tree holds everywhere, applied to a gap in this app's own seams rather than to a missing file.
fn lines(ui: &mut Ui, ig: &Ingest, data: Option<&crate::data::Snapshot>) {
    heading(ui, "THE LINES THEMSELVES");
    /* THE CACHE LIVES IN THE CONTEXT AND NOT ON THE SCREEN. See [`Coverage`]: the screen is a unit
     * struct another lane's file builds with a unit-struct expression, so it cannot grow a field.
     * Read, use, write back: `Coverage::read` is the only thing that touches the figures. */
    let id = egui::Id::new("logs_class_coverage");
    let mut cover: Coverage = ui.ctx().data_mut(|d| d.get_temp(id).unwrap_or_default());
    let (state, words, why) = coverage_words(
        ig.tail_start().is_some(),
        data.map(|d| d.spells.as_slice()),
        ig.classes(),
        &mut cover,
        Instant::now(),
    );
    ui.ctx().data_mut(|d| d.insert_temp(id, cover));
    mark(ui, state, &words).on_hover_text(why);
    /* TWO LINES AND TWO HOVERS. This section has no data at all, so it owes a reader a reason;
     * it owed him two paragraphs and that is what changed. `mark` and `note` both return the
     * row's response, so the reason rides the row it is about. */
    mark(ui, State::Idle, NO_LINES).on_hover_text(NO_LINES_WHY);
    note(ui, NO_SPLIT).on_hover_text(NO_SPLIT_WHY);
}

/// WHAT THE CLASS READING COULD NOT PLACE, or why there is no reading to report.
///
/// It is a coverage fact and so it belongs on the page about coverage: without it, a table with no
/// class beside a name is the same picture whether that person cast nothing or cast something the
/// wiki has no class table for.
///
/// # IT USED TO BE ONE `if` AND FOUR DIFFERENT ANSWERS CAME OUT OF IT AS SILENCE
///
/// The line was drawn only when `!spells.is_empty() && !ig.classes().is_empty()`, and drew nothing
/// otherwise. Four states share that else: there is no item snapshot on this machine, there is one
/// and it carries no `spells.json`, nothing has been read out of a log yet, and a tail was read
/// and nobody in it cast anything. The line was added to remove exactly this ambiguity from the
/// class column and it recreated it in its own section, in the state a fresh install is in.
///
/// AND NO TEST HAD EVER RUN IT. The screen's test harness passes `data: None` on every frame it
/// draws, so `spells` was always empty and the `if` was always false; the coverage line was drawn
/// by the app and by nothing else. That is this tree's signature defect wearing a hat, and it is
/// why the harness now takes a snapshot and two tests hand it one.
///
/// THE ORDER OF THE ARMS IS THE ORDER OF WHAT A READER DOES NOT ALREADY KNOW. A missing corpus is
/// said nowhere else on this page; that nothing has been read is said three times above by three
/// state squares, so it is the arm that yields.
///
/// A ZERO IS A MEASUREMENT AND IS ONLY PRINTED WHERE ONE WAS TAKEN. `0 casters read` on a machine
/// that never opened a log would be the drawn zero this whole page is written against, so the read
/// arm has to come before the count and not after it.
fn coverage_words(
    read: bool,
    spells: Option<&[crate::data::Spell]>,
    book: &crate::class::Book,
    cover: &mut Coverage,
    now: Instant,
) -> (State, String, &'static str) {
    let Some(spells) = spells else {
        return (
            State::Idle,
            "No item snapshot, so no class was read.".to_owned(),
            NO_CORPUS_WHY,
        );
    };
    if spells.is_empty() {
        return (
            State::Idle,
            "The snapshot carries no spells, so no class was read.".to_owned(),
            NO_SPELLS_WHY,
        );
    }
    if !read {
        return (
            State::Idle,
            "Nothing has been read, so no caster was seen.".to_owned(),
            NO_CASTS_WHY,
        );
    }
    let (casters, gaps) = cover.read(book, spells, now);
    /* GREEN IS FOR A READING THAT COVERED EVERYTHING IT SAW, and a book with nobody in it saw
     * nothing to cover. Settled on `0 casters read, 0 spells unplaced` would be the same green
     * all-clear over an empty measurement that `unreadable_words` exists to refuse. */
    let state = if gaps == 0 && casters > 0 {
        State::Settled
    } else {
        State::Idle
    };
    (
        state,
        format!("{casters} casters read, {gaps} spells unplaced"),
        COVER_WHY,
    )
}

/// The hover on the coverage count.
const COVER_WHY: &str =
    "A class beside a name is read from the spells that character was seen casting. A spell the \
     corpus carries no class table for proves nothing, so it is skipped: this is how many distinct \
     ones were. Both figures are retaken at most once a second, which is this page's own tick.";

/// The hover when there is no snapshot at all.
const NO_CORPUS_WHY: &str =
    "A class is read by matching the spells a character was seen casting against the item \
     snapshot's spells, and this machine has no snapshot loaded, so there is nothing to match \
     against and no class is guessed at. The Settings page names the folder it looked in. Until \
     then a name with no class beside it on any table means the corpus is missing, not that the \
     person cast nothing.";

/// The hover when the snapshot is there and has no spell list in it.
const NO_SPELLS_WHY: &str =
    "The snapshot loaded and carries no spells.json. That is not an error and the rest of the app \
     works without it, but the class reading has nothing to match a cast against, so no class is \
     read and none is guessed at.";

/// The hover when a corpus is loaded and no log has been read.
const NO_CASTS_WHY: &str =
    "No file is being tailed, so no line has been read and no cast has been seen. The count \
     appears here as soon as a tail is read; a 0 before that would be a measurement nobody took.";

/* ------------------------------------------------------------------- the pieces -- */

/// A leading square in a state colour and a line of text, drawn the way `chrome::nav_row` draws
/// its own: idle is a hollow ring, everything else is filled.
///
/// A FOURTH COPY OF A SEVENTEEN LINE FUNCTION, which is what `settings`, `unlocks`, `exalt` and
/// `valet` each hold. It is not lifted here because a lane may not edit a shared module, and a
/// fifth copy is a smaller debt than an edit to a file another agent owns.
fn mark(ui: &mut Ui, st: State, text: &str) -> egui::Response {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
        let sq = egui::Rect::from_center_size(rect.center(), Vec2::splat(6.0));
        if st == State::Idle {
            ui.painter()
                .rect_stroke(sq, 0.0, Stroke::new(1.0, IDLE), StrokeKind::Middle);
        } else {
            ui.painter().rect_filled(sq, 0.0, st.color());
        }
        let col = if st == State::Wrong { WRONG } else { TEXT };
        ui.label(RichText::new(text).color(col))
    })
    .inner
}

/// ONE ROW OF THE CANDIDATES TABLE: `items::list_row`'s GEOMETRY WITHOUT `items::list_row`'S SENSE.
///
/// # WHY A SECOND ROW PAINTER EXISTS AT ALL
///
/// `list_row` allocates with `Sense::click()` and returns a `Response`, because on the FIND screens
/// a row IS a choice and the caller acts on it. Here there is nothing behind a click (see
/// [`candidates`]), the caller dropped the response, and what a reader got was a table that lit up
/// under the pointer like a list of options and swallowed every click, header included.
///
/// The fix cannot be made in `items` (a lane may not edit a shared module) and must not be made by
/// inventing a click for the row to answer, because the ingest re-decides the winner on every poll
/// and a picker would be overridden a second after it was used. What is left is to draw the row
/// here, and that is what this is: the same `ROW_H`, the same 10px pad and 8px gap, the same right
/// columns laid from the right edge in reverse, the same `PANEL_2` fill and 2px `GOLD` left edge
/// for the marked row, and `Sense::hover()`.
///
/// `track` KEEPS THE HOVER FILL WITHOUT KEEPING THE PROMISE. A fill that follows the pointer across
/// a five column row is how a person reads a wide table, and with no click sense under it, it
/// promises nothing; the caller pairs it with `on_hover_text` so that hovering actually pays out.
/// The header passes false: a heading that lights under the cursor is a heading that looks
/// sortable, and this table's order is the ingest's and not the reader's.
fn table_row(ui: &mut Ui, marked: bool, track: bool, cols: &[Col<'_>]) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), egui::Sense::hover());
    let p = ui.painter();
    if marked {
        p.rect_filled(rect, egui::CornerRadius::ZERO, PANEL_2);
        p.rect_filled(
            egui::Rect::from_min_size(rect.left_top(), Vec2::new(2.0, ROW_H)),
            egui::CornerRadius::ZERO,
            GOLD,
        );
    } else if track && resp.hovered() {
        p.rect_filled(rect, egui::CornerRadius::ZERO, PANEL);
    }
    let pad = 10.0;
    let gap = 8.0;
    let mut left = rect.left() + pad;
    let mut right = rect.right() - pad;
    for c in cols.iter().rev().filter(|c| c.right) {
        let r = egui::Rect::from_min_max(
            egui::Pos2::new(right - c.width, rect.top()),
            egui::Pos2::new(right, rect.bottom()),
        );
        p.with_clip_rect(r).text(
            egui::Pos2::new(r.right(), r.center().y),
            egui::Align2::RIGHT_CENTER,
            c.text,
            cell_font(c),
            c.color,
        );
        right -= c.width + gap;
    }
    for c in cols.iter().filter(|c| !c.right) {
        let w = if c.width > 0.0 {
            c.width
        } else {
            (right - left).max(0.0)
        };
        let r = egui::Rect::from_min_max(
            egui::Pos2::new(left, rect.top()),
            egui::Pos2::new(left + w, rect.bottom()),
        );
        p.with_clip_rect(r).text(
            egui::Pos2::new(r.left(), r.center().y),
            egui::Align2::LEFT_CENTER,
            c.text,
            cell_font(c),
            c.color,
        );
        left += w + gap;
    }
    resp
}

/// `items::col_font`, which is private to that module: mono for figures, proportional for prose.
fn cell_font(c: &Col<'_>) -> FontId {
    if c.mono {
        FontId::monospace(11.5)
    } else {
        FontId::proportional(12.5)
    }
}

/// A header cell: `items::head_row`'s own recipe, dim and monospace, on this page's row painter.
fn head_col(text: &str, right: bool, width: f32) -> Col<'_> {
    Col {
        text,
        mono: true,
        color: TEXT_3,
        right,
        width,
    }
}

fn heading(ui: &mut Ui, s: &str) {
    heading_why(ui, s, "");
}

/// THE SAME HEADING, CARRYING THE PARAGRAPH THAT USED TO SIT UNDER IT.
///
/// # THIS PAGE WAS FIVE PARAGRAPHS AND A HANDFUL OF FACTS
///
/// It is the page that answers "which file, why that one, and what did it skip", and every one of
/// those answers is a FIGURE: a path, a name, a byte count, a line count. Around them sat an
/// intro, a sort rule, a folder rule and a fallback rule, together longer than everything they
/// qualified. A reader opening this page is nearly always debugging an empty parse and wants the
/// path and the filename, not an essay on how the sort works.
///
/// THE RULES ARE STILL HERE AND STILL EXACT. `on_hover_text` on the heading each one belongs to
/// costs nothing until asked for, and none of it is painted, so none of it is in the way.
fn heading_why(ui: &mut Ui, s: &str, why: &str) {
    ui.add_space(14.0);
    let drawn = ui.label(
        RichText::new(s)
            .font(crate::fonts::display(13.0))
            .color(GOLD),
    );
    if !why.is_empty() {
        drawn.on_hover_text(why.to_owned());
    }
    ui.add_space(4.0);
}

/// A dim line of prose, indented under whatever it qualifies.
fn note(ui: &mut Ui, s: &str) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(RichText::new(s).color(TEXT_3))
    })
    .inner
}

/// A labelled value, every label on one vertical line.
///
/// IT RETURNS THE VALUE'S RESPONSE so a caller can hang `on_hover_text` on the row, which is where
/// this page keeps every caveat it does not paint. `mark` and `note` have always done this; `field`
/// did not, and the one field on the page whose label needed a footnote is the one that was quietly
/// mislabelled for a build. See [`active`].
fn field(ui: &mut Ui, label: &str, value: &str, mono: bool) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(FIELD_W, 18.0), egui::Sense::hover());
        ui.painter().text(
            egui::Pos2::new(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.0),
            TEXT_3,
        );
        let t = RichText::new(value).color(TEXT);
        ui.label(if mono {
            t.font(FontId::monospace(11.5))
        } else {
            t
        })
    })
    .inner
}

/// The file name off a path, or the whole path when it has none.
fn file_name(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| p.display().to_string())
}

/// "12s ago", or "never" when the ingest has never read the thing.
///
/// The age itself is `settings::age_text`, which is the app's one implementation of a coarse age
/// and carries its own test. This wrapper is the two words around it.
fn since_text(t: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    match t {
        Some(t) => format!("{} ago", crate::settings::age_text((now - t).num_seconds())),
        None => "never".to_owned(),
    }
}

/// The filesystem's modified time for a file, as an age.
///
/// `None` IS A REAL ANSWER AND NOT AN ERROR. `LogFile::modified` is `metadata().modified().ok()`,
/// and there are filesystems that do not report one. The sort that picked this file still ran, so
/// saying so is more use than a blank.
fn modified_text(t: Option<SystemTime>, now: DateTime<Utc>) -> String {
    match t {
        Some(t) => since_text(Some(DateTime::<Utc>::from(t)), now),
        None => "the filesystem did not report one".to_owned(),
    }
}

/// The exact byte count, and the same number in megabytes beside it once that is the easier read.
///
/// BOTH, AND THE EXACT ONE FIRST. A rounded size on its own is the sort of figure somebody reads
/// off a stream and quotes; printed beside the count it came from it is plainly a convenience.
fn bytes_text(n: u64) -> String {
    if n >= 1024 * 1024 {
        let mb = n as f64 / (1024.0 * 1024.0);
        format!("{} bytes ({mb:.1} MB)", thousands(n))
    } else {
        format!("{} bytes", thousands(n))
    }
}

/// "1 eqlog file" or "4 eqlog files".
fn count_text(n: usize) -> String {
    if n == 1 {
        String::from("1 eqlog file")
    } else {
        format!("{} eqlog files", thousands(n as u64))
    }
}

/// "1 fight" or "4 fights".
fn fights_text(n: usize) -> String {
    if n == 1 {
        String::from("1 fight")
    } else {
        format!("{} fights", thousands(n as u64))
    }
}

/// "1 line" or "4 lines".
fn lines_text(n: u64) -> String {
    if n == 1 {
        String::from("1 line")
    } else {
        format!("{} lines", thousands(n))
    }
}

/// `16526` becomes `16,526`.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*c as char);
    }
    out
}

/* ---------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use std::path::PathBuf;

    /// A game folder with an empty `Logs` inside it, laid out the way a real install is, removed on
    /// drop. Taken from `settings.rs`'s own test module, for the reason given there: the ingest
    /// looks for dumps in the log folder's PARENT, so a bare `Logs` in the system temp folder would
    /// make the whole temp folder the game folder.
    struct TempTree {
        root: PathBuf,
    }

    impl TempTree {
        fn new(tag: &str) -> TempTree {
            let nanos = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let root = std::env::temp_dir().join(format!("{tag}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(root.join("Logs")).expect("create the temp game folder");
            TempTree { root }
        }

        fn logs(&self) -> PathBuf {
            self.root.join("Logs")
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Pump until the ingest has adopted its scan. The scan runs on a worker and lands on a
    /// `tail()`, and `drain_scan` is the first thing `tail` does, ahead of its own poll interval.
    fn pump_until(ig: &mut Ingest, done: fn(&Ingest) -> bool) -> bool {
        for _ in 0..600 {
            ig.tail();
            if done(ig) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// ENOUGH OF A LOG TO BE ADOPTED, AND DELIBERATELY NOT A FIGHT.
    ///
    /// A first cut of this comment claimed this line "folds into a real fight". It does not, and
    /// the claim was worth deleting rather than fixing by changing the line: every test in this
    /// module is about which FILE the app took hold of, and none of them asks anything about
    /// combat. What is actually needed of these bytes is that `list_logs` lists the file, that
    /// `read_tail` can open it, and that the tail folds to zero fights without erroring, so that
    /// `active_log` becomes Some. A stamp this build can read is what makes the third of those
    /// true; the words after it are the client's own first line and carry no combat on purpose,
    /// because a test that planted a swing would be asserting the parser's behaviour in a module
    /// that does not draw a single fight.
    const LINE: &str = "[Sun Sep 01 23:58:00 2026] Welcome to EverQuest!\n";

    /// How much log [`a_rescan_over_a_live_log_is_not_painted_as_a_fault`] plants so that a scan is
    /// still running while a frame draws. Two regex passes over this many bytes is tens of
    /// milliseconds; a frame over a warm context is a fraction of one. Well under the 40MB tail
    /// cap on purpose, so that test exercises the ordinary uncut read and not the clipped one.
    const SLOW_LOG_BYTES: usize = 6 * 1024 * 1024;

    fn boot(tree: &TempTree) -> (Settings, Ingest) {
        let settings = Settings {
            log_dir: Some(tree.logs()),
            /* Hermetic: an empty data root, so nothing here goes and reads the real snapshot. */
            data_root: Some(tree.root.clone()),
            ..Default::default()
        };
        let ingest = Ingest::new(&settings);
        (settings, ingest)
    }

    /// Every shape this screen painted, over a real frame with a real `Cx`, flattened.
    ///
    /// SHAPES AND NOT STRINGS, because the state SQUARES carry half of what this page says and
    /// they carry no text at all. A harness that could only read strings could assert what the
    /// page said and never what colour it said it in, and the two disagreeing is a whole class of
    /// defect on a page whose job is to tell a streamer at a glance whether anything is wrong.
    fn shapes(settings: &mut Settings, ingest: &mut Ingest, width: f32) -> Vec<egui::Shape> {
        shapes_in(&prepared_ctx(), settings, ingest, None, width)
    }

    /// A context with the fonts and the theme already in it.
    ///
    /// SPLIT OUT SO A TEST CAN PAY FOR IT BEFORE IT STARTS TIMING ANYTHING. Installing the display
    /// face is far and away the most expensive thing a frame here does, and
    /// [`a_rescan_over_a_live_log_is_not_painted_as_a_fault`] has to draw while a worker thread is
    /// still running: with the font install inside the measured window, the scan landed first
    /// every time and the test asserted over the state AFTER the rescan instead of during it.
    fn prepared_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        /* One throwaway frame, so the first real one is not also paying for lazy setup. */
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .drop_without_applying_deltas();
        ctx
    }

    /// # THE SNAPSHOT IS AN ARGUMENT NOW, AND IT WAS A HARDCODED `None` FOR THE LIFE OF THIS FILE
    ///
    /// Every test in this module drew the page with `data: None`, which is the one value of that
    /// field that makes [`coverage_words`] take its first arm. The class coverage line, the only
    /// line on this page with a computation behind it, had therefore never been executed by a test
    /// at all: it was drawn by the app and by nothing else, and an `unwrap` or an off by one in it
    /// would have reached the owner's stream before it reached a build.
    fn shapes_in(
        ctx: &egui::Context,
        settings: &mut Settings,
        ingest: &mut Ingest,
        data: Option<&crate::data::Snapshot>,
        width: f32,
    ) -> Vec<egui::Shape> {
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        let mut screen = LogsScreen;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(width, 4000.0),
                )),
                ..Default::default()
            },
            |ui| {
                let mut cx = Cx {
                    railed: false,
                    data,
                    data_err: None,
                    live: &live,
                    settings,
                    ingest,
                    player: Default::default(),
                    stage: None,
                    demand: None,
                    ask: Default::default(),
                    chat: crate::chat::ChatHandle::idle(),
                    chat_wanted: false,
                    auth: Default::default(),
                    auth_begin: false,
                    auth_cancel: false,
                    yt: Default::default(),
                    yt_wanted: false,
                };
                screen.ui(ui, &mut cx);
            },
        );
        assert!(!out.shapes.is_empty(), "the screen painted nothing at all");
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();

        /* `Shape::Vec` is nested by egui, and a test that read only the top level would find
         * nothing and pass, which is the shape of every reachability failure this tree has had. */
        fn flatten(sh: egui::Shape, out: &mut Vec<egui::Shape>) {
            match sh {
                egui::Shape::Vec(v) => {
                    for x in v {
                        flatten(x, out);
                    }
                }
                other => out.push(other),
            }
        }
        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.shape, &mut flat);
        }
        flat
    }

    /// Every string this screen painted, over a machine with no item snapshot.
    fn painted(settings: &mut Settings, ingest: &mut Ingest, width: f32) -> Vec<String> {
        words_of(shapes(settings, ingest, width))
    }

    /// The same, with a snapshot in hand, which is the only way to reach the coverage reading.
    fn painted_with(
        settings: &mut Settings,
        ingest: &mut Ingest,
        data: Option<&crate::data::Snapshot>,
        width: f32,
    ) -> Vec<String> {
        words_of(shapes_in(&prepared_ctx(), settings, ingest, data, width))
    }

    fn words_of(shapes: Vec<egui::Shape>) -> Vec<String> {
        shapes
            .iter()
            .filter_map(|sh| match sh {
                egui::Shape::Text(t) => Some(t.galley.text().trim().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// The fill colour of every state square this screen painted.
    ///
    /// PICKED OUT BY GEOMETRY, WHICH IS WHAT `mark` ACTUALLY DRAWS: a filled 6 by 6 rect. Every
    /// other filled rect on this page is a `list_row` band or its selection edge, all of them
    /// wider than a square, so the size test separates them without this test having to know
    /// which colours the table uses.
    fn squares_in(
        ctx: &egui::Context,
        settings: &mut Settings,
        ingest: &mut Ingest,
        width: f32,
    ) -> Vec<egui::Color32> {
        shapes_in(ctx, settings, ingest, None, width)
            .iter()
            .filter_map(|sh| match sh {
                egui::Shape::Rect(r) if r.rect.width() < 8.0 && r.rect.height() < 8.0 => {
                    Some(r.fill)
                }
                _ => None,
            })
            .filter(|c| *c != egui::Color32::TRANSPARENT)
            .collect()
    }

    fn says(words: &[String], needle: &str) -> bool {
        words.iter().any(|w| w.contains(needle))
    }

    /// DEFECT: a column of zeroes that reads as "these characters have no kills".
    ///
    /// `Source::records` is 0 on every log but the tailed one because nothing opened those files.
    /// Printing that 0 states a measurement the app never made, about a file it never read, and it
    /// is wrong about every character whose log is sitting in that folder with a week of play in
    /// it. This is the smallest invented number this page could have shipped and it would have
    /// been the easiest one to miss.
    ///
    /// WHAT MUTATION MAKES THIS RED: returning `thousands(records as u64)` unconditionally, or
    /// letting the unread branch print "0".
    #[test]
    fn an_unread_log_never_reports_a_kill_count() {
        assert_eq!(kills_cell(true, 0), "0");
        assert_eq!(kills_cell(true, 1_234), "1,234");
        assert_eq!(kills_cell(false, 0), "not read");
        /* Even a row the ingest somehow gave a count to is reported as unread, because the count
         * on a row that is not the tailed file did not come from reading that file. */
        assert_eq!(kills_cell(false, 99), "not read");
        for cell in [kills_cell(false, 0), kills_cell(false, 99)] {
            assert!(
                !cell.chars().any(|c| c.is_ascii_digit()),
                "the unread cell printed a figure: {cell}"
            );
        }
    }

    /// DEFECT: the no-modified-time answer arriving in the table as the tail of a cut off sentence.
    ///
    /// `modified_text` answers a filesystem that keeps no modified time with "the filesystem did
    /// not report one", and that is the right sentence in the `field` above, where the value has
    /// the width of the page. [`table_row`] paints a right anchored cell inside a clip rect of
    /// exactly the column's width: nothing shrinks, wraps or elides, the LEFT end is cut, and the
    /// obvious reuse of `modified_text` in the [`WRITTEN_W`] pixel column would have put the last
    /// few characters of that sentence, starting mid word, under `last written`, in the one column
    /// on this table that carries the figure the rows are sorted on.
    ///
    /// MEASURED THROUGH `cell_font` AND THE REAL FONTS, NOT COUNTED IN CHARACTERS. A character
    /// count is a guess about a font; this lays both strings out in the face the cell is actually
    /// painted in and compares pixels, so a change to `cell_font` or to the installed monospace is
    /// caught by the same assertion.
    ///
    /// THE SECOND HALF IS THE VALIDITY CHECK AND IT IS THE HALF THAT MATTERS. If the field's
    /// sentence fitted the column, this test would be green over a `written_cell` that simply
    /// returned `modified_text` and would be proving nothing at all.
    ///
    /// WHAT MUTATION MAKES THIS RED: `written_cell` returning `modified_text(t, now)` for its
    /// `None` arm, or `NO_MTIME` growing into a sentence.
    #[test]
    fn the_no_time_cell_fits_the_column_and_the_field_sentence_does_not() {
        let now = Utc::now();
        /* The Some arm is `modified_text`'s arithmetic and nothing else, which is what keeps this
         * cell and the `file last written` field above it from becoming two answers. */
        /* HALF A SECOND OFF THE BUCKET EDGE ON PURPOSE. `SystemTime` on Windows is a FILETIME and
         * keeps 100ns ticks, so a `DateTime` whose nanoseconds are finer is truncated by the round
         * trip; an exactly-12s stamp can come back 99ns short and `num_seconds` truncates that to
         * 11. A test that flakes on a rounding boundary is a test nobody trusts. */
        let t = SystemTime::from(now - chrono::Duration::milliseconds(12_500));
        assert_eq!(written_cell(Some(t), now), "12s ago");
        assert_eq!(written_cell(Some(t), now), modified_text(Some(t), now));
        assert_eq!(written_cell(None, now), NO_MTIME);

        let ctx = prepared_ctx();
        let font = cell_font(&Col {
            text: NO_MTIME,
            mono: true,
            color: TEXT_2,
            right: true,
            width: WRITTEN_W,
        });
        let width = |s: &str| {
            ctx.fonts_mut(|f| {
                f.layout_no_wrap(s.to_owned(), font.clone(), TEXT_2)
                    .rect
                    .width()
            })
        };

        assert!(
            width(NO_MTIME) <= WRITTEN_W,
            "the no-time cell is {}px wide in a {WRITTEN_W}px column, so it is painted cut off",
            width(NO_MTIME)
        );
        /* And the widest ordinary reading, because a column that only fits its unusual answer is
         * no better. `age_text` puts no thousands separator in its day count, so this is longer
         * than anything a real log folder produces. */
        assert!(
            width("9999d ago") <= WRITTEN_W,
            "an ordinary age does not fit the column: {}px",
            width("9999d ago")
        );
        assert!(
            width(&modified_text(None, now)) > WRITTEN_W,
            "not a discriminating case: the field's sentence fits {WRITTEN_W}px, so this test \
             cannot tell `written_cell` apart from `modified_text`"
        );
    }

    /// DEFECT: THE TABLE SORTED ON A NUMBER IT NEVER PRINTED.
    ///
    /// The candidate rows are in `list_logs`' order, which is by the time each file was last
    /// written, newest first, and that order is the whole reason one file is tailed and the others
    /// are not. The columns were the name, the character, a kill count and when lines were last
    /// taken: the sort key appeared nowhere, so the reader who came to find out why THAT file was
    /// chosen was given the rule in prose on a hover and never the two readings it was applied to.
    ///
    /// THE TWO MTIMES ARE SET RATHER THAN RACED. A test that wrote one file after the other would
    /// leave both ages in the same one second bucket, both cells would read `0s ago`, and `0s ago`
    /// is already painted three times by this page (the field above, the active row's `last read`,
    /// the skip section's stamp), so nothing could be told apart. `File::set_times` puts the loser
    /// two hours back and the winner five minutes back, which makes both values unique on the page
    /// AND makes the ranking deterministic instead of dependent on filesystem timestamp
    /// granularity.
    ///
    /// THE WINNER IS ASSERTED BY COUNT AND THE LOSER BY PRESENCE, and both halves are needed. The
    /// winner's reading is painted by the FILE BEING READ field as well, so it is on the page once
    /// without this column and twice with it; the loser's is on the page nowhere else at all,
    /// which is the sharper of the two and is also the row the reader actually came to compare.
    ///
    /// THE VALUES ARE READ BACK OFF `Ingest::log_modified` RATHER THAN RECOMPUTED FROM WHAT THIS
    /// TEST SET, so what is pinned is that the cell carries THE INGEST'S OWN LISTING and not a
    /// second stat of the folder taken by the page at some other moment.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `last written` column, feeding it `s.last_read`
    /// or `fs::metadata` instead of `ig.log_modified(&s.path)`, or printing it for the marked row
    /// only.
    #[test]
    fn the_candidate_table_prints_the_time_the_ingest_sorted_it_by() {
        /// Put a planted log's modified time in the past. Windows needs the handle opened for
        /// writing before it will accept a time.
        fn age_by(path: &Path, secs: u64) {
            let f = std::fs::File::options()
                .write(true)
                .open(path)
                .expect("open the planted log to age it");
            f.set_times(
                std::fs::FileTimes::new()
                    .set_modified(SystemTime::now() - Duration::from_secs(secs)),
            )
            .expect("the temp filesystem must accept a modified time or this test proves nothing");
        }

        let tree = TempTree::new("grimoire-logs-written");
        let older = tree.logs().join("eqlog_Older_legends.txt");
        let newer = tree.logs().join("eqlog_Newer_legends.txt");
        std::fs::write(&older, LINE).expect("plant a log");
        std::fs::write(&newer, LINE).expect("plant a log");
        age_by(&older, 7_200);
        age_by(&newer, 300);

        let (mut settings, mut ingest) = boot(&tree);
        assert!(
            pump_until(&mut ingest, |ig| ig.active_log().is_some()),
            "the ingest never adopted a log out of {}",
            tree.logs().display()
        );

        /* VALIDITY FIRST. The ingest has to have listed both files, with the times this test set,
         * and it has to have picked the newer one; a run where the filesystem quietly refused the
         * stamps would otherwise assert over two identical ages and pass on the old code. */
        let now = Utc::now();
        let older_mt = ingest.log_modified(&older);
        let newer_mt = ingest.log_modified(&newer);
        assert!(
            older_mt.is_some() && newer_mt.is_some(),
            "the ingest did not list both planted logs with a modified time on each"
        );
        let older_cell = written_cell(older_mt, now);
        let newer_cell = written_cell(newer_mt, now);
        assert_eq!(older_cell, "2h ago", "the loser's stamp did not take");
        assert_eq!(newer_cell, "5m ago", "the winner's stamp did not take");
        assert_eq!(
            ingest.active_log().expect("just asserted").path,
            newer,
            "the ingest tailed the older file, so this is not the ranking the column explains"
        );

        let words = painted(&mut settings, &mut ingest, 1400.0);

        /* THE HEADER, MATCHED WHOLE. `says` would not do here: the field above this table is
         * labelled `file last written`, so a `contains` check for "last written" was green on the
         * page that had no such column. */
        assert!(
            words.iter().any(|w| w == "last written"),
            "the candidate table has no column for the value it is sorted by: {words:?}"
        );

        /* The row nobody read, whose modified time is printed in exactly one place on this page. */
        assert!(
            words.iter().any(|w| w == &older_cell),
            "the unread candidate's modified time never reached the page, so the reader still \
             cannot see why it lost: {words:?}"
        );

        /* And the marked row, which the FILE BEING READ field already prints once. */
        let n = words.iter().filter(|w| w.as_str() == newer_cell).count();
        assert_eq!(
            n, 2,
            "the tailed file's modified time is painted {n} times; it belongs to the file field \
             once and to its candidate row once: {words:?}"
        );
    }

    /// DEFECT: `None` and `Some(0)` collapsing into one answer.
    ///
    /// No file being tailed and a file read from byte 0 are opposite states: one means nothing was
    /// read, the other means everything was. A `unwrap_or(0)` anywhere on that path prints "the
    /// whole file was taken" over a machine that has not opened a file.
    ///
    /// WHAT MUTATION MAKES THIS RED: folding the `None` arm into the `Some(0)` arm, or dropping
    /// the byte figure out of the clipped sentence.
    #[test]
    fn the_cap_tells_no_file_apart_from_the_whole_file() {
        let (idle, none) = cap_words(None);
        let (settled, whole) = cap_words(Some(0));
        let (wrong, cut) = cap_words(Some(41_943_040));
        assert_eq!(idle, State::Idle);
        assert_eq!(settled, State::Settled);
        assert_eq!(wrong, State::Wrong);
        assert!(none.contains("nothing was read"), "{none}");
        assert!(whole.contains("byte 0"), "{whole}");
        assert!(cut.contains("41,943,040"), "{cut}");
        assert!(cut.contains("floor"), "{cut}");
        assert!(
            none != whole && whole != cut,
            "two of the three states say the same thing"
        );
    }

    /// DEFECT: the clipped sentence quietly acquiring a denominator.
    ///
    /// The start byte was measured at the last bootstrap and `LogFile::size` on the current poll.
    /// A sentence like "byte 41,943,040 of 62,914,560" or a percentage built from the two is made
    /// of two different moments of a growing file, and it is the most quotable thing this page
    /// could print. `cap_words` is handed ONE number and cannot reach a second, which is the
    /// structural half of that refusal; this is the half that says so out loud.
    ///
    /// TWO FIGURES ARE ALLOWED IN IT AND NEITHER OF THEM IS A SECOND MEASUREMENT: the start byte,
    /// which is the argument, and the cap, which is a compile time constant and is the same on
    /// every machine. So the check is to strike both out and assert that no digit is left. A
    /// literal `" of "` will not do as the check, because the sentence legitimately says "from the
    /// END of the file"; a first cut of this test asserted that and went red on its own prose,
    /// which is a test measuring the wrong thing rather than a defect.
    ///
    /// WHAT MUTATION MAKES THIS RED: giving `cap_words` the file size and printing a share, a
    /// remainder, or "byte N of M".
    #[test]
    fn the_cap_sentence_states_one_measured_number_and_no_share() {
        let (_, cut) = cap_words(Some(41_943_040));
        assert!(!cut.contains('%'), "a share appeared in: {cut}");
        assert!(
            cut.contains("41,943,040"),
            "the start byte is missing: {cut}"
        );
        let rest = cut.replace("41,943,040", "").replace(&tail_cap_text(), "");
        assert!(
            !rest.chars().any(|c| c.is_ascii_digit()),
            "a figure that is neither the start byte nor the cap appeared in: {cut}"
        );
    }

    /// DEFECT: a coverage figure invented for the unreadable count.
    ///
    /// Nothing published by the ingest says how many lines the tail held, so "12 of about 400,000"
    /// cannot be supported. It is also the version a reader would believe, which is what makes it
    /// worth a test rather than a comment.
    ///
    /// WHAT MUTATION MAKES THIS RED: any percentage, ratio or "of N" in these words.
    #[test]
    fn the_unreadable_count_is_never_shown_as_a_share() {
        let (settled, none) = unreadable_words(0, true, "12s ago");
        let (wrong, some) = unreadable_words(12, true, "12s ago");
        assert_eq!(settled, State::Settled);
        assert_eq!(wrong, State::Wrong);
        assert!(none.contains("No unreadable stamp"), "{none}");
        assert!(some.contains("12 lines"), "{some}");
        assert!(
            unreadable_words(1, true, "12s ago").1.contains("1 line"),
            "one unreadable line was reported in the plural"
        );
        for w in [none.as_str(), some.as_str(), NO_DENOMINATOR] {
            assert!(!w.contains('%'), "a share appeared in: {w}");
        }
        assert!(
            NO_DENOMINATOR.contains("no denominator"),
            "the page stopped saying why there is no share"
        );
    }

    /// DEFECT: a rounded size shipping without the exact one beside it.
    #[test]
    fn a_size_carries_the_exact_byte_count() {
        assert_eq!(bytes_text(0), "0 bytes");
        assert_eq!(bytes_text(999), "999 bytes");
        assert_eq!(bytes_text(41_943_040), "41,943,040 bytes (40.0 MB)");
        assert_eq!(thousands(16_526), "16,526");
        assert_eq!(thousands(0), "0");
        assert_eq!(count_text(1), "1 eqlog file");
        assert_eq!(count_text(4), "4 eqlog files");
        assert_eq!(fights_text(1), "1 fight");
        assert_eq!(lines_text(1), "1 line");
    }

    /// DEFECT: "read 3s ago" printed over an ingest that has never read anything.
    #[test]
    fn a_thing_never_read_says_never() {
        let now = Utc::now();
        assert_eq!(since_text(None, now), "never");
        let t = now - chrono::Duration::seconds(12);
        assert_eq!(since_text(Some(t), now), "12s ago");
        assert_eq!(
            modified_text(None, now),
            "the filesystem did not report one"
        );
    }

    /// DEFECT: the folder placeholder row drawn as a log file.
    ///
    /// `Ingest::sources` pushes a row with the Log kind and a DIRECTORY path when the folder holds
    /// nothing. A candidate table that took the kind alone would list the Logs folder itself as a
    /// candidate log, with "not read" beside it, which is a file that does not exist.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `looks_like_eq_log` half of `is_log_file`.
    #[test]
    fn the_folder_placeholder_is_not_a_candidate() {
        let dir = Source {
            kind: SourceKind::Log,
            path: PathBuf::from("C:/Logs"),
            last_read: None,
            records: 0,
            problem: Some("No eqlog_*.txt files".to_owned()),
        };
        let file = Source {
            kind: SourceKind::Log,
            path: PathBuf::from("C:/Logs/eqlog_Stoic_legends.txt"),
            last_read: None,
            records: 0,
            problem: None,
        };
        let dump = Source {
            kind: SourceKind::Inventory,
            path: PathBuf::from("C:/Stoic_legends-Inventory.txt"),
            last_read: None,
            records: 3,
            problem: None,
        };
        assert!(!is_log_file(&dir));
        assert!(is_log_file(&file));
        assert!(!is_log_file(&dump));
    }

    /// DEFECT: the `not tailed` note repeated on every losing row, or lifted off the row that is
    /// actually being read.
    #[test]
    fn the_not_tailed_note_is_taken_from_a_losing_row() {
        let won = Source {
            kind: SourceKind::Log,
            path: PathBuf::from("C:/Logs/eqlog_Stoic_legends.txt"),
            last_read: None,
            records: 4,
            problem: None,
        };
        let lost = Source {
            kind: SourceKind::Log,
            path: PathBuf::from("C:/Logs/eqlog_Other_legends.txt"),
            last_read: None,
            records: 0,
            problem: Some(format!(
                "{} only the most recently written log is read, and that is \
                 eqlog_Stoic_legends.txt",
                crate::nav::NOT_TAILED_PREFIX
            )),
        };
        let rows = vec![&won, &lost];
        let note =
            shared_not_tailed_note(&rows, Some(won.path.as_path())).expect("the losing row's note");
        assert!(note.starts_with(crate::nav::NOT_TAILED_PREFIX));
        /* One log, which is the winner, has nothing to explain. */
        assert!(shared_not_tailed_note(&[&won], Some(won.path.as_path())).is_none());

        /* AND THE TWO STATES WHERE THAT SENTENCE NAMES THE WRONG FILE. The ingest writes it as
         * "only the most recently written log is read, and that is <the top row>", so it is only
         * true while the top row is the row being read. With nothing being read it names a file
         * the app never opened; mid camp it names the file the app has NOT adopted yet, while the
         * mark is on another row, and printed as a summary above the table it would contradict the
         * table in the app's own voice.
         *
         * WHAT MUTATION MAKES THIS RED: dropping the `top != active` gate. */
        assert!(
            shared_not_tailed_note(&rows, None).is_none(),
            "the not tailed note was printed over a table where nothing is being read"
        );
        let behind = vec![&lost, &won];
        assert!(
            shared_not_tailed_note(&behind, Some(won.path.as_path())).is_none(),
            "the not tailed note named the top row as the file being read while the mark was \
             on a lower row"
        );
    }

    /// DEFECT: THE PAGE'S LOAD BEARING SENTENCE, ASSERTED INSTEAD OF OBSERVED.
    ///
    /// "The top row is the one being read" was a constant. It is false in two ordinary states, and
    /// both of them are states a reader comes to this page IN. A log that will not open leaves
    /// `logs` full and `active` None, so nothing is read and no row is marked; and `tail` assigns
    /// the fresh folder listing BEFORE it starts the rescan, so for the seconds after camping to
    /// another character the top row is a file the app has not opened and the mark is lower down.
    /// A reader who believes the sentence over the mark goes off to check the wrong character's
    /// numbers, which is the exact failure this whole page was built to prevent.
    ///
    /// WHAT MUTATION MAKES THIS RED: hardcoding the "top row" clause again, or dropping the
    /// `scanning` gate and promising a read that is not running.
    #[test]
    fn the_ranking_sentence_only_claims_the_top_row_when_it_is_the_marked_one() {
        let top = Source {
            kind: SourceKind::Log,
            path: PathBuf::from("C:/Logs/eqlog_Newer_legends.txt"),
            last_read: None,
            records: 0,
            problem: None,
        };
        let below = Source {
            kind: SourceKind::Log,
            path: PathBuf::from("C:/Logs/eqlog_Older_legends.txt"),
            last_read: None,
            records: 4,
            problem: None,
        };
        let rows = vec![&top, &below];

        /* `.0` IS PAINTED AND `.1` IS THE HOVER. Which half a claim lives in is itself part of
         * what this test pins: the alarming states must be legible without hovering anything. */
        let steady = ranking_words(&rows, Some(top.path.as_path()), false);
        assert!(steady.0.contains("top row is the one read"), "{}", steady.0);

        /* Nothing read: no claim about a top row, and the words say no row is marked so the
         * reader is not left hunting for an edge that is not painted. */
        let none = ranking_words(&rows, None, false);
        assert!(
            !none.0.contains("top row is"),
            "a file nobody opened was reported as the file being read: {}",
            none.0
        );
        assert!(none.0.contains("none marked"), "{}", none.0);

        /* Mid camp: BOTH FILES ARE NAMED ON THE PAINTED HALF and not on the hover. Naming both
         * is the point: "not the top row" alone would leave the reader unable to tell which set
         * of numbers he is looking at, and a reader does not hover a line he has no reason to
         * suspect. This is the one arm on the page that stays loud. */
        let camp = ranking_words(&rows, Some(below.path.as_path()), true);
        assert!(camp.0.contains("eqlog_Older_legends.txt"), "{}", camp.0);
        assert!(camp.0.contains("eqlog_Newer_legends.txt"), "{}", camp.0);
        assert!(camp.0.contains("not the top row"), "{}", camp.0);
        assert!(
            camp.1.contains("the mark moves to it when that read lands"),
            "a bootstrap is in flight and the page did not say so: {}",
            camp.1
        );

        /* And with no scan running the catch up is not promised, because nothing is coming. */
        let stuck = ranking_words(&rows, Some(below.path.as_path()), false);
        assert!(
            !stuck
                .1
                .contains("the mark moves to it when that read lands"),
            "the page promised a read that is not running: {}",
            stuck.1
        );
        assert!(stuck.1.contains("no read is running"), "{}", stuck.1);

        /* THE TWO STEADY ARMS COUNT THE FILES; the two camp arms name them instead, which is
         * more useful in the state that matters and is why this is not asserted over all four. */
        for w in [&steady, &none] {
            assert!(w.0.contains("2 eqlog files"), "{}", w.0);
        }

        /* AND EVERY PAINTED HALF IS A LINE RATHER THAN A PARAGRAPH. */
        for w in [&steady, &none, &camp, &stuck] {
            assert!(
                crate::screens::words(&w.0) <= crate::screens::MAX_WORDS,
                "still a sentence: {}",
                w.0
            );
        }
    }

    /// DEFECT: AN ORDINARY CAMP PAINTED AS A FAULT.
    ///
    /// The words under the file name used to be marked `State::Wrong` as a literal, on the theory
    /// that a file being read plus a sentence about it could only mean a read fault. A rescan
    /// leaves the old file installed as the active one while the worker bootstraps the new one, so
    /// `file_state` hands back Working with the READING sentence and that literal painted a
    /// perfectly ordinary character switch red. The owner streams with this on screen: a red
    /// square reads as "broken" from across a room, and this is the page he would be opening to
    /// find out whether anything is.
    ///
    /// DRIVEN OVER A REAL INGEST AND OVER THE PAINTED SQUARES, not over `file_state` alone. The
    /// literal being guarded is in [`active`], several calls downstream of the classifier, so a
    /// test that only asked `file_state` what it returned would have gone green with the literal
    /// still in place: it would have named a mutation it could not see. The state that matters is
    /// `scanning` with an active log STILL INSTALLED, which is a race no planted enum reaches, so
    /// `rescan` is called on an ingest that has already adopted a log and the frame is drawn
    /// before that worker lands.
    ///
    /// WHAT MUTATION MAKES THIS RED: writing any literal state into the second mark in [`active`].
    /// With `State::Wrong` written back in, a WRONG square is painted under the file name and this
    /// goes red on the colour, not on the words.
    #[test]
    fn a_rescan_over_a_live_log_is_not_painted_as_a_fault() {
        /* A LOG BIG ENOUGH THAT THE WORKER IS STILL BUSY WHEN THE FRAME DRAWS. This is the whole
         * difficulty of the test: `LogsScreen::ui` pumps the ingest before it draws, so over the
         * 47 byte log the other tests plant, the rescan lands inside the frame's own `tail()` and
         * the page is painted in the state AFTER the scan rather than during it. A few megabytes
         * put two full regex passes between `rescan` and `adopt`, which is tens of milliseconds
         * against a warm frame's fraction of one. The margin is checked, not assumed: the frame is
         * only believed if `scanning` is still true on the far side of it. */
        let tree = TempTree::new("grimoire-logs-rescan");
        let mut bulk = String::with_capacity(SLOW_LOG_BYTES + LINE.len());
        while bulk.len() < SLOW_LOG_BYTES {
            bulk.push_str(LINE);
        }
        std::fs::write(tree.logs().join("eqlog_Stoic_legends.txt"), &bulk).expect("plant a log");
        let (mut settings, mut ig) = boot(&tree);
        assert!(
            pump_until(&mut ig, |ig| ig.active_log().is_some()),
            "the ingest never adopted the planted log"
        );

        /* Paid for before the window opens, so the window holds nothing but the frame. */
        let ctx = prepared_ctx();

        ig.rescan();
        /* THE VALIDITY CHECKS BEFORE THE RESULT. Without a scan in flight AND an active log still
         * installed, the branch under test is not the branch that runs and a green here would
         * mean nothing. */
        assert!(
            ig.scanning(),
            "not a discriminating case: no scan in flight"
        );
        assert!(
            ig.active_log().is_some(),
            "not a discriminating case: the old log was dropped, so `active` takes its None arm"
        );

        let (state, words) = file_state(&ig);
        assert_eq!(
            state,
            State::Working,
            "a rescan over a log that is still installed was not reported as work in progress"
        );
        let words = words.expect("a rescan has something to say");
        assert!(words.contains("still being read"), "{words}");

        /* AND NOW THE PAINT, which is the half the literal lived in. The same frame must not draw
         * a single fault square while the app is doing nothing worse than reading. */
        let fills = squares_in(&ctx, &mut settings, &mut ig, 1400.0);
        assert!(
            ig.scanning(),
            "the scan landed during the frame, so the page was painted after the rescan rather \
             than during it and this run proves nothing"
        );
        assert!(
            !fills.is_empty(),
            "no state square was painted at all, so this test is measuring nothing"
        );
        assert!(
            fills.contains(&WORKING),
            "the rescan was never painted as work in progress: {fills:?}"
        );
        assert!(
            !fills.contains(&WRONG),
            "an ordinary camp painted a fault square: {fills:?}"
        );
    }

    /// DEFECT: this page saying nothing on the machine that needs it most.
    ///
    /// A reader whose Logs folder is empty is the reader who opens this page. Every other screen
    /// answers him with an empty state; this one has to answer him with the folder, the reason
    /// there are no candidates, and the two skip facts, because that IS the content. A page that
    /// returned early on "no log" would be one more blank screen in a row of them.
    ///
    /// WHAT MUTATION MAKES THIS RED: an early return anywhere in `LogsScreen::ui` when there is no
    /// active log.
    #[test]
    fn an_empty_folder_still_gets_a_whole_page() {
        let tree = TempTree::new("grimoire-logs-empty");
        let (mut settings, mut ingest) = boot(&tree);
        assert!(
            pump_until(&mut ingest, |ig| !ig.scanning()),
            "the ingest never finished its first scan of {}",
            tree.logs().display()
        );
        let words = painted(&mut settings, &mut ingest, 1100.0);

        for head in [
            "THE FOLDER",
            "THE FILE BEING READ",
            "THE CANDIDATES",
            "WHAT WAS SKIPPED",
            "THE LINES THEMSELVES",
        ] {
            assert!(
                words.iter().any(|w| w == head),
                "the page never painted {head:?} over an empty folder: {words:?}"
            );
        }
        /* The folder it looked in, which is the fact that tells this reader the app is pointed
         * where he thinks it is. */
        assert!(
            says(&words, &tree.logs().display().to_string()),
            "the page never named the folder it read: {words:?}"
        );
        /* THE INGEST'S OWN WORDS ABOUT THE EMPTY FOLDER REACH THE PAGE, and the needle has to be
         * a phrase ONLY the ingest writes. This assertion used to look for "eqlog", which four
         * separate constants on this page say unconditionally (WHY_THIS_FILE, the folder note,
         * NO_FOLDER), so it passed whether or not `active_problem` was ever painted and proved
         * nothing at all. "Is logging on?" appears in exactly one place in this tree: the sentence
         * `scan` builds when `list_logs` returns an empty vector. */
        assert!(
            says(&words, "Is logging on?"),
            "the ingest's reason for the empty folder never reached the page: {words:?}"
        );
        /* No file was opened, so the cap section must say so rather than claim byte 0. */
        assert!(
            says(&words, "nothing was read"),
            "the skip section claimed something about a file that was never opened: {words:?}"
        );
    }

    /// DEFECT: the page naming a file the ingest is not reading.
    ///
    /// THE WHOLE POINT OF THE PAGE IS THIS ONE COUPLING. Two logs are planted with the second
    /// written after the first, both are listed, and the file the page prints in THE FILE BEING
    /// READ has to be the one `Ingest::active_log` actually picked, with the same path. The winner
    /// is read off the ingest rather than assumed from the write order, so this asserts the page
    /// follows the app and not that a filesystem stamped two files the way the test expected.
    ///
    /// AND THE CANDIDATE ORDER IS PART OF THE CLAIM. The page prints "newest first" and that is
    /// only true while `sources` hands its log rows back in the ingest's own ranking; the first
    /// log row is asserted to be the active one, which is the observable half of that sentence.
    ///
    /// WHAT MUTATION MAKES THIS RED: sorting or filtering the candidate rows here, listing the
    /// folder with `read_dir` instead of asking the ingest, or printing a file name this page
    /// chose.
    #[test]
    fn the_page_names_the_file_the_ingest_actually_reads() {
        let tree = TempTree::new("grimoire-logs-winner");
        std::fs::write(tree.logs().join("eqlog_Older_legends.txt"), LINE).expect("plant a log");
        /* The mtime sort needs the two writes to be distinguishable. The winner is still read off
         * the ingest below, so this only has to make a WINNER exist, not decide which. */
        std::thread::sleep(Duration::from_millis(30));
        std::fs::write(tree.logs().join("eqlog_Newer_legends.txt"), LINE).expect("plant a log");

        let (mut settings, mut ingest) = boot(&tree);
        assert!(
            pump_until(&mut ingest, |ig| ig.active_log().is_some()),
            "the ingest never adopted a log out of {}",
            tree.logs().display()
        );

        let picked = ingest.active_log().expect("just asserted").path.clone();
        let listed: Vec<PathBuf> = ingest
            .sources()
            .into_iter()
            .filter(is_log_file)
            .map(|s| s.path)
            .collect();
        /* THE VALIDITY CHECK BEFORE THE RESULT. With one log listed, "the first row is the one
         * being read" holds for free and this case proves nothing about the ordering. */
        assert_eq!(
            listed.len(),
            2,
            "not a discriminating case: {listed:?} is not two candidate logs"
        );
        assert_eq!(
            listed.first(),
            Some(&picked),
            "the ingest's own log rows do not lead with the file it reads, so the page's \
             'newest first' sentence is not true of them"
        );

        let words = painted(&mut settings, &mut ingest, 1400.0);
        /* The full path is printed in exactly one place on this page: the file section. */
        assert!(
            says(&words, &picked.display().to_string()),
            "the page never printed the path of the file being read: {words:?}"
        );
        /* Both candidates are listed, so this is a table and not just the winner twice. */
        for name in ["eqlog_Older_legends.txt", "eqlog_Newer_legends.txt"] {
            assert!(
                says(&words, name),
                "the candidate table dropped {name}: {words:?}"
            );
        }
        /* And the loser's cell says it was not read rather than reporting zero kills for it. */
        assert!(
            says(&words, "not read"),
            "the unread candidate reported a figure instead of saying it was not read: {words:?}"
        );
    }

    /// DEFECT: this page growing a line viewer that shows something other than the log.
    ///
    /// The ingest publishes no line of text. The refusal has to be ON THE PAGE, in words that name
    /// what is missing, because the alternative a future hand reaches for is a viewer fed from
    /// somewhere else: a re-read of the file by this screen, or the fight rows dressed up as
    /// lines. Either would be a second reader of the log disagreeing with the first about what is
    /// in it.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the section, or softening it to a bare "no data".
    #[test]
    fn the_absent_line_viewer_is_explained_and_not_faked() {
        let tree = TempTree::new("grimoire-logs-lines");
        std::fs::write(tree.logs().join("eqlog_Stoic_legends.txt"), LINE).expect("plant a log");
        let (mut settings, mut ingest) = boot(&tree);
        assert!(
            pump_until(&mut ingest, |ig| ig.active_log().is_some()),
            "the ingest never adopted the planted log"
        );
        let words = painted(&mut settings, &mut ingest, 1400.0);
        /* THE PAGE SAYS THERE IS NO VIEWER; THE HOVER SAYS WHY. Both halves are asserted, because
         * a refusal a reader cannot interrogate is indistinguishable from a stub, and a refusal
         * that lectures him is what this page used to be. */
        assert!(
            says(&words, NO_LINES),
            "the page stopped saying it has no line viewer: {words:?}"
        );
        /* THE REASON NAMES WHAT IS MISSING, AND IT MUST NOT DENY WHAT EXISTS.
         *
         * This used to require the words "does not publish" and "accessor", which pinned a claim
         * the codebase had already disproved: `Ingest::recent_tail` IS a public accessor returning
         * lines, and `screens::live` reads the EFFECTS panel out of it. A test can hold a refusal
         * to a false premise as easily as to a true one, and this one did. What is really missing
         * is a reader over the FILE rather than over the tail, and that is what is asserted now. */
        assert!(
            NO_LINES_WHY.contains("recent_tail"),
            "the refusal does not name the accessor that DOES exist, so a reader cannot tell what \
             is actually missing: {NO_LINES_WHY}"
        );
        assert!(
            !NO_LINES_WHY.contains("no accessor"),
            "the refusal claims again that nothing publishes lines: {NO_LINES_WHY}"
        );
        assert!(
            NO_SPLIT_WHY.contains("accessor"),
            "the split refusal stopped naming what would have to exist: {NO_SPLIT_WHY}"
        );
        /* The planted line itself must NOT appear: that would mean something on this page went and
         * read the file. */
        assert!(
            !says(&words, "Welcome to EverQuest"),
            "a log line reached the page, so something here is reading the file: {words:?}"
        );
    }

    /// DEFECT: the state square disagreeing with what the app is doing.
    ///
    /// Driven over two real ingests rather than over a planted enum: `file_state` asks
    /// `why_no_fights` three questions about a live `Ingest`, and the arm that matters is the one
    /// where a folder exists and holds nothing, which no pure seam can reach.
    ///
    /// WHAT MUTATION MAKES THIS RED: swapping the NoFolder and NoLog arms, or reporting Settled
    /// while `active_problem` is set.
    #[test]
    fn the_state_follows_the_ingest() {
        let empty = TempTree::new("grimoire-logs-state-empty");
        let (_, mut ig) = boot(&empty);
        assert!(
            pump_until(&mut ig, |ig| !ig.scanning()),
            "scan never landed"
        );
        let (state, words) = file_state(&ig);
        assert_eq!(state, State::Wrong, "an empty Logs folder is a fault state");
        let words = words.expect("an empty folder needs words");
        assert!(
            words.contains("eqlog"),
            "the empty folder's words did not name what was missing: {words}"
        );

        let full = TempTree::new("grimoire-logs-state-full");
        std::fs::write(full.logs().join("eqlog_Stoic_legends.txt"), LINE).expect("plant a log");
        let (_, mut ig) = boot(&full);
        assert!(
            pump_until(&mut ig, |ig| ig.active_log().is_some()),
            "the ingest never adopted the planted log"
        );
        let (state, words) = file_state(&ig);
        assert_eq!(state, State::Settled, "a log is being read");
        assert!(
            words.is_none(),
            "a healthy read still produced a complaint: {words:?}"
        );
    }

    /// DEFECT: A DATA PAGE THAT SPENDS MORE ROOM ON SENTENCES THAN ON FIGURES.
    ///
    /// # THE OWNER'S OWN WORDS, BECAUSE THEY ARE THE SPECIFICATION
    ///
    /// "The ONLY fucking words should be data." The main Grimoire window answers SHOW ME ALL THE
    /// INFORMATION ON THAT ENCOUNTER; an always-on-top overlay answers SHOW ME WHAT IS SHITTING
    /// UP AND HOW MUCH. Neither question is answered by a paragraph, and the design sheets for
    /// both surfaces carry no sentences at all: a title row, a tab row, tiles, a chart, tables.
    ///
    /// WHAT THIS CAUGHT THE DAY IT WAS WRITTEN, measured by drawing the pages and reading the
    /// text back out of the shapes: the Reports page painted the same four paragraphs above every
    /// tab, the Dashboards page put a forty four word staleness note above all eight role tabs
    /// and gave Healer, Tank, Pet and Raid Leader an essay each about panels they do not have,
    /// and the Logs page carried five rules longer than the facts they qualified. None of it was
    /// wrong. All of it was in the way.
    ///
    /// # WHY THE CAP IS WORDS AND WHY IT ONLY APPLIES WITH DATA IN HAND
    ///
    /// A character cap punishes the long strings a data page is SUPPOSED to paint: a date range,
    /// a mob's name, a log filename. Prose is long because it has many words in it. And a page
    /// with nothing to draw has nothing but words, so the cap is asserted over a frame drawn on
    /// the reference capture, where every panel has rows. The empty state is tested separately
    /// and is allowed to speak: a blank rectangle cannot be told from a broken screen.
    ///
    /// EXPLANATION IS MOVED, NOT BANNED. The tiles, headings and scope buttons on these pages
    /// carry their caveats in `on_hover_text`, which is not painted and so never reaches this.
    ///
    /// WHAT MUTATION MAKES THIS RED: painting any of the removed paragraphs again, or writing a
    /// new one.
    #[test]
    fn this_page_paints_figures_and_not_paragraphs() {
        let dir = crate::fights::probe::planted("prose-logs", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        assert_eq!(ing.fights().len(), 12, "this must run with data in hand");
        let mut s = Settings::default();
        let prose = crate::screens::prose(&painted(&mut s, &mut ing, 1100.0));
        assert!(
            prose.is_empty(),
            "the Logs page paints {} sentence(s) over its facts: {prose:#?}",
            prose.len()
        );
    }

    /// DEFECT: A GREEN ALL-CLEAR ABOUT A TAIL THAT WAS NEVER READ.
    ///
    /// `fights_unreadable` starts at 0 and is only ever assigned inside the `Ok` arm of
    /// `read_tail`, so on an empty Logs folder, during the first bootstrap, and whenever the
    /// newest log will not open, the count is 0 because NOTHING WAS READ. The page drew that as a
    /// Settled green square reading "No line in that tail carried a stamp this build could not
    /// read", one line under its own idle square saying "No file is being tailed, so nothing was
    /// read and nothing was skipped". Two squares, two colours, one line apart, disagreeing about
    /// whether a read happened.
    ///
    /// IT IS THE IMMUNE-SLICE MISTAKE UNDER ANOTHER WORD, which `Outcomes::hypothesised` exists to
    /// prevent in the engine: a drawn zero says the app looked and found none, and this one meant
    /// the app had not looked.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `read` argument, or returning Settled for it.
    #[test]
    fn a_count_of_zero_from_a_read_that_never_happened_is_not_an_all_clear() {
        let (state, words) = unreadable_words(0, false, "6h ago");
        assert_eq!(
            state,
            State::Idle,
            "a machine that never read a tail was given the green square: {words}"
        );
        assert!(
            !words.contains("stamp"),
            "it still claims it checked the stamps of a tail it never read: {words}"
        );
        /* AND IT CARRIES NO AGE. There is no moment at which nothing was read, so an age here
         * would be the page dating an event that did not happen. The stamp handed in is one this
         * arm must ignore, which is why it is not the empty string. */
        assert!(
            !words.contains("6h ago"),
            "the never-read arm dated a read that never happened: {words}"
        );

        /* AND THE TWO ZEROES DO NOT SAY THE SAME THING, which is the whole distinction. */
        let (read_state, read_words) = unreadable_words(0, true, "6h ago");
        assert_eq!(read_state, State::Settled);
        assert_ne!(
            words, read_words,
            "a tail that was read clean and a tail that was never read say the same sentence"
        );
    }

    /// DEFECT: A GREEN ALL-CLEAR WITH NO AGE ON IT, ABOUT A READ THAT FINISHED AT BREAKFAST.
    ///
    /// `fights_unreadable` is written once, by the bootstrap, and the live fold never touches it.
    /// The sentence carried no as-of at all, so on a session left open since morning the page said
    /// "No line in that tail carried a stamp this build could not read" as though it had just
    /// looked. It had not, and the line above it was by then describing a different read.
    ///
    /// THE THREE LINES OF `WHAT WAS SKIPPED` ARE ASSERTED TOGETHER, because the defect is in how
    /// they read as a group: the cap line is a live figure, the other two are as old as the
    /// bootstrap, and until this fix all three sat in one block with one demonstrative (`that
    /// tail`) tying them into a single claim.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `since` argument from either sentence, putting
    /// `LAST_FULL_READ` on the cap line, or taking `on this poll` off it.
    #[test]
    fn every_line_of_what_was_skipped_says_which_read_it_came_from() {
        let (_, clean) = unreadable_words(0, true, "6h ago");
        assert!(
            clean.contains(LAST_FULL_READ) && clean.contains("6h ago"),
            "the all-clear does not say which read it is about, or when: {clean}"
        );
        let (_, bad) = unreadable_words(12, true, "6h ago");
        assert!(
            bad.contains(LAST_FULL_READ) && bad.contains("6h ago"),
            "the unreadable count does not say which read it is about, or when: {bad}"
        );
        let (_, folded) = folded_words(4, false, "6h ago");
        assert!(
            folded.contains(LAST_FULL_READ) && folded.contains("6h ago"),
            "the fight count does not say which read it is about, or when: {folded}"
        );

        /* AND THE CAP LINE IS THE ODD ONE OUT ON PURPOSE. `tail_start` is rewritten on the poll
         * that finds the file has shrunk, so it is current, and a line that borrowed the
         * bootstrap's age would be dating a live figure to an old read. */
        let (_, cap) = cap_words(Some(0));
        assert!(
            cap.contains("on this poll"),
            "the cap line stopped saying it is a figure from this poll: {cap}"
        );
        assert!(
            !cap.contains(LAST_FULL_READ),
            "the cap line dated a live figure to the bootstrap: {cap}"
        );

        /* THE ALL-CLEAR IS STILL A LINE AND NOT A SENTENCE. It is painted on the ordinary machine
         * where everything is fine, so it is the one arm the page's own word cap applies to. */
        assert!(
            crate::screens::words(&clean) <= crate::screens::MAX_WORDS,
            "the all-clear grew into prose: {clean}"
        );
        assert!(
            crate::screens::words(&folded) <= crate::screens::MAX_WORDS,
            "the fight count grew into prose: {folded}"
        );
    }

    /// DEFECT: ONE STAMP DOING THE WORK OF TWO, UNDER THE WRONG LABEL.
    ///
    /// # WHAT WAS WRONG, AND WHAT THE FIRST FIX COULD AND COULD NOT DO
    ///
    /// This page had a field labelled `folder last scanned` and it printed
    /// `Ingest::scanned_at`, which is when the BOOTSTRAP ran. The folder itself is re-listed on every
    /// poll, about once a second. So on a session left open for an evening the page said the
    /// folder had been scanned six hours ago, about a folder the app had listed one second
    /// earlier, and the reader's conclusion is that the app stopped looking, which is the one
    /// question this page exists to answer.
    ///
    /// THE FIRST FIX WAS THE LABEL, because the VALUE was worth printing and the listing's own age
    /// could not be printed at all: nothing on `Ingest` stamped it. This test guarded that, by
    /// refusing any label that dated the listing.
    ///
    /// `Ingest::listed_at` STAMPS IT NOW, so that guard's premise is gone and the page carries
    /// BOTH stamps. Refusing the label is no longer the rule; the rule is that the two are
    /// different readings of different acts and neither may stand in for the other.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// Both fields are drawn, and the listing is NOT older than the read. That inequality is the
    /// whole defect expressed as a number: the bootstrap happens once, at launch, and the listing
    /// happens on every poll after it, so a listing stamp older than the read stamp means the page
    /// has them the wrong way round or is printing one value twice.
    ///
    /// WHAT MUTATION MAKES THIS RED: feeding the listing field from `scanned_at` (they would be
    /// equal and the ordering check would still pass, which is why the SOURCES are compared as
    /// well as the page), dropping either field, or bringing the old label back.
    #[test]
    fn the_page_dates_the_folder_listing_and_the_read_apart() {
        let tree = TempTree::new("grimoire-logs-stamp");
        std::fs::write(tree.logs().join("eqlog_Stoic_legends.txt"), LINE).expect("plant a log");
        let (mut settings, mut ingest) = boot(&tree);
        assert!(
            pump_until(&mut ingest, |ig| ig.active_log().is_some()),
            "the ingest never adopted the planted log"
        );
        /* THE VALIDITY CHECK: with no file adopted, the whole field section is skipped and this
         * test would be asserting over a page that never drew either label. */
        assert!(ingest.scanned_at().is_some(), "no bootstrap has landed");

        /* THE TWO SOURCES ARE DIFFERENT ACTS, and this is the half a page reading could not
         * catch: printing `scanned_at` twice under two labels draws two fields that look right
         * and are one measurement. */
        let listed = ingest.listed_at().expect("the folder has been listed");
        let read = ingest.scanned_at().expect("the bootstrap has landed");
        /* NOT EQUAL, AND NOT AN ORDERING. The first draft asserted `listed >= read` on the
         * reasoning that the bootstrap happens once at launch and the listing happens on every
         * poll after it. That is true of a session and false of the FIRST poll, which is the only
         * one this test sees: `tail` lists the folder near its top and the bootstrap's `adopt`
         * lands later in the same call, so at launch the read is a few hundred microseconds NEWER
         * than the listing. The test failed on the truth.
         *
         * WHAT ACTUALLY HAS TO HOLD IS THAT THEY ARE TWO READINGS AND NOT ONE. The defect this
         * guards is a page that prints `scanned_at` under both labels, which would draw two fields
         * that look right, agree forever, and answer only one of the two questions a reader has.
         * Two stamps taken at two moments are never equal; one value printed twice always is. */
        assert_ne!(
            listed, read,
            "the folder listing and the read carry the same instant, so the page is printing one \
             measurement under two labels"
        );

        let words = painted(&mut settings, &mut ingest, 1400.0);
        for label in [FOLDER_LISTED, LAST_FULL_READ] {
            assert!(
                words.iter().any(|w| w == label),
                "the file section does not draw {label:?}: {words:?}"
            );
        }
        /* AND THE RETIRED LABEL STAYS RETIRED. It named the listing and printed the read, which
         * is the defect this whole block is about. */
        for lie in ["folder last scanned", "last scanned"] {
            assert!(
                !says(&words, lie),
                "the label that named one act and printed another is back: {lie:?} in {words:?}"
            );
        }
    }

    /// Run one widget over four frames with the pointer at `pos`: a frame to register the rect, a
    /// move, a press and a release. Returns the last frame's `(clicked, hovered)`.
    ///
    /// FOUR FRAMES BECAUSE EGUI'S INTERACTION IS A FRAME BEHIND. A widget is interacted with using
    /// the rect it registered on the PREVIOUS frame, and a click is a press on one frame and a
    /// release on a later one. A test that pressed and released inside one frame would find
    /// `clicked()` false over a widget that senses clicks perfectly well, and would then pass for
    /// the wrong reason forever.
    fn press_at(
        ctx: &egui::Context,
        pos: egui::Pos2,
        mut draw: impl FnMut(&mut Ui) -> egui::Response,
    ) -> (bool, bool) {
        let button = |pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let mut out = (false, false);
        for events in [
            Vec::new(),
            vec![egui::Event::PointerMoved(pos)],
            vec![button(true)],
            vec![button(false)],
        ] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(600.0, 200.0),
                )),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                let r = draw(ui);
                out = (r.clicked(), r.hovered());
            })
            .drop_without_applying_deltas();
        }
        out
    }

    /// DEFECT: A TABLE THAT LIGHTS UNDER THE CURSOR AND EATS EVERY CLICK.
    ///
    /// Every candidate row, and the column header with it, was drawn by `items::list_row`, which
    /// allocates with `Sense::click()` for the FIND screens, where a row IS a choice. This page
    /// dropped the `Response`. So the rows behaved like a selectable list, and clicking one did
    /// nothing at all, on a page whose whole subject is which file the app has hold of. A reader
    /// who clicks a row that lights and gets nothing concludes the app is broken, and this is the
    /// page he opened to find out whether it was.
    ///
    /// THE POSITIVE CONTROL IS THE HALF THAT MAKES THIS A MEASUREMENT. `items::list_row` is driven
    /// through the identical four frames at the identical point and MUST report the click. Without
    /// that, a harness that could not deliver a click at all would report "the row is not
    /// clickable" over a row that is, which is a test that proves its own claim by failing to test
    /// it.
    ///
    /// WHAT MUTATION MAKES THIS RED: allocating [`table_row`] with `Sense::click()`, or drawing the
    /// candidates with `items::list_row` again.
    #[test]
    fn a_candidate_row_is_not_a_control() {
        let ctx = prepared_ctx();
        let cols = || {
            vec![Col {
                text: "eqlog_Stoic_legends.txt",
                mono: true,
                color: TEXT_2,
                right: false,
                width: 0.0,
            }]
        };
        /* Inside the first row of a default panel: ROW_H is 22 and the panel's own margin is
         * single digit, so a point ten across and ten down is in it. The control below proves the
         * point lands on the widget rather than beside it. */
        let pos = egui::Pos2::new(120.0, 14.0);

        let control = press_at(&ctx, pos, |ui| {
            crate::screens::items::list_row(ui, false, &cols())
        });
        assert!(
            control.1,
            "the pointer never reached the row, so this test measures nothing"
        );
        assert!(
            control.0,
            "the harness could not deliver a click to a row that senses clicks, so a quiet row \
             below would prove nothing"
        );

        let ours = press_at(&ctx, pos, |ui| table_row(ui, false, true, &cols()));
        assert!(
            ours.1,
            "the candidate row is not even hoverable, so it cannot carry its own hover text"
        );
        assert!(
            !ours.0,
            "the candidate row still senses clicks, and there is nothing behind a click here"
        );
    }

    /// DEFECT: A WHOLE-CORPUS WALK ON EVERY FRAME OF A PAGE THAT NEVER STOPS REPAINTING.
    ///
    /// `Book::unplaceable` looks every spell of every caster up with a linear scan over the 2,001
    /// record corpus, and `LogsScreen::ui` asks for a repaint on every frame while a log is being
    /// tailed. The module note claimed for a build that nothing on this page computes anything,
    /// which is how a corpus walk came to be running sixty times a second unnoticed.
    ///
    /// THE CACHE IS PROVED BY MAKING IT WRONG ON PURPOSE. A caster the book already knows casts a
    /// spell nobody has cast before: neither the caster count nor the corpus length moves, so
    /// nothing in the key changes, and inside the second the reading MUST still be the one already
    /// taken. That is the only observation that separates a cache from a recomputation, because a
    /// recomputation would be right and would still be the defect.
    ///
    /// WHAT MUTATION MAKES THIS RED: calling `unplaceable` from `coverage_words` directly, or
    /// dropping either half of the key, or the tick.
    #[test]
    fn the_coverage_reading_is_kept_for_a_second_and_no_longer() {
        let spells = vec![crate::data::Spell {
            name: "Ignite".to_owned(),
            classes: vec![("Druid".to_owned(), Some(8.0))],
            ..Default::default()
        }];
        let mut book = crate::class::Book::default();
        book.saw("Fylasem", "Ignite");
        let mut cover = Coverage::default();
        let t0 = Instant::now();
        assert_eq!(cover.read(&book, &spells, t0), (1, 0));

        book.saw("Fylasem", "Not A Spell On The Wiki");
        assert_eq!(
            cover.read(&book, &spells, t0),
            (1, 0),
            "the corpus walk ran again inside the same instant, so the reading is not cached"
        );
        assert_eq!(
            cover.read(&book, &spells, t0 + COVER_TTL),
            (1, 1),
            "the reading never went stale, so a gap opened after the first frame would never show"
        );

        /* AND A NEW CASTER DOES NOT WAIT FOR THE TICK: the key catches it on the spot. */
        book.saw("Poguhy", "Not A Spell On The Wiki");
        assert_eq!(
            cover.read(&book, &spells, t0 + COVER_TTL),
            (2, 1),
            "a caster the log had just named waited for the next tick to be counted"
        );
    }

    /// DEFECT: THE COVERAGE LINE VANISHING WITH NO WORDS, WHICH IS THE AMBIGUITY IT WAS ADDED FOR.
    ///
    /// The line was drawn behind `!spells.is_empty() && !classes.is_empty()` and drew nothing at
    /// all otherwise. A machine with no item snapshot is the commonest of those states, and it is a
    /// fresh install: the class column on every other page is blank there, and this page, the one
    /// that exists to say what was not read, said nothing about why.
    ///
    /// AND NO TEST HAD EVER EXECUTED IT: the harness passed `data: None` on every frame, which is
    /// the arm that drew nothing. Reachability is this tree's signature defect and this line had it
    /// twice over, in the app and in the tests.
    ///
    /// WHAT MUTATION MAKES THIS RED: returning an empty string, or an early return, on any arm of
    /// [`coverage_words`].
    #[test]
    fn the_coverage_line_says_something_in_every_state() {
        let spells = vec![crate::data::Spell {
            name: "Ignite".to_owned(),
            classes: vec![("Druid".to_owned(), Some(8.0))],
            ..Default::default()
        }];
        let mut book = crate::class::Book::default();
        book.saw("Fylasem", "Ignite");
        let mut cover = Coverage::default();
        let now = Instant::now();

        let none = coverage_words(true, None, &book, &mut cover, now);
        let empty = coverage_words(true, Some(&[]), &book, &mut cover, now);
        let unread = coverage_words(false, Some(&spells), &book, &mut cover, now);
        let read = coverage_words(true, Some(&spells), &book, &mut cover, now);

        for (state, words, why) in [&none, &empty, &unread, &read] {
            assert!(
                !words.is_empty(),
                "a state of the coverage line drew nothing"
            );
            assert!(
                !why.is_empty(),
                "a state of the coverage line has no reason"
            );
            assert!(
                crate::screens::words(words) <= crate::screens::MAX_WORDS,
                "the coverage line is prose: {words}"
            );
            let _ = state;
        }
        /* FOUR STATES, FOUR ANSWERS. Any two of them sharing a sentence is the ambiguity back. */
        let said = [&none.1, &empty.1, &unread.1, &read.1];
        for (i, a) in said.iter().enumerate() {
            for b in said.iter().skip(i + 1) {
                assert_ne!(a, b, "two states of the coverage line say the same thing");
            }
        }
        /* THE NO-SNAPSHOT ARM NAMES THE SNAPSHOT, because "no class was read" alone would leave a
         * reader thinking his log was the problem. */
        assert!(none.1.contains("snapshot"), "{}", none.1);
        assert!(
            none.2.contains("Settings"),
            "the reason names nowhere to go"
        );
        /* AND THE READ ARM IS THE ONLY ONE THAT PRINTS A FIGURE, because it is the only one where
         * anything was measured. A `0 casters read` over an unopened log is a drawn zero. */
        assert!(
            read.1.contains("1 casters read, 0 spells unplaced"),
            "{}",
            read.1
        );
        for w in [&none.1, &empty.1, &unread.1] {
            assert!(
                !w.chars().any(|c| c.is_ascii_digit()),
                "a state that measured nothing printed a figure: {w}"
            );
        }
        assert_eq!(
            read.0,
            State::Settled,
            "a complete reading is the green one"
        );
        assert_eq!(unread.0, State::Idle);
    }

    /// DEFECT: the coverage line reaching the owner's screen without a test ever drawing it.
    ///
    /// The unit test above drives [`coverage_words`] at its seam. This one drives the PAGE with a
    /// real snapshot in `Cx::data` and a real ingest that has read a real log, which is the path
    /// the app takes and the path no test in this file had ever taken.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting the line back behind a condition that skips it, or
    /// printing a caster count that is not the book's.
    #[test]
    fn the_coverage_line_is_painted_over_a_real_snapshot() {
        let Some(snap) = crate::data::testdata::snapshot() else {
            return;
        };
        let dir = crate::fights::probe::planted("cover-logs", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        /* THE VALIDITY CHECKS. Without casters in the book and spells in the corpus, the arm under
         * test is not the arm that runs. */
        assert!(
            !ing.classes().is_empty(),
            "the capture named no caster, so the coverage arm is not reachable from here"
        );
        assert!(!snap.spells.is_empty(), "the snapshot carries no spells");

        let mut s = Settings::default();
        let words = painted_with(&mut s, &mut ing, Some(snap), 1400.0);
        let wanted = format!("{} casters read", ing.classes().len());
        assert!(
            says(&words, &wanted),
            "the page never painted the coverage reading {wanted:?}: {words:?}"
        );
    }

    /// DEFECT: A ROLLED LOG DESCRIBED BY THE COUNTS OF THE FILE THAT IS GONE.
    ///
    /// # WHAT THIS TEST USED TO ASSERT, AND WHY THAT PREMISE IS DEAD
    ///
    /// The client rolls or truncates its log and `Ingest::tail` sees a file SMALLER than the
    /// cursor it was reading at. That used to reset the byte cursor and nothing else, so
    /// `fights`, `fights_unreadable` and `scanned_at` went on describing the file that no longer
    /// existed. This page's job is to say which read a number came from, so the test asserted
    /// that the stale count DATED ITSELF: it checked `fights().len()` was still the old number
    /// and that the page said `folded out of the last full read`.
    ///
    /// `Ingest::tail` NOW RESCANS ON A ROLL, and that is the better fix rather than a change this
    /// test has to be bent around. Forty lines up, in the same function, a DIFFERENT log file
    /// taking over is already answered with `self.rescan(); return 0;`, because the history in
    /// hand is about another file. A rotation is that same event with the same path. Dating a
    /// stale number is what you do when you cannot replace it; replacing it is better.
    ///
    /// SO THE OLD TEST WAS ASSERTING THAT THE INGEST DOES NOT RE-FOLD, in as many words: `the
    /// ingest re-folded after all, so this test is no longer about anything`. It was also RACING
    /// the rescan worker, which is worse than failing: it passed under a loaded machine and
    /// failed on an idle one, because `painted` pumps `tail` and a landed rescan drains there.
    ///
    /// # WHAT IS ASSERTED NOW
    ///
    /// The same defect, at the other end: after a roll, no count on this page describes the file
    /// that was replaced. It waits for the rescan to LAND rather than reading mid-flight, so it
    /// is deterministic in both directions.
    ///
    /// WHAT MUTATION MAKES THIS RED: taking the `self.rescan()` out of `Ingest::tail`'s roll
    /// branch and leaving only the cursor reset, which is the defect this replaces.
    #[test]
    fn a_rolled_log_does_not_hand_its_counts_to_the_file_that_replaced_it() {
        let dir = crate::fights::probe::planted("rolled-logs", crate::fights::probe::CAPTURE);
        let mut ing = crate::fights::probe::booted(&dir);
        let before = ing.fights().len();
        assert!(
            before > 0,
            "the capture folded no fight, so a stale count could not be told from a fresh one"
        );
        let path = ing.active_log().expect("a log is being read").path.clone();

        /* THE ROLL. Same path, far fewer bytes: `tail` sees size < offset and starts again. */
        std::fs::write(&path, LINE).expect("roll the log");
        assert!(
            pump_until(&mut ing, |ig| ig
                .active_log()
                .is_some_and(|f| f.size == LINE.len() as u64)),
            "the ingest never noticed the log had been rolled"
        );
        assert_eq!(ing.tail_start(), Some(0), "the cursor did not restart");

        /* AND THE RE-FOLD LANDS BEFORE ANYTHING IS READ OFF IT.
         *
         * `rescan` hands the work to a thread and `adopt` installs the answer inside a later
         * `tail`. Reading between those two is reading a state the app passes through rather than
         * rests in, and asserting on it is what made the old test flaky: it raced the worker and
         * its verdict depended on how busy the machine was. */
        assert!(
            /* NON-CAPTURING: `pump_until` takes a fn pointer, so the predicate cannot close over
             * `before`. Empty is the right test anyway: the rolled file is one non-combat line. */
            pump_until(&mut ing, |ig| !ig.scanning() && ig.fights().is_empty()),
            "the roll never re-folded: the ingest still holds {before} fights out of a file that \
             was replaced"
        );
        assert_eq!(
            ing.fights().len(),
            0,
            "the re-fold of a file with one non-combat line in it produced fights"
        );

        /* AND THE PAGE SAYS NOTHING ABOUT THE FILE THAT IS GONE.
         *
         * COMPARED WHOLE AND NOT BY `contains`, which is a trap this page sets and which the
         * first draft of this assertion walked into: the capture folds FOUR fights, and `4` is a
         * substring of the temp folder's own name in the path this page prints twice. A test that
         * greps a painted page for a bare digit is a test that fails on the machine's process id.
         *
         * AND THE POSITIVE HALF MATTERS MORE THAN THE NEGATIVE ONE. `not printing a stale count`
         * is also what a page that printed nothing at all would do, so the two claims are made
         * together: the skipped section states the whole of the NEW file was read, and the
         * unreadable line dates itself to the read that just happened. */
        let mut s = Settings::default();
        let words = painted(&mut s, &mut ing, 1400.0);
        assert!(
            !words.iter().any(|w| *w == before.to_string()),
            "the page is still printing the replaced file's fight count of {before}: {words:?}"
        );
        assert!(
            says(&words, "whole file"),
            "the skipped section does not say the replacement was read whole: {words:?}"
        );
        assert!(
            says(&words, LAST_FULL_READ),
            "nothing on the page dates its counts to the read that replaced them: {words:?}"
        );
    }
}
