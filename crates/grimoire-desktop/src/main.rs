//! EQL Grimoire, as a native desktop application.
//!
//! WHY THIS IS NOT A TAURI SHELL, AND WHAT THAT BUYS.
//! There is no browser in this process. That is not a preference, it deletes a whole subsystem:
//! `web/grimoire.js` carries an endpoint LADDER because a page cannot know where the maths lives.
//! It tries `POST /engine` (the local agent, `grimoire serve`), then a wasm module, then gives up,
//! and every rung is a failure mode a user can land on. On a loopback host it tries `/engine`
//! FIRST, which is exactly why serving `web/` from a plain static server shows a page that loads,
//! renders, and then cannot price anything.
//!
//! A native binary has none of that. `grimoire_wasm::dispatch` is a function in this address space.
//! One rung, no transport, no 404, and no "engine is not built" state to design a screen for.
//!
//! AND IT IS THE SAME FUNCTION. Not a native reimplementation that agrees with the web build until
//! it quietly does not. `dispatch` is the one entry the wasm module wraps and `grimoire serve`
//! answers with, so this window computes what the shipped page computes, by construction. The
//! call itself lives in `screens::commission`; this file never touches the engine.
//!
//! WHAT THIS FILE OWNS. The App: one `Settings`, one snapshot (loaded on a thread of its own, with
//! the rail saying WORKING while it parses), one `Watcher`, one `Ingest`, the tool-window registry,
//! the global hotkeys, the find box, and exactly one instance of every screen. Every module it
//! names was written against the module contract fixed by decision D9; this is the one
//! place they meet, and the order of the frame below is the order the modules asked for.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use egui::{Align, FontId, Layout, RichText, Stroke, Vec2, ViewportCommand};
use grimoire_desktop::chat;
use grimoire_desktop::chrome::{self, nav_row, section, State};
use grimoire_desktop::data::{self, DataError, Hit, HitKind, Snapshot};
use grimoire_desktop::hotkeys::{self, Hotkeys};
use grimoire_desktop::ingest::Ingest;
use grimoire_desktop::nav::{self, ScreenId};
use grimoire_desktop::persona::{self, Persona, PersonaAction, PersonaFooter};
use grimoire_desktop::player;
use grimoire_desktop::screens::commission::CommissionScreen;
use grimoire_desktop::screens::exalt::ExaltScreen;
use grimoire_desktop::screens::gear::GearScreen;
use grimoire_desktop::screens::inventory::InventoryScreen;
use grimoire_desktop::screens::items::ItemsScreen;
use grimoire_desktop::screens::lfg::LfgScreen;
use grimoire_desktop::screens::parser::ParserScreen;
use grimoire_desktop::screens::quests::QuestsScreen;
use grimoire_desktop::screens::sky::SkyScreen;
use grimoire_desktop::screens::spells::SpellsScreen;
use grimoire_desktop::screens::unlocks::UnlocksScreen;
use grimoire_desktop::screens::valet::ValetScreen;
use grimoire_desktop::screens::watch::WatchScreen;
use grimoire_desktop::screens::zones::ZonesScreen;
use grimoire_desktop::screens::{self, Ask, Cx};
use grimoire_desktop::settings::{Settings, SettingsScreen};
use grimoire_desktop::theme::*;
use grimoire_desktop::titlebar;
use grimoire_desktop::twitch_auth::{self, AuthView};
use grimoire_desktop::watcher::{Status, Watcher};
use grimoire_desktop::windows::{self, LfgMode, Tool, Windows};
use grimoire_desktop::ytchat;
use grimoire_desktop::{fonts, theme};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// Set this to a number of milliseconds and the app closes itself after that long, printing
/// `smoke: N frames` to stdout first. A launch that exits 0 with N above zero has built every
/// panel, registered the hotkeys, started the watcher and the ingest, kicked off the snapshot
/// load, and drawn N frames without a panic. That is the "the binary launches" proof a script can
/// check without a screen. Nothing else reads this variable. Only the debug binary has a console
/// to print to; the release build is a windows-subsystem executable with no stdout.
const SMOKE_ENV: &str = "GRIMOIRE_SMOKE_MS";

fn main() -> eframe::Result<()> {
    /* RUST_LOG=info shows the watcher's polls and the snapshot's load time; the modules log at
     * warn for anything that failed and info for anything that changed. */
    env_logger::init();

    /* THE TRAMPOLINE, BEFORE A WINDOW EXISTS AND BEFORE ANYTHING ELSE THIS PROCESS SETS UP.
     *
     * ONE DELIBERATE DEVIATION FROM THE SPEC, WHICH PUTS THIS LINE ABOVE `env_logger::init()`.
     * Every failure inside `trampoline` is reported with `log::warn!`, and a `log` call made
     * before the logger is installed is dropped on the floor: the release binary is a
     * windows-subsystem executable with no stdout, so `eprintln!` would reach nobody either. An
     * app that quietly declines to bounce and never says why is the hardest of these failures to
     * diagnose, and `env_logger::init()` costs microseconds. Nothing else has been built yet. */
    let smoke = smoke_from_env();

    /* A PREFLIGHT NEVER TRAMPOLINES, AND THIS IS NOT AN OPTIMISATION.
     *
     * # THE DEFECT, WHICH THIS FILE HAD FOR AS LONG AS THE PREFLIGHT HAD A CALLER
     *
     * `install::Spawn` runs the STAGED executable with `GRIMOIRE_SMOKE_MS` set to ask one
     * question: does this new binary start on this machine. A staged payload lives under
     * `update\staging\`, which is not inside `app\`, so `plan`'s payload-position guard does not
     * hold it and it goes on to consult `current.json` like any other launch. Two things then go
     * wrong and the second is serious.
     *
     *   * IT MAY SMOKE THE WRONG BINARY. Whenever the installed version is higher than the staged
     *     one, which is exactly what a publisher rollback is, `plan` answers `Exec` and the
     *     preflight measures the binary that is already installed. It exits 0, the preflight
     *     passes, and it has proved nothing about the payload about to be installed.
     *   * IT COUNTS A LAUNCH IT CANNOT CLEAR. `trampoline` writes `launches_failed += 1` before
     *     every `spawn`, and the half that clears it is in `App::ui`, guarded by
     *     `self.smoke.is_none()` because a preflight must not write its parent's pointer. So each
     *     preflight leaves the count one higher with nothing to bring it down, and
     *     `FAILED_LAUNCHES_BEFORE_ROLLBACK` is two. Two installs on one machine would have rolled
     *     back a build that was working perfectly.
     *
     * # WHY THE ANSWER IS TO SKIP IT RATHER THAN TO TEACH `plan` ABOUT STAGING
     *
     * A preflight asks whether THIS executable starts. There is no version of that question whose
     * right answer is to run a different executable, so there is nothing for `plan` to get right
     * here and no argument to hand it. `install::Spawn`'s own doc already states the neighbouring
     * half of this rule, that `App::new` must construct no `Updater` under the smoke switch
     * because a preflight must not race its own parent over `state.json`. `current.json` is the
     * same parent's file and the same rule reaches it; the doc simply stopped one file short. */
    if smoke.is_none() {
        trampoline();
    }

    /* The channel artwork may make requests, which it may not do in a test binary: see
     * `channel_art::allow_fetching`. This is the whole of that switch and it has no other caller.
     * It still fetches nothing until the Watch screen is looked at with the channel off. */
    grimoire_desktop::channel_art::allow_fetching();
    /* THE UPDATER MAY DIAL, FOR THE SAME REASON AND BY THE SAME MEANS. `updates.ragnarok.systems`
     * is this project's own bucket rather than a host that owes it nothing, but the rule is the
     * same one: `cargo test --workspace` must not touch the network (`tests/live_twitch.rs:1-23`
     * is the stated design), and several tests in this file build a whole `App`. One production
     * flag on one line of `main` is what `channel_art` chose over a `cfg(test)`, and its comment
     * says why: a `cfg(test)` changes what the compiled-for-test code DOES, which hides defects in
     * exactly the path nobody runs twice. */
    grimoire_desktop::updater::run::allow_updating();

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            /* THE SIZE FOR THE FIRST FRAME ONLY. The real one is a quadrant of the work area,
             * which is not known until a frame has run: `App::fit_to_quadrant` asks for it. */
            .with_inner_size(chrome::FALLBACK)
            .with_min_inner_size(chrome::HARD_FLOOR)
            .with_title("EQL Grimoire")
            /* D3 and D8: the app draws its own title strip (`titlebar::strip`) with the live pill
             * and the pin glyph, so the OS frame is off. The strip supplies drag, double click to
             * maximise, minimise and close; the grip in the bottom right corner supplies resize. */
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "EQL Grimoire",
        opts,
        Box::new(move |cc| {
            fonts::install(&cc.egui_ctx);
            theme::install(&cc.egui_ctx);
            Ok(Box::new(App::new(smoke, &cc.egui_ctx)))
        }),
    )
}

/// WHICH EXECUTABLE THIS PROCESS SHOULD ACTUALLY BE.
///
/// Returns when the answer is "this one", which is every launch of a build nobody has updated and
/// EVERY launch of a development build. On `Exec` it starts the managed payload, passes this
/// process's arguments along, and exits 0 without ever opening a window.
///
/// # THE RULE IS NOT HERE, IT IS IN `updater::launch::plan`
///
/// This function is the plumbing: `current_exe`, a spawn, an exit. Everything that DECIDES lives
/// in a pure function with its own tests, because `main` cannot be called from one and a rule
/// that chooses which binary runs is the last rule in this crate that should be reachable only by
/// running it.
///
/// # EVERY FAILURE HERE RUNS THIS BUILD, AND SAYS SO
///
/// There is no error path that stops the app starting. A missing local data directory, an
/// unreadable `current_exe`, a version this binary cannot parse, a spawn that fails: each one
/// falls through to running the executable that is already loaded, which is always a working app.
fn trampoline() {
    use grimoire_desktop::updater::{install, launch};

    let Some(layout) = install::Layout::platform() else {
        return;
    };
    let Ok(own) = std::env::current_exe() else {
        return;
    };
    let Ok(me) = semver::Version::parse(titlebar::version()) else {
        return;
    };
    let app_dir = layout.app_dir();

    /* THE WITHDRAWN SET, OUT OF THE NEWEST MANIFEST THIS CLIENT ACCEPTED. It was an empty slice
     * while nothing wrote it; `updater::run::Worker::check` writes it into `update\state.json`
     * now, so the trampoline reads it and a release that bricks can be withdrawn from machines
     * that already took it.
     *
     * AN UNPARSABLE ENTRY IS DROPPED AND NOT AN ERROR. `plan` compares versions, so a string that
     * is not one cannot match anything; refusing to start over a bad row in a bookkeeping file
     * would be the app declining to run because of a file that has no say in whether it can. */
    let yanked: Vec<semver::Version> = grimoire_desktop::updater::run::read_state(&layout)
        .yanked
        .iter()
        .filter_map(|v| semver::Version::parse(v).ok())
        .collect();

    let mut current = install::read_current(&layout);
    let mut plan = launch::plan(
        &own,
        &app_dir,
        current.as_ref(),
        &me,
        &yanked,
        cfg!(debug_assertions),
        grimoire_desktop::updater::verify::KEYS,
    );

    if let launch::Plan::Rollback { to, why } = &plan {
        log::warn!("going back to {to}: {why}");
        match install::roll_back(&layout) {
            Ok(back) => {
                /* THE VERSION JUST FLED IS RECORDED, AND UNTIL NOW IT WAS THROWN AWAY.
                 *
                 * `roll_back` returns `away_from` and its own doc says what it is for: so the
                 * caller can write it into `state.json` and decline to offer it again. This line
                 * was `current = Some(back.now)` and nothing else, and `grep away_from` found one
                 * production write and no production read. The loop that left: a build that passes
                 * the preflight and cannot draw on real launches is rolled back at the third
                 * launch, re-offered by the next check, re-downloaded (10.8 MB), re-installed, and
                 * gives the reader two dead launches and another rollback the session after, for
                 * ever.
                 *
                 * A FAILURE TO RECORD IT IS NOT A FAILURE TO START. The app is already going back;
                 * the worst case of an unwritten note is the loop above, which is what the app did
                 * before this existed. */
                if let Err(e) =
                    grimoire_desktop::updater::run::note_rolled_back(&layout, &back.away_from)
                {
                    log::warn!("could not record the rollback away from {}: {e}", back.away_from);
                }
                current = Some(back.now);
            }
            Err(e) => {
                log::warn!("could not go back: {e}; running this build instead");
                return;
            }
        }
        /* ASKED AGAIN EXACTLY ONCE AND NEVER IN A LOOP. The pointer has moved, so the answer can
         * be different; if it is still `Rollback` then the version just rolled back to is also
         * failing, and the right thing to do is run the entry point rather than walk backwards
         * through every version ever installed. */
        plan = launch::plan(
            &own,
            &app_dir,
            current.as_ref(),
            &me,
            &yanked,
            cfg!(debug_assertions),
            grimoire_desktop::updater::verify::KEYS,
        );
        if matches!(plan, launch::Plan::Rollback { .. }) {
            return;
        }
    }

    let launch::Plan::Exec(path) = plan else {
        return;
    };

    /* COUNTED BEFORE THE SPAWN AND CLEARED BY THE CHILD'S FIRST COMPLETED FRAME. A launch that
     * starts and never draws is what this count is for, so it has to be written before the thing
     * that might not draw. See `App::ui` for the clearing half. */
    if let Err(e) = install::note_launch_started(&layout) {
        log::warn!("could not record the launch: {e}");
    }
    /* THE PAYLOAD IS TOLD WHERE THE ENTRY POINT IS, and this is the only place that can tell it.
     * A managed payload's own `current_exe()` is inside the managed directory, which `plan`
     * answers `RunHere` for, so a payload asked to restart itself after an update would relaunch
     * the version that was just replaced and keep doing so forever. See `launch::ENTRY_ENV`. */
    match std::process::Command::new(&path)
        .env(launch::ENTRY_ENV, &own)
        .args(std::env::args_os().skip(1))
        .spawn()
    {
        Ok(_) => std::process::exit(0),
        Err(e) => log::warn!(
            "could not start {}: {e}; running this build instead",
            path.display()
        ),
    }
}

fn smoke_from_env() -> Option<Duration> {
    parse_smoke(std::env::var(SMOKE_ENV).ok().as_deref())
}

/// The switch's rule, with the environment lifted out so a test can drive every case. Reading the
/// variable is one line above; deciding what it means is this, and only this was ever worth a test.
fn parse_smoke(raw: Option<&str>) -> Option<Duration> {
    let raw = raw?;
    match raw.trim().parse::<u64>() {
        Ok(ms) => Some(Duration::from_millis(ms)),
        Err(e) => {
            /* Not a panic and not a silent no-op: say so on stderr, where the script that set it
             * is looking, and run normally. */
            eprintln!("{SMOKE_ENV}={raw:?} is not a number of milliseconds ({e}); ignoring it");
            None
        }
    }
}

/* ------------------------------------------------------------------- the snapshot -- */

/// The snapshot, in the four states it can be in. Decision D6 says 21MB is not `include_bytes!`;
/// it is also not a frame, so the parse runs on its own thread and the rail draws that state as
/// WORKING rather than as absent or failed, because it is neither.
enum Data {
    /// No candidate directory held gear-data.json. The words list every directory tried.
    Absent(String),
    Loading {
        root: PathBuf,
        rx: Receiver<Result<Snapshot, DataError>>,
        since: Instant,
    },
    /// Boxed: the loaded snapshot is several hundred bytes of vectors and indexes, and clippy
    /// rightly objects to an enum that pads every variant to that.
    Loaded(Box<Snapshot>),
    /// A root was found and a file in it could not be read or parsed; the words name the file.
    Failed(String),
}

impl Data {
    /// Start a load for the root the settings name (or `Snapshot::locate` finds). Returns at once.
    fn start(settings: &Settings) -> Data {
        let Some(root) = settings.effective_data_root() else {
            return Data::Absent(no_data_words(settings));
        };
        let (tx, rx) = mpsc::channel();
        let path = root.clone();
        let spawned = std::thread::Builder::new()
            .name("grimoire-snapshot".to_owned())
            .spawn(move || {
                let _ = tx.send(Snapshot::load(&path));
            });
        match spawned {
            Ok(_) => Data::Loading {
                root,
                rx,
                since: Instant::now(),
            },
            Err(e) => Data::Failed(format!("could not start the snapshot loader thread: {e}")),
        }
    }

    /// Collect a finished load. Never blocks.
    fn poll(&mut self) {
        let next = match self {
            Data::Loading { rx, root, since } => match rx.try_recv() {
                Ok(Ok(snapshot)) => {
                    let r = snapshot.report();
                    log::info!(
                        "snapshot: {} items, {} zones, {} drops, {} sky items, {} quests from {} in {:?}",
                        r.items,
                        r.zones,
                        r.drops,
                        r.sky_items,
                        r.quests,
                        root.display(),
                        since.elapsed()
                    );
                    Some(Data::Loaded(Box::new(snapshot)))
                }
                Ok(Err(e)) => Some(Data::Failed(format!(
                    "the snapshot at {} could not be read. {e}",
                    root.display()
                ))),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Data::Failed(format!(
                    "the snapshot loader thread for {} ended without answering",
                    root.display()
                ))),
            },
            _ => None,
        };
        if let Some(n) = next {
            *self = n;
        }
    }

    fn snapshot(&self) -> Option<&Snapshot> {
        match self {
            Data::Loaded(s) => Some(s),
            _ => None,
        }
    }

    /// The words for `Cx::data_err`: only an absent or failed snapshot is an error. While it loads
    /// the screens see neither data nor error; the App draws the loading notice instead of ANY
    /// screen that reads the snapshot, snapshot rows and character rows alike (see `reads_data`).
    fn err(&self) -> Option<&str> {
        match self {
            Data::Absent(w) | Data::Failed(w) => Some(w),
            _ => None,
        }
    }

    fn loading(&self) -> Option<(&std::path::Path, Duration)> {
        match self {
            Data::Loading { root, since, .. } => Some((root, since.elapsed())),
            _ => None,
        }
    }

    /// The footer's one line about the data. Counts only once they were counted.
    fn footer_line(&self) -> String {
        match self {
            /* THE HEADING IT NAMES HAS TO BE ONE THE RAIL HAS. `FIND` was one of seven sections
             * in the D5 rail and that rail is gone; a reader following it opens the app and
             * finds no such heading. Settings is where a snapshot is pointed at, and it is
             * reachable from the persona footer two lines below this one. */
            Data::Absent(_) => "snapshot: absent, see Settings".to_owned(),
            Data::Loading { since, .. } => {
                format!("snapshot: loading {:.1}s", since.elapsed().as_secs_f32())
            }
            Data::Loaded(s) => {
                let r = s.report();
                format!(
                    "snapshot: {} items · {} zones · {} drops · {} sky · {} quests",
                    r.items, r.zones, r.drops, r.sky_items, r.quests
                )
            }
            /* SETTINGS AND NOT `FIND`, for the reason the Absent arm above says: there is no
             * FIND heading in this rail and has not been for some time. Both arms were written
             * together and only one of them was corrected. */
            Data::Failed(_) => "snapshot: failed, see Settings".to_owned(),
        }
    }
}

/// What the FIND screens print when there is nothing to read: every directory that was tried, in
/// the order it was tried, and what to put there.
fn no_data_words(settings: &Settings) -> String {
    let mut tried: Vec<String> = Vec::new();
    if let Some(p) = &settings.data_root {
        tried.push(format!("{} (from Settings)", p.display()));
    }
    tried.extend(data::candidates().iter().map(|p| p.display().to_string()));
    format!(
        "No snapshot found. Looked for {} in: {}. Put the data folder ({}, and the {}/ directory) at \
         the first of those, or name its path on the Settings screen.",
        data::GEAR_FILE,
        tried.join("; "),
        data::FILES.join(", "),
        data::ATLAS_DIR
    )
}

/// The rows whose screens read `Cx::data`, so a load in progress is shown as the loading notice
/// on every one of them and never as "no snapshot loaded, put gear-data.json in data/" while the
/// file is being parsed. The rail's square is a separate question (`nav::square`): a CHARACTER
/// row's square follows the dump, but its BODY still wants the item records.
fn reads_data(id: ScreenId) -> bool {
    /* SPELLS WAS MISSING, AND IT IS THE MOST EMPHATIC PAGE IN THE APP ABOUT NOT HAVING DATA.
     *
     * `SpellsScreen::ui` falls through to `items::no_data` when `cx.data` is `None`, and
     * `Data::err` answers `None` while the snapshot is Loading, so `no_data` takes its
     * there-was-no-error branch: NO SPELL DATA LOADED in gold display type, over a bar reading
     * "No snapshot is loaded and the loader reported no error. Nothing has been read yet.",
     * over the list of places to go and put gear-data.json. All of it while the footer of the
     * same window prints `snapshot: loading 0.4s` and the file is being parsed on a worker.
     *
     * THAT IS A PAGE TELLING A READER HIS INSTALL IS BROKEN WHILE IT WORKS. This list is the
     * gate that swaps the body for `screens::loading_notice` until the load lands, and a screen
     * missing from it gets its own empty state in the meantime. */
    matches!(
        id,
        ScreenId::Items
            | ScreenId::Zones
            | ScreenId::Quests
            | ScreenId::Spells
            | ScreenId::Inventory
            | ScreenId::Gear
            | ScreenId::Exalt
            | ScreenId::Sky
    )
}

/* ------------------------------------------------------------------------ the App -- */

/// What the central panel shows. Settings is not a D5 row (the nav test holds NAV to exactly the
/// 29 screens), so it is a body of its own.
///
/// ONE DOOR, AND IT IS THE PERSON. The persona block at the foot of the rail is the web build's
/// `.pgfoot` and the way in that Discord, Slack and VS Code all landed on: the person is the last
/// thing in the rail and the person is the way into settings.
///
/// THERE USED TO BE A SETTINGS ROW ABOVE IT AND THIS IS WHY IT IS GONE. The rail ended with the
/// sections, then a hairline, then a row labelled "Settings", and then the persona block, whose
/// name AND whose gear both opened the same screen: three doors to one place, stacked. The row was
/// argued for on one ground, that it carried `settings_state` and the footer could not, so the
/// argument was answered rather than overruled: the state moved onto the footer's gear, which is
/// the control that still opens the screen (`persona::gear_ink`). The row's own state mark was in
/// any case the weaker of the two, because `chrome::nav_row` paints a trailing bar for
/// `State::You` and NOTHING AT ALL for `State::Wrong`, so a settings file that would not load was
/// reported by that row in no visible way whatsoever.
///
/// `settings_state` stays, and it is now read at exactly one site: the App fills
/// `Persona::settings` with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Body {
    Screen(ScreenId),
    Settings,
}

/// One instance of every screen. The App draws exactly one per frame; the rest keep their state
/// (a selection, a filter, a walk in progress) between visits, which is why they are owned here
/// and not built on entry.
#[derive(Default)]
struct Screens {
    chat: screens::chat::ChatScreen,
    commission: CommissionScreen,
    parser: ParserScreen,
    /// The fight in progress. See [`screens::live`].
    live: crate::screens::live::LiveScreen,
    /// A session rolled up across fights. See `screens::reports`.
    reports: crate::screens::reports::ReportsScreen,
    /// The landing page, by role. See `screens::dashboards`.
    dashboards: crate::screens::dashboards::DashboardsScreen,
    /// Which log is being read, and what was skipped. See `screens::logs`.
    logs: crate::screens::logs::LogsScreen,
    items: ItemsScreen,
    zones: ZonesScreen,
    quests: QuestsScreen,
    inventory: InventoryScreen,
    gear: GearScreen,
    valet: ValetScreen,
    exalt: ExaltScreen,
    sky: SkyScreen,
    unlocks: UnlocksScreen,
    lfg: LfgScreen,
    watch: WatchScreen,
    spells: SpellsScreen,
}

struct Smoke {
    after: Duration,
    since: Instant,
    fired: bool,
}

/* THERE IS NO CAP ON HOW MANY SECTIONS MAY STAND OPEN, AND THERE USED TO BE.
 *
 * D5 capped it at two, evicting the least recently opened to make room for a third. The stated
 * reason was that seven sections fully expanded is 29 rows, which scrolls, turning the rail
 * from a map you read into a list you hunt through.
 *
 * That reason does not survive contact with the code. The rail is ALREADY inside a
 * `ScrollArea::vertical()` bounded by `persona::room_above`, so 29 rows scrolls correctly and
 * always did. The cap was not preventing a broken layout; it was preventing the reader from
 * seeing what they asked to see. And it failed silently in the worst way: you open a third
 * section and a section you opened earlier, somewhere else on the rail, quietly shuts. The app
 * takes something away every time you ask for something.
 *
 * Seven is not many. A person who opens all of them has decided they want all of them, and the
 * rail is theirs to arrange. The scroll bar is the answer to a long list, not a rule about how
 * much of your own navigation you are allowed to look at. */

/// The find, D6: one search over every list with the `i:` `z:` `d:` `q:` prefixes. Its hits come
/// from `Snapshot::search`; picking one routes to the FIND screen that owns the record, exactly as
/// a cross-screen link does.
///
/// # IT WAS A BOX ON THE CONTEXT HEADER AND IT IS A PALETTE NOW
///
/// A 240 pixel text field sat on the right of the context bar on every screen in the app, taking
/// header room from the page under it and offering a search most of those pages have their own
/// box for. The owner asked for it off that row.
///
/// OFF THE ROW IS NOT DELETED, AND THE DIFFERENCE MATTERS. `data::search`, `data::parse_query`,
/// `HitKind` and `SEARCH_CAP` are a real capability with their own tests; removing their last
/// caller to tidy a header would leave a tested library function nothing in the app can reach,
/// which is this tree's signature defect arriving through the front door. So the same search is
/// the same function, drawn over the page instead of in the header, on Ctrl+K and off on Escape.
#[derive(Default)]
struct Finder {
    query: String,
    /// IS THE PALETTE UP? False until Ctrl+K, and false again on Escape or on a pick.
    ///
    /// A FLAG AND NOT `!query.is_empty()`, because an empty palette is a real state: it is what
    /// the reader sees for as long as it takes him to type the first letter, and a palette that
    /// vanished when he cleared the box would be a control that closes under the cursor.
    open: bool,
}

/// How many hits the dropdown shows. `data::SEARCH_CAP` bounds the search; a dropdown is read,
/// not scrolled, so it shows the first few and says how many more there are.
const FINDER_ROWS: usize = 12;

struct App {
    /// Which sections stand open in the wide rail, in the order they were opened.
    ///
    /// A SET AND NOT ONE SELECTION, AND THAT IS THE CORRECTION. A pass at this made the wide rail
    /// show one realm at a time, on the reading that a narrow rail beside it turned the wide one
    /// into a detail view. The owner wanted the rail he had AND another one to its left, which is
    /// not the same request: the wide rail still lists every realm and folds them, and the narrow
    /// rail is a way INTO one rather than a filter over the other.
    open: Vec<usize>,

    body: Body,
    /// The context bar's tab per screen, indexed by `ScreenId::ordinal`, so a row with two faces
    /// (Gear, Keys) comes back on the face it was left on rather than resetting on every visit.
    tabs: [usize; ScreenId::ALL.len()],
    /// TIER 3: which of a destination's sections is showing, one slot per destination.
    ///
    /// PER DESTINATION AND NOT ONE GLOBAL SLOT, which is the sitemap's rule 10 and also the
    /// only shape that can be right: leaving Log Parser on Fights and coming back to it later
    /// has to land on Fights, and a single slot would carry that choice onto Plane of Sky.
    section: [usize; ScreenId::ALL.len()],
    settings: Settings,
    settings_screen: SettingsScreen,
    data: Data,
    /// Dropping it stops the poll thread, so it lives as long as the App.
    watcher: Watcher,
    /// THE SELF-UPDATER: one per process, beside the watcher and for the same reasons.
    ///
    /// ONE, AND NOT ONE PER WINDOW. Each pop-out tool window builds its own `Ingest`
    /// (`windows.rs:729`), which is why `TrackerSettings` had to move onto `Settings`; an updater
    /// following that shape would have several windows fetching the same ten megabytes at once.
    /// Dropping it asks its thread to stop.
    ///
    /// `None` IS A REAL STATE AND NOT A FAILURE. A preflight run constructs none on purpose (see
    /// `App::new`), and neither does a platform with no local data directory. The Settings screen
    /// says so in words rather than drawing an empty section.
    updater: Option<grimoire_desktop::updater::run::Updater>,
    /// THE READER PRESSED RESTART NOW, AND THE WINDOW HAS BEEN ASKED TO CLOSE.
    ///
    /// A LATCH READ IN `on_exit` AND NOT A `spawn` AT THE PRESS. Starting the entry point while
    /// this process still holds the window, the global hotkeys and the settings file would put two
    /// instances over each other for as long as the shutdown takes, and the second one would
    /// register its hotkeys against a set this one has not released yet. `on_exit` already runs
    /// after the last frame and is where the settings flush lives, so the handover happens once,
    /// in order, with everything this process owns already given up.
    restart_at_exit: bool,
    /// The watcher's status, cloned once per frame so every panel this frame agrees.
    live: Status,
    ingest: Ingest,
    /// The embedded player surface, and the process's one `wry::WebContext`.
    ///
    /// OWNED BY THE APP AND BY NOTHING ELSE, for two reasons that both bite. The context reads its
    /// profile folder inside `build_as_child` and cannot be repointed afterwards, so it has to
    /// exist before any webview and outlive every one of them; and the surface attaches to the ROOT
    /// window's handle, which arrives as the `&mut eframe::Frame` only this function is handed. A
    /// screen states where the video goes (`Cx::stage`) and this is what puts it there.
    player: player::Player,
    windows: Windows,
    /// Dropping it unregisters the globals.
    hotkeys: Hotkeys,
    /// The Twitch sign-in. Idle until the Chat screen asks; holds the token and hands out no
    /// copy of it. Dropping it stops the thread. See `twitch_auth::Auth`.
    auth: twitch_auth::Auth,
    /// Whether the saved sign-in has been looked for yet. Once per process; see `heartbeat`.
    restored: bool,

    /// The YouTube half of the merged feed: a hidden browser pane running the live chat page.
    ///
    /// OWNED HERE FOR THE SAME REASON THE PLAYER IS. It holds a `wry::WebContext` whose profile
    /// folder is read inside the build and cannot be repointed afterwards, and its pane is a child
    /// of the ROOT window, whose handle arrives as the `&mut eframe::Frame` only `App::ui` is
    /// given. A screen states that it wants the feed (`Cx::yt_wanted`) and this acts.
    ///
    /// IDLE COSTS NOTHING. No window, no browser process, and no folder on disk until the first
    /// `show`; a launch that never opens Chat leaves nothing behind.
    ytchat: ytchat::surface::YtChat,

    /// The Twitch chat reader. Idle until the Chat screen asks, and dropping it stops the thread.
    ///
    /// OWNED BY THE APP AND NOT BY THE SCREEN, for the reason `screens::chat` states at length:
    /// `Screens` derives `Default` and is built whole at startup for every user, so a reader held
    /// by the screen would be constructed on every launch by people who never open chat. Here it
    /// is constructed too, but `ChatReader::idle()` opens nothing at all; `start` is the only door
    /// to the network and `Cx::chat_wanted` is the only thing that opens it.
    chat: chat::ChatReader,
    screens: Screens,
    /// The persona block pinned to the foot of the rail. It holds no state today (nothing in this
    /// build raises the alert whose ring is the original's one animation) and is owned here anyway,
    /// so the day one of its fields gets a source there is somewhere for the state to live.
    persona: PersonaFooter,
    finder: Finder,
    /// HAS THE WINDOW BEEN PUT AT ITS QUADRANT YET? See [`App::fit_to_quadrant`].
    ///
    /// A LATCH AND NOT A COUNTER, and it is what makes the quadrant a DEFAULT rather than a policy:
    /// without it the window would be dragged back out of wherever the owner had just moved it, on
    /// every frame, for the life of the run.
    sized: bool,
    frames: u64,
    /// HOW MANY TIMES THIS APP HAS COMPLETED A PASS, DRAWN OR NOT.
    ///
    /// # WHY IT IS NOT `frames`
    ///
    /// `frames` counts `ui`, which eframe calls only while the viewport is visible. The
    /// failed-launch counter that drives the automatic rollback was incremented on every launch and
    /// CLEARED on `frames == 2`, so two launches that never became visible left it at two on a
    /// build that was working perfectly and the third launch silently downgraded the reader. The
    /// app minimizes ITSELF on the Ctrl+Alt+G toggle, and Windows can open the window behind a
    /// fullscreen-exclusive game and never composite it, so both of those are ordinary Tuesday
    /// rather than a corner.
    ///
    /// This is incremented in `heartbeat`, which BOTH callbacks reach, so "the app has completed a
    /// pass" means the same thing whether or not anything was painted. It still does not clear on
    /// the first pass, for the reason the count exists: a glow or wgpu failure happens during the
    /// first drawn frame, and clearing before it would call a crash a success.
    passes: u64,
    smoke: Option<Smoke>,
}

impl App {
    /// `ctx` IS TAKEN AND NOT REACHED FOR. The updater's worker wakes the UI with
    /// `request_repaint_of(ViewportId::ROOT)` from a thread inside no viewport callback
    /// (`chat.rs:1077`), so it needs a `Context` at construction, and the one eframe hands its
    /// creator is the only one this process should ever hold.
    fn new(smoke: Option<Duration>, ctx: &egui::Context) -> App {
        let settings = Settings::load();
        /* THE DATA SELF-HEAL, BEFORE THE SNAPSHOT IS LOOKED FOR AND NOT AFTER.
         *
         * `install::restore_previous_data` puts a snapshot back by renaming three directories past
         * each other, and there is an instant in the middle of that in which `data\` does not
         * exist. `install::swap_data` has the same instant and `install::swap_data`'s own comment
         * argues at length that the instant is ACCEPTABLE, on the grounds that a missing snapshot
         * is `Data::Absent`, which is a designed state, AND that it self-heals. This is the half
         * that makes the second half of that argument true, so wiring the button that can leave
         * the window open without wiring this would be taking the argument and not the thing it
         * rests on.
         *
         * BEFORE `Data::start` BECAUSE `Data::start` IS WHAT LOOKS. Run after it, the heal would
         * put the directory back a fraction of a second too late and the reader would be told
         * their snapshot is missing while it sits on disk beside them. */
        if let Some(l) = grimoire_desktop::updater::install::Layout::platform() {
            match grimoire_desktop::updater::install::heal_missing_data(&l) {
                Ok(true) => log::info!(
                    "the installed data folder was missing and the previous generation was put \
                     back at {}",
                    l.data_dir().display()
                ),
                Ok(false) => { /* the ordinary case: nothing to heal */ }
                Err(e) => log::warn!("could not put the previous data back: {e}"),
            }
        }
        let data = Data::start(&settings);
        let watcher = Watcher::start();
        let live = watcher.status();
        let mut ingest = Ingest::new(&settings);
        /* On the main thread, inside eframe's creator: RegisterHotKey delivers to the thread that
         * pumps window messages, and this is it. D4: the overrides Settings carries are applied
         * here, so a rebound chord is the one registered from the first frame. */
        let hotkeys = Hotkeys::install(&settings.hotkeys);
        /* COPIED BEFORE `settings` IS MOVED INTO THE STRUCT BELOW. Three small fields, and cloning
         * them is cheaper than reordering a constructor that reads `settings` a dozen times. */
        let updater_settings = settings.updater.clone();
        let mut settings_screen = SettingsScreen::default();
        settings_screen.set_bindings(hotkeys.bindings());
        let mut screens = Screens::default();
        /* The corpus read starts now, not on the first visit to the Tradeskill Hall, so the rail's
         * square for that row is true from the first frame (`nav::Facts::corpus`). */
        screens.commission.start();
        /* THE ONE PLACE THE FIGHT STORE IS OPENED. `Ingest` never reaches for it itself: see
         * `Ingest::use_store` for the accident that rule prevents. */
        ingest.use_store(grimoire_desktop::store::Store::app_data());
        let mut me = App {
            open: nav::default_open(),

            /* THE PARSER IS WHAT YOU WANT WITH THE GAME RUNNING. */
            body: Body::Screen(ScreenId::Parser),
            tabs: [0; ScreenId::ALL.len()],
            /* EVERY DESTINATION STARTS ON THE SECTION IT CAN DRAW, not on index 0. See
             * `nav::default_section`. THIS COMMENT USED TO EXPLAIN THE RULE WITH A PARSER WHOSE
             * FIRST SECTION WAS LIVE AND WHOSE LIVE HAD NO SCREEN; both halves have moved.
             * Dashboards leads that list now and Live is routed, so for the parser the function
             * returns 0 and this rule changes nothing. It still fires for every destination
             * whose first section is a screen nobody has written, and the original case was
             * built, on the first click, every time. */
            section: {
                let mut on = [0usize; ScreenId::ALL.len()];
                for id in ScreenId::ALL {
                    on[id.ordinal()] = nav::default_section(id);
                }
                on
            },
            settings,
            settings_screen,
            data,
            watcher,
            /* NOT CONSTRUCTED IN A PREFLIGHT, AND `install::Spawn` RECORDS WHY WHERE IT BITES. A
             * preflight is a second process that the installer started, with `GRIMOIRE_SMOKE_MS`
             * set, to ask whether the NEW binary starts on this machine. An updater inside it
             * would poll the network and write `update\state.json` while its own parent was
             * about to, which is two writers on one file for no gain at all.
             *
             * NOT CONSTRUCTED IN A TEST BINARY EITHER: `updating_allowed` is false until `main`
             * calls `allow_updating`, the same switch `channel_art` uses and for the same
             * stated reason. */
            updater: if smoke.is_some() {
                None
            } else {
                grimoire_desktop::updater::run::Updater::start(ctx, &updater_settings)
            },
            restart_at_exit: false,
            live,
            ingest,
            /* Before any webview, once per process. Building it costs a path and a mkdir, not a
             * browser: the webview itself is created on the first frame that asks for one, and
             * most launches never open the Watch screen. */
            player: player::Player::new(),
            windows: Windows::default(),
            hotkeys,
            chat: chat::ChatReader::idle(),
            ytchat: ytchat::surface::YtChat::idle(),
            auth: twitch_auth::Auth::default(),
            restored: false,
            screens,
            /* NOT `PersonaFooter::default()`: on a unit struct that is
             * `clippy::default_constructed_unit_structs`, which is deny here. */
            persona: PersonaFooter,
            finder: Finder::default(),
            sized: false,
            frames: 0,
            passes: 0,
            smoke: smoke.map(|after| Smoke {
                after,
                since: Instant::now(),
                fired: false,
            }),
        };

        /* THE FIRST SCREEN ARRIVES THE SAME WAY EVERY LATER ONE DOES.
         *
         * `on_view` is what puts a shared screen into the state its destination names, and it ran
         * only from `enter`, which is navigation. The body the app STARTS on never navigated, so
         * it never ran, and the parser screen sat on its own default view: Log Parser, whose whole
         * job is combat, opened on the kill tracker. The rail said Fights and the body drew kills,
         * which is the rail and the body disagreeing about where the reader is. */
        if let Body::Screen(id) = me.body {
            on_view(
                id,
                me.section[id.ordinal()],
                me.tabs[id.ordinal()],
                &mut me.screens,
            );
        }
        me
    }

    /// The smoke clock: print the frame count and close when the deadline passes, and keep the
    /// frame loop awake until it does so an idle window still reaches it.
    fn smoke(&mut self, ctx: &egui::Context) {
        let Some(s) = &mut self.smoke else { return };
        if s.fired {
            return;
        }
        let left = s.after.saturating_sub(s.since.elapsed());
        if left.is_zero() {
            s.fired = true;
            println!("smoke: {} frames", self.frames);
            ctx.send_viewport_cmd(ViewportCommand::Close);
        } else {
            ctx.request_repaint_after(left);
        }
    }

    /// Show a screen another screen (or the find box) asked for: open its section if it is
    /// folded, then enter it.
    fn reveal(&mut self, id: ScreenId) {
        /* OPEN THE SCREEN`S SECTION IF IT IS FOLDED, and leave every other section as it is. A
         * screen reached from a search or from another screen has to be visible in the rail when
         * it arrives; shutting the rest to get there would be the rail taking something away on
         * every jump. */
        if let Some((si, _)) = nav::find(id) {
            reveal_head(&mut self.open, si);
        }
        enter(
            Body::Screen(id),
            &mut self.body,
            &mut self.tabs,
            &mut self.section,
            &mut self.screens,
        );
    }

    /// Route what a screen asked for. The target screen already holds the selection in its own
    /// static; the App's job is the nav switch, and opening the section it sits in.
    fn answer(&mut self, ask: Ask) {
        match ask {
            /* A PLACE AND NOT A RECORD, which is the one thing the other asks could not say.
             * `screens::dashboards` raises it: every tile there is a window onto a larger page and
             * its `open` control is what jumps to it.
             *
             * THE SECTION IS RESOLVED BY NAME AGAINST `SECTIONS` and a name that names nothing
             * leaves the section where it was rather than landing on index 0, because opening the
             * WRONG page is worse than opening the right destination on the page you were last
             * on. `dashboards::every_tile_opens_a_section_that_exists` is what stops that arm
             * from ever being taken.
             *
             * AND THE TAB RESETS WITH THE SECTION, for the reason `pick_section` resets it: tier 4
             * belongs to the SECTION, so the slot left behind by the last one indexes a different
             * list entirely. */
            Ask::Open(id, section) => {
                if let Some(name) = section {
                    if let Some(i) = nav::sections_of(id).iter().position(|(n, _, _)| *n == name) {
                        self.section[id.ordinal()] = i;
                        self.tabs[id.ordinal()] = 0;
                    }
                }
                self.reveal(id);
            }
            Ask::ShowZone(name) => {
                screens::zones::jump_to(&name);
                self.reveal(ScreenId::Zones);
            }
            Ask::ShowItem(name) => {
                screens::items::jump_to(&name);
                self.reveal(ScreenId::Items);
            }
            Ask::ShowQuest(title) => {
                screens::quests::jump_to(&title);
                self.reveal(ScreenId::Quests);
            }
            Ask::ShowSpell(name) => {
                screens::spells::jump_to(&name);
                self.reveal(ScreenId::Spells);
            }
            Ask::CheckLive => self.watcher.refresh(),
            Ask::StopPlayer => self.player.stop(),
            /* THE PILL'S CLICK, ANSWERED. Enter the Watch screen (opening STOIC if it is folded,
             * which `reveal` does) and turn its playback on. Two steps and both are needed: the
             * screen without the flag would show its Watch here button and wait for a second
             * click, and the flag without the screen would start a stream in a body the reader is
             * not looking at. Whether anything plays is still decided on the next frame by
             * `feed_for`, so an offline channel lands on the screen that says so. */
            /* AND THE PLAYER GETS ANOTHER GO, WHICH IS THE ONLY WAY BACK FROM A BUILD THAT FAILED.
             * `Player::retry` clears a `player::Problem::Failed` and leaves a `Refused` standing,
             * so this is a second attempt on a machine that could host a surface and a no-op on
             * one that cannot. It is deliberately not on a timer: `build_as_child` fails when
             * there is no WebView2 runtime, and retrying that every frame would burn the reader's
             * CPU to learn nothing. A placement that failed needs none of this; it heals itself on
             * the next frame (see `player::keeps_syncing`). */
            Ask::WatchHere => {
                self.reveal(ScreenId::Watch);
                self.screens.watch.play_here();
                self.player.retry();
            }
            /* SETTINGS IS A `Body` AND NOT A `ScreenId`, so this cannot go through `reveal`: there
             * is no nav row to open and no section to unfold. `enter` is the same door the persona
             * footer's gear uses (`persona_pending` hands back exactly this `Body` and line 1840
             * puts it through this same call), which is the point: one route into Settings, so a
             * screen that asks for it lands where the gear lands and not somewhere adjacent.
             *
             * IT IS RAISED FROM A POP-OUT AS OFTEN AS FROM THE BODY, which is what it was added
             * for. `Windows::show` moves a child's ask onto the root's `Cx` and sets
             * `CompanionReq::Show`, so the main window comes forward with Settings already
             * showing; see `Ask::OpenSettings` for why the alternative (a control in the pop-out
             * that SET the folder) would have opened a worse defect than it closed. */
            Ask::OpenSettings => enter(
                Body::Settings,
                &mut self.body,
                &mut self.tabs,
                &mut self.section,
                &mut self.screens,
            ),
            Ask::None => {}
        }
    }
}

/// Switch the body and tell the screen behind the row which of its faces the row is. Several D5
/// rows are views of one screen (the parser's three, the sky's three, the LFG modes); the screen is
/// told once, on entry, and its own controls own the view after that. The context bar's tab is
/// the one remembered for that row.
fn enter(
    next: Body,
    body: &mut Body,
    tabs: &mut [usize; ScreenId::ALL.len()],
    section: &mut [usize; ScreenId::ALL.len()],
    s: &mut Screens,
) {
    *body = next;
    if let Body::Screen(id) = next {
        on_view(id, section[id.ordinal()], tabs[id.ordinal()], s);
    }
}

/// ARRIVING AT A DESTINATION: put the screen behind it into the state the row and its section
/// name. Two tiers, applied in that order, and the order is the whole of the function.
///
/// THIS USED TO BE ONE MATCH ON `(id, tab)` AND NINE OF ITS TEN ARMS WERE TIER 3 IN HIDING.
/// `Islands` set the sky screen's Boss view, `Motes` set the group finder's Motes mode: a rail
/// ROW, which is a destination, was the only way to reach a VIEW, which is a section. The rows
/// that were the only door to a view keep their arm below, because they are still real
/// destinations in the rail; what changed is that the screens they share now also carry a
/// section list (`nav::SECTIONS`) and an inner rail to pick from it.
///
/// TIER 2 FIRST AND TIER 3 SECOND, because tier 3 is the finer answer and must win. Entering
/// `Log Parser` runs no tier 2 arm and lands on whichever section that destination was left on;
/// entering `Loot Journal`, which is a destination of its own with no sections, runs its arm and
/// stops there. Reversing these would let a stale section overrule the row you just pressed.
fn on_view(id: ScreenId, section: usize, tab: usize, s: &mut Screens) {
    match id {
        /* THREE DESTINATIONS SHARE THE PARSER SCREEN AND EACH FIXES IT TO ONE VIEW.
         *
         * `Log Parser` gained its arm when it lost its section list: it used to list Kills, Loot
         * and Fights, which named the other two destinations' views a second time, so the rail
         * lit differently depending on which door you came through. It is the fights
         * destination now and nothing else, which is what the sitemap asks for in as many words.
         *
         * `Islands`, `Raid` and `Motes` had arms here and are gone entirely: each was a row that
         * existed only to reach a view, and each of those views is a section of its own
         * destination now. */
        ScreenId::Parser => s.parser.show(screens::parser::View::Fights),
        ScreenId::KillTracker => s.parser.show(screens::parser::View::Kills),
        ScreenId::Loot => s.parser.show(screens::parser::View::Loot),
        _ => {}
    }
    on_section(id, section, s);
    /* THE TAB GOES TO THE SCREEN THE SECTION OPENS, AND NOT TO THE BODY.
     *
     * # THE FIRST FIX FOR THE DEAD TAB ROW DID NOT WORK, AND ITS TEST SAID IT DID
     *
     * `on_tab` grew arms for `ParserDashboards` and `ParserReports` and was called as
     * `on_tab(id, tab, s)`. `id` here is the BODY, and the body is never either of those: `NAV`
     * carries one Log Parser row and it is `ScreenId::Parser`. A sub-row click sets
     * `ask.section = (Parser, i)` and the page is reached by `draw_screen`'s recursion, which
     * hops to the section's inner screen WITHOUT changing the body. So `on_tab` was always
     * handed `ScreenId::Parser` and fell straight through its `_ => {}`.
     *
     * THE EFFECT WAS THE DEFECT UNCHANGED: eleven context-bar tabs that filled and turned FLARE
     * when pressed while the body did not move a pixel, and on Reports the Personal and
     * Encounters bodies were unreachable in the shipped binary. The doc above `on_tab` said the
     * opposite in as many words.
     *
     * SO THIS MAKES THE SAME HOP `draw_screen` MAKES, out of the same table, and the two now
     * answer with the same screen for the same (body, section). */
    on_tab(target_of(id, section), tab, s);
}

/// WHICH SCREEN A (BODY, SECTION) PAIR ACTUALLY DRAWS.
///
/// THE SAME HOP `draw_screen` MAKES AND OUT OF THE SAME TABLE. A section whose entry names a
/// screen of its own is drawn by that screen; anything else is drawn by the body. Lifted out so
/// the router and the painter cannot answer differently, which is exactly what they did while this
/// was written once inside `draw_screen` and assumed in `on_view`.
fn target_of(id: ScreenId, section: usize) -> ScreenId {
    nav::sections_of(id)
        .get(section)
        .and_then(|(_, inner, _)| *inner)
        .unwrap_or(id)
}

/// WHERE YOU ARE, AS THE BREADCRUMB SAYS IT: the rail's word for the destination, then the
/// section's own word, where there is one.
///
/// # EVERY WORD IS LOOKED UP AND NONE IS SPELLED HERE
///
/// `nav::name_of` reads `NAV` and `nav::sections_of` reads `SECTIONS`, which are the two tables
/// the rail itself paints from. A crumb that typed `Log Parser` would be a fourth copy of that
/// phrase and the one a reader is most likely to catch disagreeing with the rail, because the two
/// are on screen together six inches apart.
///
/// TWO PARTS AND NOT THE WHOLE PATH. A rail HEAD is a realm and not a place you can stand in, and
/// tier 4 is a tab whose name is already lit on this same row. What is left is the pair the owner
/// asked for in as many words, `Log Parser / Dashboards`.
///
/// SETTINGS IS ONE PART, because it is not in `NAV` at all: it is reached from the gear and has no
/// row, and a crumb naming a parent it does not have would be an invention.
fn crumb_of(body: Body, section: &[usize; ScreenId::ALL.len()]) -> Vec<&'static str> {
    match body {
        Body::Settings => vec!["Settings"],
        Body::Screen(id) => {
            let mut out = Vec::new();
            if let Some(name) = nav::name_of(id) {
                out.push(name);
            }
            if let Some((name, _, _)) = nav::sections_of(id).get(section[id.ordinal()]) {
                out.push(*name);
            }
            /* A SCREEN WITH NO ROW AND NO SECTION STILL SAYS SOMETHING. Nothing in this build
             * reaches that state (`Body::Screen` is only ever set to a `NAV` row), which is
             * exactly why the fallback is here rather than an `unwrap`: an empty crumb is a
             * header that silently says nothing, and this at least says which. */
            if out.is_empty() {
                out.push("Grimoire");
            }
            out
        }
    }
}

/// TIER 4: the context bar's tab row, which used to reach nothing at all.
///
/// # THE TABS WERE DRAWN, THEY LIT WHEN PRESSED, AND THE PAGE DID NOT MOVE
///
/// This function's body was `let _ = tab;` under a comment saying "TIER 4 REACHES NO SCREEN IN
/// THIS BUILD". That was true when it was written and stopped being true the day the parser's
/// four sections got screens: `nav::SECTIONS` still listed a tab row under each of them, the
/// shell still drew every entry as a real `egui::Button` that repaints itself in FLARE when
/// clicked, and nothing between the button and the body read the number.
///
/// WORSE ON DASHBOARDS, WHICH DREW ITS OWN ROW AS WELL. The owner's screenshot has DPS, Healer,
/// Tank, Pet, Solo, Group, Raid Leader and Custom on it TWICE, once in the context bar and once
/// six pixels below, the top row inert and the bottom one live. A reader cannot tell which of
/// two identical rows is the one that works except by pressing both.
///
/// SO THE OUTER ROW IS THE CONTROL AND THE INNER ONE IS GONE. Every parser page that has tabs
/// now takes them from here, and `nav::SECTIONS` lists exactly the tabs the page can honour:
/// `the_parser_tab_rows_are_the_pages_own` is what holds those two lists together, and Live and
/// Logs carry no tab row at all because neither page has views to switch between.
///
/// DASHBOARDS IS NO LONGER ONE OF THEM, and its arm is gone rather than kept as a no-op. Its
/// eight roles were removed (a role is a claim about the person reading and no log line makes
/// one), the section carries no tab row, and the page is a grid of tiles the reader arranges. An
/// arm here for a section with no tabs would be a route nothing can take, which is the shape this
/// whole function was written to remove.
fn on_tab(id: ScreenId, tab: usize, s: &mut Screens) {
    if id == ScreenId::ParserReports {
        s.reports.show(tab);
    }
}

/// TIER 3: put the screen into the section the inner rail is pointing at.
///
/// EVERY ARM IS A VIEW THE SCREEN ALREADY HAD. Nothing here is a new capability; the indices are
/// positions in `nav::SECTIONS` and `the_sections_and_their_arms_are_one_list` is what stops the
/// two lists drifting apart, because an index that names no arm is a rail row that does nothing
/// when pressed and looks exactly like one that works.
fn on_section(id: ScreenId, section: usize, s: &mut Screens) {
    /* THE LOG PARSER'S SECTIONS ARE MATCHED BY NAME AND NOT BY INDEX.
     *
     * The Sky arms below are literal indices and the comment on them records what that costs:
     * `these indices are positions in nav::SECTIONS and nothing else, so inserting a section
     * shifts every arm after it`. The parser's section list has already moved once in this tree
     * and has just grown by two, so its arms are looked up rather than counted.
     *
     * WITHOUT THIS THE VIEW NEVER MOVED AT ALL. Fights, Analysis and Overlays are all views of
     * ONE body screen, so entering LOG PARSER / Analysis drew whatever view `ParserScreen`
     * happened to be on, which after a fresh launch is Kills. The rail lit `Analysis` and the
     * page showed the kill tracker. */
    if id == ScreenId::Parser {
        let name = nav::sections_of(id).get(section).map(|(n, _, _)| *n);
        match name {
            Some("Fights") => s.parser.show(screens::parser::View::Fights),
            Some("Analysis") => s.parser.show(screens::parser::View::Analysis),
            Some("Overlays") => s.parser.show(screens::parser::View::Overlays),
            /* Dashboards, Live, Reports and Logs are screens of their own: `target_of` hops to
             * them and they hold their own state, so there is no view here to set. */
            _ => {}
        }
        return;
    }
    match (id, section) {
        (ScreenId::Sky, 0) => s.sky.set_view(screens::sky::View::Giver),
        (ScreenId::Sky, 1) => s.sky.set_view(screens::sky::View::Boss),
        (ScreenId::Sky, 2) => s.sky.set_view(screens::sky::View::Keys),
        /* 4 AND NOT 3, BECAUSE ACHIEVEMENTS SITS AT 3 AND IS A SCREEN RATHER THAN A VIEW. These
         * indices are positions in `nav::SECTIONS` and nothing else, so inserting a section shifts
         * every arm after it; the guard below is what catches that rather than a reader. */
        (ScreenId::Sky, 4) => s.sky.set_view(screens::sky::View::Cleanout),
        (ScreenId::Lfg, 0) => s.lfg.mode = LfgMode::Generic,
        (ScreenId::Lfg, 1) => s.lfg.mode = LfgMode::Raid,
        (ScreenId::Lfg, 2) => s.lfg.mode = LfgMode::Motes,
        /* Gear draws a different SCREEN for its second section rather than a different view of
         * one, so it is answered in `draw_screen` and there is nothing to set here. */
        (ScreenId::Gear, _) => {}
        _ => {}
    }
}

/// Is this body the Watch screen?
///
/// IT USED TO ANSWER TRUE FOR THE VIDEOS ROW AS WELL, AND THAT WAS THE DEFECT.
/// `draw_screen` sent both rows to `WatchScreen`, so both really were the Watch screen and this
/// really did have to name both. That was harmless while the two drew byte-identical pages and
/// stopped being harmless when the Watch folio became a full bleed player: the Videos row opened
/// the TWITCH player under the crumb `stoic/videos`. The Videos row has a screen of its own now
/// (`screens::videos`), so this is one row again, and [`is_videos`] is the other.
///
/// NAMED RATHER THAN WRITTEN OUT, because the context header asks it to decide whose controls to
/// draw and the central panel asks [`is_full_bleed`] to decide the margin; a `matches!` at each
/// site is two places for one fact.
/// DOES THE WINDOW DRAWING THIS BODY HAVE A RAIL? See `Cx::railed` for what a screen does with
/// the answer.
///
/// A FUNCTION FOR A CONSTANT, ON PURPOSE. It was an expression at the call site reading
/// `!nav::sections_of(id).is_empty()`, and that was wrong for one build in a way nothing could
/// catch: a test that renders a screen has to be handed a `Cx` it built itself, so it exercised
/// its own idea of the flag and never this one. Named here, the guard
/// (`a_railed_screen_does_not_offer_its_sections_twice`) drives the real rule.
///
/// WHY IT IS TRUE FOR EVERY BODY. The question a screen is asking is not `are my sections listed`
/// but `is there a rail in this window at all`, because the rail is what navigates, whether by a
/// destination row, by a nested section, or by having put you on a fixed view of a shared screen.
/// Hunt Journal is the Parser screen fixed to Kills and has no sections of its own; under the old
/// reading the flag went false there and the parser drew its three-way row on a page that is Kills
/// by definition, so pressing Loot left the rail saying Hunt Journal and the body showing loot.
/// Only a pop-out tool window has no rail, and only there does a screen owe its own switcher.
/// KEEP A REMEMBERED TAB INSIDE THE LIST IT IS ABOUT TO INDEX.
///
/// TIER 4 BELONGS TO A SECTION, and the slot that remembers it belongs to a DESTINATION, so the
/// two can disagree the moment you change section: Roster has four tabs and Calendar has three,
/// so arriving at Calendar holding 3 points past the end. `main` resets the slot on a section
/// change, which handles the ordinary path, and this handles every other one: a restored
/// preference, a jump straight to a section, a table edited under a running app.
///
/// AN EMPTY LIST ANSWERS 0 rather than panicking on `len() - 1`, which is the case every screen
/// with no tier 4 takes, meaning almost all of them.
fn tab_in_range(tab: usize, tabs: &[&str]) -> usize {
    if tabs.is_empty() {
        0
    } else {
        tab.min(tabs.len() - 1)
    }
}

fn railed_here(body: Body) -> bool {
    let _ = body;
    true
}

fn is_watch(body: Body) -> bool {
    matches!(body, Body::Screen(ScreenId::Watch))
}

/// Is this body the Videos screen? See [`is_watch`] for why the two are separate now.
fn is_videos(body: Body) -> bool {
    matches!(body, Body::Screen(ScreenId::Videos))
}

/// Does this body take the whole folio for a webview surface?
///
/// BOTH SURFACE SCREENS, AND THE MARGIN IS WHAT THEY SHARE. Watch live stages the player and
/// Videos stages the channel page; each takes its folio's own `available_rect_before_wrap` whole,
/// and a margin around either is a frame this app has drawn around somebody else's picture or
/// somebody else's site.
fn is_full_bleed(body: Body) -> bool {
    is_watch(body) || is_videos(body)
}

/// THE FOLIO'S MARGIN, AND THE TWO SURFACE SCREENS ARE THE ONES THAT HAVE NONE.
///
/// Every other screen is words and rows, and 18 points is what keeps them off the rail and off the
/// footer. The Watch folio is a PICTURE: the video, or Broken Stoic's own offline screen. The
/// Videos folio is somebody else's whole web page. A margin around either is not breathing room, it
/// is a frame this app has drawn around somebody else's work, and the same defect was measured once
/// already inside the Watch screen (`watch::art_band` records the run where a 16 by 9 picture sat
/// in a slab 350 points wider than itself and read as a small thing adrift in a dark strip). "Edge
/// to edge" is the owner's phrase for what the leaves should be, and this is the line that makes it
/// true: the folio's own `available_rect_before_wrap` is what the stage and the picture both take,
/// so what this leaves out is what they get.
///
/// IT IS A FUNCTION SO A TEST CAN ASK IT. `App::ui` takes an `&mut eframe::Frame`, which has no
/// public constructor, so the panel itself is not reachable from a test; the decision inside it is.
fn folio_margin(body: Body) -> egui::Margin {
    if is_full_bleed(body) {
        egui::Margin::ZERO
    } else {
        egui::Margin::same(18)
    }
}

/// The tool window a row can pop out into, if it has one (D3: Watch, Parser, Sky, LFG). The
/// context bar offers "Pop out" for these rows, or "Focus window" when that window is open.
fn tool_of(id: ScreenId, s: &Screens) -> Option<Tool> {
    match id {
        ScreenId::Parser | ScreenId::KillTracker | ScreenId::Loot => Some(Tool::Parser),
        ScreenId::Sky => Some(Tool::Sky),
        ScreenId::Lfg => Some(Tool::Lfg(s.lfg.mode)),
        ScreenId::Watch | ScreenId::Videos => Some(Tool::Watch),
        _ => None,
    }
}

/// WHICH PAGE A POP-OUT SHOULD ARRIVE ON, given where the reader was standing when he pressed the
/// picture in picture control.
///
/// # DEFECT: POPPING OUT FROM ONE PAGE HANDED YOU A WINDOW SHOWING ANOTHER
///
/// [`windows::Windows::open_at`] carries the whole account. In short: `Tool::Lfg` CARRIES its mode
/// and `Tool::Parser` carries nothing, so the LFG pop-out arrived on the page you left and the
/// parser pop-out arrived on `ParserWindow::default`, which is Dashboards. Standing on LOG PARSER
/// / Fights, or on the Hunt Journal, and pressing a control whose hover says "put THIS in its own
/// window" put something else in a window, and the owner's reason for a pop-out is to keep a page
/// over the game while he plays, so the page is the entire point of the press.
///
/// THE PAGE IS A FACT ABOUT THE PRESS AND ONLY THIS FUNCTION'S CALLER KNOWS IT. A `Tool` is a
/// hotkey's currency and `Ctrl+Alt+P`, pressed from inside the game, is not pressed from a page at
/// all; that is why `Windows::open` still exists unchanged for every hotkey caller and why this is
/// an argument rather than a payload on the variant.
///
/// # WHY THIS IS A FUNCTION AND NOT THREE LINES IN THE CLOSURE THAT NEEDS THEM
///
/// The same reason `rail_plan`, `rail_list` and `persona_pending` are functions: `App::ui` takes an
/// `&mut eframe::Frame`, which has no public constructor, so nothing inside it can be called from a
/// test. A transposition here would open a plausible-looking window on the wrong page and no test
/// in this crate could see it.
///
/// # THE TWO LITERALS, AND WHY THE THIRD ARM IS NOT ONE
///
/// Hunt Journal and Loot Journal are rows of MY LEGEND in `nav::NAV` rather than sections of the
/// parser, and `tool_of` sends all three destinations to `Tool::Parser` because a window is a place
/// to stand rather than a page. `ParserWindow::PAGES` carries both names verbatim for that reason,
/// and `the_pop_out_opens_on_a_page_that_window_actually_has` pins the pair together so a rename on
/// either side is a failing test rather than a pop-out that quietly does not move.
///
/// The parser's own arm READS the name out of `nav::SECTIONS` instead of spelling it, because that
/// list is the only thing that knows the section order and it has moved twice in this tree. A name
/// and not an index, for the reason `Ask::Open` and `open_at` both give: an index that no longer
/// names what it named opens the wrong page in silence, while an unknown name leaves the window
/// where it is.
///
/// `None` FOR EVERYTHING ELSE IS AN ANSWER AND NOT A GAP. Sky, LFG and Watch have no page list,
/// `open_at` ignores the argument for all of them, and a section index out of range answers `None`
/// rather than panicking, exactly as `nav::tabs_of` does.
/// DOES THE PAGE BEING STOOD ON OFFER A POP-OUT AT ALL?
///
/// NOT ON DASHBOARDS. The owner pressed the pop-out there and got a Log Parser window over his
/// game, which is not what anybody looking at a dashboard is asking for: the dashboard is a page for
/// looking back over a night at full size, and what on it belongs in a window over a game is the
/// overlays, which the Overlays page pops out one widget at a time. Every other page keeps its
/// pop-out exactly as it was.
fn offers_pop_out(body: Body, section: &[usize; ScreenId::ALL.len()]) -> bool {
    match body {
        Body::Screen(id @ ScreenId::Parser) => !matches!(
            nav::sections_of(id).get(section[id.ordinal()]),
            Some((_, Some(ScreenId::ParserDashboards), _))
        ),
        _ => true,
    }
}

fn pop_out_page_of(body: Body, section: &[usize; ScreenId::ALL.len()]) -> Option<&'static str> {
    match body {
        Body::Screen(ScreenId::KillTracker) => Some("Hunt Journal"),
        Body::Screen(ScreenId::Loot) => Some("Loot Journal"),
        Body::Screen(id @ ScreenId::Parser) => nav::sections_of(id)
            .get(section[id.ordinal()])
            .map(|(n, _, _)| *n),
        _ => None,
    }
}

/// What the persona footer's gear is saying: red when the settings file could not be read, the
/// gold "act" state when a hotkey did not register (a person has to resolve the conflict, D4, by
/// rebinding it on that screen or closing the program that owns it), settled otherwise. The
/// Settings screen paints the same gold state beside the same rows, so the two never disagree
/// about one fact.
///
/// IT USED TO COLOUR A ROW IN THE RAIL AND NOW IT COLOURS THE GEAR. The row is gone (see `Body`);
/// this is the fact that outlived it, and `persona::gear_ink` is where it lands.
fn settings_state(settings: &Settings, bindings: &[hotkeys::Binding]) -> State {
    if settings.load_problem.is_some() {
        State::Wrong
    } else if bindings.iter().any(|b| !b.registered) {
        State::You
    } else {
        State::Settled
    }
}

/* ---------------------------------------------------------------------- the rail --
 *
 * WHY THE RAIL IS DECIDED BEFORE IT IS DRAWN, AND WHY THAT IS A FIX RATHER THAN A FLOURISH.
 *
 * Every module the rail leans on is covered by its own tests: `nav::square` has the state table
 * and `chrome` has the drawing. (Two others are gone rather than uncalled: `section_wants_you`,
 * which had the one-dot rule, left with the Sources row that was its only cause, and `count_of`,
 * which had the badge rule, left with the badge; `nav.rs` carries both arguments where they
 * stood.) What NOTHING covered was the lines that JOIN them, because
 * they lived inside a closure inside `App::ui`, and `App::ui` cannot be called from a test: it
 * takes an `&mut eframe::Frame`, which has no public constructor. So the rail's wiring, which row
 * is asked about, which square goes on which label, was the one part of the rail asserted by
 * nothing at all, and a transposition there would have drawn a perfectly plausible rail that
 * lied.
 *
 * `rail_plan` is those joins, lifted out as a value. It reaches every module the closure reached
 * and answers the same questions in the same order, and it returns them instead of painting them,
 * so a test can read what the App is about to draw. The drawing loop below it then paints the plan
 * and decides nothing: it has no access to `Facts` or to `Cx` at all, which is what stops the two
 * copies drifting apart. This is the same rule `persona::slots` states for the same reason: when
 * the painting owns the arithmetic, every hit box is a second copy of it and a second copy is free
 * to disagree. */

/// One row of the rail, decided before anything is painted.
struct RailRow {
    label: &'static str,
    id: ScreenId,
    /// Whether the body is showing this row now.
    selected: bool,
    /// The leading square, from `nav::square`.
    state: State,
    /// TIER 3, nested under this row, and EMPTY UNLESS THIS IS THE ROW YOU ARE ON.
    ///
    /// A rail that showed every destination's sections at once would be a tree of a hundred and
    /// twenty rows, and would answer a question nobody asked: the sections of a workspace matter
    /// while you are in it. So they open with the destination and close behind you, which is the
    /// same bargain the headings already make.
    subs: Vec<RailSub>,
}

/// One section of the destination above it.
struct RailSub {
    label: &'static str,
    /// Its index in `nav::SECTIONS`, which is what a click reports and `on_section` answers.
    at: usize,
    selected: bool,
}

/// One section of the wide rail: its heading, its marks, and its rows when it stands open.
struct RailSection {
    /// The index into `nav::NAV`, which is what a click on the heading toggles.
    index: usize,
    head: &'static str,
    marks: chrome::SectionHead,
    /// Empty when the section is shut. A shut section draws its heading and no rows.
    rows: Vec<RailRow>,
}

/// Everything the wide rail will draw this frame.
struct RailPlan {
    sections: Vec<RailSection>,
}

/// Decide the wide rail. Reads only facts; paints nothing.
fn rail_plan(
    open: &[usize],
    body: Body,
    section: &[usize; ScreenId::ALL.len()],
    facts: &nav::Facts,
) -> RailPlan {
    let sections = nav::NAV
        .iter()
        .enumerate()
        .map(|(index, (head, rows))| {
            let is_open = open.contains(&index);
            RailSection {
                index,
                head,
                marks: chrome::SectionHead {
                    open: is_open,
                    /* A heading with no rows has nothing to open, which today is the launcher. */
                    foldable: !rows.is_empty(),
                    /* The launcher is lit at all times: it is the primary act rather than a drawer,
                     * so it does not spend most of its life in shadow like the headings that fold. */
                    lit: *head == nav::LAUNCHER,
                },
                rows: if is_open {
                    rows.iter()
                        .map(|(label, id)| {
                            /* THE ROW LIGHTS FOR ITS SECTIONS TOO, and this is not a nicety.
                             * A section that is its own screen can be arrived at directly, from
                             * the finder or from another screen's link, and the body is then
                             * that screen rather than the destination it lives in. Matching only
                             * the row's own id left the whole rail dark on those arrivals: no
                             * heading lit, no row lit, nothing saying where you were. */
                            let (selected, forced) = match body {
                                Body::Screen(b) if b == *id => (true, None),
                                Body::Screen(b) => match nav::parent_of(b) {
                                    Some((owner, at)) if owner == *id => (true, Some(at)),
                                    _ => (false, None),
                                },
                                Body::Settings => (false, None),
                            };
                            let parts = nav::sections_of(*id);
                            RailRow {
                                label,
                                id: *id,
                                selected,
                                state: nav::square(*id, facts),
                                subs: if selected {
                                    /* `min` because the slot outlives the table: shortening a
                                     * section list would otherwise leave a stale index pointing
                                     * past the end and light nothing at all. */
                                    /* WHAT THE BODY SAYS BEATS WHAT THE SLOT REMEMBERS. Arriving
                                     * straight at a section is a statement about where you are;
                                     * the slot is only where you were last. */
                                    let on = forced.unwrap_or_else(|| {
                                        section[id.ordinal()].min(parts.len().saturating_sub(1))
                                    });
                                    parts
                                        .iter()
                                        .enumerate()
                                        .map(|(at, (label, _, _))| RailSub {
                                            label,
                                            at,
                                            selected: at == on,
                                        })
                                        .collect()
                                } else {
                                    Vec::new()
                                },
                            }
                        })
                        .collect()
                } else {
                    Vec::new()
                },
            }
        })
        .collect();
    RailPlan { sections }
}

/// What one frame of the wide rail was asked for. At most one of each: a frame carries one
/// pointer and a click lands on one thing.
#[derive(Default)]
struct RailAsk {
    toggle: Option<usize>,
    body: Option<Body>,
    /// A nested section was pressed: which destination's, and which one.
    section: Option<(ScreenId, usize)>,
}

/// Open a shut section, shut an open one. Nothing else happens, and nothing else SHOULD: clicking
/// one section must never disturb another.
fn toggle_head(open: &mut Vec<usize>, i: usize) {
    if let Some(at) = open.iter().position(|&x| x == i) {
        open.remove(at);
        return;
    }
    open.push(i);
}

/// Make sure a section stands open, without shutting anything.
///
/// THE NARROW RAIL USES THIS AND A HEADING DOES NOT, and the difference is the whole reason it is
/// a function of its own. A heading is a toggle: press it twice and you are back where you were.
/// A realm mark in the narrow rail is a way IN, so pressing it must never be the thing that shuts
/// the list you were reaching for.
fn reveal_head(open: &mut Vec<usize>, i: usize) {
    if !open.contains(&i) {
        open.push(i);
    }
}

/// Paint the wide rail and report what was pressed. Decides nothing; see [`rail_plan`].
fn rail_list(ui: &mut egui::Ui, plan: &RailPlan) -> RailAsk {
    let mut ask = RailAsk::default();
    /* THE PERSONA BLOCK IS PINNED UNDER THIS LIST, so the list is handed the rail MINUS that block
     * before it draws a row. `persona::room_above` is the reservation and `persona::push_to_floor`
     * is the drop to the floor; both live in persona.rs beside the height they protect. */
    let room = persona::room_above(ui);
    egui::ScrollArea::vertical()
        .max_height(room)
        .show(ui, |ui| {
            for sec in &plan.sections {
                if section(ui, sec.head, sec.marks).clicked() {
                    /* Deferred: the plan is being iterated. One pending change per frame is all a
                     * click can produce. */
                    ask.toggle = Some(sec.index);
                }
                for r in &sec.rows {
                    if nav_row(ui, r.label, r.selected, Some(r.state), false).clicked() {
                        ask.body = Some(Body::Screen(r.id));
                    }
                    for sub in &r.subs {
                        if chrome::sub_row(ui, sub.label, sub.selected).clicked() {
                            ask.section = Some((r.id, sub.at));
                        }
                    }
                }
            }
            /* The list's own bottom padding, so the last row of a list long enough to fill the
             * rail does not sit on the persona footer's hairline. */
            ui.add_space(11.0);
        });
    ask
}

/// What a persona footer answer does to the body. The footer is the only door to Settings, and
/// this is the whole route: one function, so the join is a thing a test can call rather than three
/// words inside a closure no test can enter.
fn persona_pending(action: PersonaAction) -> Option<Body> {
    match action {
        PersonaAction::OpenSettings => Some(Body::Settings),
        PersonaAction::None => None,
    }
}

/* ---------------------------------------------------------------------- the body -- */

/// `section` AND NOT `tab`, WHICH IS THE TIER THIS ARGUMENT ALWAYS MEANT. Gear and Keys each
/// draw a different SCREEN for their second face, so the choice cannot live inside one screen
/// and has to be made here; a face of a destination is tier 3, and it now arrives from the
/// inner rail rather than from the context bar.
fn draw_screen(
    ui: &mut egui::Ui,
    id: ScreenId,
    section: usize,
    tab: usize,
    s: &mut Screens,
    cx: &mut Cx,
) {
    /* A DESTINATION WHOSE SECTIONS ARE SCREENS OF THEIR OWN DRAWS THE ONE YOU ARE IN.
     *
     * THE LOG PARSER IS THE ONE THIS MATTERS FOR: four of its five sections are screens, and
     * the fifth, Fights, is the Parser screen's own body. This comment named the Bazaar, which
     * is a heading with six plain rows under it and has never taken this path.
     *
     * ONE HOP AND NOT A LOOP, because a section's own sections would be tier 4 and this build
     * has none; if that ever changes this is the line that has to grow a depth guard rather
     * than recurse until the stack ends. */
    /* THE HOP IS `target_of`, which `on_view` also uses, so the screen this paints and the
     * screen the tab row is routed to cannot be two different screens. */
    let inner = target_of(id, section);
    if inner != id {
        return draw_screen(ui, inner, 0, tab, s, cx);
    }
    match id {
        ScreenId::Parser | ScreenId::KillTracker | ScreenId::Loot => s.parser.ui(ui, cx),
        /* THE FIGHT YOU ARE IN. Built on `Ingest::current_fight`, which is the door
         * `nav::unbuilt_why` used to say did not exist. */
        ScreenId::ParserLive => s.live.ui(ui, cx),
        ScreenId::ParserReports => s.reports.ui(ui, cx),
        ScreenId::ParserDashboards => s.dashboards.ui(ui, cx),
        ScreenId::ParserLogs => s.logs.ui(ui, cx),
        ScreenId::Commission => s.commission.ui(ui, cx),
        ScreenId::Items => s.items.ui(ui, cx),
        ScreenId::Zones => s.zones.ui(ui, cx),
        ScreenId::Quests => s.quests.ui(ui, cx),
        ScreenId::Inventory => s.inventory.ui(ui, cx),
        ScreenId::Gear => {
            if section == 1 {
                s.valet.ui(ui, cx)
            } else {
                s.gear.ui(ui, cx)
            }
        }
        ScreenId::Exalt => s.exalt.ui(ui, cx),
        ScreenId::Sky => s.sky.ui(ui, cx),
        /* THE ACHIEVEMENTS DUMP, which was reachable only as the second tab of a `Keys` row and
         * is a section of PLANE OF SKY now, where a reader looking for sky progress will pass
         * it. That row is gone: the ladder it drew is `Island ladder`, one of the same
         * destination's sections. */
        ScreenId::Achievements => s.unlocks.ui(ui, cx),
        ScreenId::Lfg => s.lfg.ui(ui, cx),
        ScreenId::Watch => s.watch.ui(ui, cx),
        /* CHAT IS A REAL SCREEN NOW AND IT CONNECTS TO NOTHING. Those are different states and
         * the unbuilt page could only say the first. What is missing here is a socket rather
         * than a decision; see the module note for the three things the old page got wrong. */
        ScreenId::Chat => s.chat.ui(ui, cx),
        /* THE ROW FINALLY SHOWS WHAT IT SAYS. This arm read
         * `ScreenId::Watch | ScreenId::Videos => s.watch.ui(..)`, which was harmless while the two
         * rows drew byte-identical pages and became a lie the day the Watch folio became a full
         * bleed player: clicking Videos opened the TWITCH player under the crumb `stoic/videos`,
         * and the word VIDEOS appeared nowhere in the main window. `screens::videos` is the
         * channel's own page on YouTube, on both platform settings; see its module note. It holds
         * no state, so there is nothing of it in `Screens`. */
        ScreenId::Videos => screens::videos::folio(ui, cx),
        ScreenId::Spells => s.spells.ui(ui, cx),
        ScreenId::WorkOrders
        | ScreenId::Workshop
        | ScreenId::Trio
        | ScreenId::Aa
        | ScreenId::Levelling
        | ScreenId::SpawnTimers
        | ScreenId::Standing
        /* The rows the owner`s rail names that this build has no screen for. Every one of
         * them says what it would be and what it is waiting on; the words are `nav::UNBUILT`s. */
        | ScreenId::Gina
        | ScreenId::CharacterSheet
        | ScreenId::Loadouts
        | ScreenId::Theorycraft
        | ScreenId::Compendium
        | ScreenId::Guild
        /* And the nine the Norrath rail added. Same page and the same reason.
         *
         * NO COMMA IN THIS COMMENT AND THAT IS NOT AN ACCIDENT. The reachability test below
         * (`the_unbuilt_route_and_the_unbuilt_list_hold_the_same_rows`) finds this arm by walking
         * back to the last comma before the marker and reads every screen named after it. A comma
         * anywhere in here truncates what it reads and the test then compares half an arm against
         * the whole of `nav::UNBUILT`. It caught exactly that on the first run of this edit. */
        | ScreenId::Collections
        | ScreenId::Archive
        | ScreenId::Bestiary
        | ScreenId::LootTables
        | ScreenId::Crafting
        | ScreenId::RaidTargets
        | ScreenId::Schedule
        | ScreenId::RaidHistory
        | ScreenId::Lockouts
        | ScreenId::FarmPlan
        /* The two halves of the Bazaar`s stock. Both wait on a live market price and this app
         * has no source for one.
         *
         * STILL NO COMMA IN HERE. I put one in this exact comment and the reachability test below
         * went red for the second time: it walks back to the last comma before the marker to find
         * where the arm starts. The note above says so and I wrote past it anyway. */
        | ScreenId::TradeGoods
        | ScreenId::DroppedItems => unbuilt(
            ui,
            id,
            nav::sections_of(id).get(section).map(|(name, _, _)| *name),
            nav::tabs_of(id, section).get(tab).copied(),
        ),
    }
}

/// A row with nothing behind it says so, in words that name what is missing.
///
/// THE WORDS ARE NOT KEPT HERE. They live in `nav::UNBUILT`, beside the `nav::square` that decides
/// the same row's hollow ring, because this function used to hold its own copy and the two came
/// apart: Standing's reason here blamed a missing THREAD while the rail's reason blamed a missing
/// TRADING PARTNER, and a comment on this line asserted they "cannot disagree" while nothing made
/// that true. Now one list answers both, and `nav::unbuilt_why` returning None is itself the
/// routing bug report.
/// THE PAGE A ROW WITH NO SCREEN LANDS ON, and it names WHERE you are as well as what is missing.
///
/// THE PATH IN THE HEADING IS WHAT MAKES AN UNWRITTEN SUBTREE WALKABLE. Guild is seven sections
/// and twenty-four tabs of nothing built: without the path every one of them would draw a page
/// identical to the last, and thirty-one controls that select and change nothing on screen are
/// thirty-one controls a reader cannot tell from broken ones. With it, pressing Roster changes
/// the page and pressing Keys & Flags changes it again, so the shape of the thing can be walked
/// before a line of it exists. `every_section_of_every_destination_reaches_its_view` is what
/// holds this: a section that leaves the page unchanged fails there.
fn unbuilt(ui: &mut egui::Ui, id: ScreenId, section: Option<&str>, tab: Option<&str>) {
    const MISROUTED: &[&str] = &["This row is built; the App routed it here by mistake."];
    let why: &[&str] = nav::unbuilt_why(id).unwrap_or(MISROUTED);
    let mut head = nav::label(id).to_ascii_uppercase();
    for step in [section, tab].into_iter().flatten() {
        head.push_str(" / ");
        head.push_str(&step.to_ascii_uppercase());
    }
    ui.label(
        RichText::new(head)
            .font(fonts::display(16.0))
            .color(GOLD_HI),
    );
    ui.add_space(8.0);
    ui.label(RichText::new("Not built in this release.").color(TEXT));
    for line in why {
        ui.label(RichText::new(*line).color(TEXT_2));
    }
    ui.add_space(8.0);
    /* THIS LINE USED TO SAY `the rail draws this row as a hollow ring for the same reason`, and
     * the rail does not. `nav::square` answers Idle for every one of these, but `chrome::nav_row`
     * reads that answer only to decide whether to paint the trailing attention bar, which is for
     * `You` alone; Idle paints nothing at all. The sentence was describing a leading status
     * square that was cut from the rail as an invention, and it outlived it. */
    ui.label(
        RichText::new("Nothing in the rail marks it, so this page is where a row says so.")
            .color(TEXT_3),
    );
}

/* ------------------------------------------------------------------- the find box -- */

/// The box and its dropdown. Returns the hit that was picked, if one was.
///
/// The dropdown is an `Area` in the foreground order under the box, so it floats over the body
/// without the body having to make room; it stands while the query is non-empty and goes with
/// What the find box offers, BUILT FROM `HitKind::ALL` RATHER THAN WRITTEN OUT.
///
/// That is a fix and not a flourish. The hint was the literal "find: i: z: d: q:", and it was
/// wrong before this unit touched it: `s:` and `p:` have been accepted prefixes for as long as
/// the sky list and the spell list have been in the snapshot, and neither was ever offered here.
/// Deleting `d:` with the Drops screen would have left the same class of lie, one letter shorter.
/// Derived, the box cannot advertise a letter the parser refuses or hide one it takes.
fn find_hint() -> String {
    let prefixes: Vec<String> = data::HitKind::ALL
        .iter()
        .map(|k| format!("{}:", k.prefix()))
        .collect();
    format!("find: {}", prefixes.join(" "))
}

/// What the finder says when an unprefixed needle matched nothing: the needle it looked for, and
/// every kind it looked in. A prefixed query gets the kind's own label instead, from `HitKind`.
///
/// NAMED AND NOT INLINE so a test can hold the list to `HitKind::ALL`. English plurals are not
/// derivable from the labels ("sky" is not "skys"), so the sentence is written and pinned rather
/// than generated: the test asserts every label appears in it, which is what catches a kind added
/// or removed while this line kept describing the old set.
fn nothing_found(needle: &str) -> String {
    format!("nothing named like \"{needle}\" in items, zones, quests, spells or sky items")
}

/// Escape or a pick. Rows are the hit's kind, its name and the data module's one computed detail
/// line, in the monospace face because a person compares names character by character.
fn finder_ui(ui: &mut egui::Ui, finder: &mut Finder, data: Option<&Snapshot>) -> Option<Hit> {
    let offered = find_hint();
    let hint: &str = match data {
        Some(_) => &offered,
        None => "find (needs the snapshot)",
    };
    let box_resp = ui.add_enabled(
        data.is_some(),
        egui::TextEdit::singleline(&mut finder.query)
            .hint_text(hint)
            .desired_width(240.0)
            .font(FontId::monospace(12.0)),
    );
    let q = finder.query.trim().to_owned();
    if q.is_empty() {
        return None;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        finder.query.clear();
        return None;
    }
    let data = data?;
    let hits = data.search(&q);
    let mut picked: Option<Hit> = None;
    let pos = box_resp.rect.left_bottom() + Vec2::new(0.0, 4.0);
    egui::Area::new(egui::Id::new("grimoire.finder"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .interactable(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::NONE
                .fill(PANEL)
                .stroke(Stroke::new(1.0, GOLD_DEEP))
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.set_min_width(420.0);
                    if hits.is_empty() {
                        let (kind, needle) = data::parse_query(&q);
                        let scope = match kind {
                            Some(k) => format!("no {} named like \"{needle}\"", k.label()),
                            None => nothing_found(&needle),
                        };
                        ui.label(
                            RichText::new(scope)
                                .font(FontId::monospace(11.5))
                                .color(TEXT_2),
                        );
                        return;
                    }
                    for h in hits.iter().take(FINDER_ROWS) {
                        let label = format!("{:<5} {}", h.kind.label(), h.name);
                        let r = ui.add(
                            egui::Button::new(
                                RichText::new(label)
                                    .font(FontId::monospace(11.5))
                                    .color(TEXT),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                        );
                        let r = r.on_hover_text(&h.detail);
                        if r.clicked() {
                            picked = Some(h.clone());
                        }
                    }
                    let more = hits.len().saturating_sub(FINDER_ROWS);
                    if more > 0 {
                        let cap = if hits.len() >= data::SEARCH_CAP {
                            format!(" (search stops at {})", data::SEARCH_CAP)
                        } else {
                            String::new()
                        };
                        ui.label(
                            RichText::new(format!("{more} more; type more of the name{cap}"))
                                .font(FontId::proportional(11.0))
                                .color(TEXT_3),
                        );
                    }
                });
        });
    if picked.is_some() {
        finder.query.clear();
    }
    picked
}

/// Where a hit goes: the FIND screen that owns its kind, through the same asks a cross-screen
/// link makes.
fn ask_for(h: &Hit) -> Ask {
    match h.kind {
        HitKind::Item => Ask::ShowItem(h.name.clone()),
        HitKind::Zone => Ask::ShowZone(h.name.clone()),
        HitKind::Quest => Ask::ShowQuest(h.name.clone()),
        /* A sky piece is an item; the Items screen shows it and says so if gear-data lacks it. */
        HitKind::SkyItem => Ask::ShowItem(h.name.clone()),
        HitKind::Spell => Ask::ShowSpell(h.name.clone()),
    }
}

/* --------------------------------------------------------------------- the frame -- */

impl App {
    /// OPEN AT ONE QUADRANT OF THE SCREEN, so the window drops into a 2x2 snap grid.
    ///
    /// # WHY THIS IS NOT IN THE VIEWPORT BUILDER, WHERE A WINDOW SIZE BELONGS
    ///
    /// The builder runs before there is a window, so it cannot know the screen. It was given
    /// `[1920.0, 1080.0]`, half of 3840 by 2160, which is right in PHYSICAL PIXELS and wrong in
    /// the units it is read in: `with_inner_size` takes egui POINTS, and a point is a pixel only
    /// at 100% scaling. The owner's desktop is at 175%, so that asked for a window covering most
    /// of his screen, and the matching minimum was bigger than his whole logical desktop.
    ///
    /// SO THE SIZE IS ASKED FOR ON THE FIRST FRAME, when `native_pixels_per_point` is known and
    /// the work area can be read, and both are facts rather than assumptions.
    ///
    /// # ONCE, AND NEVER AGAIN IN THIS PROCESS
    ///
    /// `sized` is what makes this a DEFAULT rather than a policy. Re-applying it would drag a
    /// window back out of wherever the owner had just dragged it, once a second, forever.
    ///
    /// THE MINIMUM GOES WITH IT AND FOR THE SAME REASON. The owner's rule is that the design size
    /// is also the floor, so no page ever needs a narrower answer; see `chrome::floor_for` for
    /// the four points of slack and why they are tolerance rather than a second target.
    ///
    /// A MACHINE THAT WILL NOT ANSWER KEEPS `chrome::FALLBACK` and its 640 by 480 minimum, which
    /// is a window a person can see and move. Guessing a work area would risk one that is not.
    fn fit_to_quadrant(&mut self, ctx: &egui::Context) {
        if self.sized {
            return;
        }
        let Some(work) = chrome::work_area_px() else {
            /* Asked once and answered no: nothing on this machine will ever answer, so stop
             * asking rather than calling into the OS on every frame for the life of the run. */
            self.sized = true;
            return;
        };
        /* THE ZOOM AND NOT THE SCALE, AND THE DIFFERENCE IS THE WHOLE DEFECT THIS REPLACES.
         *
         * This divided by `native_pixels_per_point` and produced a window a quarter of the size
         * it should have been, because `SPI_GETWORKAREA` already answers in the same virtualized
         * units egui's points live in: dividing by the native scale converted twice. Measured on
         * the owner's 175% desktop, the window came up 1097 by 593 PHYSICAL where 1097 by 593
         * POINTS was wanted.
         *
         * `zoom_factor` IS THE HALF THAT IS NOT ACCOUNTED FOR. `pixels_per_point` is the native
         * scale times the app's own zoom; the native half cancels and the zoom does not, so a
         * reader who has zoomed the interface still gets a window that is half his screen rather
         * than half of it again. It is 1.0 by default, which is exactly why leaving it out would
         * have looked correct forever. */
        /* THE WHOLE CONVERSION IS `quadrant_for`'s, AND THAT IS THE POINT.
         *
         * Two passes at this got the units wrong here, in the caller, while a test of the
         * halving stayed green through both: the test divided the way it believed this line
         * divided, and nothing compared them. There is nothing left to get wrong here now.
         *
         * `pixels_per_point` AND NOT `zoom_factor`. It is the native scale times the zoom, and
         * both have to divide out: the work area arrives in real pixels and the window is built
         * in points. */
        let quadrant = chrome::quadrant_for(work, ctx.pixels_per_point());
        /* THE MINIMUM FIRST. Sent the other way round, the window would be asked to take a size
         * its own minimum still forbids, and the window manager would clamp the very size this
         * is trying to set. */
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(chrome::floor_for(
            quadrant,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(quadrant));
        self.sized = true;
    }

    /// DRAIN THE HOTKEY RECEIVER AND ACT ON WHAT CAME OUT. Runs on EVERY eframe callback,
    /// painting or not, which is the whole point of it being a method rather than a block.
    ///
    /// D4: Ctrl+Alt+G toggles the main window, every other binding opens its tool. `summon` is the
    /// one place that knows which verb belongs to which.
    fn heartbeat(&mut self, ctx: &egui::Context) {
        /* THIS BUILD IS ALIVE, WHICH IS THE OTHER HALF OF THE FAILED-LAUNCH COUNT.
         *
         * THE DEFECT THIS FIXES, AND IT IS THE FOURTH TIME THIS FILE HAS LEARNED THE SAME LESSON.
         * The increment lived in the trampoline, before every spawn, and the CLEAR lived in
         * `App::ui` behind `frames == 2`. eframe calls `ui` only while the viewport is visible, so
         * two launches in which the window was minimized or never composited (the reader presses
         * Ctrl+Alt+G while the 24.7 MB snapshot is still loading; Windows opens the window behind a
         * fullscreen-exclusive game) left the count at two on a build that was working perfectly,
         * and the third launch rolled the reader back to an older version for no reason they could
         * see. `heartbeat` is the callback both paths reach, which is why every other rule that
         * made this mistake now lives here too.
         *
         * ON THE SECOND PASS AND NOT THE FIRST, which is the rule the count is for: a glow or wgpu
         * failure happens DURING the first drawn frame, and clearing at the top of it would call a
         * crash a success and the automatic rollback would never fire.
         *
         * NOT IN A SMOKE RUN. A preflight is a second process the installer started to ask whether
         * the NEW binary runs; it must not reach into the pointer its parent is about to write.
         * See `updater::install::Spawn`. */
        self.passes = self.passes.saturating_add(1);
        if self.passes == 2 && self.smoke.is_none() {
            if let Some(l) = grimoire_desktop::updater::install::Layout::platform() {
                if let Err(e) = grimoire_desktop::updater::install::note_first_frame(&l) {
                    log::warn!("could not record that this build is running: {e}");
                }
            }
        }

        /* THE WINDOW TAKES ITS QUADRANT AS SOON AS THERE IS A SCREEN TO MEASURE.
         *
         * IN THE HEARTBEAT AND NOT IN `ui`, which is the rule this file has had to learn twice
         * already (`Hotkeys::poll` and `Ingest::tail` both shipped in `ui` and both were wrong):
         * eframe runs `ui` only when something is being drawn, and an app launched straight to the
         * tray draws no frame at all. A window that sized itself only while visible would open at
         * the fallback and stay there.
         *
         * IT COSTS ONE BRANCH AFTER THE FIRST FRAME: see `App::sized`. */
        self.fit_to_quadrant(ctx);
        /* THE SAVED SIGN-IN, PICKED UP ONCE. In the heartbeat and not in `ui` because the
         * heartbeat is the one thing both eframe callbacks reach, and an app launched straight
         * to the tray draws no frame at all; a restore that lived in `ui` would never run there.
         * `restore` is silent and spawns nothing when there is nothing saved, which is the first
         * launch and every launch off Windows. */
        if !self.restored {
            self.restored = true;
            self.auth.restore(ctx);
        }
        for t in self.hotkeys.poll(ctx) {
            self.windows.summon(t);
        }
        self.windows.set_hint(self.hotkeys.leader_hint());

        /* THE LOG IS READ HERE, AND IT USED TO BE READ ONLY WHILE SOMETHING WAS BEING DRAWN.
         *
         * # THE DEFECT, IN THE OWNER'S WORDS: "its not updating in real fucking time"
         *
         * `Ingest::tail` was called once per frame from `ui`. eframe runs `ui` only while there
         * is something to draw, and `logic` is the callback it runs when there is not -- the doc
         * on `logic` says exactly that, and it exists because `Hotkeys::poll` had this same
         * problem first. So while the reader is IN THE GAME, with this window behind it, `ui`
         * does not run: the file is never read, `Ingest::live` is never re-folded, and the Live
         * header, the overlays and every counter sit frozen at whatever they held when the window
         * last painted. They catch up in one jump the moment it comes forward, which is precisely
         * how a person notices: the numbers are stale exactly when they are being played over.
         *
         * AND AN ALWAYS-ON-TOP OVERLAY IS THE WHOLE POINT OF THIS APP. It is read while the game
         * has focus, by definition. A meter that only advances when its own window is in front is
         * a meter that never advances when anybody is using it.
         *
         * THE COMMENT AT THE OLD CALL SITE ARGUED ITSELF MOST OF THE WAY HERE -- "or the tail
         * stops moving whenever another screen is showing" -- and stopped one step short of "or
         * whenever no screen is showing".
         *
         * `heartbeat` IS THE RIGHT HOME BECAUSE IT IS THE ONE THING BOTH CALLBACKS REACH, which
         * is what its own doc says it is for. `tail` is cheap when nothing is due: it takes an
         * `Instant` and returns zero inside `TAIL_POLL`. */
        if self.ingest.tail() > 0 {
            ctx.request_repaint();
        }

        /* THE UPDATER IS PUMPED HERE FOR THE REASON THE PARAGRAPH ABOVE SPENDS TWENTY LINES ON,
         * and it is the third thing in this file to learn it. `pump` hands the worker the three
         * facts only the UI thread has: what the encounter is doing, whether the snapshot has
         * finished loading, and what the settings block says. A reader who is IN THE GAME with
         * this window behind it draws no frames at all, and that is exactly the state in which
         * the fight gate matters most: a `pump` in `ui` would leave the worker looking at a
         * pulse from whenever the window was last in front, which is to say at a stale answer to
         * the one question the gate exists to ask.
         *
         * IT IS CHEAP. Two atomic stores and a mutex that is compared before it is written
         * (`Updater::pump`), never held across a network call or a draw. */
        if let Some(u) = &self.updater {
            u.pump(
                self.ingest.pulse(),
                self.data.loading().is_none(),
                &self.settings.updater,
            );
        }
    }
}

impl eframe::App for App {
    /// THE FRAME EFRAME RUNS WHEN THERE IS NOTHING TO DRAW, and until this existed the app had
    /// no such frame at all.
    ///
    /// THE BUG THIS FIXES, MEASURED RATHER THAN SUSPECTED. `Hotkeys::poll` lived inside `ui`.
    /// eframe calls `ui` only while `show_ui` is true, and `show_ui` is
    /// `is_visible || is_viewport_or_descendant_visible(..)` (glow_integration.rs:622); a
    /// minimized root reads not-visible (viewport_info.rs:95-101) and with no tool window open
    /// the second half is false too, so eframe calls `update_logic_only` -> `App::logic`
    /// (glow_integration.rs:637-649), which was eframe's own empty default (epi.rs:167-169)
    /// because this file implemented only `ui` and `on_exit`.
    ///
    /// So the OS receiver stopped being drained in the one state a GLOBAL hotkey is for. And the
    /// app minimizes ITSELF on the main window's toggle (`windows::Windows::wake`), so pressing
    /// Ctrl+Alt+G to put the app away made Ctrl+Alt+G unable to bring it back. `hotkeys.rs`
    /// asserted the opposite in prose: "eframe keeps running the root logic in those states,
    /// which is what makes Ctrl+Alt+G able to bring the window back". It did not.
    ///
    /// WHY THE ROOT IS RAISED FOR A TOOL. A tool window's viewport is built by
    /// `show_viewport_deferred` inside the root's pass, so a tool summoned from here has nowhere
    /// to be built until a pass happens, and no repaint request can cause one: `show_ui` is
    /// computed from visibility, never from the repaint flag. Raising the root is the only way
    /// to honour the press at all, so it is done deliberately and said out loud here rather than
    /// leaving the hotkey silently dead.
    ///
    /// AND IT IS ASKED OF `tool_pending`, WHICH IS A NARROWER QUESTION THAN IT LOOKS. It is not
    /// "is a tool window open": answering that made the main window impossible to minimize while
    /// any pop-out was open, because this callback runs interleaved with `ui` rather than only
    /// instead of it, and so it restored the root on every pass. It is "has a tool window asked
    /// for something that no root pass has carried out yet", which is true for the press that
    /// needs this and false a pass later. See `windows::Window::serviced`.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.heartbeat(ctx);
        /* THE SMOKE CLOCK RUNS HERE TOO, AND THAT IS WHAT STOPS A PREFLIGHT HANGING THE UPDATER.
         *
         * `smoke` was called from `ui` only, and `ui` runs only while the viewport is visible. A
         * preflight is a launch whose window is most likely NOT to become visible: the reader
         * presses Install while EverQuest is fullscreen-exclusive, or is on an RDP session, or the
         * new window opens minimized. In every one of those the child never reached its own
         * deadline and never exited, and the parent's `Command::output()` waited for it for ever:
         * the update worker stopped, the Settings screen froze, and quitting Grimoire left an
         * orphan process with no window the reader could find.
         *
         * The parent half of the fix is a bounded wait that kills (`install::wait_within`). This is
         * the half that makes the child exit in the one state the preflight is most likely to hit,
         * which is better than being killed. `smoke` latches on `fired`, so being called from both
         * callbacks costs one comparison. */
        self.smoke(ctx);
        self.windows.wake(ctx);
        if self.windows.tool_pending() {
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Minimized(false),
            );
        }
    }

    /* egui 0.36 hands the app a Ui rather than a Context, and panels attach to that Ui instead of
     * to the context. Same layout, different plumbing. */
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;
        self.smoke(&ctx);

        /* THE FAILED-LAUNCH COUNT IS CLEARED IN `heartbeat` AND NOT HERE ANY MORE. See the note
         * there: this callback runs only while the viewport is visible, and two launches that
         * never became visible used to leave a perfectly good build looking like a broken one. */

        /* WARM THE ARTWORK EVERY FRAME, WHATEVER SCREEN IS SHOWING, and the reason is a defect
         * the owner saw before any test did: the picture was there about half the time.
         *
         * The fetch used to start on the first frame the WATCH screen drew, so every visit raced a
         * cold network request and whether you got a picture depended on how long you spent
         * getting there. The disk cache never warmed either: a run that never reached Watch never
         * fetched, so it never wrote one, so the next run was cold again. Two launches in a row
         * could look different for no reason a person could see.
         *
         * These are the same requests the Watch screen would have made, moved to a moment when
         * nobody is waiting on them. The first run fills the cache and every run after it paints
         * from disk.
         *
         * IT IS CHEAP TO REPEAT, so there is no first-frame flag here to get wrong. The call is a
         * mutex, a match and a try_recv; once the art is in the slot it clones it and starts
         * nothing, and it starts at most one fetch per platform per run either way. The answer is
         * discarded because this call is here for the side effect; the Watch screen asks again
         * when it actually has somewhere to paint it. */
        let _ = grimoire_desktop::channel_art::artwork(&ctx, self.settings.watch_on);

        /* Background work, collected. None of it blocks: the snapshot arrives on a channel, the
         * watcher's status is a clone from under a mutex, the ingest reads only appended bytes and
         * rate limits itself to once a second, the hotkey receiver is a try_recv, the corpus
         * reader answers on a channel. */
        self.data.poll();
        /* The Settings screen's DATA section reads this so a parse in progress is drawn as
         * WORKING there too, never as "no snapshot loaded" for the length of the parse. */
        self.settings_screen.data_loading = self
            .data
            .loading()
            .map(|(root, for_)| (root.to_owned(), for_));
        /* AND THE UPDATER'S STATE, THE SAME WAY AND FOR THE SAME REASON. One small struct cloned
         * out from under the worker's mutex, so the UPDATES section draws what is true this frame
         * instead of holding a lock across a draw. `Cx` deliberately carries no updater: a screen
         * that could reach it could start a download from inside a draw. */
        self.settings_screen.updater = self.updater.as_ref().map(|u| u.view());
        self.live = self.watcher.status();
        /* THE POLL IS IN `heartbeat` NOW, which both eframe callbacks reach; see the note there.
         * Calling it again here would be harmless (it rate-limits itself) and would say something
         * false about where the work happens. */
        /* THE CLASS READING, AGAINST THE SNAPSHOT THE APP OWNS. `Ingest` keeps the cast book
         * because it reads the lines; the corpus that turns spells into classes is the App's,
         * so the two meet here. It returns at once unless somebody cast something new. */
        if let Data::Loaded(snap) = &self.data {
            self.ingest.resolve_classes(&snap.spells);
        }
        if self.screens.commission.poll() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.heartbeat(&ctx);

        /* The frame proper. Everything the screens draw with is borrowed into one `Cx` for the
         * length of this block, and what they asked for comes out of it at the end. */
        let mut asks: Vec<Ask> = Vec::new();
        let mut pop_out: Option<Tool> = None;
        /* AND WHICH PAGE THE PRESS CAME FROM, WHICH ONLY THIS FUNCTION KNOWS.
         *
         * `Tool::Parser` carries no page, deliberately: a `Tool` is a hotkey's currency and
         * `Ctrl+Alt+P`, pressed from inside the game, is not pressed from a page at all (see
         * `Windows::open_at`). So the page is a fact about the CALL, and the only place that holds
         * it is the frame that drew the control: the body being shown and the section of it the
         * rail has selected. Without this the picture in picture control, whose hover says "put
         * THIS in its own window", handed back `ParserWindow::default`, which is Dashboards, from
         * every page of the parser and from both journals. */
        let mut pop_out_page: Option<&'static str> = None;
        let stage: Option<player::Stage>;
        /* WHAT THE WATCH SCREEN WOULD PLAY, whichever window ends up playing it. See `Cx::demand`
         * and `player::choose_stage`. */
        let demand: Option<(player::Feed, bool)>;

        /* THE TWO FACTS A POP-OUT'S PASS IS ALLOWED TO KNOW ABOUT THE SURFACE, handed over before
         * that pass runs. It can act on neither: it cannot reach the `Player` and cannot name a
         * `Feed`, so "which window is playing" stays a question answered in one place. */
        self.windows
            .tell_player(self.player.root_hwnd(), self.player.view().hosted_elsewhere);
        {
            let App {
                open,
                body,
                tabs,
                section,
                settings,
                settings_screen,
                data,
                live,
                ingest,
                player,
                windows,
                screens,
                persona: persona_foot,
                finder,
                chat,
                auth,
                ytchat,
                ..
            } = self;
            let mut cx = Cx {
                data: data.snapshot(),
                railed: railed_here(*body),
                data_err: data.err(),
                live: &*live,
                settings,
                ingest,
                /* THE LOG TRAVELS AND THE THREAD DOES NOT. See `chat::ChatHandle`. */
                chat: chat.handle(),
                /* THE LOG TRAVELS, THE PANE DOES NOT. See `ytchat::surface::YtHandle`. */
                yt: ytchat.handle(),
                yt_wanted: false,
                chat_wanted: false,
                /* A VIEW, NOT THE `Auth`. The token stays behind `Auth::with_token`. */
                auth: auth.view(),
                auth_begin: false,
                auth_cancel: false,
                player: player.view(),
                stage: None,
                demand: None,
                ask: Ask::None,
            };
            /* The rail's squares, decided once for every row from the same facts. The loader flag
             * and the corpus state are the App's alone: `facts` cannot see either through `Cx`. */
            let mut facts = nav::facts(&cx);
            facts.data_loading = data.loading().is_some();
            facts.corpus = screens.commission.corpus();

            /* The title strip, full width, first. D8: the live pill lives here; D3: the pin glyph
             * shows the level last pushed to the OS (`Windows::is_pinned`), which mirrors
             * `settings.always_on_top` once the registry has run a pass, and a click hands the
             * registry the new value, which saves it and pushes the window level. */
            let mut hits = titlebar::Hits::default();
            let pinned = windows.is_pinned(Tool::Companion);
            egui::Panel::top("strip")
                .exact_size(titlebar::STRIP_H)
                .resizable(false)
                .frame(egui::Frame::NONE.fill(PANEL))
                .show(ui, |ui| {
                    titlebar::strip(
                        ui,
                        titlebar::Lead::Maker,
                        cx.live,
                        cx.settings.watch_on,
                        pinned,
                        &mut hits,
                    );
                });
            if hits.pin {
                windows.pin(Tool::Companion, !pinned);
            }
            /* THE PILL IN THE TITLE STRIP, CLICKED. It goes on the frame's ask list rather than
             * calling `answer` here, because `self` is destructured for the length of this block
             * and the App's own routing runs after it. Same road every other ask takes. */
            if hits.watch {
                asks.push(Ask::WatchHere);
            }
            if hits.close {
                /* The strip has already sent Close to the root, which quits. Write anything the
                 * Settings screen was still debouncing before the process goes. */
                settings_screen.flush(cx.settings);
            }

            let mut pending: Option<Body> = None;
            let mut toggle: Option<usize> = None;
            let mut pick_section: Option<(ScreenId, usize)> = None;
            /* Every decision the rail makes, made here, off the facts and the ingest, before the
             * panels open. `rail_list` paints it and decides nothing. See `rail_plan`. */
            let plan = rail_plan(open, *body, section, &facts);

            egui::Panel::left("rail")
                .exact_size(chrome::RAIL_WIDE)
                .resizable(false)
                .frame(egui::Frame::NONE.fill(PANEL))
                .show(ui, |ui| {
                    chrome::brand(ui, false);
                    let ask = rail_list(ui, &plan);
                    toggle = ask.toggle;
                    pending = ask.body;
                    pick_section = ask.section;
                    /* The floor of the rail. NOTHING HERE IS INVENTED, and every field is written
                     * at this one site with no rest pattern, which is what
                     * `persona::tests::every_persona_field_is_filled_by_the_app` reads it to check:
                     * round one carried a standing seal and an alert bell whose fields nothing here
                     * ever wrote, so both drew their absent state forever.
                     *
                     *   name      the character on the log the ingest is tailing, None until one
                     *             has been read, and the footer says that absence in words.
                     *   server    the server off the same file name, by a different pattern, so it
                     *             can be present or absent independently of the name. Passed
                     *             through verbatim; the footer does not title case it.
                     *   standing  the ENGINE'S value and not one made up here.
                     *             `grimoire_core::Standing::UNPROVEN` is 3.0 on zero ratings, which
                     *             is where the engine says a hand nobody has rated stands. Nothing
                     *             in this build writes a rating yet, so the footer draws its
                     *             unproven face on every launch, and it says so in words and in the
                     *             ring. It is written here rather than left to `Default` because a
                     *             field this site does not name is a field nobody notices going
                     *             unwritten. `Persona` no longer HAS a `Default` for the same
                     *             reason.
                     *   settings  `settings_state`, the fact the deleted Settings row used to
                     *             carry: the file would not load, or a chord did not register.
                     *             The gear takes its colour from this (`persona::gear_ink`), so
                     *             the one control that still opens Settings is the one that says
                     *             something is wrong with them. */
                    persona::push_to_floor(ui);
                    let who = Persona {
                        name: cx.ingest.active_character().map(str::to_owned),
                        server: cx.ingest.active_log().and_then(|f| f.server.clone()),
                        standing: grimoire_core::Standing::UNPROVEN,
                        settings: settings_state(cx.settings, &settings_screen.bindings),
                    };
                    if let Some(next) = persona_pending(persona_foot.ui(ui, &who)) {
                        pending = Some(next);
                    }
                });
            if let Some(si) = toggle {
                toggle_head(open, si);
            }

            /* A NESTED SECTION WAS PRESSED. It changes the workspace's view and never the body:
             * you are already on the destination, or its sections would not be drawn. */
            if let Some((id, i)) = pick_section {
                section[id.ordinal()] = i;
                /* AND ITS TABS START AT THE FIRST. Tier 4 belongs to the SECTION, so the slot
                 * left behind by the last one indexes a different list: Roster's fourth tab is
                 * Raid Readiness and Calendar has no fourth at all. Carrying it over would land
                 * on an unrelated tab at best and past the end at worst. */
                tabs[id.ordinal()] = 0;
                on_section(id, i, screens);
                /* AND THE RESET IS ROUTED, WHICH IT WAS NOT.
                 *
                 * `tabs[..] = 0` moves the context bar's highlight to the first tab and nothing
                 * told the page. Press Dashboards, pick `Custom`, press Reports, press
                 * Dashboards again: the bar lights `DPS` and the page draws Custom, because
                 * `DashboardsScreen::role` still held the last one. Two controls on one screen
                 * disagreeing about which view is showing, and the reader has to press a tab
                 * twice to get back in step.
                 *
                 * THROUGH `on_tab` AND THE SAME HOP EVERYTHING ELSE USES, so the reset lands on
                 * the screen the section opens rather than on the body. */
                on_tab(target_of(id, i), 0, screens);
            }

            if let Some(next) = pending {
                enter(next, body, tabs, section, screens);
            }

            /* The context bar. LEFT TO RIGHT: the breadcrumb saying where you are, the
             * picture-in-picture control where the row has a tool window, the tabs where a
             * section has more than one view, and the WATCH screen's playback controls where
             * that is the screen showing. RIGHT TO LEFT: the lock and the Widgets button, on the
             * one page whose layout the reader owns.
             *
             * # THE FIND BOX IS GONE FROM THIS ROW AND THE SEARCH IS NOT
             *
             * A 240 pixel text field sat on the right of this bar on every screen in the app,
             * and the owner's ruling on it was not gentle. It is off the header. It is NOT
             * deleted: `finder_ui` draws the same search as a palette over the page, opened with
             * Ctrl+K and closed with Escape, which is where a cross-kind jump box belongs and is
             * how every editor built in the last decade offers one.
             *
             * DELETING IT OUTRIGHT WAS THE OTHER OPTION AND IT WAS THE WRONG ONE. `data::search`,
             * `data::parse_query`, `HitKind` and `SEARCH_CAP` are a real capability with their
             * own tests, and a library function whose last caller was removed to tidy a header is
             * this tree's signature defect arriving through the front door.
             *
             * WHY A SCREEN'S OWN CONTROLS ARE IN THE APP'S BAR, WHICH IS NOT WHERE THEY BELONG BY
             * DEFAULT. The Watch folio is the video, edge to edge, and nothing may be drawn over a
             * native child window: egui paints UNDER it, so a control on the video is not on top of
             * it, it is gone. A control row above the folio would mean the folio is not edge to
             * edge, which is the whole of what the owner asked for. This bar is what is left. It is
             * the ONE exception, and `screens::watch::WatchScreen::header` is the one function
             * allowed in here; nothing else in this match may grow a screen of its own.
             *
             * THE CONTROLS COME BEFORE POP OUT ON PURPOSE. They belong to the screen and Pop out
             * belongs to the window, so the screen's own controls sit next to the screen's name.
             * Nothing moves for any other screen: every other body draws no controls here at all,
             * so Pop out is where it always was. */
            let tabs_here: &[&str] = match *body {
                Body::Screen(id) => nav::tabs_of(id, section[id.ordinal()]),
                Body::Settings => &[],
            };
            /* WHICH SCREEN THIS TAB ROW BELONGS TO, by the same hop the router and the painter
             * make. Needed here because the layout controls on the right of this row belong to
             * ONE screen and the body is not it: LOG PARSER / Dashboards paints
             * `ParserDashboards` while `Body::Screen` stays `Parser`. */
            let tabs_for: Option<ScreenId> = match *body {
                Body::Screen(id) => Some(target_of(id, section[id.ordinal()])),
                Body::Settings => None,
            };
            let tab_slot: Option<usize> = match *body {
                Body::Screen(id) => Some(id.ordinal()),
                Body::Settings => None,
            };
            let mut tab = tab_in_range(tab_slot.map(|i| tabs[i]).unwrap_or(0), tabs_here);
            let tab_before = tab;
            let tool = match *body {
                Body::Screen(id) => tool_of(id, screens),
                Body::Settings => None,
            };
            let watch_body = is_watch(*body);
            let videos_body = is_videos(*body);
            let mut picked: Option<Hit> = None;
            egui::Panel::top("ctx")
                .frame(egui::Frame::NONE.fill(INK).inner_margin(egui::Margin::symmetric(0, 8)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        /* WHERE YOU ARE, FIRST, and it comes off `NAV` and `SECTIONS` rather
                         * than being spelled here: see `chrome::crumb`. */
                        ui.add_space(14.0);
                        chrome::crumb(ui, &crumb_of(*body, section));

                        /* THE PICTURE IN PICTURE CONTROL, next to the name of the thing it would
                         * put in a window. It was the words `Pop out` on the far side of the
                         * tabs; the door is identical and only the shape changed. */
                        if let Some(t) = tool.filter(|_| offers_pop_out(*body, section)) {
                            ui.add_space(8.0);
                            let is_open = windows.is_open(t);
                            let tip = if is_open {
                                format!("bring the {} window to the front", t.title())
                            } else {
                                format!("put this in its own window, titled {}, which can be pinned on top of the game with the pin in its title bar", t.title())
                            };
                            if chrome::pip(ui, is_open).on_hover_text(tip).clicked() {
                                pop_out = Some(t);
                                pop_out_page = pop_out_page_of(*body, section);
                            }
                        }

                        /* AND THEN THE TABS, where the section showing has more than one view.
                         * Dashboards no longer has any: its eight roles are gone and the two
                         * controls that replaced them act on the page's LAYOUT rather than
                         * switching between views of it, so they are on the right of this row
                         * with the other window-scoped controls and not in this list. */
                        if !tabs_here.is_empty() {
                            ui.add_space(6.0);
                            chrome::context_bar(ui, tabs_here, &mut tab);
                        }
                        if watch_body {
                            screens.watch.header(ui, &mut cx);
                        }
                        /* The Videos screen's own control, for the same mechanical reason: its
                         * folio is a webview surface edge to edge, and nothing may be drawn over a
                         * native child window. `else if` and not a second `if`, because the two
                         * predicates are exclusive by construction and a bar that ever drew both
                         * would be two screens' controls on one row. */
                        else if videos_body {
                            screens::videos::header(ui, &mut cx);
                        }
                        /* THE RIGHT HAND SIDE, WHICH BELONGS TO THE PAGE'S LAYOUT.
                         *
                         * ONLY DASHBOARDS DRAWS THESE, and that is not a special case sneaking
                         * into the shell: they are the controls for a page whose arrangement the
                         * READER owns, and it is the only such page. Every other screen's layout
                         * is the build's, so a lock over one of those would be a control with
                         * nothing to hold.
                         *
                         * RIGHT TO LEFT, SO THE LOCK IS THE OUTERMOST, exactly as the owner laid
                         * it out: a lock on the right, to the left of that a Widgets button. The
                         * mock has the same pair in the same order, as `Layout locked` and
                         * `+ Widgets`, and this row is those two controls and no others.
                         *
                         * THE HOP IS `target_of`'s AND NOT A GUESS ABOUT THE BODY. `tabs_for` is
                         * the screen this section actually paints, which for LOG PARSER /
                         * Dashboards is `ParserDashboards` while the body stays `Parser`. Asking
                         * the body would have put these on Live, Fights, Reports and Logs too. */
                        if tabs_for == Some(ScreenId::ParserDashboards) {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(14.0);
                                ui.spacing_mut().item_spacing.x = 7.0;
                                let locked = screens.dashboards.locked();
                                let tip = if locked {
                                    "The dashboard is pinned. Unlock it to drag a widget's heading to move it, or any edge or corner to resize it."
                                } else {
                                    "The dashboard can be rearranged. Lock it so a stray click cannot move anything."
                                };
                                if chrome::lock(ui, locked).on_hover_text(tip).clicked() {
                                    screens.dashboards.toggle_lock();
                                }
                                /* THE FILTER KEY, where the owner asked for it: the corner, not a
                                 * row of dates across the head. It says what is being looked at
                                 * on its hover, so the corner still answers the question the
                                 * chips answered without spending the head on it. */
                                let filtering = screens.dashboards.filtering();
                                if chrome::ghost_btn(ui, "Filter", filtering)
                                    .on_hover_text(
                                        "Which night, which zone, which mob, and how many of them. A raid night is a night and a zone: nothing in a log line says a fight was a raid.",
                                    )
                                    .clicked()
                                {
                                    screens.dashboards.toggle_filter();
                                }
                                let picking = screens.dashboards.picking();
                                if chrome::ghost_btn(ui, "+ Widgets", picking)
                                    .on_hover_text(
                                        "Every widget this build has, with what each one reads. Add one to the dashboard or take one off. Opening this unlocks the page.",
                                    )
                                    .clicked()
                                {
                                    screens.dashboards.toggle_picker();
                                }
                            });
                        }
                    });
                });
            /* THE FIND PALETTE (D6), OVER THE PAGE AND NOT ON THE HEADER.
             *
             * Ctrl+K OPENS IT AND ESCAPE CLOSES IT, which is the shape every editor written in
             * the last decade uses for exactly this control, and it is why the header does not
             * have to carry a permanently visible box for a search that is used once an hour.
             *
             * `command` AND NOT `ctrl`, because egui folds the platform's own modifier into that
             * flag; on Windows it IS Ctrl and on a Mac it would be Cmd, which is what a reader on
             * either machine expects without this file knowing which one he is on.
             *
             * THE BOX IS `finder_ui`, UNCHANGED. Same search, same prefixes, same routing through
             * `ask_for`: what moved is where it is drawn, and nothing about what it does.
             *
             * DRAWN HERE AND NOT INSIDE THE BAR because it is no longer part of the bar: an Area
             * over the body belongs to the frame, and putting it inside the panel's closure would
             * tie its position to a row it is deliberately not in any more. */
            if ui.input(|i| i.key_pressed(egui::Key::K) && i.modifiers.command) {
                finder.open = !finder.open;
                finder.query.clear();
            }
            if finder.open && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                finder.open = false;
                finder.query.clear();
            }
            if finder.open {
                let screen = ui.max_rect();
                egui::Area::new(egui::Id::new("grimoire.palette"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(egui::pos2(screen.center().x - 220.0, screen.top() + 110.0))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::NONE
                            .fill(PANEL)
                            .stroke(Stroke::new(1.0, GOLD_DEEP))
                            .corner_radius(crate::theme::RADIUS)
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_min_width(440.0);
                                picked = finder_ui(ui, finder, cx.data);
                            });
                    });
                if picked.is_some() {
                    finder.open = false;
                }
            }

            if let Some(i) = tab_slot {
                tabs[i] = tab;
            }
            if tab != tab_before {
                if let Body::Screen(id) = *body {
                    on_view(id, section[id.ordinal()], tab, screens);
                }
            }
            if let Some(h) = &picked {
                asks.push(ask_for(h));
            }

            /* The footer: version, licence, source (D8) and the snapshot's count. NO PILL.
             *
             * IT DREW ONE, AND IT WAS THE SAME CALL AS THE TITLE STRIP'S. Literally
             * `titlebar::pill` handed `ui`, `cx.live` and `cx.settings.watch_on`: the same
             * function with the same three arguments as the strip makes a hundred lines above, in
             * the same frame, on every
             * screen. Two pills that read one set of inputs cannot disagree, so the second was the
             * app saying one thing twice, top and bottom, for the life of every screen. On the
             * Watch screen, which had a THIRD in its own body, an offline channel was announced
             * four times before the reader reached a control.
             *
             * D8 IS WHERE THE CHOICE OF SURVIVOR COMES FROM, not from taste. "Title bar carries
             * the live pill", and its footer is spelled out as `v{version} · <licence> · Source on
             * GitHub` with no pill in the list. `web/app.html`, the authority for anything visual,
             * has no pill anywhere and no bottom bar of this kind at all, so there was never an
             * original to copy this from.
             *
             * AND IT WAS NOT A FALLBACK FOR A SQUEEZED ONE. `strip` clamps its pill when a window
             * is too narrow for the lead, the pill and the controls at once, but this window's
             * `min_inner_size` is 880 wide and the pill needs about 150 of it, so the strip's is
             * drawn whole at every size this window can be. `the_strip_pill_is_never_squeezed_in
             * _the_main_window` in titlebar.rs measures that against this file's own number.
             *
             * NOTHING ELSE WENT. The click was `Ask::WatchHere`; the strip's pill sends the same
             * ask from the same row of every window, including this one. */
            egui::Panel::bottom("foot")
                .frame(
                    egui::Frame::NONE
                        .fill(PANEL)
                        .inner_margin(egui::Margin::symmetric(14, 6)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        titlebar::footer_left(ui);
                        ui.add_space(18.0);
                        ui.label(
                            RichText::new(data.footer_line())
                                .font(FontId::monospace(10.5))
                                .color(TEXT_3),
                        );
                    });
                });

            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(INK)
                        .inner_margin(folio_margin(*body)),
                )
                .show(ui, |ui| match *body {
                    Body::Settings => settings_screen.ui(ui, &mut cx),
                    Body::Screen(id) => match data.loading() {
                        /* Every screen that reads the snapshot waits on the loading notice, not
                         * only the ones whose rail square follows the snapshot: a CHARACTER row
                         * printing "no snapshot loaded, put gear-data.json in data/" while the
                         * file is being parsed would be a lie for the length of the parse. */
                        Some((root, for_)) if reads_data(id) => {
                            screens::loading_notice(ui, nav::label(id), root, for_)
                        }
                        _ => draw_screen(
                            ui,
                            id,
                            section[id.ordinal()],
                            tabs[id.ordinal()],
                            screens,
                            &mut cx,
                        ),
                    },
                });

            /* THE STAGE IS TAKEN HERE, between the body and the tool windows, and the order is the
             * point. `eframe::Frame` carries the ROOT window's handle even inside a deferred
             * viewport's callback, so a surface asked for by a pop-out would land over the main
             * window's body. Taking the stage BEFORE `windows.show` runs is what makes that
             * impossible rather than merely discouraged: whatever a tool window's pass writes
             * into `Cx::stage` is written into a slot this frame has already emptied and will
             * not read again. No tool window asks for one today, and this is why it would not
             * matter if one started. */
            stage = cx.stage.take();
            demand = cx.demand.take();

            /* Every open tool window, from the root pass. MOVED ABOVE THE CHAT START deliberately:
             * the Chat screen is drawn in a pop-out as well as in the body, and a pop-out's ask has
             * to reach the one place allowed to dial. `Windows::show` ors what its windows asked
             * into `cx.chat_wanted` before it returns. */
            windows.show(&ctx, &mut cx);

            /* WHAT AN OVERLAY WINDOW CHANGED ABOUT ITSELF, WRITTEN BACK.
             *
             * A deferred viewport cannot reach Settings: it runs on its own clock holding the
             * registry lock. So a chip toggling the pin, the OS close flipping open, and a drag
             * changing the remembered size all land on the window and are collected here, which
             * is the only pass that holds both the registry and the settings.
             *
             * MATCHED BY id. The list may have been reordered or shortened by the Parser page on
             * the same frame, and a positional write would then land on the wrong overlay.
             */
            let edits = windows.overlay_edits();
            if !edits.is_empty() {
                /* THE STORED LIST AND NOT THE RESOLVED ONE, AND THAT IS THE WHOLE POINT.
                 *
                 * This read `or_default(&cx.settings.overlays)`, which MATERIALISES the shipped
                 * overlay when the file holds none, and then wrote the result back. A drag, a
                 * resize, a pin or an OS close therefore froze the shipped widget list into the
                 * settings file as though the person had chosen it, and from then on no change
                 * to the shipped default could ever reach him again. Measured on the owner's
                 * own machine on 2026-09-08 and written out on `overlay::Overlay::widgets`.
                 *
                 * AN EDIT FOR AN OVERLAY THE FILE HAS NEVER HEARD OF IS STILL STORED, because
                 * he really did move that window and it must come back where he put it. It is
                 * stored with the geometry he chose and `widgets: None`, which is the truth:
                 * he moved a window, he did not pick its contents. */
                cx.settings.overlays =
                    grimoire_desktop::overlay::apply_edits(&cx.settings.overlays, edits);
                if let Err(err) = cx.settings.save() {
                    log::warn!("an overlay change was not saved: {err}");
                }
            }

            /* THE CHAT SOCKET, ASKED FOR BY EITHER WINDOW AND OPENED HERE.
             *
             * `start` spawns a thread that dials Twitch, and the Chat screen must not be the
             * thing that calls it: four tests below draw every screen of every destination to
             * prove the router reaches them, and a screen that dialled on draw would have
             * `cargo test` opening real sockets to a real service on every machine that runs it.
             * So the screen states that it wants lines, exactly as it states where the video goes
             * (`Cx::stage`), and the App is what acts.
             *
             * ASKED EVERY FRAME AND DIALLED ONCE. `start` swaps an atomic and returns on the
             * second and every later call, so this costs nothing after the first frame the screen
             * is visible, and NOT asking (any frame that draws another screen) never hangs up: a
             * reader that is running keeps reading whether or not anyone is looking at it, which
             * is what makes the backlog there when you come back to the row. */
            if cx.chat_wanted {
                chat.start(&ctx, grimoire_desktop::settings::TWITCH_HANDLE);
            }
            /* THE SIGN-IN, ASKED FOR BY THE SCREEN AND STARTED HERE, for the reason every other
             * ask in this function has: the thread owner lives on the App and a screen may only
             * state what it wants. `begin` is idempotent while one is already running, so a
             * button held down does not start six of them. */
            /* THE SIGN-IN, HANDED TO THE CHAT READER. Here and not in the Auth thread because
             * this is the one place that holds both, and because `with_token` deliberately lends
             * the token rather than returning it: it cannot be copied into anything, only used
             * inside the call. `set_creds` is idempotent on the same pair, so doing this every
             * frame costs an equality check and never a reconnect. */
            if let AuthView::In { login } = &cx.auth {
                auth.with_token(|t| {
                    if let Some(t) = t {
                        chat.set_creds(chat::Creds::new(login, t));
                    }
                });
            }
            /* THE PAGE STEPS ASIDE. The flow keeps running; see `Cx::auth_cancel`. */
            if cx.auth_cancel {
                auth.unwatch();
            }
            /* THE YOUTUBE FEED, ASKED FOR BY THE SCREEN AND LOADED HERE.
             *
             * THE VIDEO ID IS THE WATCHER'S AND NOT A GUESS. A YouTube live chat page is addressed
             * by the VIDEO, not by the channel, so there is nothing to load until the poller has
             * found one; `Channel::video_id` is `None` whenever he is not live there, and asking
             * for the feed then is asking for a page that does not exist. `start` is idempotent on
             * the same id, so this costs a string compare per frame and reloads when he goes live
             * again on a new video. */
            if cx.yt_wanted {
                if let Some(id) = live.youtube.video_id.clone() {
                    ytchat.start(&ctx, &id);
                }
            }
            if cx.auth_begin {
                auth.begin(
                    &ctx,
                    grimoire_desktop::settings::TWITCH_CLIENT_ID,
                    grimoire_desktop::settings::TWITCH_CHAT_SCOPES,
                );
            }

            let ask = std::mem::take(&mut cx.ask);
            if ask != Ask::None {
                asks.push(ask);
            }
        }

        /* WHICH WINDOW GETS IT. The pop-out published an offer during `windows.show` above, which
         * is why this is read here and not with the stage: the body draws first and the tool
         * windows after it. An offer names a window and the shapes the video must keep out of, and
         * nothing else; pairing it with the demand is what turns it into a seat.
         *
         * READ, NOT TAKEN. It was taken, and that hid the video on every root frame that fell
         * between two pop-out passes, which is most of them: see `Windows::pip_offer` for the
         * clocks, and the test named there for the frame by frame account. The offer lives as
         * long as the pop-out is open and the registry's `open` flag is what ends it. */
        /* AND THE OFFER IS REFUSED OUTRIGHT ON A MACHINE WHERE THE CHROME WOULD BE UNREACHABLE.
         *
         * The pop-out`s controls exist because the video WITHDRAWS from their rectangles, which
         * needs `SetWindowRgn` to be honoured for hit testing by whatever composites WebView2.
         * That was measured working here, but a runtime or a driver that stopped honouring it
         * would take the close button with it, and a floating window with no way to shut it is the
         * one thing this window may not become. `Player` probes it once, the first time it carves,
         * and answers false ever after if the probe failed.
         *
         * FAILING TO A WORKING APP BEATS FAILING TO A PRETTY ONE. The video simply stays in the
         * main window, where Stop, Sound and Check now all still work, and the pop-out goes back to
         * being a picture with two chips on it. */
        let offer = self
            .windows
            .pip_offer()
            .filter(|_| self.player.can_host_elsewhere());

        let stage = player::choose_stage(stage, offer, demand);

        /* THE SURFACE, PLACED. After layout, because the rectangle is not known until the screen
         * above it has drawn; before the asks, because Stop is one of them and a stop should not
         * wait a frame behind a placement it is about to undo. `None` hides the surface rather
         * than dropping it, so stepping to another screen and back does not restart the stream. */
        self.player.sync(frame, stage.as_ref());
        /* THE HIDDEN PANE, PLACED. After the player, and for the same reason it is here at all:
         * both are children of the ROOT window and `frame` is the only handle to it. This one
         * takes no rectangle from any screen; it sizes and hides itself. */
        self.ytchat.sync(frame, &ctx);

        for ask in asks {
            self.answer(ask);
        }
        if let Some(t) = pop_out {
            /* `open_at` AND NOT `open`, because this caller HAS a page. `open` is `open_at` with
             * no page and is still the right door for a hotkey, which names a window and nothing
             * else. Sending the press here is what makes `open_at`'s page argument reachable at
             * all: until this line it had no production caller, and an argument nothing passes is
             * this tree's signature defect wearing a tested function's clothes. */
            self.windows.open_at(t, pop_out_page);
        }

        /* What the Settings screen changed. It cannot act on these itself: the snapshot is lent
         * immutably, the ingest's rescan takes no new folder, the watcher has no hot reconfigure,
         * and the hotkey manager lives with the event loop. */
        let changed = self.settings_screen.take_changed();
        if changed.data_root {
            self.data = Data::start(&self.settings);
        }
        if changed.data_root || changed.log_dir {
            self.ingest.reconfigure(&self.settings);
        }
        /* `changed.handles` used to restart the poller here, because Settings carried two text
         * boxes that could repoint it. The handles are constants now, nothing can change them at
         * run time, and a restart nothing can ask for is a branch nothing can enter, so the flag
         * and this block went together. */
        /* THE UPDATES SECTION'S PRESSES. The two CONTROLS need nothing here: the checkbox and the
         * channel buttons write into `Settings` themselves and reach the worker through
         * `heartbeat`'s `pump`, which hands it the whole block every callback. These are the
         * three that ask the worker to DO something, and the screen cannot: it has no `Updater`,
         * on purpose. */
        if let Some(a) = changed.updater {
            if let Some(u) = &self.updater {
                u.ask(a);
            }
        }
        /* AND THE ONE THE WORKER CANNOT DO EITHER. It cannot close a window, and the executable
         * that has to be started is the ENTRY POINT rather than this one
         * (`updater::run::entry_point` says why). The close is asked for here and the handover
         * happens in `on_exit`, once everything this process owns has been given up. */
        if changed.restart {
            self.restart_at_exit = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if changed.hotkeys {
            /* D4: every binding can be changed on Settings. Release every global and register
             * the new table on the same manager, then hand the outcome back to the screen so a
             * chord that failed to register says so at once. */
            self.hotkeys.reinstall(&self.settings.hotkeys);
            self.settings_screen.set_bindings(self.hotkeys.bindings());
        } else if self.hotkeys.take_dirty() {
            /* An outcome changed between rebinds: a bare second key that would not register when
             * a leader armed, or a release that failed. Settings shows it now, not after the
             * next rebind. */
            self.settings_screen.set_bindings(self.hotkeys.bindings());
        }
        /* always_on_top has no flag: `Windows::show` reads the setting every pass and pushes the
         * window level when it differs from the one last applied. */

        /* The leader hint follows whatever is on screen (D4); the tool windows draw their own. */
        if let Some(h) = self.hotkeys.leader_hint() {
            hotkeys::draw_hint(&ctx, &h);
        }
        /* The frameless main window's resize grip, same as the tool windows'. */
        windows::resize_corner(&ctx, egui::Id::new("grimoire.resize.root"));
        /* The pill's age and the ingest's tail move on the clock, not on input. */
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        /* The Settings screen debounces its writes and only runs while drawn; a close from any
         * other screen would otherwise lose the last edit. */
        self.settings_screen.flush(&self.settings);
        /* AND A FIGHT NOTE TYPED AND NEVER BLURRED, for the same reason one line up.
         *
         * `screens::analysis` keeps the note being typed in its own buffer and writes it to
         * `Settings::fight_notes` when the reader leaves the field or changes fight. Quitting is
         * neither: nothing draws a frame after this, so the page cannot notice its own close and
         * the last thing typed went in the bin.
         *
         * THE TOOL WINDOW'S COPY IS FLUSHED BY THE REGISTRY, not here. Each Parser window holds
         * its own `ParserScreen` and therefore its own note buffer, and `windows::Windows` is
         * what knows one is closing. */
        self.screens.parser.flush_notes(&mut self.settings);

        /* THE RESTART, LAST, AFTER EVERY FLUSH ABOVE IT.
         *
         * AFTER, AND NOT AT THE PRESS. The new process reads the same settings file this one is
         * still writing two lines up, registers the same global hotkeys this one has not released
         * yet, and opens a window over this one's. Starting it here means the file is written, the
         * notes are in, and this process is a few microseconds from gone.
         *
         * A FAILURE HERE IS NOT A FAILURE OF THE UPDATE. The pointer flipped when the install
         * finished; the new version starts whenever the reader next opens the app, whatever
         * happens to this spawn. So it is logged and nothing else. */
        if self.restart_at_exit {
            match grimoire_desktop::updater::run::entry_point() {
                Some(exe) => match std::process::Command::new(&exe).spawn() {
                    Ok(_) => log::info!("restarting through {}", exe.display()),
                    Err(e) => log::warn!(
                        "could not restart through {}: {e}; the update is installed and starts at                          the next launch anyway",
                        exe.display()
                    ),
                },
                None => log::warn!(
                    "nothing to restart through; the update is installed and starts at the next                      launch anyway"
                ),
            }
        }
    }
}

/* ------------------------------------------------------------------- the tests --
 *
 * THIS FILE CARRIED NONE, AND THAT WAS THE FINDING. `cargo test` printed
 * "Running unittests src\main.rs ... 0 passed" beside a library suite of several hundred, so every
 * rule the App applies BETWEEN the modules, the rail's wiring, the settings state, the D5 two open
 * cap, the smoke switch, was covered by nothing at all. `App::ui` itself still cannot be called
 * from here: it takes an `&mut eframe::Frame`, which has no public constructor. That is exactly
 * why the joins were lifted out into `rail_plan`, `rail_list`, `persona_pending` and
 * `parse_smoke`. What a test cannot enter, it can at least be handed. */
#[cfg(test)]
mod tests {
    use super::*;
    /// DEFECT: THE DASHBOARD'S POP-OUT OPENED A LOG PARSER WINDOW OVER THE GAME.
    ///
    /// The owner pressed it on Dashboards and got a Log Parser window, which is not what that page
    /// is for. Every other page of the parser still offers its pop-out, and the header asks before
    /// it draws one.
    ///
    /// WHAT MUTATION MAKES THIS RED: `offers_pop_out` answering yes on Dashboards or no elsewhere,
    /// or the header drawing the pop-out without asking.
    #[test]
    fn the_dashboard_offers_no_pop_out_and_every_other_page_still_does() {
        let sections = nav::sections_of(ScreenId::Parser);
        let mut on_dashboards = 0;
        for (i, (name, id, _)) in sections.iter().enumerate() {
            let mut at = [0usize; ScreenId::ALL.len()];
            at[ScreenId::Parser.ordinal()] = i;
            let offered = offers_pop_out(Body::Screen(ScreenId::Parser), &at);
            if *id == Some(ScreenId::ParserDashboards) {
                on_dashboards += 1;
                assert!(
                    !offered,
                    "Dashboards still offers a pop-out onto a Log Parser window"
                );
            } else {
                assert!(offered, "LOG PARSER / {name} lost its pop-out");
            }
        }
        assert_eq!(
            on_dashboards, 1,
            "the parser has no Dashboards section to test"
        );
        assert!(offers_pop_out(
            Body::Screen(ScreenId::Sky),
            &[0usize; ScreenId::ALL.len()]
        ));

        let whole = strip_comments(include_str!("main.rs"));
        let src = whole
            .split("mod tests {")
            .next()
            .expect("the file has a body");
        assert!(
            src.contains("tool.filter(|_| offers_pop_out(*body, section))"),
            "the header draws the pop-out without asking whether this page offers one"
        );
    }

    /* THREE FIXTURES STOOD HERE AND ALL THREE ARE GONE WITH THE TESTS THAT NEEDED THEM.
     *
     * A `TempTree` game folder and a `pump_until` that drove the ingest went first: the badge
     * wiring test planted a log in that tree because the Sources row badged what the ingest found,
     * and that row became a section of the Settings screen. `quiet_status`, a watcher that had
     * never answered, and the whole `Cx` every rail test built to hold it went with the badge
     * itself: `rail_plan` asked `nav::count_of` for a number and needed a live context to do it,
     * and now it reads `nav::Facts` alone. A plan that is a pure function of the facts is a plan
     * a test can ask for in three lines, which is the shape those tests have now.
     *
     * The TempTree and pump_until pair still lives in `settings.rs`, where the surface that counts
     * source files moved to. */

    /* -------------------------------------------------- the rail's section headings -- */

    /// A SHUT SECTION LISTS NO ROWS AND AN OPEN ONE LISTS ALL OF THEM.
    ///
    /// Both halves, because a plan that returned every row whatever the state would satisfy the
    /// second on its own and a plan that returned none would satisfy the first.
    #[test]
    fn a_shut_section_lists_no_rows_and_an_open_one_lists_all_of_them() {
        let facts = nav::Facts::default();
        let none: Vec<usize> = Vec::new();
        let shut = rail_plan(&none, Body::Settings, &[0; ScreenId::ALL.len()], &facts);
        assert_eq!(shut.sections.len(), nav::NAV.len());
        for sec in &shut.sections {
            assert!(sec.rows.is_empty(), "{} listed rows while shut", sec.head);
            assert!(!sec.marks.open);
        }

        let all: Vec<usize> = (0..nav::NAV.len()).collect();
        let open = rail_plan(&all, Body::Settings, &[0; ScreenId::ALL.len()], &facts);
        for sec in &open.sections {
            assert_eq!(
                sec.rows.len(),
                nav::NAV[sec.index].1.len(),
                "{} did not list all of its rows",
                sec.head
            );
            assert!(sec.marks.open);
        }
    }

    /// EVERY ROW CARRIES ITS OWN LABEL, SQUARE AND SCREEN, and none of the three is read off a
    /// neighbour. A plan that filled a row from the wrong entry would still have the right NUMBER
    /// of rows, which is what the test above checks and not this one.
    #[test]
    fn every_row_carries_its_own_label_square_and_screen() {
        let facts = nav::Facts::default();
        let all: Vec<usize> = (0..nav::NAV.len()).collect();
        let plan = rail_plan(&all, Body::Settings, &[0; ScreenId::ALL.len()], &facts);
        for sec in &plan.sections {
            let want = nav::NAV[sec.index].1;
            for (row, (label, id)) in sec.rows.iter().zip(want.iter()) {
                assert_eq!(
                    row.label, *label,
                    "{}: a row took another's label",
                    sec.head
                );
                assert_eq!(row.id, *id, "{}: a row took another's screen", sec.head);
                assert_eq!(
                    row.state,
                    nav::square(*id, &facts),
                    "{}/{label}: the square is not this row's own",
                    sec.head
                );
                assert!(
                    !row.selected,
                    "nothing is selected while Settings is the body"
                );
            }
        }
    }

    /// EXACTLY ONE ROW IS SELECTED, AND IT IS THE ONE ON SCREEN. Driven over every screen in the
    /// app, because `selected` is computed per row and a version that compared the wrong thing
    /// would light one row in every section or none at all.
    #[test]
    fn one_row_is_selected_and_it_is_the_one_on_screen() {
        let facts = nav::Facts::default();
        let all: Vec<usize> = (0..nav::NAV.len()).collect();
        for id in ScreenId::ALL {
            let plan = rail_plan(&all, Body::Screen(id), &[0; ScreenId::ALL.len()], &facts);
            let lit: Vec<ScreenId> = plan
                .sections
                .iter()
                .flat_map(|s| s.rows.iter())
                .filter(|r| r.selected)
                .map(|r| r.id)
                .collect();
            /* A SECTION THAT IS ITS OWN SCREEN LIGHTS THE ROW IT STANDS INSIDE, because that is
             * the row the rail draws: Commission has no row of its own, it is a room in the
             * Tradeskill Hall. Lighting nothing was the alternative and it left the whole rail
             * dark on a jump straight to one of those screens. */
            let want = nav::parent_of(id).map(|(owner, _)| owner).unwrap_or(id);
            assert_eq!(lit, [want], "{id:?}: the rail lit {lit:?}");

            /* AND THE SECTION UNDER IT IS THE ONE YOU ARE ON, which is the other half: lighting
             * the Hall while pointing at the wrong room would pass the assertion above. The
             * slots are all zero here, so anything but the first room proves the body won. */
            if let Some((owner, at)) = nav::parent_of(id) {
                let subs: Vec<usize> = plan
                    .sections
                    .iter()
                    .flat_map(|s| s.rows.iter())
                    .filter(|r| r.id == owner)
                    .flat_map(|r| r.subs.iter())
                    .filter(|s| s.selected)
                    .map(|s| s.at)
                    .collect();
                assert_eq!(subs, [at], "{id:?}: the rail pointed at room {subs:?}");
            }
        }
    }

    /// A HEADING TOGGLES AND A REALM MARK ONLY REVEALS, and that difference is the whole reason
    /// there are two functions.
    ///
    /// THE NARROW RAIL IS A WAY IN. Pressing a realm mark twice must not shut the list you were
    /// reaching for, which is exactly what would happen if it shared the heading's toggle. The
    /// heading keeps the toggle, because a heading you cannot fold is a heading with a dead
    /// chevron on it.
    #[test]
    fn a_heading_toggles_and_a_realm_mark_only_ever_reveals() {
        let mut open: Vec<usize> = Vec::new();

        toggle_head(&mut open, 2);
        assert_eq!(open, vec![2]);
        toggle_head(&mut open, 2);
        assert_eq!(
            open,
            Vec::<usize>::new(),
            "a heading did not shut on its second press"
        );

        reveal_head(&mut open, 2);
        assert_eq!(open, vec![2]);
        reveal_head(&mut open, 2);
        assert_eq!(
            open,
            vec![2],
            "a realm mark shut the section it was meant to open"
        );

        /* AND NEITHER DISTURBS ANOTHER SECTION. A click on one end of the rail must never close
         * something at the other, which this rail did once and was corrected for. */
        reveal_head(&mut open, 5);
        toggle_head(&mut open, 1);
        assert!(open.contains(&2) && open.contains(&5) && open.contains(&1));
        toggle_head(&mut open, 5);
        assert!(
            open.contains(&2) && open.contains(&1),
            "shutting one section disturbed the others: {open:?}"
        );
    }

    /* ------------------------------------------------------------------ the find box -- */

    /// EVERY LETTER THE BOX OFFERS IS A LETTER THE PARSER TAKES, AND EVERY KIND IS OFFERED.
    ///
    /// The first half is the one that has teeth: it crosses `main`'s hint against `data`'s
    /// `from_prefix`, so a hint carrying a letter the parser does not know (which is exactly what
    /// `d:` became the moment the drop table left the search) fails here. The second half reads
    /// the hint as TEXT rather than trusting the iterator that built it, so a hint that formatted
    /// its prefixes into a shape a reader cannot type is caught too.
    #[test]
    fn the_find_box_offers_every_prefix_the_parser_takes_and_no_others() {
        let hint = find_hint();
        assert!(hint.starts_with("find: "), "{hint}");
        let offered: Vec<&str> = hint["find: ".len()..].split(' ').collect();
        assert_eq!(
            offered.len(),
            data::HitKind::ALL.len(),
            "the box offers {offered:?} against {} kinds",
            data::HitKind::ALL.len()
        );
        for token in &offered {
            /* Typed as a query with a needle behind it, which is how a reader would use it. */
            let (kind, needle) = data::parse_query(&format!("{token}bone"));
            assert!(
                kind.is_some(),
                "the box offers \"{token}\" and the parser does not take it: {hint}"
            );
            assert_eq!(needle, "bone", "\"{token}\" ate the needle");
        }
        for k in data::HitKind::ALL {
            assert!(
                offered.contains(&format!("{}:", k.prefix()).as_str()),
                "{k:?} is searchable by prefix and the box does not offer it: {hint}"
            );
        }
    }

    /// THE EMPTY LINE NAMES EVERY KIND A BARE NEEDLE IS SEARCHED AGAINST.
    ///
    /// It is the sentence a reader gets when nothing matched, so it is the sentence that tells
    /// them where the app looked. It listed "items, zones, drops or quests" and was wrong in both
    /// directions at once: drops are no longer searched, and spells and the sky list were being
    /// searched without being named. Pinned to `HitKind::ALL` by label, which is what a plural
    /// contains.
    #[test]
    fn the_finders_empty_line_names_every_kind_a_bare_needle_searches() {
        let line = nothing_found("bone");
        assert!(
            line.contains("\"bone\""),
            "the needle is quoted back: {line}"
        );
        for k in data::HitKind::ALL {
            assert!(
                line.contains(k.label()),
                "{k:?} is searched and the empty line does not name it: {line}"
            );
        }
    }

    /* --------------------------------------------------------------------- the rail -- */

    /* ------------------------------------------------------ what the rail actually paints -- */

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

    fn rail_strings(plan: &RailPlan) -> Vec<String> {
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        let w = chrome::RAIL_WIDE;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(w, 2400.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            ui.set_max_width(w);
            rail_list(ui, plan);
        });
        let shapes = std::mem::take(&mut out.shapes);
        /* a TexturesDelta panics if it is dropped with the font atlas still unapplied */
        out.drop_without_applying_deltas();
        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.shape, &mut flat);
        }
        flat.iter()
            .filter_map(|sh| match sh {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// THE RAIL PAINTS NO ROW CALLED SETTINGS, AND THIS READS THE PAINT TO SAY SO.
    ///
    /// The rail used to end with a hairline and a row labelled "Settings", above a persona footer
    /// whose name and whose gear both open the same screen. Three doors to one place; the row went
    /// (see `Body`), and what it carried moved to the gear (`persona::gear_ink`).
    ///
    /// IT DRIVES A FRAME RATHER THAN READING `RailPlan`, and the difference is the whole test. A
    /// row painted from a literal, which is exactly what the deleted one nearly was, never appears
    /// in the plan at all; an assertion on the plan would have gone green over a rail that still
    /// drew the row. So this paints the plan through the same function the App paints it with and
    /// reads the strings that came out.
    ///
    /// THE EQUALITY IS THE ASSERTION AND THE NAME CHECK IS THE MESSAGE. Holding the painted labels
    /// to the plan's rows, in order and with nothing left over, fails on ANY extra row rather than
    /// only on one that happens to be called Settings. Single characters are dropped first because
    /// `chrome::section` lays its headings out one glyph at a time (`chrome::tracked`), and digits
    /// because a count badge is a painted string too.
    #[test]
    fn the_rail_paints_no_row_called_settings() {
        let facts = nav::Facts::default();
        /* EVERY REALM, one after another, because the rail shows one at a time now. A Settings
         * row could hide in any of them and a test that looked at one would find it in none. */
        /* EVERY SECTION OPEN AT ONCE, because the wide rail can show them that way and a row
         * called Settings could hide in any of them. */
        let all: Vec<usize> = (0..nav::NAV.len()).collect();
        {
            let head = "the rail";
            let plan = rail_plan(&all, Body::Settings, &[0; ScreenId::ALL.len()], &facts);
            let want: Vec<String> = plan
                .sections
                .iter()
                .flat_map(|s| s.rows.iter())
                .map(|r| r.label.to_owned())
                .collect();
            /* ROWS PLUS THE SCREENS NESTED IN THEM. A screen is reachable as a row of the rail
             * or as a section of one, and this fixture is only discriminating if it is showing
             * every place there is. */
            let nested = nav::SECTIONS
                .iter()
                .flat_map(|(_, parts)| parts.iter().filter(|(_, inner, _)| inner.is_some()))
                .count();
            assert_eq!(
                want.len() + nested,
                ScreenId::ALL.len(),
                "the fixture is not discriminating: every section is meant to be open"
            );
            let got: Vec<String> = rail_strings(&plan)
                .into_iter()
                .filter(|s| s.chars().count() > 1 && !s.chars().all(|c| c.is_ascii_digit()))
                .collect();
            assert!(
                !got.iter().any(|s| s.eq_ignore_ascii_case("settings")),
                "{head} painted a row labelled Settings; the persona footer is the way in, and \
                 the row it had was a third door to one screen: {got:?}"
            );
            assert_eq!(
                got, want,
                "{head} painted labels the plan does not carry, or lost ones it does"
            );
        }
    }

    /* ------------------------------------------------------ the persona footer's route -- */

    /// THE FOOTER IS THE PRIMARY DOOR TO SETTINGS AND NOTHING EXERCISED IT. The click lives inside
    /// the rail closure, the smoke launch presses nothing, and the footer's own tests stop at the
    /// `PersonaAction` it returns. This is the other half: what the App does with that answer.
    #[test]
    fn the_persona_footer_opens_settings_and_a_quiet_row_moves_nothing() {
        assert_eq!(
            persona_pending(PersonaAction::OpenSettings),
            Some(Body::Settings)
        );
        assert_eq!(persona_pending(PersonaAction::None), None);
    }

    /* --------------------------------------------------- the pop-out's page -- */

    /// DEFECT: THE PICTURE IN PICTURE CONTROL HANDED BACK A WINDOW SHOWING A DIFFERENT PAGE.
    ///
    /// Its hover says "put THIS in its own window". `Tool::Parser` carries no page, so before
    /// `Windows::open_at` took one there was nothing for the registry to sync and the pop-out came
    /// up on `ParserWindow::default`, which is Dashboards, from every one of the nine pages that
    /// press reaches. This is the half that names the page; the registry half is `open_at`'s own.
    ///
    /// # EVERY NAME THIS CAN RETURN IS CHECKED AGAINST THE WINDOW'S OWN LIST
    ///
    /// `ParserWindow::show_named` moves to a page by NAME and an unknown name leaves the window
    /// where it is, deliberately: opening Dashboards because a caller misspelled a page would be
    /// worse than not moving. That is the right rule and it is also silent, so a rename on either
    /// side would turn this whole fix off with nothing on screen to say so. The membership
    /// assertion below is what makes that a failing test instead.
    ///
    /// WHAT MUTATION MAKES THIS RED: spelling either journal any other way, returning the section
    /// INDEX or the section's target `ScreenId` label instead of the row name `nav::SECTIONS`
    /// carries, or dropping an arm so that page pops out onto Dashboards.
    #[test]
    fn the_pop_out_opens_on_a_page_that_window_actually_has() {
        use grimoire_desktop::windows::ParserWindow;
        let none = [0usize; ScreenId::ALL.len()];

        /* THE TWO JOURNALS, WHICH ARE ROWS OF MY LEGEND AND NOT SECTIONS OF THE PARSER. Their
         * names have to be `nav::NAV`'s, because that is what the rail calls them and what the
         * window's page row prints. */
        assert_eq!(
            pop_out_page_of(Body::Screen(ScreenId::KillTracker), &none),
            Some(nav::label(ScreenId::KillTracker)),
            "the Hunt Journal popped out onto something else"
        );
        assert_eq!(
            pop_out_page_of(Body::Screen(ScreenId::Loot), &none),
            Some(nav::label(ScreenId::Loot)),
            "the Loot Journal popped out onto something else"
        );

        /* EVERY SECTION OF THE PARSER, AND THE INDEX IS THE ONE THE RAIL HOLDS. A fixture that
         * only checked section 0 would pass for an implementation that ignored `section`
         * entirely, which is the near miss this loop exists to fail. */
        let sections = nav::sections_of(ScreenId::Parser);
        assert!(
            sections.len() > 1,
            "the parser has one section, so this loop cannot tell a working lookup from a \
             constant"
        );
        let mut seen = Vec::new();
        for (i, (name, _, _)) in sections.iter().enumerate() {
            let mut at = none;
            at[ScreenId::Parser.ordinal()] = i;
            let got = pop_out_page_of(Body::Screen(ScreenId::Parser), &at);
            assert_eq!(
                got,
                Some(*name),
                "standing on LOG PARSER / {name} pops out onto {got:?}"
            );
            seen.push(*name);
        }

        for name in seen.iter().chain(
            [
                nav::label(ScreenId::KillTracker),
                nav::label(ScreenId::Loot),
            ]
            .iter(),
        ) {
            assert!(
                ParserWindow::PAGES.contains(name),
                "{name:?} is not a page of the pop-out, so `show_named` would silently leave the \
                 window where it was: it has {:?}",
                ParserWindow::PAGES
            );
        }

        /* A SECTION INDEX PAST THE END IS `None` AND NOT A PANIC. `pick_section` resets the slot
         * with the section, but a shorter `SECTIONS` list on the next build would leave a stale
         * index behind, and a pop-out that does not move beats a process that stops. */
        let mut past = none;
        past[ScreenId::Parser.ordinal()] = sections.len() + 4;
        assert_eq!(pop_out_page_of(Body::Screen(ScreenId::Parser), &past), None);

        /* AND EVERY OTHER BODY NAMES NO PAGE, which `open_at` ignores for the tools that have no
         * page list at all. */
        assert_eq!(pop_out_page_of(Body::Settings, &none), None);
        assert_eq!(pop_out_page_of(Body::Screen(ScreenId::Sky), &none), None);
        assert_eq!(pop_out_page_of(Body::Screen(ScreenId::Watch), &none), None);
    }

    /// AND THE PRESS ACTUALLY HANDS THAT PAGE OVER, which is the half a pure test cannot see.
    ///
    /// A TEXT FLOOR, for the same reason `the_app_fills_the_gear_state_by_calling_settings_state`
    /// is one: both sites live inside `App::ui`, which takes an `&mut eframe::Frame` and has no
    /// public constructor, so nothing in there can be called from a test. `pop_out_page_of` could
    /// be perfect and the frame could still call `windows.open(t)` and throw the answer away,
    /// which is precisely the state this crate was in before this wave: a tested function with no
    /// production caller.
    ///
    /// ITS BLIND SPOT IS STATED PLAINLY. It proves the two calls are written, not that the values
    /// handed to them are the live body and the live section slot.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting `windows.open(t)` back at the pop-out site, or
    /// dropping the `pop_out_page_of` call so the press stops naming a page.
    #[test]
    fn the_pop_out_press_hands_its_page_to_the_registry() {
        let whole = strip_comments(include_str!("main.rs"));
        /* EVERYTHING BEFORE THE TEST MODULE, AND THAT IS NOT TIDINESS. The two strings this test
         * looks for are also written down inside this test, and `strip_comments` keeps a string
         * literal as code because a comment marker inside one is not a comment. Searching the
         * whole file would find this function's own words and pass with the production site
         * deleted, which is a test that proves nothing wearing the costume of one that does. */
        let src = whole
            .split("mod tests {")
            .next()
            .expect("the file has a body");
        assert!(
            src.contains("pop_out_page = pop_out_page_of(*body, section);"),
            "the picture in picture press no longer works out which page it came from"
        );
        assert!(
            src.contains("self.windows.open_at(t, pop_out_page);"),
            "the press does not hand its page to the registry, so `open_at`'s page argument has \
             no production caller and the pop-out comes up on Dashboards"
        );
    }

    /// AND A SCREEN CAN NOW ASK FOR SETTINGS, WHICH IS THE ONE DESTINATION NO ASK COULD NAME.
    ///
    /// # WHY THIS WAS MISSING AND WHY IT MATTERS MOST WHERE IT IS HARDEST TO TEST
    ///
    /// Settings is not a `nav::ScreenId`: it is a `Body` the persona footer's gear opens, and the
    /// gear is at the foot of the MAIN window's rail. Every screen that had to send a reader there
    /// therefore described the trip in words, and in `windows::ParserWindow` those words named a
    /// route that does not exist from inside a pop-out: no rail, no gear, no Settings page. See
    /// `screens::Ask::OpenSettings` and `screens::live::empty_state`.
    ///
    /// TWO HALVES, BECAUSE THE ROUTE IS NOT REACHABLE FROM A TEST. `App::answer` takes `&mut self`
    /// on a struct that owns the watcher, the player and the hotkey manager and is built from an
    /// `eframe::CreationContext`, so the arm can only be read; what the arm CALLS is a free
    /// function and is driven for real below. The blind spot is stated: the text floor proves the
    /// arm routes through `enter`, not that it ran.
    ///
    /// WHAT MUTATION MAKES THIS RED: answering `Ask::OpenSettings` with anything but `enter` onto
    /// `Body::Settings` (a `reveal` of some adjacent screen, say), or `enter` failing to move the
    /// body it is handed.
    #[test]
    fn a_screen_that_asks_for_settings_lands_exactly_where_the_gear_lands() {
        let whole = strip_comments(include_str!("main.rs"));
        let src = whole
            .split("mod tests {")
            .next()
            .expect("the file has a body");
        assert!(
            src.contains("Ask::OpenSettings => enter("),
            "the App answers Ask::OpenSettings some other way than the door the gear uses; one \
             route into Settings is the whole point of the variant"
        );

        /* AND THE DOOR ITSELF, DRIVEN. `persona_pending` hands back this exact `Body` and the
         * frame puts it through this exact call, so proving `enter` lands it is proving both
         * routes arrive in the same place. */
        let mut body = Body::Screen(ScreenId::Parser);
        let mut tabs = [0usize; ScreenId::ALL.len()];
        let mut section = [0usize; ScreenId::ALL.len()];
        let mut screens = Screens::default();
        assert_eq!(
            persona_pending(PersonaAction::OpenSettings),
            Some(Body::Settings),
            "the gear stopped opening Settings, so the two routes are no longer one place"
        );
        enter(
            Body::Settings,
            &mut body,
            &mut tabs,
            &mut section,
            &mut screens,
        );
        assert_eq!(body, Body::Settings);
        assert_eq!(
            section,
            [0usize; ScreenId::ALL.len()],
            "opening Settings disturbed a destination's section; it is a Body and owns no rail row"
        );
    }

    /// THE GEAR'S STATE IS COMPUTED AT THE APP'S SITE AND IS NOT A LITERAL SITTING THERE.
    ///
    /// `persona::tests::every_persona_field_is_filled_by_the_app` proves the App NAMES every field
    /// of `Persona`. It cannot prove the value is alive: `settings: State::Settled` would satisfy
    /// it exactly, compile, lint clean and pass every other test in this tree, and the warning the
    /// deleted Settings row used to carry would then be permanently off, on a build where nothing
    /// on screen said so. That is this codebase's own recurring defect, a field whose only
    /// reachable value is its absent one, and it is the reason the round-one seal and bell were
    /// deleted.
    ///
    /// A TEXT FLOOR, and it is one for the same reason `the_unbuilt_route_and_the_unbuilt_list`
    /// is: the site is inside `App::ui`, which takes an `&mut eframe::Frame` and so cannot be
    /// called from a test at all. It reads the one construction site and asks that the settings
    /// field is filled by calling `settings_state`, whose own answers are pinned below. Its blind
    /// spot is stated plainly: it proves the CALL is there, not that the arguments are the App's
    /// live settings.
    #[test]
    fn the_app_fills_the_gear_state_by_calling_settings_state() {
        let src = &strip_comments(include_str!("main.rs"));
        let site = src
            .split_once("let who = Persona {")
            .expect("the App builds exactly one Persona")
            .1
            .split_once("};")
            .expect("the construction closes")
            .0;
        let line = site
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("settings:"))
            .expect("the App fills Persona::settings");
        assert!(
            line.contains("settings_state("),
            "the App fills Persona::settings with {line:?} rather than by calling settings_state; \
             a literal there is a warning that can never come on"
        );
    }

    /* ------------------------------------------------- the rest of what this file decides -- */

    /// A binding to build the two cases out of. Every field is named so a new one on `Binding`
    /// makes this fail to compile rather than silently take a default.
    fn binding(registered: bool) -> hotkeys::Binding {
        hotkeys::Binding {
            id: "companion",
            chord: "Ctrl+Alt+G".to_owned(),
            default: "Ctrl+Alt+G",
            tool: Tool::Companion,
            alias: false,
            overridden: false,
            registered,
            conflict: if registered {
                None
            } else {
                Some("another program holds this chord".to_owned())
            },
        }
    }

    /// The persona footer gear's state, the one state in the rail `nav::square` does not decide. A
    /// settings file that would not load OUTRANKS an unregistered hotkey: one is a refusal, the
    /// other is something a person can resolve on that screen.
    ///
    /// It used to colour a Settings row in the rail. The row is gone and this is not: it is what
    /// the App fills `Persona::settings` with, and `persona::gear_ink` paints.
    #[test]
    fn the_settings_square_reads_the_file_first_and_the_bindings_second() {
        let ok = Settings::default();
        let broken = Settings {
            load_problem: Some("settings are not valid JSON".to_owned()),
            ..Settings::default()
        };
        assert_eq!(settings_state(&ok, &[]), State::Settled);
        assert_eq!(settings_state(&ok, &[binding(true)]), State::Settled);
        assert_eq!(settings_state(&ok, &[binding(false)]), State::You);
        assert_eq!(
            settings_state(&ok, &[binding(true), binding(false)]),
            State::You,
            "one chord that did not register is enough"
        );
        assert_eq!(settings_state(&broken, &[]), State::Wrong);
        assert_eq!(
            settings_state(&broken, &[binding(false)]),
            State::Wrong,
            "a file that will not load outranks a chord that will not register"
        );
    }

    /// The smoke switch. A value that is not a number of milliseconds is not a panic and not a
    /// silent no-op: the app runs normally and says so on stderr.
    #[test]
    fn the_smoke_switch_reads_milliseconds_and_ignores_anything_else() {
        assert_eq!(parse_smoke(Some("250")), Some(Duration::from_millis(250)));
        assert_eq!(
            parse_smoke(Some("  250  ")),
            Some(Duration::from_millis(250))
        );
        assert_eq!(parse_smoke(Some("0")), Some(Duration::ZERO));
        assert_eq!(parse_smoke(Some("soon")), None);
        assert_eq!(parse_smoke(Some("-1")), None, "not a duration");
        assert_eq!(parse_smoke(Some("")), None);
        assert_eq!(parse_smoke(None), None, "unset is the ordinary launch");
    }

    /// The rows this file routes to `unbuilt` and the rows `nav::UNBUILT` holds words for are one
    /// set, held to each other here rather than by the comment that used to assert it.
    ///
    /// A TEXT FLOOR, AND WHY IT IS ONE. The routing lives in a `match` over `ScreenId`, which
    /// nothing outside the App can ask about, and turning that arm into a guard (`id if
    /// nav::is_unbuilt(id)`) would buy this test at the price of exhaustiveness: a `ScreenId` added
    /// later would stop being a compile error and start silently landing on the unbuilt page. The
    /// arm therefore stays explicit and this reads it, the way `reach.rs` reads `lib.rs`. Its blind
    /// spot is the same as that file's: it proves the two LISTS agree, not that the arm it found is
    /// the one the App reaches.
    ///
    /// Drift in either direction is a real defect. A row routed here with no entry paints "routed
    /// here by mistake" at the reader. A row with an entry and no route has words nothing can
    /// reach, and the rail's hollow ring then promises a page that never opens.
    #[test]
    fn the_unbuilt_route_and_the_unbuilt_list_hold_the_same_rows() {
        let src = &strip_comments(include_str!("main.rs"));
        /* THE MARKER IS BUILT RATHER THAN WRITTEN, so it cannot appear in this test`s own source.
         *
         * It was the literal `=> unbuilt(ui, id)`, which reads fine until the arm is reformatted:
         * the call went multi-line, the literal stopped matching the real arm, and the first thing
         * in the file that still matched was THIS LINE. The test then parsed its own body, found no
         * screens in it, and reported that the App routes nothing to the unbuilt page. A test that
         * can match itself is a test that can pass or fail for reasons that have nothing to do with
         * the code it is about. */
        let marker = format!("=> {}(", "unbuilt");
        let end = src
            .find(&marker)
            .expect("the App routes every unbuilt row through one arm");
        /* Back to the end of the previous arm. The patterns in between are `ScreenId::X` joined
         * by `|`, and no arm PATTERN contains a comma, so the last comma before the marker is the
         * boundary.
         *
         * WHICH IS ONLY TRUE OF CODE, AND THAT IS WHY THE COMMENTS ARE GONE BEFORE WE LOOK.
         * A comment inside the arm has whatever punctuation prose has, and a single comma in one
         * silently truncates the arm to whatever follows it: the test then compares half a list
         * against the whole of `nav::UNBUILT` and reports rows as unrouted that are routed fine.
         * That happened twice, the second time inside a comment warning about the first. Asking
         * the author to write comma-free prose was the wrong fix; reading code was the right one. */
        let start = src[..end].rfind(',').map(|i| i + 1).unwrap_or(0);
        let arm = &src[start..end];

        let mut routed: Vec<String> = Vec::new();
        let mut from = 0usize;
        while let Some(rel) = arm[from..].find("ScreenId::") {
            let at = from + rel + "ScreenId::".len();
            let name: String = arm[at..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            assert!(!name.is_empty(), "a bare ScreenId:: in the unbuilt arm");
            routed.push(name);
            from = at;
        }
        assert!(
            !routed.is_empty(),
            "the arm parsed to nothing, so the comparison below would be asserting about an empty \\
             set; the marker has probably stopped matching the real arm"
        );
        routed.sort();

        let mut listed: Vec<String> = nav::UNBUILT
            .iter()
            .map(|(id, _)| format!("{id:?}"))
            .collect();
        listed.sort();

        assert_eq!(
            routed, listed,
            "the rows routed to the unbuilt page and the rows nav::UNBUILT has words for must be one set"
        );
        assert!(
            !routed.is_empty(),
            "the arm parsed to nothing, so this test is asserting about an empty set"
        );
    }

    /// THE WATCH BODY IS THE ONE SCREEN DRAWN EDGE TO EDGE, AND EVERY OTHER ONE KEEPS ITS MARGIN.
    ///
    /// "leaves should be our player or the offline image" is the owner's sentence, and edge to edge
    /// is half of what it asks for: a video or a channel's own offline screen sitting inside an 18
    /// point border is a frame this app has drawn around somebody else's picture. The screen cannot
    /// give itself the room; the panel has to leave it out, and that is this decision.
    ///
    /// BOTH DIRECTIONS ARE ASSERTED AND THE SECOND IS THE ONE THAT WOULD ACTUALLY GO WRONG. A
    /// margin dropped from every screen would give the Watch screen exactly what it wants and put
    /// every list in the app hard against the rail, and nothing on the Watch screen would look
    /// wrong at all.
    ///
    /// AND BOTH SURFACE ROWS, WHICH ARE NOW TWO SCREENS AND NOT ONE.
    ///
    /// THIS ASSERTION USED TO READ `is_watch(Body::Screen(Videos))` AND IT WAS TRUE FOR THE WRONG
    /// REASON. `draw_screen` sent both rows to `WatchScreen`, so the Videos row really did draw the
    /// Watch screen, which is exactly the defect: it opened the TWITCH player under the crumb
    /// `stoic/videos`. Changing that line was a decision, not tidying. What the test was actually
    /// protecting is the MARGIN, and that is unchanged and asserted for both rows through
    /// [`is_full_bleed`]; what it additionally asserts now is that the two rows are two screens,
    /// which is the thing that was wrong.
    #[test]
    fn the_watch_body_is_the_one_screen_drawn_edge_to_edge() {
        assert!(is_watch(Body::Screen(ScreenId::Watch)));
        assert!(
            !is_watch(Body::Screen(ScreenId::Videos)),
            "the Videos row has a screen of its own; routing it to WatchScreen is the defect this \
             replaces"
        );
        assert!(is_videos(Body::Screen(ScreenId::Videos)));
        assert!(!is_videos(Body::Screen(ScreenId::Watch)));
        for id in [ScreenId::Watch, ScreenId::Videos] {
            assert!(
                is_full_bleed(Body::Screen(id)),
                "{id:?} takes its whole folio for a webview surface"
            );
            assert_eq!(
                folio_margin(Body::Screen(id)),
                egui::Margin::ZERO,
                "{id:?} draws a surface edge to edge and may not be inset"
            );
        }
        let mut others = 0;
        for id in ScreenId::ALL {
            if is_full_bleed(Body::Screen(id)) {
                continue;
            }
            others += 1;
            assert_eq!(
                folio_margin(Body::Screen(id)),
                egui::Margin::same(18),
                "{id:?} lost the margin that keeps its rows off the rail"
            );
        }
        assert!(others > 20, "only {others} screens were checked");
        assert_eq!(
            folio_margin(Body::Settings),
            egui::Margin::same(18),
            "Settings is not a screen the nav can reach and still needs its margin"
        );
    }

    /* ---------------------------------------------- the two STOIC rows are two screens -- */

    /// THE ROW FINALLY DRAWS WHAT IT SAYS, ASSERTED AT THE LINE WHERE IT DID NOT.
    ///
    /// `draw_screen` carried one arm reading `ScreenId::Watch | ScreenId::Videos => s.watch.ui(..)`.
    /// That was harmless while the two rows drew byte-identical pages. It became a lie the day the
    /// Watch folio turned into a full bleed player: clicking the rail row "Videos" opened the
    /// TWITCH player under the crumb `stoic/videos`, and the word VIDEOS appeared nowhere in the
    /// main window. A row whose name points at nothing is worse than a missing row, because the row
    /// is the only thing telling a reader what they clicked.
    ///
    /// IT DRIVES `draw_screen` ITSELF AND READS THE SURFACE BACK OUT OF A REAL FRAME. `App::ui`
    /// cannot be called from a test, because it takes an `&mut eframe::Frame` and that has no
    /// public constructor; `draw_screen` takes a `Ui` and can, and it is the exact function that
    /// held the defect. Both rows are driven with the SAME live status and the SAME player, so the
    /// routing is the only thing that can differ between the two passes, and what is read back is
    /// the KIND of surface each row asked the App for rather than a label somebody could rename.
    ///
    /// THE TWITCH HALF IS NOT DECORATION. Asserting only that Videos stages YouTube would pass on a
    /// build that had broken Watch live in the other direction, which is the mirror image of the
    /// defect and would be found by nobody.
    #[test]
    fn the_watch_row_and_the_videos_row_stage_two_different_surfaces() {
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        theme::install(&ctx);

        let staged = |id: ScreenId| -> player::Feed {
            let mut live = Status {
                twitch: grimoire_desktop::watcher::Channel::unchecked(
                    grimoire_desktop::settings::TWITCH_HANDLE,
                ),
                youtube: grimoire_desktop::watcher::Channel::unchecked(
                    grimoire_desktop::settings::YOUTUBE_HANDLE,
                ),
            };
            live.twitch.live = Some(true);
            let mut settings = Settings::default();
            let mut ingest = Ingest::new(&settings);
            let mut screens = Screens::default();
            let mut cx = Cx {
                data: None,
                railed: false,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                chat: grimoire_desktop::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                player: player::PlayerView {
                    problem: None,
                    playing: true,
                    ..Default::default()
                },
                stage: None,
                demand: None,
                ask: Ask::None,
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                draw_screen(ui, id, 0, 0, &mut screens, &mut cx);
            });
            out.shapes.clear();
            /* headless: there is no renderer to hand the font atlas to, and epaint panics on a
             * dropped delta unless told the drop is deliberate */
            out.drop_without_applying_deltas();
            cx.stage
                .unwrap_or_else(|| panic!("{id:?} staged no surface at all"))
                .feed
        };

        assert!(
            matches!(staged(ScreenId::Watch), player::Feed::Twitch { .. }),
            "Watch live plays the preferred platform, which defaults to Twitch"
        );
        let videos = staged(ScreenId::Videos);
        assert!(
            matches!(
                &videos,
                player::Feed::YouTubeChannel { handle }
                    if handle == grimoire_desktop::settings::YOUTUBE_HANDLE
            ),
            "the Videos row must stage the channel page, not the Twitch player; it staged {videos:?}"
        );
    }

    /// The house forbids both dashes in code, comments and UI strings alike.
    ///
    /// This file no longer holds the "not built in this release" reasons; they moved to
    /// `nav::UNBUILT`, and `nav`'s own guard covers them there.
    #[test]
    fn no_dashes_anywhere_in_this_file() {
        let src = include_str!("main.rs");
        for (i, line) in src.lines().enumerate() {
            assert!(!line.contains('\u{2014}'), "em dash on line {}", i + 1);
            assert!(!line.contains('\u{2013}'), "en dash on line {}", i + 1);
        }
    }
    /// EVERY SECTION IN THE RAIL DOES SOMETHING WHEN IT IS PRESSED.
    ///
    /// THE FAILURE THIS CATCHES IS SILENT AND LOOKS LIKE NOTHING AT ALL. `nav::SECTIONS` names the
    /// rows and `on_section` answers them by index; add a fourth row to a list and the rail draws
    /// four, and the fourth selects, and highlights, and leaves the screen showing whatever was
    /// there. No panic, no warning, no visible mark: a row that is indistinguishable from a working
    /// one and does nothing.
    ///
    /// IT IS DRIVEN THROUGH THE REAL SCREENS, not through a reading of the match. `on_section` is
    /// called for every index of every list and the screen state is read back after each, so an arm
    /// that exists but sets the wrong thing fails here too.
    #[test]
    fn every_section_of_every_destination_reaches_its_view() {
        for (id, rows) in nav::SECTIONS {
            let mut seen: Vec<String> = Vec::new();
            for (i, (_, inner, _)) in rows.iter().enumerate() {
                /* A SECTION THAT IS ITS OWN SCREEN IS NOT `on_section`'S TO ANSWER. It routes in
                 * `draw_screen`, it carries its own `ScreenId`, and `every_screen_appears_
                 * exactly_once` already proves no two of them are the same one. Feeding it
                 * through this loop would demand that a screen change some OTHER screen's state
                 * to prove it exists, which is not a thing it should ever do. */
                if let Some(inner) = inner {
                    seen.push(format!("{inner:?}"));
                    continue;
                }
                let mut s = Screens::default();
                on_section(*id, i, &mut s);
                seen.push(match id {
                    ScreenId::Parser => format!("{:?}", s.parser.showing()),
                    ScreenId::Sky => format!("{:?}", s.sky.showing()),
                    ScreenId::Lfg => format!("{:?}", s.lfg.mode),
                    /* GEAR ANSWERS IN `draw_screen` RATHER THAN BY HOLDING STATE, and Guild has
                     * no screen at all yet, so neither can be distinguished by reading a screen
                     * back. Recording the index keeps them in the length check above and proves
                     * nothing else about them, which is why they are covered instead by
                     * `every_section_and_tab_of_an_unwritten_destination_changes_the_page`,
                     * which paints them. Saying so here rather than leaving a bare fallback:
                     * this line is exactly the shape a tautology takes. */
                    _ => format!("section {i}"),
                });
            }
            let mut uniq = seen.clone();
            uniq.sort();
            uniq.dedup();
            assert_eq!(
                uniq.len(),
                rows.len(),
                "{id:?}: {} sections but only {} distinct results {seen:?}; a row here selects and \
                 changes nothing",
                rows.len(),
                uniq.len()
            );
        }
    }
    /// A SECTION IS OFFERED ON ONE CONTROL, NEVER TWO.
    ///
    /// THE DEFECT THIS EXISTS FOR WAS ON SCREEN AND I SHIPPED IT ONCE. The inner rail listed
    /// `Kills | Loot | Fights` and the parser went on drawing the same three words in its own row a
    /// hundred points to the right, both live, both selectable, and nothing keeping them agreed.
    /// The screens have to keep those rows for the pop-out windows, where there is no rail at all,
    /// so the answer is `Cx::railed` and this is what holds it.
    ///
    /// IT READS PAINTED TEXT, not a flag. Each screen is drawn twice into a real headless frame,
    /// once as a pop-out would draw it and once as the main window does, and the section names are
    /// counted in what came out. A version that asked `cx.railed` back would pass on a screen that
    /// read the field and ignored it.
    ///
    /// ONLY THE SCREENS THAT OWN A ROW ARE DRIVEN, and they are named rather than derived, because
    /// deriving the list from `SECTIONS` would silently pass for the ones that never had a row
    /// (Gear picks its section in `draw_screen`, Groups shows its mode in the window title) and
    /// would then be asserting nothing about anything.
    #[test]
    fn a_railed_screen_does_not_offer_its_sections_twice() {
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        theme::install(&ctx);

        /* THE SECTION IS AN ARGUMENT NOW, and it has to be. A destination's section 0 is not
         * necessarily its own screen: Log Parser's first section is Live, which is declared and
         * unwritten, so `draw_screen` hops straight to the unbuilt page and the parser screen
         * never runs. This test painted that page twice and compared it with itself, which is a
         * comparison that cannot fail and reported 5 things against 5.
         */
        let words = |id: ScreenId, section: usize, railed: bool| -> Vec<String> {
            let live = Status {
                twitch: grimoire_desktop::watcher::Channel::unchecked(
                    grimoire_desktop::settings::TWITCH_HANDLE,
                ),
                youtube: grimoire_desktop::watcher::Channel::unchecked(
                    grimoire_desktop::settings::YOUTUBE_HANDLE,
                ),
            };
            let mut settings = Settings::default();
            let mut ingest = Ingest::new(&settings);
            let mut screens = Screens::default();
            let mut cx = Cx {
                data: None,
                railed,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                chat: grimoire_desktop::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                player: player::PlayerView::default(),
                stage: None,
                demand: None,
                ask: Ask::None,
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                draw_screen(ui, id, section, 0, &mut screens, &mut cx);
            });
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();
            let mut said = Vec::new();
            let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
            while let Some(sh) = stack.pop() {
                match sh {
                    egui::Shape::Vec(v) => stack.extend(v),
                    egui::Shape::Text(t) => said.push(t.galley.text().to_owned()),
                    _ => {}
                }
            }
            said
        };

        /* THE TWO SCREENS THAT OWN A SWITCHER, named rather than derived from `nav::SECTIONS`.
         *
         * Deriving them would be wrong in both directions now. Log Parser owns a row and has NO
         * section list: its three views are three destinations in the rail, so a derived list would
         * skip the one screen this test most needs to cover. Gear has a section list and owns no
         * row, so a derived list would demand that it drop a control it never drew.
         *
         * WHAT IS ASSERTED IS A STRICT SUBSET, not a count of known words. The switcher's labels
         * are the screens' own and are not in any table here to compare against; what is certainly
         * true is that drawing beside a rail must paint LESS than drawing alone, and everything it
         * still paints must be something it painted alone. A screen that ignored `railed` paints
         * exactly the same set and fails on the first of those.
         */
        /* EACH AT THE SECTION THAT REALLY IS ITS OWN BODY, FOUND RATHER THAN COUNTED.
         *
         * Log Parser draws itself at Fights, and this said `1` because Fights sat second while
         * Live led the list. Dashboards moving to the top made that a different section, and a
         * literal here would have gone on testing whichever page happened to land on index 1
         * rather than the one this test is about. The position is looked up by name. */
        let fights = nav::sections_of(ScreenId::Parser)
            .iter()
            .position(|(name, _, _)| *name == "Fights")
            .expect("LOG PARSER has a Fights section");
        for (id, section) in [(ScreenId::Parser, fights), (ScreenId::Sky, 0)] {
            let alone = words(id, section, false);
            let railed = words(id, section, true);
            assert!(
                !alone.is_empty() && !railed.is_empty(),
                "{id:?} painted no text in one of the two passes"
            );
            for w in &railed {
                assert!(
                    alone.contains(w),
                    "{id:?} paints {w:?} only when it is beside a rail, which is backwards"
                );
            }
            assert!(
                railed.len() + 2 <= alone.len(),
                "{id:?} paints {} things beside the rail and {} alone; a switcher is at least two \
                 controls, so it is still drawing its own",
                railed.len(),
                alone.len()
            );
        }

        /* AND EVERY SCREEN IN THE MAIN WINDOW IS TOLD THERE IS A RAIL.
         *
         * This is the half a rendering test cannot reach: `words` is handed the flag, so it
         * exercises its own idea of the rule and never the App's. `railed_here` is the App's, and
         * it read `!sections_of(id).is_empty()` for one build. Hunt Journal has no sections, so the
         * flag went false there and the parser drew its three-way row on a page that is Kills by
         * definition; pressing Loot on it left the rail saying Hunt Journal while the body showed
         * loot. */
        for id in ScreenId::ALL {
            assert!(
                railed_here(Body::Screen(id)),
                "{id:?}: the main window has a rail whatever it is showing, and a screen told \
                 otherwise draws its own switcher beside the one in the rail"
            );
        }
        assert!(railed_here(Body::Settings));
    }

    /// SECTIONS APPEAR UNDER THE DESTINATION YOU ARE ON, AND NOWHERE ELSE.
    ///
    /// Both halves, and the second is the one that matters. A plan that nested every destination's
    /// sections all the time would satisfy "the one you are on has them" perfectly, and would draw
    /// a rail of a hundred and twenty rows.
    ///
    /// AND THE SELECTED SECTION IS THE ONE THE APP HOLDS, driven with a non-zero slot, because a
    /// plan that always lit the first would pass every assertion above it.
    #[test]
    fn sections_are_nested_under_the_destination_you_are_on_and_nowhere_else() {
        let facts = nav::Facts::default();
        let all: Vec<usize> = (0..nav::NAV.len()).collect();

        for (id, rows) in nav::SECTIONS {
            let mut slots = [0usize; ScreenId::ALL.len()];
            let want = rows.len() - 1;
            slots[id.ordinal()] = want;
            let plan = rail_plan(&all, Body::Screen(*id), &slots, &facts);

            for sec in &plan.sections {
                for r in &sec.rows {
                    if r.id == *id {
                        let names: Vec<&str> = r.subs.iter().map(|s| s.label).collect();
                        let labels: Vec<&str> = rows.iter().map(|(l, _, _)| *l).collect();
                        assert_eq!(names, labels, "{id:?} did not nest its own sections");
                        let lit: Vec<usize> =
                            r.subs.iter().filter(|s| s.selected).map(|s| s.at).collect();
                        assert_eq!(
                            lit,
                            [want],
                            "{id:?}: the rail lit {lit:?} while the App holds section {want}"
                        );
                    } else {
                        assert!(
                            r.subs.is_empty(),
                            "{:?} nested its sections while the body is on {id:?}",
                            r.id
                        );
                    }
                }
            }
        }
    }
    /// A DESTINATION WHOSE SECTION IS A SCREEN DRAWS THAT SCREEN, AND DRAWS IT WHOLE.
    ///
    /// `nav::Section` lets a section be `Some(id)`, meaning a screen of its own standing inside
    /// another destination, and one `if let` at the top of `draw_screen` is the entire mechanism.
    /// Everything else about such a section (its square, its label, its place in the rail) is
    /// asserted elsewhere against tables, and every one of those would still pass if that hop
    /// opened onto nothing at all.
    ///
    /// SO IT IS DRAWN AND READ. Every `Some` section in the table is rendered twice into a real
    /// headless frame, once through its owner at that index and once as the screen itself, and the
    /// two must paint the same words. Comparing against the screen's OWN output rather than a fixed
    /// string keeps it true when that page is rewritten.
    ///
    /// DERIVED FROM THE TABLE, NOT NAMED. This test used to name the Bazaar, which was six such
    /// sections until the Bazaar became a heading with six plain rows. A named test would now be
    /// asserting about a destination that no longer exists, or worse, quietly passing over an empty
    /// list; the count assertion at the end is what stops that.
    #[test]
    fn a_section_that_is_a_screen_draws_that_screen() {
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        theme::install(&ctx);

        let paint = |id: ScreenId, section: usize| -> Vec<String> {
            let live = Status {
                twitch: grimoire_desktop::watcher::Channel::unchecked(
                    grimoire_desktop::settings::TWITCH_HANDLE,
                ),
                youtube: grimoire_desktop::watcher::Channel::unchecked(
                    grimoire_desktop::settings::YOUTUBE_HANDLE,
                ),
            };
            let mut settings = Settings::default();
            let mut ingest = Ingest::new(&settings);
            let mut screens = Screens::default();
            let mut cx = Cx {
                data: None,
                railed: true,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                chat: grimoire_desktop::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                player: player::PlayerView::default(),
                stage: None,
                demand: None,
                ask: Ask::None,
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                draw_screen(ui, id, section, 0, &mut screens, &mut cx);
            });
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();
            let mut said = Vec::new();
            let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
            while let Some(sh) = stack.pop() {
                match sh {
                    egui::Shape::Vec(v) => stack.extend(v),
                    egui::Shape::Text(t) => said.push(t.galley.text().to_owned()),
                    _ => {}
                }
            }
            said.sort();
            said
        };

        let mut checked = 0usize;
        for (owner, rows) in nav::SECTIONS {
            for (i, (name, inner, _)) in rows.iter().enumerate() {
                let Some(inner) = inner else { continue };
                let through_the_door = paint(*owner, i);
                let direct = paint(*inner, 0);
                assert!(!direct.is_empty(), "{inner:?} painted nothing at all");
                assert_eq!(
                    through_the_door, direct,
                    "{owner:?} at section {i} ({name}) does not draw {inner:?}"
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "no section in the table is a screen of its own, so the hop in `draw_screen` has no \
             consumer and this test proved nothing"
        );
    }
    /// THREE DESTINATIONS SHARE THE PARSER SCREEN AND EACH FIXES IT TO ITS OWN VIEW.
    ///
    /// THIS TEST EXISTS BECAUSE ITS ABSENCE WAS MEASURED. Deleting `on_view`'s Log Parser arm left
    /// the whole suite green: the screen defaults to Kills, so Log Parser would have opened on Hunt
    /// Journal's page under the crumb `chronicle/log-parser`, and every table, square and rail
    /// assertion would have gone on passing. That is the same defect the Log Parser row had before
    /// any of this began, when it inherited whichever view the last row had left behind.
    ///
    /// EACH IS SEEDED ON A DIFFERENT VIEW FIRST, so an arm that does nothing at all cannot pass by
    /// landing on the default, and the state is read back off the screen rather than off the match.
    #[test]
    fn each_parser_destination_fixes_the_screen_to_its_own_view() {
        use screens::parser::View;
        /* (destination, the view it must show, the index that view has in the screen) */
        let want = [
            (ScreenId::Parser, View::Fights, 2usize),
            (ScreenId::KillTracker, View::Kills, 0),
            (ScreenId::Loot, View::Loot, 1),
        ];
        for (id, _, on) in want {
            for seed in [View::Kills, View::Loot, View::Fights] {
                let mut s = Screens::default();
                s.parser.show(seed);
                on_view(id, 0, 0, &mut s);
                assert_eq!(
                    s.parser.showing(),
                    on,
                    "{id:?} left the parser on view {} after arriving from {seed:?}",
                    s.parser.showing()
                );
            }
        }

        /* AND THE THREE DO NOT ALL WANT THE SAME VIEW, which is what makes the loop above mean
         * anything: a screen stuck on one view would satisfy every assertion in it if they did. */
        let mut ons: Vec<usize> = want.iter().map(|(_, _, on)| *on).collect();
        ons.sort();
        ons.dedup();
        assert_eq!(ons.len(), want.len());
    }
    /// EVERY SECTION AND EVERY TAB OF AN UNWRITTEN DESTINATION CHANGES THE PAGE.
    ///
    /// Guild is seven sections and twenty-four tabs with no code behind any of it. That is
    /// deliberate: the rail is a map, and a design drawn with its unwritten half missing cannot be
    /// read. But it is only honest while the controls WORK. Thirty-one rows and tabs that each
    /// select, highlight, and leave the page exactly as it was are thirty-one controls a reader
    /// cannot tell from broken ones, and that is worse than not drawing them.
    ///
    /// SO THE PAGE IS PAINTED AND READ. `main::unbuilt` puts the path in its heading, which is what
    /// makes each of them do something; this drives every combination through a real headless frame
    /// and requires the text to differ. Comparing painted output rather than asserting the heading
    /// string keeps it true if that page is rewritten.
    #[test]
    fn every_section_and_tab_of_an_unwritten_destination_changes_the_page() {
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        theme::install(&ctx);

        let paint = |id: ScreenId, section: usize, tab: usize| -> String {
            let live = Status {
                twitch: grimoire_desktop::watcher::Channel::unchecked(
                    grimoire_desktop::settings::TWITCH_HANDLE,
                ),
                youtube: grimoire_desktop::watcher::Channel::unchecked(
                    grimoire_desktop::settings::YOUTUBE_HANDLE,
                ),
            };
            let mut settings = Settings::default();
            let mut ingest = Ingest::new(&settings);
            let mut screens = Screens::default();
            let mut cx = Cx {
                data: None,
                railed: true,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                chat: grimoire_desktop::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                player: player::PlayerView::default(),
                stage: None,
                demand: None,
                ask: Ask::None,
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                draw_screen(ui, id, section, tab, &mut screens, &mut cx);
            });
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();
            let mut said = Vec::new();
            let mut stack: Vec<egui::Shape> = shapes.into_iter().map(|c| c.shape).collect();
            while let Some(sh) = stack.pop() {
                match sh {
                    egui::Shape::Vec(v) => stack.extend(v),
                    egui::Shape::Text(t) => said.push(t.galley.text().to_owned()),
                    _ => {}
                }
            }
            said.sort();
            said.join("\u{1f}")
        };

        let mut walked = 0usize;
        for (id, rows) in nav::SECTIONS {
            if nav::unbuilt_why(*id).is_none() {
                continue;
            }
            let mut pages: Vec<String> = Vec::new();
            for (i, (name, _, tabs)) in rows.iter().enumerate() {
                /* NO TAB NAMED, so the only thing that can tell these pages apart is the
                 * SECTION. Painting at tab 0 was the first cut and it passed with the section
                 * dropped from the heading entirely: the guild's seven sections happen to have
                 * seven different first tabs, so the tab name alone was carrying the
                 * distinctness and the assertion proved nothing about the section. An index past
                 * every list names no tab and isolates the half under test. */
                pages.push(paint(*id, i, usize::MAX));
                /* AND EVERY TAB WITHIN THE SECTION, which is the half a section-only sweep would
                 * miss: a page that named the section and ignored the tab would pass it. */
                let mut within: Vec<String> = Vec::new();
                for t in 0..tabs.len() {
                    within.push(paint(*id, i, t));
                }
                let mut uniq = within.clone();
                uniq.sort();
                uniq.dedup();
                assert_eq!(
                    uniq.len(),
                    within.len(),
                    "{id:?} / {name}: {} tabs and {} distinct pages; a tab here selects and \
                     changes nothing",
                    within.len(),
                    uniq.len()
                );
                walked += within.len().max(1);
            }
            let mut uniq = pages.clone();
            uniq.sort();
            uniq.dedup();
            assert_eq!(
                uniq.len(),
                pages.len(),
                "{id:?}: {} sections and {} distinct pages; a row here selects and changes nothing",
                pages.len(),
                uniq.len()
            );
        }
        assert!(
            walked >= 24,
            "only {walked} section/tab combinations were walked; the unwritten destinations have \
             lost their sections and this test is asserting almost nothing"
        );
    }

    /// A REMEMBERED TAB NEVER INDEXES PAST THE LIST IT LANDS IN.
    ///
    /// Found by mutation: deleting the reset in the frame body broke nothing in the suite, because
    /// nothing asked what happens when the slot outlives the list. Roster has four tabs and
    /// Calendar three, so moving between them carries a 3 into a list of 3 and the bar draws three
    /// tabs with none of them active.
    ///
    /// DRIVEN OVER EVERY REAL LIST IN THE TABLE, and over an empty one, which is the case almost
    /// every screen takes and the one where a bare `len() - 1` would panic.
    #[test]
    fn a_remembered_tab_never_indexes_past_its_list() {
        assert_eq!(tab_in_range(0, &[]), 0);
        assert_eq!(
            tab_in_range(9, &[]),
            0,
            "an empty list has no index to clamp to"
        );

        let mut lists = 0usize;
        for (id, rows) in nav::SECTIONS {
            for (i, (name, _, tabs)) in rows.iter().enumerate() {
                if tabs.is_empty() {
                    continue;
                }
                lists += 1;
                for from in 0..12usize {
                    let on = tab_in_range(from, tabs);
                    assert!(
                        tabs.get(on).is_some(),
                        "{id:?} / {name}: {from} clamped to {on}, which is not a tab it has"
                    );
                }
                assert_eq!(
                    tab_in_range(0, tabs),
                    0,
                    "{id:?} / {name}: an index already in range must be left alone"
                );
                assert_eq!(tab_in_range(tabs.len() - 1, tabs), tabs.len() - 1);
                let _ = i;
            }
        }
        assert!(
            lists > 0,
            "no section in the table carries tabs, so this walked nothing"
        );
    }
    /// THE HOTKEY HEARTBEAT RUNS WHEN NOTHING IS BEING DRAWN.
    ///
    /// THE DEFECT THIS EXISTS FOR SHIPPED, and the suite was green the whole time. `Hotkeys::poll`
    /// sat inside `App::ui`; eframe calls `ui` only while `show_ui` is true, and a minimized root
    /// with no tool window open makes it false, so eframe called `App::logic` instead, which was
    /// its own empty default. The app minimizes ITSELF on the main window's toggle, so
    /// Ctrl+Alt+G put the app away and then could not bring it back. `hotkeys.rs` claimed the
    /// opposite in prose.
    ///
    /// IT IS A SOURCE TEST AND THAT IS DELIBERATE. The failure is an eframe callback that is never
    /// called, which no unit test can observe: a test can always call `heartbeat` itself and pass
    /// while nothing in the app does. What has to be true is structural, so it is asserted
    /// structurally: `poll` has exactly one caller, that caller is `heartbeat`, and `heartbeat` is
    /// called from BOTH `logic` and `ui`. Losing either arm is the bug coming back.
    #[test]
    fn the_hotkey_heartbeat_runs_whether_or_not_a_frame_is_drawn() {
        let src = &strip_comments(include_str!("main.rs"));
        let body = &src[..src
            .find("#[cfg(test)]")
            .expect("this file has a test module")];

        assert_eq!(
            body.matches("self.hotkeys.poll(").count(),
            1,
            "the OS receiver must be drained in exactly one place; two drains race for the same \
             press and one of them will be on a path that does not run"
        );
        assert!(
            body.contains("fn heartbeat(&mut self, ctx: &egui::Context) {"),
            "the drain lives in `heartbeat` so both eframe callbacks can reach it"
        );

        /* Each callback's body, taken up to the next `fn ` at method indent. */
        let arm = |name: &str| -> &str {
            let at = body
                .find(name)
                .unwrap_or_else(|| panic!("{name} is not implemented on App"));
            let rest = &body[at + name.len()..];
            let end = rest.find("\n    fn ").unwrap_or(rest.len());
            &rest[..end]
        };

        for callback in [
            "fn logic(&mut self, ctx: &egui::Context",
            "fn ui(&mut self, ui: &mut egui::Ui",
        ] {
            assert!(
                arm(callback).contains("self.heartbeat("),
                "{callback} does not call the heartbeat. eframe runs `ui` only while it has \
                 something to draw and `logic` only while it has not, so a heartbeat in one of \
                 them is a hotkey that works in half the states the app can be in"
            );
        }

        /* AND THE QUEUED REQUEST IS APPLIED THERE TOO. Draining alone leaves `Ctrl+Alt+G` dead:
         * `summon` only records the wish, and the raise that answers it used to live inside the
         * pass that does not run. */
        assert!(
            arm("fn logic(&mut self, ctx: &egui::Context").contains("self.windows.wake("),
            "`logic` drains the receiver but never acts on it, so the window stays minimized"
        );
    }
    /// Rust source with every comment replaced by whitespace, so a source-text test reads code.
    ///
    /// NEWLINES ARE KEPT so any line number a failure quotes still points at the right line, and
    /// each removed character becomes a space rather than nothing so byte offsets do not shift.
    ///
    /// STRING LITERALS ARE HONOURED, because `"// not a comment"` is code and this file is full of
    /// message strings. Raw strings and char literals are not handled: neither appears in the arms
    /// this is used on, and a stripper that quietly mangled one would be worse than none.
    fn strip_comments(src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        let b: Vec<char> = src.chars().collect();
        let mut i = 0usize;
        let mut depth = 0usize;
        let (mut line, mut string, mut escape) = (false, false, false);
        while i < b.len() {
            let c = b[i];
            let next = b.get(i + 1).copied().unwrap_or('\0');
            if line {
                if c == '\n' {
                    line = false;
                    out.push(c);
                } else {
                    out.push(' ');
                }
            } else if depth > 0 {
                if c == '/' && next == '*' {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                    continue;
                }
                if c == '*' && next == '/' {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    continue;
                }
                out.push(if c == '\n' { '\n' } else { ' ' });
            } else if string {
                out.push(c);
                if escape {
                    escape = false;
                } else if c == '\\' {
                    escape = true;
                } else if c == '"' {
                    string = false;
                }
            } else if c == '"' {
                string = true;
                out.push(c);
            } else if c == '/' && next == '/' {
                line = true;
                out.push_str("  ");
                i += 2;
                continue;
            } else if c == '/' && next == '*' {
                depth = 1;
                out.push_str("  ");
                i += 2;
                continue;
            } else {
                out.push(c);
            }
            i += 1;
        }
        out
    }

    /// THE STRIPPER KEEPS CODE AND LOSES PROSE, and the cases are the ones that bit.
    #[test]
    fn strip_comments_keeps_the_code_and_the_shape() {
        assert_eq!(strip_comments("a /* x, y */ b").replace(' ', ""), "ab");
        assert_eq!(strip_comments("a // x, y\nb").replace(' ', ""), "a\nb");
        assert_eq!(
            strip_comments("let s = \"// not, a comment\";"),
            "let s = \"// not, a comment\";",
            "a comment marker inside a string is code"
        );
        assert_eq!(
            strip_comments("a /* one /* two */ still */ b").replace(' ', ""),
            "ab",
            "rust block comments nest, and a stripper that stops at the first close leaves the tail"
        );
        let src = "x /* a\nb */ y";
        assert_eq!(
            strip_comments(src).lines().count(),
            src.lines().count(),
            "line count must survive or every quoted line number in a failure is wrong"
        );
        assert_eq!(
            strip_comments(src).len(),
            src.len(),
            "byte offsets must survive or the arm bounds move"
        );
    }
    /// THE SCREEN THE APP OPENS ON IS ALREADY IN THE STATE ITS DESTINATION NAMES.
    ///
    /// `on_view` is what puts a shared screen into that state, and it used to run only from
    /// `enter`, which is navigation. The startup body never navigates, so it never ran: Log Parser
    /// is the fights destination and the app opened it showing the KILL TRACKER, with the rail
    /// lighting Fights beside it. The rail and the body disagreed about where the reader was, on
    /// the first frame, every launch.
    ///
    /// NOTHING IN THE SUITE COULD SEE IT, which is why this is here rather than an assertion added
    /// to something existing: every other test either calls `on_view` itself or draws a screen it
    /// put into state by hand, so all of them were testing the navigated path and none of them the
    /// arrived-at one.
    ///
    /// IT READS THE SCREEN BACK rather than asserting the call happened, so an `App::default` that
    /// reorders its work still has to leave the parser on Fights.
    #[test]
    fn the_app_opens_with_its_first_screen_already_in_the_right_state() {
        let app = App::new(None, &egui::Context::default());
        assert_eq!(
            app.body,
            Body::Screen(ScreenId::Parser),
            "this test is about the startup body; if that moved, move the expectation with it"
        );
        let on = app.screens.parser.showing();
        assert_eq!(
            on, 2,
            "the app opened Log Parser on view {on}, and Log Parser is the fights destination. \
             View 0 is the kill tracker, which is Hunt Journal's page, drawn under a rail that is \
             lighting Fights"
        );
    }

    /// DEFECT: THIRTEEN TAB BUTTONS THAT LIT WHEN PRESSED AND MOVED NOTHING.
    ///
    /// # WHAT THE OWNER SAW
    ///
    /// A screenshot of the Dashboards page with `DPS Healer Tank Pet Solo Group Raid Leader
    /// Custom` on it TWICE, six pixels apart. The top row is the shell's, drawn from
    /// `nav::SECTIONS`; the bottom was the page's own. Only the bottom one worked, because
    /// `on_tab` was `let _ = tab;` under a comment reading "TIER 4 REACHES NO SCREEN IN THIS
    /// BUILD". A reader cannot tell two identical rows apart except by pressing both.
    ///
    /// AND THE OTHER THREE SECTIONS WERE WORSE, because they had no working row underneath to
    /// rescue them. Live offered `Encounter Status`, naming the health bar this app refuses to
    /// draw for want of a denominator. Logs offered `Raw Log` and `Search & Filters` directly
    /// above the section explaining that the ingest publishes no lines, so there is nothing to
    /// show and nothing to filter. Reports offered `Group` and `Raid`, which `reports` has a
    /// guard specifically to keep off that page.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// Every tab the rail draws over a parser page is a view that page HAS, named the way the
    /// page names it. Two sections have rows because they have views; the other two have none.
    /// A list of literals compared against a list of literals would pass while both were wrong,
    /// so the one row that has tabs is taken from the page's own vocabulary: `reports::TABS`.
    ///
    /// # DASHBOARDS MOVED FROM THE FIRST GROUP TO THE SECOND
    ///
    /// It carried eight role tabs and they are gone: a role is a claim about the person reading
    /// and nothing in a log line makes one, so eight tabs reordered one set of panels and two of
    /// them refused outright. The page is a grid of tiles the reader arranges. The controls that
    /// replaced the tabs are a lock and a widget picker, which act on the page's LAYOUT rather
    /// than switching between views of it, so they are not tabs and are not in this table.
    ///
    /// WHAT MUTATION MAKES THIS RED: putting a tab back over Live or Logs, reordering either
    /// live row, or renaming a tab on one side only.
    #[test]
    fn the_parser_tab_rows_are_the_pages_own() {
        let tabs_of = |name: &str| -> &'static [&'static str] {
            nav::sections_of(ScreenId::Parser)
                .iter()
                .find(|(n, _, _)| *n == name)
                .map(|(_, _, t)| *t)
                .unwrap_or_else(|| panic!("LOG PARSER has no {name} section"))
        };

        /* THE ONE THAT HAS VIEWS, taken from the page rather than retyped. */
        assert_eq!(
            tabs_of("Reports"),
            screens::reports::TABS,
            "the rail's Reports tabs are not the page's own"
        );

        /* AND THE THREE THAT HAVE NONE DRAW NONE. */
        for name in ["Dashboards", "Live", "Logs"] {
            assert!(
                tabs_of(name).is_empty(),
                "{name} has one view and a tab row over it: {:?}",
                tabs_of(name)
            );
        }
    }

    /// AND PRESSING ONE ACTUALLY MOVES THE PAGE, THROUGH THE DOOR PRODUCTION USES.
    ///
    /// # THE FIRST VERSION OF THIS TEST PASSED ON A DEAD PATH
    ///
    /// It called `on_tab(ScreenId::ParserReports, want, &mut s)` directly. Nothing in the running
    /// app ever calls it with that id: `on_view` is only ever handed the BODY, `NAV` has one Log
    /// Parser row and it is `ScreenId::Parser`, and a section's own screen is reached by
    /// `draw_screen`'s recursion without the body changing. So the arm this exercised was
    /// unreachable, the eleven context-bar tabs went on lighting and moving nothing, and this test
    /// was green through all of it.
    ///
    /// A TEST THAT SUPPLIES AN INPUT PRODUCTION CANNOT PRODUCE proves the function works and
    /// nothing whatever about whether it runs. That is this tree's signature defect wearing a
    /// green tick, and it was written into the fix for the same defect.
    ///
    /// SO IT ENTERS AT `on_view`, WITH A BODY AND A SECTION INDEX, which is the shape `App::update`
    /// passes and the only shape that exists. The section is looked up by name, for the reason the
    /// last literal index in this file went: the parser's section order has already moved once.
    ///
    /// WHAT MUTATION MAKES THIS RED: `on_tab(id, tab, s)` in `on_view`, which is the defect this
    /// replaces, or emptying `on_tab`.
    #[test]
    fn pressing_a_parser_tab_moves_the_page_it_names() {
        let at = |name: &str| {
            nav::sections_of(ScreenId::Parser)
                .iter()
                .position(|(n, _, _)| *n == name)
                .unwrap_or_else(|| panic!("LOG PARSER has no {name} section"))
        };

        for (name, n) in [("Reports", screens::reports::TABS.len())] {
            let section = at(name);
            let mut s = Screens::default();
            let read = |s: &Screens| s.reports.showing();
            /* EVERY TAB, AND EACH LANDS ON ITS OWN INDEX. An arm that always set zero would pass
             * a test that only pressed the first. */
            for want in 0..n {
                on_view(ScreenId::Parser, section, want, &mut s);
                assert_eq!(
                    read(&s),
                    want,
                    "pressing {name} tab {want} did not reach the page"
                );
            }
        }
    }

    /// AND THE ROUTER AND THE PAINTER AGREE ABOUT WHICH SCREEN A SECTION OPENS.
    ///
    /// `draw_screen` hops to a section's own screen and `on_view` routes the tab to one. While
    /// those were two separate readings of `nav::SECTIONS` they disagreed, silently, for every
    /// parser section. They are one function now and this is what says so.
    ///
    /// WHAT MUTATION MAKES THIS RED: `target_of` returning `id` unconditionally.
    #[test]
    fn the_screen_a_section_paints_is_the_screen_its_tabs_are_routed_to() {
        let mut hops = 0;
        for (body, sections) in nav::SECTIONS {
            for (i, (name, inner, _)) in sections.iter().enumerate() {
                let got = target_of(*body, i);
                match inner {
                    Some(screen) => {
                        hops += 1;
                        assert_eq!(
                            got, *screen,
                            "{body:?} / {name} paints {screen:?} and routes its tabs to {got:?}"
                        );
                    }
                    /* A section with no screen of its own is drawn by the body, so that is where
                     * its tabs go too. */
                    None => assert_eq!(got, *body, "{body:?} / {name}"),
                }
            }
        }
        assert!(
            hops > 4,
            "only {hops} sections open a screen of their own, so this is passing by not looking"
        );
    }

    /// DEFECT: A HEADER CONTROL DRAWN FOR A SCREEN THAT IS NOT THE ONE SHOWING.
    ///
    /// # WHAT THIS REPLACES, AND WHY THE OLD ONE COULD NOT SURVIVE
    ///
    /// This slot held `a_tab_that_has_a_reason_can_be_reached_from_the_row_that_draws_it`, which
    /// pinned the wire between the Dashboards role tabs and the paragraphs `role_hover` carried
    /// for them. The roles are gone, so the wire it guarded is gone with them.
    ///
    /// THE DEFECT IT GUARDED AGAINST IS NOT. The two controls that replaced those tabs, the lock
    /// and the Widgets button, are drawn by the SHELL and act on ONE screen, and the shell knows
    /// which screen is showing through `target_of` and not through the body: LOG PARSER /
    /// Dashboards paints `ParserDashboards` while `Body::Screen` stays `Parser`. Asking the body
    /// would have put a dashboard's lock on Live, Fights, Reports and Logs as well, on a page
    /// with no layout to hold.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// The same predicate the bar builds, over every section of every destination: it is true
    /// for exactly one of them, and that one is the Dashboards section of the log parser.
    ///
    /// WHAT MUTATION MAKES THIS RED: testing the body instead of `target_of`, comparing against
    /// `ScreenId::Parser`, or dropping the guard so every screen draws the lock.
    #[test]
    fn the_layout_controls_are_drawn_for_one_screen_and_it_is_the_dashboard() {
        let mut found: Vec<(ScreenId, &str)> = Vec::new();
        for (body, sections) in nav::SECTIONS {
            for (i, (name, _, _)) in sections.iter().enumerate() {
                if target_of(*body, i) == ScreenId::ParserDashboards {
                    found.push((*body, name));
                }
            }
        }
        assert_eq!(
            found,
            vec![(ScreenId::Parser, "Dashboards")],
            "the lock and the Widgets button are drawn over a screen that does not own a \
             layout, or over none at all"
        );

        /* AND THE SECTION THEY BELONG TO CARRIES NO TAB ROW, so the row they sit on is the one
         * the owner laid out: a crumb and a picture-in-picture on the left, these two on the
         * right, and nothing in between. */
        assert!(
            nav::tabs_of(
                ScreenId::Parser,
                nav::sections_of(ScreenId::Parser)
                    .iter()
                    .position(|(n, _, _)| *n == "Dashboards")
                    .expect("the log parser has a Dashboards section")
            )
            .is_empty(),
            "the Dashboards section grew a tab row back, which is the eight roles returning"
        );
    }

    /// DEFECT: A BREADCRUMB THAT SAYS SOMETHING THE RAIL DOES NOT.
    ///
    /// The crumb is the only thing on the left of the context header now, so it is the only
    /// thing telling a reader where he is standing on that row. Every word in it has to come out
    /// of `NAV` and `SECTIONS`, or it is a fourth copy of a phrase that can drift from the rail
    /// six inches to its left.
    ///
    /// WHAT MUTATION MAKES THIS RED: spelling a destination in `crumb_of`, dropping the section
    /// part, or an empty crumb for any body this shell can be in.
    #[test]
    fn the_breadcrumb_is_the_rails_own_words() {
        let mut section = [0usize; ScreenId::ALL.len()];

        /* THE ONE THE OWNER ASKED FOR, BY NAME. */
        let dash = nav::sections_of(ScreenId::Parser)
            .iter()
            .position(|(n, _, _)| *n == "Dashboards")
            .expect("the log parser has a Dashboards section");
        section[ScreenId::Parser.ordinal()] = dash;
        assert_eq!(
            crumb_of(Body::Screen(ScreenId::Parser), &section),
            vec!["Log Parser", "Dashboards"],
            "the crumb the owner asked for is not what the header draws"
        );

        /* AND IT FOLLOWS THE SECTION rather than naming one place forever. */
        let logs = nav::sections_of(ScreenId::Parser)
            .iter()
            .position(|(n, _, _)| *n == "Logs")
            .expect("the log parser has a Logs section");
        section[ScreenId::Parser.ordinal()] = logs;
        assert_eq!(
            crumb_of(Body::Screen(ScreenId::Parser), &section),
            vec!["Log Parser", "Logs"]
        );

        /* EVERY DESTINATION THE RAIL CAN REACH SAYS SOMETHING, and every word of it is the
         * rail's own. A crumb that came back empty is a header that silently says nothing. */
        let mut n = 0;
        for (_, rows) in nav::NAV {
            for (name, id) in *rows {
                let got = crumb_of(Body::Screen(*id), &[0usize; ScreenId::ALL.len()]);
                assert!(!got.is_empty(), "{id:?} has no crumb");
                assert_eq!(
                    got[0], *name,
                    "{id:?} is spelled differently from its rail row"
                );
                if let Some(part) = got.get(1) {
                    assert!(
                        nav::sections_of(*id).iter().any(|(s, _, _)| s == part),
                        "{id:?} has a crumb naming a section it does not have: {part}"
                    );
                }
                n += 1;
            }
        }
        assert!(
            n > 20,
            "only {n} rows were looked at, so this is passing by not looking"
        );

        /* AND SETTINGS, WHICH IS NOT IN `NAV` AT ALL. */
        assert_eq!(crumb_of(Body::Settings, &section), vec!["Settings"]);
    }

    /// DEFECT: THE LOG ONLY BEING READ WHILE THE WINDOW WAS BEING DRAWN.
    ///
    /// # "its not updating in real fucking time"
    ///
    /// `Ingest::tail` was called once per frame from `ui`, and eframe runs `ui` only while there
    /// is something to draw. `logic` is the callback it runs when there is NOT. So with the game
    /// in front and this window behind it, the file was never read: the live fold never advanced,
    /// and the Live header, every overlay and every counter held whatever they had when the window
    /// last painted, then jumped in one step when it came forward.
    ///
    /// AN ALWAYS-ON-TOP OVERLAY IS READ WHILE THE GAME HAS FOCUS BY DEFINITION, so this was the
    /// app failing in exactly the case it exists for.
    ///
    /// # WHY A TEST AND NOT JUST THE MOVE
    ///
    /// This is the SECOND time this exact shape has bitten this file: `Hotkeys::poll` lived in
    /// `ui` for the same reason and had to move to `heartbeat` for the same reason, and the doc on
    /// `logic` records it. A rule that has been broken twice needs something that says so out loud
    /// rather than a third comment.
    ///
    /// READ OUT OF THE SOURCE, because what is being asserted is WHERE a call lives, and no
    /// runtime harness can tell `ui` from `logic` without an event loop.
    ///
    /// WHAT MUTATION MAKES THIS RED: moving the poll back into `ui`, or dropping it.
    #[test]
    fn the_log_is_polled_from_the_callback_that_runs_when_nothing_is_drawn() {
        let src = include_str!("main.rs");

        let body_of = |name: &str| -> &str {
            let at = src
                .find(name)
                .unwrap_or_else(|| panic!("{name} is gone from this file"));
            /* To the next item at the same indentation, which is the next `    fn ` or the impl's
             * closing brace. Crude and enough: these two are short. */
            let rest = &src[at..];
            let end = rest[1..]
                .find("\n    fn ")
                .map(|i| i + 1)
                .unwrap_or(rest.len());
            &rest[..end]
        };

        let heartbeat = body_of("fn heartbeat(&mut self, ctx: &egui::Context) {");
        assert!(
            heartbeat.contains("self.ingest.tail()"),
            "the log poll is not in `heartbeat`, so it does not run while the window is behind \
             the game and every live number freezes until it comes forward"
        );

        /* AND `logic` REACHES IT. The poll is only as good as the callback that carries it. */
        let logic =
            body_of("fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {");
        assert!(
            logic.contains("self.heartbeat(ctx)"),
            "the no-draw callback stopped calling `heartbeat`, so nothing polls the log when the \
             window is not painting"
        );
    }

    /// DEFECT THIS PREVENTS: THE UPDATER BEING PUMPED ONLY WHILE A FRAME IS BEING DRAWN.
    ///
    /// THE THIRD TIME THIS FILE HAS HAD TO LEARN IT. `Hotkeys::poll` shipped in `ui` and the
    /// hotkeys stopped working when the window was behind the game; `Ingest::tail` shipped in `ui`
    /// and every live number froze in exactly the state the app exists for. Both write-ups are at
    /// `main.rs:1709-1760` and both have a source-text test standing over them now. This is the
    /// same test for the same mistake.
    ///
    /// # WHY IT IS WORSE HERE THAN IT LOOKS
    ///
    /// A missed pump is not a check that happens late. `pump` is the ONLY path by which the worker
    /// learns what the encounter is doing, and the worker asks that question at the instant it is
    /// about to start a ten megabyte download. Pumped from `ui`, a reader who is IN THE GAME with
    /// this window behind it draws no frames, so the worker would be holding whatever pulse was
    /// true the last time the window was in front, which on a raid night is `Closed` from before
    /// the pull. The gate would then pass during the fight it exists to protect.
    ///
    /// # AND THE SETTINGS WOULD GO STALE THE SAME WAY
    ///
    /// `pump` is also the only path by which the two controls in the UPDATES section reach the
    /// worker. Pumped from `ui` alone they would still work, because a person changing them is by
    /// definition looking at the screen, which is the half that makes this defect easy to miss in
    /// a review and impossible to miss in a raid.
    ///
    /// WHAT MUTATION MAKES THIS RED: move the `self.updater` block out of `heartbeat` and into
    /// `ui`; or delete it; or stop `logic` calling `heartbeat`, which is the one the other two
    /// tests share and which would silently undo all three at once.
    #[test]
    fn the_updater_is_pumped_from_the_callback_that_runs_when_nothing_is_drawn() {
        let src = include_str!("main.rs");

        /* BOUNDED TO ONE FUNCTION, AND THAT IS NOT TIDINESS. A search over the whole file finds
         * its own assertion: the string `"self.updater.pump"` written in this test is itself
         * source text, so `src.contains(..)` would stay true with the call deleted from
         * `heartbeat`. A mutation run against the trampoline's test found exactly that, and it
         * was green for a build that never cleared the failed-launch count. */
        let body_of = |name: &str| -> &str {
            let at = src
                .find(name)
                .unwrap_or_else(|| panic!("{name} is gone from this file"));
            let rest = &src[at..];
            let end = rest[1..]
                .find("\n    fn ")
                .map(|i| i + 1)
                .unwrap_or(rest.len());
            &rest[..end]
        };

        let heartbeat = body_of("fn heartbeat(&mut self, ctx: &egui::Context) {");
        assert!(
            heartbeat.contains("u.pump("),
            "the updater is not pumped from `heartbeat`, so the worker only learns what the \
             encounter is doing while a frame is being drawn, and a download can start mid-pull \
             on a window that is behind the game"
        );
        assert!(
            heartbeat.contains("self.ingest.pulse()"),
            "`heartbeat` hands the updater something other than the real pulse; the fight gate is \
             `Ingest::pulse` and nothing else"
        );
        assert!(
            !heartbeat.contains("fight_is_live"),
            "the gate is written as `fight_is_live`, which is `pulse().fighting()` and is already \
             false during `Pulse::Holding`, the six seconds after a kill with the next mob \
             incoming"
        );

        let ui_body = body_of("fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {");
        assert!(
            !ui_body.contains("u.pump("),
            "the updater is pumped from `ui` as well, so there are two answers to where this \
             happens and the one that runs when nothing is drawn is not obviously the one that \
             matters"
        );

        /* AND `logic` REACHES IT. The pump is only as good as the callback that carries it, which
         * is the same sentence the log poll's test ends on. */
        let logic =
            body_of("fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {");
        assert!(
            logic.contains("self.heartbeat(ctx)"),
            "the no-draw callback stopped calling `heartbeat`, so nothing pumps the updater when \
             the window is not painting"
        );
    }

    /// DEFECT THIS PREVENTS: THE TRAMPOLINE EXISTING AND NOT BEING CALLED.
    ///
    /// `updater::launch::plan` has tests for every branch, and every one of them would still pass
    /// with `trampoline()` deleted from `main`, because a rule nothing calls is a rule that
    /// answers correctly and changes nothing. That is this tree's signature defect, and it is why
    /// the SAME pair of source-text tests already stands over `Hotkeys::poll` and `Ingest::tail`.
    ///
    /// THREE THINGS HAVE TO HOLD AND EACH ONE FAILS DIFFERENTLY.
    ///
    ///   * `main` calls it. Without this the app never runs an installed update at all: it starts,
    ///     it works, and it is the version the installer wrote, forever.
    ///   * It is called BEFORE `run_native`. After it, a window would already be open and the
    ///     reader would see two.
    ///   * `ui` clears the failed-launch count. Without this half, every machine that ever takes
    ///     an update rolls it back after two launches, whether or not anything was wrong with it.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete the `trampoline()` line from `main`; move it below
    /// `eframe::run_native`; or delete the `note_first_frame` call from `ui`.
    #[test]
    fn the_launch_actually_asks_which_binary_it_should_be() {
        let src = &strip_comments(include_str!("main.rs"));

        /* EACH SEARCH IS BOUNDED TO THE FUNCTION IT IS ABOUT, and that is not tidiness. A
         * source-text test that searches the WHOLE file finds its own assertion: the string
         * `"note_first_frame"` written here is itself source text, so `src.contains(..)` would be
         * true with the call deleted from `ui`. A mutation run found exactly that, and this test
         * was green for a build that never cleared the count. */
        let body_of = |head: &str, end: &str| -> String {
            let at = src
                .find(head)
                .unwrap_or_else(|| panic!("{head} is gone from this file"));
            let rest = &src[at..];
            let to = rest
                .find(end)
                .unwrap_or_else(|| panic!("{end} no longer follows {head}"));
            rest[..to].to_owned()
        };

        let main_body = body_of("fn main() -> eframe::Result<()> {", "\nfn trampoline() {");
        let called = main_body
            .find("trampoline();")
            .expect("`main` does not call `trampoline`, so an installed update is never run");
        let ran = main_body
            .find("eframe::run_native(")
            .expect("`main` no longer calls run_native");
        assert!(
            called < ran,
            "`trampoline` is called after `run_native`, so a window is already open when the \
             second process starts"
        );

        /* AND A PREFLIGHT DOES NOT BOUNCE. See the long note at the call site: a staged payload
         * sits under `update\staging\`, which `plan`'s payload-position guard does not cover, so
         * a preflight that trampolined would measure whichever binary `current.json` names and
         * would leave `launches_failed` one higher with nothing able to clear it, because the
         * half that clears it is guarded by `self.smoke.is_none()`. Two installs on one machine
         * then roll back a build that works. */
        let guard = main_body.find("if smoke.is_none() {").expect(
            "`main` calls `trampoline` unguarded, so every preflight bounces to a different \
                 binary and counts a launch it can never clear",
        );
        assert!(
            guard < called,
            "the smoke guard is written after the `trampoline()` call rather than around it"
        );
        assert!(
            main_body
                .find("let smoke = smoke_from_env();")
                .is_some_and(|at| at < guard),
            "the smoke switch is read after the guard that tests it"
        );

        let heartbeat = body_of("fn heartbeat(&mut self, ctx: &egui::Context) {", "\n    fn ");
        assert!(
            heartbeat.contains("note_first_frame"),
            "nothing clears the failed-launch count, so every machine that takes an update rolls \
             it back after two launches"
        );

        /* AND IT IS NOT IN `ui`, WHICH IS WHERE IT WAS AND WHY IT WAS WRONG. See the note in
         * `heartbeat`: the increment happens on every launch and the clear happened only on
         * launches that reached a second VISIBLE frame, so two launches with the window minimized
         * or never composited left a perfectly good build at the rollback threshold. The app
         * minimizes ITSELF on Ctrl+Alt+G, so that is an ordinary evening rather than a corner. */
        let ui_body = body_of(
            "fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {",
            "\n    fn ",
        );
        assert!(
            !ui_body.contains("note_first_frame"),
            "the failed-launch count is cleared from `ui`, which eframe calls only while the \
             viewport is visible; a launch the reader immediately put away with Ctrl+Alt+G would \
             be counted as one that never drew"
        );
        assert!(
            heartbeat.contains("self.passes == 2"),
            "the clear is not guarded on the app having completed a pass, so either it clears \
             before the first frame (which calls a glow or wgpu crash a success) or it never does"
        );
    }

    /// DEFECT THIS PREVENTS: A PREFLIGHT THAT NEVER BECOMES VISIBLE NEVER EXITING, AND WEDGING THE
    /// UPDATE WORKER FOR THE REST OF THE SESSION.
    ///
    /// # THE TWO HALVES, AND THIS IS THE CHILD'S
    ///
    /// `install::Spawn` runs the staged binary with `GRIMOIRE_SMOKE_MS` set and waits for it to
    /// exit. The only thing that makes it exit is `App::smoke`, which was called from `App::ui`
    /// alone. This file's own `logic` doc states the rule: eframe calls `ui` only while the
    /// viewport is visible and calls `logic` otherwise. A preflight is the launch MOST likely to
    /// be invisible: the reader presses Install while EverQuest is fullscreen-exclusive, or over
    /// RDP, or the window opens minimized. In every one of those the child ran for ever, the
    /// parent's `output()` waited for ever, the Settings screen froze on `Downloaded`, and quitting
    /// Grimoire left an orphan process holding a log tail, a watcher poll and a hotkey
    /// registration with no window the reader could find.
    ///
    /// The parent's half is `install::wait_within`, which has a test of its own that spawns a real
    /// process and kills it. This is the half that cannot be reached from a test at all, because
    /// `App::ui` and `App::logic` both need an `eframe::Frame` and that type has no public
    /// constructor. A source-text floor is what is available, and it is the same instrument the
    /// three tests above use for the same class of mistake, which this file has now made four
    /// times.
    ///
    /// WHAT MUTATION MAKES THIS RED: delete `self.smoke(ctx)` from `logic`.
    #[test]
    fn the_smoke_clock_runs_in_the_callback_that_runs_when_nothing_is_drawn() {
        let src = &strip_comments(include_str!("main.rs"));
        let at = src
            .find("fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {")
            .expect("the no-draw callback is gone from this file");
        let rest = &src[at..];
        let logic = &rest[..rest[1..].find("\n    fn ").map(|i| i + 1).unwrap_or(rest.len())];
        assert!(
            logic.contains("self.smoke(ctx)"),
            "the smoke clock runs only while the window is visible, so a preflight launched over a \
             fullscreen game never exits and the update worker waits for it for ever"
        );
    }
}
