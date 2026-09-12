//! Settings screen and persisted config: hotkeys, data path. Decisions D2, D4, D6, D7.
//!
//! THE CHANNEL IS NOT A SETTING, AND THIS FILE IS WHERE THAT IS DECIDED.
//! This build is for one channel. The title strip paints its name (`BROKEN STOIC`, with `BUILT
//! FOR` on the maker mark's hover, see `titlebar::maker_block`), the Watch screen is that
//! channel's window, and [`TWITCH_HANDLE`] and [`YOUTUBE_HANDLE`] below are what every surface
//! reads. They used to be two `String` fields on `Settings` with two text boxes
//! on this screen, which is a setting that can only ever be wrong: pointing this app at somebody
//! else does not give you their app, it gives you a broken one whose title strip contradicts
//! its own status pill. The owner asked for the boxes to go. They are gone, the fields with them,
//! and `LEGACY_KEYS` cleans the retired keys out of files written by the builds that had them.
//!
//! AND THE SECTION THAT SURVIVED THE BOXES IS GONE TOO, so the Settings SCREEN now says nothing
//! about the channel at all. Losing the editors left a `CHANNEL` heading over a sentence and two
//! read only rows: which login on Twitch, which on YouTube, and how often either is polled. Every
//! word of it was true and not one word of it was a decision, which is what a settings screen is
//! for, so it went. The facts did not evaporate with it: the two handles are these constants, a
//! reader of the code meets them here, and the poll cadence is on the Watch screen's `Check now`
//! hover, where somebody is deciding whether to wait for the next tick. That leaves this header
//! as the prose about the channel, and there is no `channel` function on the screen any more.
//! `the_settings_screen_carries_nothing_about_the_channel` holds the screen to it.
//!
//! WHAT IS PERSISTED AND WHERE.
//! One JSON file at `<config_dir>/eql-grimoire/settings.json`, which on Windows is
//! `%APPDATA%\eql-grimoire\settings.json`. The typed fields are the five the contract names; every
//! other key in the file rides in `extra` untouched, so a lane that stores its own state under a
//! key of its own (`lfg_board`, `priorities`) neither has to edit this struct nor loses that state
//! when this struct saves. `#[serde(default)]` on the container is what makes a partial or older
//! file load: a missing field takes its default rather than failing the whole read.
//!
//! WHY LOAD NEVER FAILS BUT REMEMBERS THAT IT SHOULD HAVE.
//! An app that refuses to start because its settings file is malformed has traded a small problem
//! for a large one. So `load` always returns a usable `Settings`, and if the file was there and could
//! not be read, the reason is carried in `load_problem` and printed on the Settings screen in the
//! WRONG colour, where the person who can fix it will see it.
//!
//! AND WHY REMEMBERING IS NOT ENOUGH ON ITS OWN. Carrying the reason was the whole guard once,
//! and it did not hold: `load_problem` is READ by two surfaces and the file is WRITTEN by five,
//! so the three writers that never look at it (the gear screen, the LFG board, the valet) and the
//! always-on-top pin, which is not even a screen, each wrote the defaults `load` handed them over
//! a file they could not read, and `save_to`'s rename made that atomic and final. A hand edited
//! `"always_on_top": "true"` cost the log folder, the hotkey table and another lane's
//! `lfg_board`, with nothing left to recover from. So the guard is no longer a string a caller
//! may consult: [`Settings::save_to`] READS WHAT IS THERE and refuses to replace bytes it could
//! not parse, which every writer reaches because every writer goes through it. The Settings screen
//! alone takes [`Settings::save_replacing_unreadable`], because it is the one surface that says
//! what is about to happen before it happens and only writes because a person typed into it, and
//! even that keeps the unreadable bytes beside the file first.
//!
//! A TEST NEVER WRITES THE OPERATOR'S FILE. `save()` resolves the platform path, and a unit test
//! that reaches it through any helper (a screen's "remember this" call, say) would overwrite
//! `%APPDATA%\eql-grimoire\settings.json` with whatever fixture it was holding. That happened once:
//! a valet test wrote its fixture over James's saved folders. So under `cfg(test)` `save()` refuses
//! outright and says so; a test that wants a round trip through disk uses `save_to` on a scratch
//! path, which is the only honest way to test a write anyway.

use crate::chrome::State;
use crate::hotkeys::Binding;
use crate::ingest::Source;
use crate::screens::Cx;
use crate::theme::*;
use crate::windows::{LfgMode, Tool};
use egui::{FontId, RichText, Stroke, StrokeKind, Ui, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The channel's Twitch login, `twitch.tv/Broken_Stoic`. Decision D2 names it; nothing else may.
///
/// THE ID IS HERE BECAUSE THE HANDLE IS NOT PROOF. A login can be changed by whoever owns it, and
/// a constant that silently starts naming a different channel is worse than a text box. Twitch
/// user id `29737511` is the account this login belonged to when it was resolved (2026-09-02), so
/// years from now the constant is checkable rather than merely plausible.
pub const TWITCH_HANDLE: &str = "Broken_Stoic";
/// The same channel on YouTube, `youtube.com/@broken_stoic` ("Broken Stoic").
///
/// Channel id `UCf4fNJTJt8F1MZAQ2iqIZ9A`, `canonicalBaseUrl` `/@broken_stoic`, resolved and
/// verified 2026-09-02. The id is the checkable half, for the reason [`TWITCH_HANDLE`] gives.
///
/// STORED WITHOUT THE DISPLAY `@`. YouTube shows `@broken_stoic`; every reader here builds either
/// a `/@{handle}` URL or a validated login, and both want the bare form, so the bare form is what
/// is stored and `watcher::youtube_login` is held to it by a test.
pub const YOUTUBE_HANDLE: &str = "broken_stoic";

/// THIS APP'S OWN TWITCH APPLICATION ID, WHICH IS PUBLIC AND IS NOT A SECRET.
///
/// Registered by the owner on 2026-09-04 and VERIFIED HERE RATHER THAN TRUSTED: a Twitch Client
/// ID and a Twitch Client SECRET are both thirty lowercase alphanumeric characters and are
/// indistinguishable by shape, so this value was put to `POST id.twitch.tv/oauth2/device`, which
/// answered `200` with a `device_code`, an `interval` of 5 and a `user_code`. A secret is not
/// accepted as a `client_id`, so that response is the proof, and it is why this constant may sit
/// in source at all.
///
/// IN SOURCE, ON TWITCH'S OWN INSTRUCTION. Their registration guide says it plainly: "Client IDs
/// are considered public and can be embedded in a web page's source." It is not a credential and
/// nothing about it needs protecting; what needs protecting is the TOKEN a sign-in returns, which
/// never comes near this file. The same guide's other rule is why this is a constant and not a
/// shared value: "Do not share client IDs among applications; each application must have its own
/// client ID."
///
/// WHAT IT CANNOT DO ON ITS OWN. Nothing. It identifies the application, not a person: no request
/// carrying only this can read a message, send one, or see anything about an account. Sending
/// needs a USER token, which only exists after the owner types a code on `twitch.tv/activate`.
///
/// IF THE APP IS RE-REGISTERED, THIS CHANGES. There is no way to check it offline, so a wrong
/// value fails at the first device request with a named refusal rather than silently.
pub const TWITCH_CLIENT_ID: &str = "qkb5teaofmti7b2gk1xkz0t3uqls8g";

/// What the app asks permission for, and it is the SHORTEST LIST THAT SENDS A MESSAGE.
///
/// `chat:read` and `chat:edit` are the two IRC scopes: read the room, and speak in it. Nothing
/// here asks for `channel:*`, `moderator:*`, `user:read:email` or anything that touches the
/// account itself, because the app does none of those things and an authorisation screen listing
/// permissions a program does not use is how people learn to stop reading them.
///
/// SPACE DELIMITED, which is what `POST /oauth2/device` documents for its `scopes` parameter.
/// Note the plural: Twitch spells this field `scopes` where most services spell it `scope`.
pub const TWITCH_CHAT_SCOPES: &str = "chat:read chat:edit";

/// The channel's NAME, the way a person writes it: `Broken Stoic`, with a space.
///
/// A HANDLE IS NOT A NAME, AND ONE ROW OF THIS APP WAS PRINTING BOTH AS IF IT WERE.
/// [`TWITCH_HANDLE`] is a LOGIN. It carries an underscore because a Twitch login may not carry a
/// space, and it is what every URL and every poll has to send. The live pill printed that login
/// (`Broken_Stoic`) while the title strip five pixels away set a hand written literal
/// (`BROKEN STOIC`), so one channel had two spellings on one row and the pill's was the machine's.
/// This constant is the one source for the name a reader sees. The handles do not change, because
/// the network needs them exactly as they are.
///
/// IT IS NOT INVENTED. `youtube.com/@broken_stoic` answers with `"title":{"simpleText":"Broken
/// Stoic"}` in its own channel header, fetched and read on 2026-09-02, so this is the channel's
/// own spelling of itself and not a guess at how to unpick an underscore.
///
/// THE STRIP DERIVES ITS CAPS FROM THIS instead of holding a second literal:
/// `titlebar::maker_block` upper-cases it, so `BROKEN STOIC` cannot drift from `Broken Stoic`
/// again without a compile.
pub const DISPLAY_NAME: &str = "Broken Stoic";

/// The YouTube CHANNEL ID, `UCf4fNJTJt8F1MZAQ2iqIZ9A`, resolved and verified against the live
/// channel page on 2026-09-02 (it occurs there four times).
///
/// A LINK USES THIS AND NOT THE HANDLE. `@broken_stoic` can be changed by whoever owns it, and a
/// link built from a stale handle lands on a 404 or, worse, on whoever picked the name up; a `UC`
/// id is assigned once and is never reassigned. The WATCHER still fetches `/@{handle}/live`,
/// because that page is what carries the live markers it reads and there is no `/channel/` form of
/// it that does, so both spellings live here and each is used for the one thing it is right for.
pub const YOUTUBE_CHANNEL_ID: &str = "UCf4fNJTJt8F1MZAQ2iqIZ9A";

/// Where a YouTube link goes. See [`YOUTUBE_CHANNEL_ID`] for why it is the id form.
pub fn youtube_channel_url() -> String {
    format!("https://www.youtube.com/channel/{YOUTUBE_CHANNEL_ID}")
}

/// Which of the two platforms this person would rather watch on. Persisted as `watch_on`.
///
/// THIS IS A REAL SETTING, WHICH THE HANDLES ABOVE ARE NOT. Pointing the app at another channel
/// can only ever make it wrong; preferring YouTube to Twitch is a thing a person may simply want,
/// and both answers are correct. It became answerable only when the YouTube poll started running:
/// the handle used to default to empty, so `watcher::Status::youtube` was permanently unchecked
/// and a preference would have had one working position and one dead one.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[default]
    Twitch,
    YouTube,
}

impl Platform {
    /// Every platform, in the order they are offered and drawn. Iterated rather than listed by
    /// hand, so a third one is added in one place and appears everywhere at once.
    pub const ALL: [Platform; 2] = [Platform::Twitch, Platform::YouTube];

    /// The word on screen. The platform's own spelling, capital Y and all.
    pub fn label(self) -> &'static str {
        match self {
            Platform::Twitch => "Twitch",
            Platform::YouTube => "YouTube",
        }
    }

    /// The word in the file, which is `label` lower cased by `rename_all` above. Written out
    /// rather than computed because the derived `Serialize` is the authority and a `to_lowercase`
    /// here would be a second opinion; a test holds the two together.
    pub fn key(self) -> &'static str {
        match self {
            Platform::Twitch => "twitch",
            Platform::YouTube => "youtube",
        }
    }

    /// The platform a stored key names, or None when this build does not know it.
    pub fn from_key(s: &str) -> Option<Platform> {
        Platform::ALL.into_iter().find(|p| p.key() == s)
    }

    /// Where a click on this platform goes: the Twitch login, and the YouTube channel id.
    ///
    /// DERIVED FROM THE CONSTANTS, NEVER SPELLED OUT. `titlebar` used to hold a whole literal
    /// `https://www.twitch.tv/Broken_Stoic` beside these, which is a second place a login is
    /// written down and therefore a place it can rot: a changed login would be fixed here and left
    /// alone there, and the maker's mark would open a stranger's channel while the pill polled the
    /// right one. There is one spelling of each now and this builds both.
    pub fn url(self) -> String {
        match self {
            Platform::Twitch => format!("https://www.twitch.tv/{TWITCH_HANDLE}"),
            Platform::YouTube => youtube_channel_url(),
        }
    }
}

/// TOLERANT ON THE WAY IN, AND DELIBERATELY SO.
///
/// A derived `Deserialize` refuses a string it does not know, and refusing here does not cost a
/// FIELD, it costs the WHOLE FILE: `serde_json::from_str::<Settings>` fails as a unit, `load`
/// hands back defaults with a `load_problem`, and every writer in the app is then refused by the
/// guard in [`Settings::save_to`]. So a later build that learns a third platform, writes
/// `"watch_on":"kick"`, and is then rolled back would wedge this one out of saving ANYTHING, over
/// a preference. Anything unrecognised, of any JSON type, reads as the default instead and the
/// rest of the file loads exactly as it did.
///
/// THE UNRECOGNISED VALUE IS NOT PRESERVED, and that is the difference between this and `extra`.
/// `extra` carries another lane's keys through untouched because they are not this struct's to
/// interpret. `watch_on` IS this struct's, it has exactly the values this build understands, and
/// the next save writes the one that is on screen.
impl<'de> Deserialize<'de> for Platform {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Platform, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(v.as_str().and_then(Platform::from_key).unwrap_or_default())
    }
}

/// Directory under the platform config dir.
pub const APP_DIR: &str = "eql-grimoire";
/// File name inside it.
pub const FILE: &str = "settings.json";

/// Keys that WERE typed fields on `Settings` and are now constants above. A file written by an
/// older build still carries them, and `#[serde(flatten)] extra` would otherwise adopt them and
/// write them back out for ever: a setting nobody reads, sitting in the file inviting an edit that
/// does nothing. `load_from` drops exactly these and touches no other unknown key, so another
/// lane's state (`lfg_board`, `priorities`) rides through untouched.
const LEGACY_KEYS: [&str; 2] = ["twitch_handle", "youtube_handle"];

/// WHAT ONE TOOL WINDOW'S OWNER HAS CHANGED. Absent fields take the window's own default.
///
/// `Option` AND NOT `bool`, WHICH IS THE WHOLE VALUE OF THIS TYPE. A `bool` here would freeze the
/// default at whatever it was on the day the file was first written: flipping
/// `Slot::pin_default` in a later build would then change nothing for anybody who had ever opened
/// the window. `None` means "follow the code", and it is what a fresh file says about everything.
/// NO `Eq`, AND IT IS `rect` THAT TAKES IT AWAY. `f32` is not `Eq` (NaN is not equal to itself),
/// so a rectangle of points cannot sit on a struct that derives it. Nothing needs `Eq` here:
/// `Settings` itself derives only `PartialEq`, the map is a `BTreeMap` keyed on `String`, and the
/// one comparison this type is used in is `*e == WindowPrefs::default()` in [`Settings::win_prune`],
/// which asks whether every field is `None`. `PartialEq` stays, which also keeps this struct inside
/// `reach::every_field_behind_a_masking_derive_is_read_in_production`'s net.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowPrefs {
    /// Keep this window above the others. `None` follows `windows::Slot::pin_default`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    /// Was this window on screen when the app last closed? `None` means it was not, which is
    /// every window's own default and is why this one takes no `fallback` the way `pinned` does:
    /// there is no table in the code saying a tool window opens itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    /// Where the OS last reported this window, in points: `[x, y, w, h]` of its OUTER rect.
    /// `None` means the registry places it beside the main window on its first open.
    ///
    /// THE OUTER RECT AND NOT THE INNER ONE, because it is what goes back in: the viewport builder
    /// takes a position and a size, and a size measured inside the frame would shrink the window by
    /// the title strip on every relaunch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<[f32; 4]>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Overrides `data::Snapshot::locate()` when set. None means "use what locate finds".
    pub data_root: Option<PathBuf>,
    /// The EverQuest Legends Logs folder. None means the ingest has nowhere to look.
    pub log_dir: Option<PathBuf>,
    /// Whether the main window starts pinned above other windows. D3.
    pub always_on_top: bool,
    /// Which platform the live pill reports, and where an unqualified click on the channel goes.
    ///
    /// A FILE WITHOUT IT LOADS AND KEEPS EVERYTHING. `#[serde(default)]` on the container is what
    /// makes that true: the key is missing from every settings.json written before this build, and
    /// a missing field takes `Default`, which is Twitch, the platform this app has always shown.
    /// A file with an unrecognised VALUE loads too; [`Platform`]'s `Deserialize` says why that
    /// matters more than it sounds.
    pub watch_on: Platform,
    /// WHAT THE OWNER HAS CHANGED ABOUT A TOOL WINDOW, keyed by `windows::Slot::id`.
    ///
    /// ONLY WHAT DIFFERS IS STORED, exactly like `hotkeys` below it and for the same reason: an
    /// absent key takes the window's own default, so changing a default in the code reaches every
    /// machine that has not overridden it. Each field is an `Option` for the same reason one level
    /// down, so a file that pins a window without mentioning where it sat still lets the registry
    /// place it.
    ///
    /// THIS IS WHERE A TOOL WINDOW'S PIN LIVES NOW, AND IT DID NOT USED TO. Every tool window's pin
    /// was per session on purpose: the mechanism was shared with the main window and only the
    /// POLICY differed. The owner asked for it in Settings (2026-09-05, D11), and he is right about
    /// the case that broke it: an overlay you have to re-pin on every launch is an overlay you
    /// stop using. See `windows::Slot::pin_default`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub windows: BTreeMap<String, WindowPrefs>,
    /// EVERY COMBAT OVERLAY THE OWNER HAS, IN HIS ORDER. D11 stage two.
    ///
    /// THE WHOLE LIST AND NOT JUST THE DIFFERENCES, unlike `windows` and `hotkeys` above. Those
    /// two record deviations from a table that lives in the code, so an absent key can sensibly
    /// mean "follow the build". An overlay has no such table: it IS the owner's, invented by him,
    /// and there is nothing in the code for an absent entry to fall back to. `overlay::or_default`
    /// handles the one case that matters, an empty list on a fresh install.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlays: Vec<crate::overlay::Overlay>,
    /// THE DASHBOARD THE OWNER ARRANGED: which tiles, in his order, at his widths.
    ///
    /// THE WHOLE LIST AND NOT JUST THE DIFFERENCES, exactly as `overlays` above it and for the
    /// same reason: there is no table in the code for an absent entry to fall back to, because
    /// the list IS the arrangement. `screens::dashboards::layout` handles the one case that
    /// matters, an empty list on a fresh install, by drawing the shipped dashboard.
    ///
    /// A TILE IS SAVED BY ITS STRING ID AND NOT AS AN ENUM, which is the whole reason
    /// `Placement` exists. A settings file written by a build with a tile this one does not have
    /// must still load: a serde enum would fail the entire file on an unknown variant, taking the
    /// reader`s hotkeys and log folder down with a widget. See `screens::dashboards::Tile::id`.
    ///
    /// THE GRID POSITION IS THREE MORE FIELDS BEHIND `serde(default)`, so a file from the build
    /// that packed tiles by span alone loads with them at zero and `dashgrid::resolve` places
    /// those tiles under everything that has a position. See `screens::dashgrid::Placement`.
    ///
    /// AN `Option`, BECAUSE `NEVER ARRANGED` AND `DELIBERATELY EMPTY` ARE DIFFERENT FILES. As a
    /// bare `Vec` the two were the same empty list, so taking the last card off the dashboard
    /// put all thirteen back on the next frame. `None` is a fresh install and every settings
    /// file written before this field existed (a missing key deserialises to `None`, so the
    /// migration is free); `Some(vec![])` is a reader who emptied it and means it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard: Option<Vec<crate::screens::dashgrid::Placement>>,
    /// WHAT A PERSON WROTE ABOUT A FIGHT, keyed on the fight own start stamp.
    ///
    /// THE STAMP AND NOT AN INDEX. The history list is rebuilt on every rescan and a fight can
    /// leave the 40MB tail entirely; an index would slide a note onto a different pull. The stamp
    /// is the log own text and never moves.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fight_notes: BTreeMap<String, String>,
    /// THE KILL TRACKER'S COUNTING FILTERS: witnessed kills, generic kills outside their zone, and
    /// whether city zones are ignored.
    ///
    /// # THEY WERE PER WINDOW AND PER RUN, WHICH IS TWO DEFECTS IN ONE PLACE
    ///
    /// The three checkboxes on the Kills view wrote `TrackerState::settings`, which lives on the
    /// `Ingest`, and there is one `Ingest` per window. So ticking one moved that window's
    /// completion percentage and left the other window's alone: two headline percentages for one
    /// roster, side by side on a stream, with nothing saying why. And nothing wrote them anywhere,
    /// so all three were back at their defaults on the next launch and a reader who does not count
    /// witnessed kills had to say so every time he opened the app.
    ///
    /// THE WHOLE STRUCT AND NOT A DIFFERENCE LIST, unlike `windows` and `hotkeys` above. Those
    /// record deviations from a table in the code so a changed default reaches every machine; these
    /// three are the counting rule the reader chose and there is no build-time table to defer to.
    /// [`crate::ingest::TrackerSettings`] carries the serde discipline that makes an older file
    /// load correctly, and it explains at length why `#[serde(default)]` is on that container and
    /// must never be moved onto its fields.
    ///
    /// READ BY [`crate::ingest::Ingest::new`] AND [`crate::ingest::Ingest::reconfigure`], which is
    /// what makes both windows agree from the first frame rather than from whenever the Kills view
    /// is next drawn.
    pub tracker: crate::ingest::TrackerSettings,
    /// WHETHER THIS BUILD LOOKS FOR A NEWER ONE, WHERE IT LOOKS, AND WHETHER IT FETCHES WITHOUT
    /// BEING ASKED.
    ///
    /// THE SAME SHAPE AS `tracker` ABOVE, for the same reasons and with one extra that matters
    /// more here. [`crate::updater::run::UpdaterSettings`] carries the serde discipline in its own
    /// doc: `#[serde(default)]` is on the CONTAINER and must never move onto a field, because two
    /// of its three defaults are not the zero value. Every settings.json that exists today was
    /// written before this key existed, so every one of them takes those defaults, and the wrong
    /// spelling would ship an updater that is switched off on every machine in the field with no
    /// symptom but the absence of updates.
    ///
    /// READ BY `App::new`, which starts the worker, and by `App::heartbeat`, which hands the
    /// current block to it on every eframe callback so the two controls on the Settings screen
    /// take effect in the same session rather than at the next launch.
    pub updater: crate::updater::run::UpdaterSettings,
    /// Hotkey overrides, D4: `hotkeys::DEFAULTS` row id to chord text (`Ctrl+Alt+S then K`).
    /// Empty means every binding is the D4 default. Only rows that differ are stored, so a change
    /// to the default table reaches every machine that did not rebind that row.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub hotkeys: BTreeMap<String, String>,
    /// Every key this struct does not name. Other lanes' state lives here and survives a save.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
    /// Set by `load` when the file existed and could not be read. Never written back.
    #[serde(skip)]
    pub load_problem: Option<String>,
}

impl Settings {
    /// Is this window pinned? `fallback` is the window's own default.
    pub fn win_pinned(&self, id: &str, fallback: bool) -> bool {
        self.windows
            .get(id)
            .and_then(|w| w.pinned)
            .unwrap_or(fallback)
    }

    /// WAS THIS WINDOW ON SCREEN WHEN THE APP LAST CLOSED?
    ///
    /// NO `fallback` PARAMETER, UNLIKE [`Settings::win_pinned`], AND THAT IS NOT AN OVERSIGHT. The
    /// pin has a table in the code to fall back to (`windows::Slot::pin_default`) because a window
    /// has an opinion about whether it wants to float; "was it open" has no such table. A build
    /// cannot decide that a window the owner never opened should open itself, so the absent answer
    /// is `false` and there is nothing for a caller to override it with.
    pub fn win_open(&self, id: &str) -> bool {
        self.windows.get(id).and_then(|w| w.open).unwrap_or(false)
    }

    /// Where the OS last reported this window: `[x, y, w, h]` of its outer rect, in points.
    /// `None` means the registry places it, which is what a window that has never been moved gets.
    pub fn win_rect(&self, id: &str) -> Option<[f32; 4]> {
        self.windows.get(id).and_then(|w| w.rect)
    }

    /// Set the pin, and FORGET IT AGAIN when it lands back on the window's own default.
    ///
    /// THE ERASURE IS THE POINT AND IT IS NOT TIDINESS. A key that records agreement with the
    /// default is indistinguishable from a key that records a deliberate choice, so it pins the
    /// old default in place for ever: change `Slot::pin_default` in a later build and everybody
    /// who ever toggled the checkbox twice keeps the old behaviour with no way to tell why.
    /// `hotkeys` stores only rows that differ for exactly this reason.
    pub fn set_win_pinned(&mut self, id: &str, fallback: bool, on: bool) {
        self.win_set(id, fallback, on, |w| &mut w.pinned);
    }

    /// Set the open state. Erased against `false`, which [`Settings::win_open`] explains is the
    /// only default a window's own visibility can have.
    pub fn set_win_open(&mut self, id: &str, on: bool) {
        self.win_set(id, false, on, |w| &mut w.open);
    }

    /// Remember where the OS says this window is, or forget it with `None`.
    ///
    /// NOT THROUGH [`Settings::win_set`], which takes an `Option<bool>` field and a code default to
    /// erase against; a rectangle has neither. It shares the erasure rule instead, which is the
    /// half that had to stay in one place: see [`Settings::win_prune`].
    pub fn set_win_rect(&mut self, id: &str, r: Option<[f32; 4]>) {
        self.windows.entry(id.to_owned()).or_default().rect = r;
        self.win_prune(id);
    }

    /// The one body the bool setters share, so the erasure rule cannot be written twice and drift.
    fn win_set(
        &mut self,
        id: &str,
        fallback: bool,
        on: bool,
        field: impl Fn(&mut WindowPrefs) -> &mut Option<bool>,
    ) {
        let e = self.windows.entry(id.to_owned()).or_default();
        *field(e) = if on == fallback { None } else { Some(on) };
        self.win_prune(id);
    }

    /// DROP AN ENTRY THAT HAS NOTHING LEFT IN IT, so a settings file that has been toggled back
    /// and forth is byte for byte the one that was never touched.
    ///
    /// ONE COPY, CALLED BY EVERY SETTER, and it is one copy because [`Settings::set_win_rect`]
    /// could not go through `win_set` and would otherwise have written this rule out a second
    /// time. Two copies of an erasure rule is how a `windows` map grows entries full of `None`
    /// that then pin a shipped default in place for the readers who have them.
    fn win_prune(&mut self, id: &str) {
        if self
            .windows
            .get(id)
            .is_some_and(|e| *e == WindowPrefs::default())
        {
            self.windows.remove(id);
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dashboard: None,
            data_root: None,
            log_dir: None,
            always_on_top: false,
            watch_on: Platform::Twitch,
            windows: BTreeMap::new(),
            overlays: Vec::new(),
            fight_notes: BTreeMap::new(),
            tracker: crate::ingest::TrackerSettings::default(),
            updater: crate::updater::run::UpdaterSettings::default(),
            hotkeys: BTreeMap::new(),
            extra: serde_json::Map::new(),
            load_problem: None,
        }
    }
}

impl Settings {
    /// Where the file lives on this machine, or None when the platform offers no config dir.
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join(APP_DIR).join(FILE))
    }

    /// Load from the platform path. Never fails: see the module doc. Then fills a detected log
    /// folder in when none was saved; a chosen folder always wins over a detected one.
    pub fn load() -> Settings {
        let mut s = match Self::path() {
            Some(p) => match Self::load_from(&p) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("{e}");
                    Settings {
                        load_problem: Some(e),
                        ..Settings::default()
                    }
                }
            },
            None => {
                let e =
                    "no config directory on this platform; settings will not persist".to_owned();
                log::warn!("{e}");
                Settings {
                    load_problem: Some(e),
                    ..Settings::default()
                }
            }
        };
        s.fill_detected_log_dir(detect_log_dir());
        s
    }

    /// Read one file. A missing file is the default settings, not an error: a first launch has no
    /// file and is not broken. An unreadable or unparsable file IS an error, named with its path.
    ///
    /// AN OLD FILE MUST NOT COST THE USER THEIR REAL SETTINGS. Files written before the handles
    /// became constants carry `twitch_handle` and `youtube_handle`. They are not typed fields any
    /// more, and `#[serde(flatten)] extra` means unknown keys are collected rather than refused, so
    /// nothing here fails and `data_root`, `log_dir`, `always_on_top` and the hotkey table all come
    /// back. `LEGACY_KEYS` then drops the two retired keys from `extra` so the next save does not
    /// write them out again.
    pub fn load_from(path: &Path) -> Result<Settings, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(format!("{}: cannot read settings: {e}", path.display())),
        };
        let mut s = serde_json::from_str::<Settings>(&text)
            .map_err(|e| format!("{}: settings are not valid JSON: {e}", path.display()))?;
        for k in LEGACY_KEYS {
            s.extra.remove(k);
        }
        /* AND A PANEL LIST NOBODY EVER CHOSE IS HANDED BACK, which is the same kind of repair
         * as the two retired keys above: a file carrying something an older build put there on
         * the user's behalf. See `overlay::forget_unchosen` for what it recognises and what it
         * refuses to touch. */
        crate::overlay::forget_unchosen(&mut s.overlays);
        /* AND A DASHBOARD THAT IS EXACTLY ONE OF THE SHIPPED ARRANGEMENTS, which is what a click
         * that moved nothing used to write. See `screens::dashboards::forget_unchosen`. */
        crate::screens::dashboards::forget_unchosen(&mut s.dashboard);
        /* AND POPPED WINDOWS SAVED BEFORE THE NAMES, CALLED BY THEIR NAMES. See
         * overlay::rename_popped. */
        crate::overlay::rename_popped(&mut s.overlays);
        Ok(s)
    }

    /// Save to the platform path. Refused under `cfg(test)`: see the module doc for the file a
    /// test once overwrote. Tests round trip through `save_to` on a scratch path.
    ///
    /// Refuses too when the file already there cannot be read, which is [`Settings::save_to`]'s
    /// guard and is described there. This is what four of the five writers in the app call.
    pub fn save(&self) -> Result<(), String> {
        self.save_at(OnUnreadable::Refuse).map(|_| ())
    }

    /// Save to the platform path, KEEPING an unreadable file beside itself and then replacing it.
    /// Returns where the copy went, or None when there was nothing unreadable to keep.
    ///
    /// FOR THE SETTINGS SCREEN AND NOTHING ELSE. Replacing a file whose contents nobody could
    /// read is a decision, not a side effect, and that screen is the one surface that states what
    /// is about to happen before it happens (the WRONG block at the top of `SettingsScreen::ui`)
    /// and only ever writes because a person typed into it. Every other writer takes `save`, is
    /// refused, and shows the refusal.
    pub fn save_replacing_unreadable(&self) -> Result<Option<PathBuf>, String> {
        self.save_at(OnUnreadable::KeepACopy)
    }

    /// The platform path half of both, so the `cfg(test)` refusal is written once and neither
    /// public entry can be the one that forgot it.
    fn save_at(&self, on_unreadable: OnUnreadable) -> Result<Option<PathBuf>, String> {
        if cfg!(test) {
            return Err("refused: a unit test may not write the operator's settings file; use save_to on a scratch path".to_owned());
        }
        let p = Self::path().ok_or_else(|| "no config directory on this platform".to_owned())?;
        self.write_file(&p, on_unreadable)
    }

    /// Write one file, through a sibling temp file and a rename so a crash mid-write leaves the
    /// previous settings intact rather than a half file that will not parse next launch.
    ///
    /// AND REFUSE OUTRIGHT WHEN THE FILE ALREADY THERE CANNOT BE READ. That guard is here, and
    /// not at the call sites, because this is the one line every writer in the app reaches:
    /// `save` ends here, and `save` is what the gear screen, the LFG board, the valet, the
    /// Settings screen's debounced flush and the always-on-top pin all call. A guard written at
    /// those five call sites is a guard the sixth writer will not have.
    ///
    /// WHAT IT PREVENTS, WHICH HAPPENED: see the module doc. The short of it is that `load`
    /// answers an unparsable file with DEFAULTS, and defaults written over that file are a total
    /// loss the rename makes atomic.
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        self.write_file(path, OnUnreadable::Refuse).map(|_| ())
    }

    /// [`Settings::save_to`] on the deliberate path: keep the unreadable bytes, then write.
    /// Returns where they were kept, or None when there was nothing unreadable to keep.
    pub fn save_to_replacing_unreadable(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        self.write_file(path, OnUnreadable::KeepACopy)
    }

    /// The choke point. Every save this program performs is this function.
    fn write_file(
        &self,
        path: &Path,
        on_unreadable: OnUnreadable,
    ) -> Result<Option<PathBuf>, String> {
        /* THE FIRST QUESTION IS ABOUT SELF, NOT ABOUT THE FILE, AND ASKING ONLY THE SECOND ONE
         * WAS WORSE THAN ASKING NEITHER.
         *
         * The guard below asks whether the file on disk parses. Its premise was that a file which
         * parses is safe to replace, because whatever it held is already in the `Settings` about
         * to be written, since `extra` collects every key this struct does not name. That premise
         * is FALSE, and the sequence that breaks it is the one this app's own error message used
         * to recommend:
         *
         *   1. settings.json is malformed, so `load` hands the app `..Settings::default()` with
         *      `load_problem` set. `load` runs ONCE, at startup (main.rs), and is never re-read.
         *   2. A save is attempted. The file guard refuses. The file is still intact. Good.
         *   3. The message said "fix its JSON by hand", so the operator does exactly that, WITHOUT
         *      restarting, because nothing told them to. The file on disk is now valid and whole.
         *   4. Anything saves. The file parses now, so the file guard stands down, and the stale
         *      defaults this process has been holding since step 1 are written over a good file,
         *      atomically, returning Ok.
         *
         * `extra` holds the keys of the file that was LOADED. After step 3 the file has moved on
         * and this process has not. So the question that actually protects the operator is not
         * "can that file be read" but "did THIS SESSION ever read it": a process running on
         * defaults has nothing worth writing and must never write over anything.
         *
         * `KeepACopy` is exempt because it is the deliberate path: the Settings screen has told
         * the person what is about to happen and they chose it, and `keep_a_copy` preserves the
         * bytes first regardless. */
        if on_unreadable == OnUnreadable::Refuse && self.load_problem.is_some() {
            return Err(format!(
                "{}: this session never managed to read those settings, so it is running on \
                 defaults and will not write them over that file. RESTART the app to pick up a \
                 file you have repaired, or open Settings to replace it deliberately",
                path.display()
            ));
        }

        /* BRANCH ON THE INTENT FIRST, because the two paths are asking different questions and
         * one shared helper cannot answer both. Refuse asks "is what is there unreadable";
         * KeepACopy asks "what is there", full stop. */
        let kept = match on_unreadable {
            OnUnreadable::KeepACopy => match existing_bytes(path, "settings")? {
                None => None,
                Some(bytes) => Some(keep_a_copy(path, &bytes, "settings")?),
            },
            OnUnreadable::Refuse => match unreadable_bytes::<Settings>(path, "settings")? {
                None => None,
                Some(_bytes) => match on_unreadable {
                    OnUnreadable::Refuse => {
                        return Err(format!(
                            "{}: the settings already there could not be read, so they were NOT \
                         replaced and that file is untouched. RESTART the app after repairing \
                         its JSON by hand, or open Settings to replace it deliberately",
                            path.display()
                        ));
                    }
                    /* Before the temp file, not after: if the write below fails, a spare copy of a
                     * file that is still there costs nothing, and the other order costs everything. */
                    OnUnreadable::KeepACopy => unreachable!("handled above"),
                },
            },
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("{}: cannot create settings folder: {e}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("{}: cannot encode settings: {e}", path.display()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)
            .map_err(|e| format!("{}: cannot write settings: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("{}: cannot replace settings: {e}", path.display()))?;
        Ok(kept)
    }

    /// If no log folder was ever chosen and the default install folder exists, use it, so a
    /// default install works with no setup at all. A chosen folder is never overridden, even by
    /// a detected one.
    pub fn fill_detected_log_dir(&mut self, detected: Option<PathBuf>) {
        if self.log_dir.is_none() {
            self.log_dir = detected;
        }
    }

    /// The data root the app should read: the override when set, else what `locate` finds.
    pub fn effective_data_root(&self) -> Option<PathBuf> {
        self.data_root
            .clone()
            .or_else(crate::data::Snapshot::locate)
    }
}

/* -------------------------------------------------------------------- guard -- */

/// What a write does about a file it would replace but could not read.
///
/// `pub(crate)` with the two functions under it because this guard is not about settings, it is
/// about any hand-held file this app replaces wholesale. The Plane of Sky marks
/// (`screens::sky::Marks`) are the second one, and they take these same three pieces rather than
/// a second copy of them that can drift.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OnUnreadable {
    /// Leave it alone and return the reason. Every writer but the Settings screen.
    Refuse,
    /// Keep the bytes beside it under a name of their own, then write. The Settings screen.
    KeepACopy,
}

/// The bytes at `path` when replacing them would destroy something, else None.
///
/// None means one of three things, each of them honest: there is no file (a first launch), the
/// file is empty (there is nothing in it to lose, and refusing on an empty file would wedge the
/// app out of saving for ever over nothing), or it parses as `T`, in which case whatever it held
/// is already in the value about to be written. For `Settings` that last clause holds because
/// `extra` collects every key the struct does not name.
///
/// A file that cannot be read AT ALL (locked, permissions, a directory in its place) is an error
/// rather than a yes or a no. That is precisely the case where the true answer is "I cannot tell
/// what is there", and nothing may overwrite what it was unable to look at.
///
/// `T` must be the type the loader of that file reads through, with the same serde attributes, so
/// the guard and the loader cannot disagree about which files are readable. `what` is the word for
/// that file in the sentence a person reads.
/// The bytes already at `path`, whatever they are, or None when there is nothing worth keeping.
///
/// THIS IS NOT `unreadable_bytes`, AND CONFUSING THE TWO COST A FILE.
///
/// `unreadable_bytes` answers the REFUSAL question: it yields bytes only when they do NOT parse,
/// because its caller is deciding whether to stand in the way. The deliberate-replacement path is
/// not deciding anything. It has already been told what it is about to overwrite and been given
/// the go-ahead, so what it needs is EVERYTHING that is there, parseable or not.
///
/// Using the refusal helper for it meant `keep_a_copy` only ever ran on a file that failed to
/// parse. A comment above the deliberate path asserted that it "preserves the bytes first
/// regardless", which was simply untrue: repair a broken file by hand, press the button the
/// screen is still telling you to press, and a perfectly good file was replaced with no copy
/// kept anywhere.
///
/// NotFound and an empty or whitespace-only file both answer None, for the same reason they do
/// in `unreadable_bytes`: there are no bytes to lose.
pub(crate) fn existing_bytes(path: &Path, what: &str) -> Result<Option<Vec<u8>>, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "{}: cannot read the {what} already there, so they were not replaced: {e}",
                path.display()
            ));
        }
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    Ok(Some(bytes))
}

pub(crate) fn unreadable_bytes<T: serde::de::DeserializeOwned>(
    path: &Path,
    what: &str,
) -> Result<Option<Vec<u8>>, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "{}: cannot read the {what} already there, so they were not replaced: {e}",
                path.display()
            ));
        }
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    match serde_json::from_slice::<T>(&bytes) {
        Ok(_) => Ok(None),
        Err(_) => Ok(Some(bytes)),
    }
}

/// Put `bytes` in a file of their own beside `path` and say where they went.
///
/// `create_new` IS THE PROMISE HERE, NOT THE TIMESTAMP. Two failures inside one second pick the
/// same name, and a plain write would put the second copy on top of the first: the very loss this
/// guard exists to stop, repeated one file over. `create_new` fails when the name is taken and
/// the counter walks to a free one, so no copy already kept is ever written over.
///
/// The bytes are copied rather than the file renamed, so the unreadable file stays where it is
/// until the caller replaces it. A rename would leave no settings at all in the window between.
pub(crate) fn keep_a_copy(path: &Path, bytes: &[u8], what: &str) -> Result<PathBuf, String> {
    use std::io::Write;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{what}.json"));
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    for n in 0..100u32 {
        let candidate = dir.join(if n == 0 {
            format!("{name}.unreadable-{stamp}")
        } else {
            format!("{name}.unreadable-{stamp}-{n}")
        });
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut f) => {
                f.write_all(bytes).map_err(|e| {
                    format!(
                        "{}: cannot write the copy of the unreadable {what}: {e}",
                        candidate.display()
                    )
                })?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(format!(
                    "{}: cannot keep a copy of the unreadable {what}: {e}",
                    candidate.display()
                ));
            }
        }
    }
    Err(format!(
        "{}: cannot keep a copy of the unreadable {what}; every name for this second is taken",
        dir.display()
    ))
}

/* ---------------------------------------------------------------- detection -- */

/// Where the EQL client writes logs on a default install: the Public profile's `Daybreak Game
/// Company\Installed Games\EverQuest Legends\Logs` on Windows, and on a Mac the same layout inside
/// the osxEQL wineprefix. Linux has no default install to point at and gets none here.
pub fn default_log_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| {
            h.join("Library")
                .join("Application Support")
                .join("osxEQL")
                .join("prefix")
                .join("drive_c")
                .join("users")
                .join("Public")
                .join("Daybreak Game Company")
                .join("Installed Games")
                .join("EverQuest Legends")
                .join("Logs")
        })
    } else if cfg!(windows) {
        Some(PathBuf::from(
            r"C:\Users\Public\Daybreak Game Company\Installed Games\EverQuest Legends\Logs",
        ))
    } else {
        None
    }
}

/// The default log folder, only if it is actually there.
pub fn detect_log_dir() -> Option<PathBuf> {
    default_log_dir().filter(|p| p.is_dir())
}

/// The square and the words under the LOG FOLDER field: what this build knows about the folder
/// that is set, in the four states it can be in.
///
/// A NUMBER NOBODY COMPUTED, AND WHAT IT COST. This line used to read
/// `cx.ingest.sources().len()`, which is how many ROWS the ingest listed, not how many source
/// FILES it found. The ingest lists a row for every folder it looked in and found nothing in, so
/// on a fresh install with a valid Logs folder and no game files that call returns three
/// placeholders and this line said "3 sources found; see Sources" beside a filled SETTLED square
/// on a machine with no sources at all. Worse, it made the honest empty state below unreachable:
/// `sources()` never returns an empty vector once a folder is set, so `n == 0` could not happen
/// and the invented count always won. The rail's badge, which counted through
/// [`crate::nav::source_files`], was right the whole time; this line counts through the same
/// function, so the two surfaces could not answer one question two ways again.
///
/// THE RAIL NO LONGER CARRIES THAT BADGE, because the row it sat on is gone: the ledger it
/// pointed at is the SOURCES section of this screen now, four lines below this one. The function
/// stays and this line still counts through it, which is what makes the sentence below
/// ("the SOURCES section below lists them") a count of the same files that section tabulates.
///
/// The `is_dir` probe is the same one the field above it makes, and it is kept here rather than at
/// the call site so all four branches are one decision a test can drive.
fn log_folder_mark(dir: Option<&Path>, listed: &[Source]) -> (State, String) {
    match dir {
        None => (
            State::You,
            "no log folder set; the parser, loot, kill tracker and Sky all read from it".to_owned(),
        ),
        Some(p) if !p.is_dir() => (State::Wrong, "that folder does not exist".to_owned()),
        Some(_) => match crate::nav::source_files(listed) {
            0 => (
                State::Idle,
                "folder exists, nothing found in it yet; the game only writes a log while logging \
                 is on (/log on)"
                    .to_owned(),
            ),
            n => (
                State::Settled,
                format!(
                    "{n} source{} found; the SOURCES section below lists them",
                    if n == 1 { "" } else { "s" }
                ),
            ),
        },
    }
}

/* ------------------------------------------------------------------ hotkeys -- */

/// The D4 wording for what a chord opens.
pub fn tool_label(t: &Tool) -> &'static str {
    match t {
        Tool::Companion => "the whole app, toggled",
        Tool::Watch => "Watch: Broken Stoic live",
        /* "Log Parser" AND NOT "Parser", WHICH THIS ROW WAS THE LAST COPY OF. `Tool::Parser::title`
         * is "Log Parser" and so is `nav::label(ScreenId::Parser)`, so the window's own title
         * strip, its taskbar entry and the rail row all say one name; this table is what the
         * rebinding list on Settings prints, and it was still calling the same window something
         * else. Cosmetic on its own and exactly the shape of thing that stays behind and rots. */
        Tool::Parser => "Log Parser",
        Tool::Sky => "Plane of Sky",
        Tool::Chat => "Chat, over the game",
        Tool::Overlays => "the combat overlays, all of them, toggled",
        Tool::Lfg(LfgMode::Generic) => "LFG, generic",
        Tool::Lfg(LfgMode::Raid) => "LFG, looking for raid",
        Tool::Lfg(LfgMode::Motes) => "LFG, looking for motes",
    }
}

/// The "opens" column for one row. The hotkeys table carries three direct chords the D4 table
/// does not list (Ctrl+Alt+K, R, M) so a chord tool can be summoned with one press, and marks them
/// `alias`. They are labelled here so a reader holding the goal doc is not left counting ten rows
/// against a table of seven.
pub fn tool_text(b: &Binding) -> String {
    if b.alias {
        format!("{}, direct alias", tool_label(&b.tool))
    } else {
        tool_label(&b.tool).to_owned()
    }
}

/// The binding table as plain text, one line per chord, for the clipboard. Columns are padded so it
/// reads as a table in Discord or a bug report.
pub fn bindings_text(b: &[Binding]) -> String {
    let w_chord = b.iter().map(|x| x.chord.chars().count()).max().unwrap_or(0);
    let w_tool = b
        .iter()
        .map(|x| tool_text(x).chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for x in b {
        let status = match (&x.registered, &x.conflict) {
            (true, _) => "registered".to_owned(),
            (false, Some(c)) => format!("not registered: {c}"),
            (false, None) => "not registered".to_owned(),
        };
        let tool = tool_text(x);
        out.push_str(&format!(
            "{:<w_chord$}  {tool:<w_tool$}  {status}\n",
            x.chord
        ));
    }
    out
}

/* ------------------------------------------------------------------- screen -- */

/// What changed since the integrator last asked, so it can reload the snapshot, rebuild the ingest
/// or restart the watcher. The screen cannot do those itself: `Cx` lends the snapshot immutably and
/// the ingest's `rescan` takes no new folder.
///
/// No flag for `always_on_top`: `Windows::show` reads that setting on every pass and pushes the
/// window level whenever it differs from the one last applied, so the checkbox only has to mark
/// the file dirty. A flag nothing read sat here through round one.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Changed {
    pub data_root: bool,
    pub log_dir: bool,
    /// A hotkey override was applied or reset; the integrator reinstalls the globals and hands
    /// the new bindings back through `set_bindings`.
    pub hotkeys: bool,
    /// A BUTTON IN THE UPDATES SECTION WAS PRESSED, and which one.
    ///
    /// AN ASK AND NOT A CALL, exactly like `Cx::auth_begin` and `Cx::chat_wanted`: the `Updater`
    /// owns a thread, `App` owns the `Updater`, and a screen that could reach it could start a
    /// download from inside a draw. There is no updater flag for the two CONTROLS (the switch and
    /// the channel) because those are settings: they are written into `Settings` by the control
    /// itself and reach the worker through `App::heartbeat`'s `pump`, which hands it the whole
    /// block every callback. A flag for them would be a second path to the same fact.
    ///
    /// `Option` AND NOT FOUR `bool`s, because two presses in one frame is not a state that can
    /// mean anything: a person cannot press Install and Restart now in the same frame, and a shape
    /// that could record it would need a rule for what to do about it.
    pub updater: Option<crate::updater::run::Ask>,
    /// The reader pressed Restart now. Separate from `updater` above because it is not something
    /// the worker does: the `Updater` cannot close the window, and the process that has to be
    /// started is the ENTRY POINT rather than this executable
    /// (`crate::updater::run::entry_point` says why). `App` answers it.
    pub restart: bool,
}

/// How long after the last keystroke the file is written. Long enough that typing a path does not
/// write once per character, short enough that closing the window a second later loses nothing.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(700);
/// How long "copied" stays up after the clipboard button.
const COPIED_FOR: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct SettingsScreen {
    /// The hotkey table. The manager lives with the event loop, not with this screen, so the
    /// integrator assigns `Hotkeys::bindings()` here after `install()` and whenever it changes.
    pub bindings: Vec<Binding>,
    /// The snapshot load in progress, if one is: the root being read and how long it has taken.
    /// `Cx` carries neither the data nor an error while the loader thread runs, and the DATA
    /// section saying "no snapshot loaded" for the length of the parse would be a lie, the same
    /// one the main window guards every snapshot screen against. The App sets this each frame.
    pub data_loading: Option<(PathBuf, Duration)>,
    /// WHAT THE UPDATER IS DOING, written by the App every frame.
    ///
    /// THE SAME SHAPE AS `data_loading` ABOVE AND FOR THE SAME REASON. `Cx` carries no updater,
    /// and it should not: the `Updater` owns a thread, the App owns the `Updater`, and a screen
    /// that could reach it could start a download from inside a draw. The App clones one small
    /// struct out from under the worker's mutex once a frame and leaves it here, exactly as
    /// `watcher::Status` reaches every panel.
    ///
    /// `None` MEANS NO UPDATER IN THIS PROCESS, which is a real state and not a missing value: a
    /// preflight run constructs none on purpose, and neither does a platform with no local data
    /// directory. The section says so in words rather than drawing an empty one.
    pub updater: Option<crate::updater::run::UpdateView>,

    /* Text buffers for the two path fields. A path is edited as text and applied when the field
     * loses focus, so a half typed path never lands in settings as a folder that does not exist.
     * `*_seen` is the value the buffer was seeded from, so a change made elsewhere (another lane
     * setting log_dir) reseeds the buffer instead of being overwritten by stale text. */
    data_root_text: String,
    data_root_seen: Option<Option<PathBuf>>,
    log_dir_text: String,
    log_dir_seen: Option<Option<PathBuf>>,

    /* What detection found, memoised: locate() and detect_log_dir() touch the filesystem and this
     * screen redraws every frame. Refreshed when the "use detected" button is pressed. */
    detected_data: Option<Option<PathBuf>>,
    detected_log: Option<Option<PathBuf>>,

    dirty_since: Option<Instant>,
    last_save: Option<(Instant, Result<PathBuf, String>)>,
    /// Where a save from THIS screen put the bytes of an unreadable settings file, once it has.
    /// Kept for the rest of the session rather than cleared by the next clean save: the line that
    /// says where a person's file went is the only way back to it, and a save two seconds later
    /// erasing that line would make the copy as good as lost.
    kept: Option<PathBuf>,
    copied_at: Option<Instant>,
    changed: Changed,

    /* The rebinding editor, D4: one text buffer per row id, seeded from the binding's chord text
     * and applied on its Apply button (never on every keystroke, since a half typed chord is not a
     * chord). A parse error stays beside the row until the text changes. */
    chord_text: BTreeMap<&'static str, String>,
    chord_problem: BTreeMap<&'static str, String>,

    /// When the SOURCES section's `Re-read` button was last pressed, so the line beside it can say
    /// how long ago. The ingest's own `scanned_at` is a different fact and is printed beside it:
    /// a scan the folder watcher started on its own has no button press behind it.
    last_reread: Option<Instant>,
}

impl SettingsScreen {
    /// Hand the screen the current bindings. Reseeds the editor buffers from them.
    pub fn set_bindings(&mut self, b: Vec<Binding>) {
        self.chord_text = b.iter().map(|x| (x.id, x.chord.clone())).collect();
        self.chord_problem.clear();
        self.bindings = b;
    }

    /// Flags raised since the last call, then cleared.
    pub fn take_changed(&mut self) -> Changed {
        std::mem::take(&mut self.changed)
    }

    /// Write now if anything is pending. For the integrator to call on exit, since the debounce
    /// only runs while this screen is being drawn.
    pub fn flush(&mut self, settings: &Settings) {
        if self.dirty_since.take().is_some() {
            self.write(settings);
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty_since = Some(Instant::now());
    }

    /// THE ONE WRITER ALLOWED TO REPLACE AN UNREADABLE FILE, and the reason it is allowed is
    /// printed a few lines below in `ui`: this screen states that saving from it replaces that
    /// file, and it only ever writes because a person typed into it (`mark_dirty` has no other
    /// caller). The unreadable bytes are kept beside the file first and `self.kept` remembers
    /// where, so `ui` can show the way back to them.
    fn write(&mut self, settings: &Settings) {
        let r = match settings.save_replacing_unreadable() {
            Ok(kept) => {
                if kept.is_some() {
                    self.kept = kept;
                }
                Settings::path().ok_or_else(|| "no config directory".to_owned())
            }
            Err(e) => Err(e),
        };
        if let Err(e) = &r {
            log::warn!("settings not saved: {e}");
        }
        self.last_save = Some((Instant::now(), r));
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        /* The debounce. While dirty, keep the frame clock ticking so the write actually happens. */
        if let Some(since) = self.dirty_since {
            if since.elapsed() >= SAVE_DEBOUNCE {
                self.dirty_since = None;
                self.write(cx.settings);
            } else {
                ui.ctx()
                    .request_repaint_after(SAVE_DEBOUNCE - since.elapsed());
            }
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            /* THE WARNING IS NOW A DESCRIPTION OF WHAT THE CODE DOES, which it was not. It said
             * saving from this screen replaces the file, which was true, and left unsaid that
             * saving from any OTHER screen replaced it too, which was also true and was the data
             * loss. Both halves are stated because both are now facts: every other writer is
             * refused at `Settings::save_to`, and this one keeps the old bytes first. */
            if let Some(p) = &cx.settings.load_problem {
                mark(ui, State::Wrong, p);
                ui.label(
                    RichText::new(
                        "Running on defaults. Every other screen now refuses to save rather than \
                         write those defaults over that file. Saving from THIS screen replaces \
                         it, and keeps the unreadable file beside it first.",
                    )
                    .color(TEXT_2),
                );
                if let Some(k) = &self.kept {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Kept as").color(TEXT_2));
                        mono(ui, &k.display().to_string(), TEXT);
                    });
                }
                ui.add_space(8.0);
            }

            self.watch(ui, cx);
            self.data(ui, cx);
            self.log(ui, cx);
            self.sources(ui, cx);
            self.window(ui, cx);
            /* UPDATES SITS BETWEEN WINDOW AND FIGHT NOTES. The rule the comment below states is
             * that everything above FIGHT NOTES is about where the app READS from; this one is
             * not, so it goes at the end of that group rather than inside it, next to WINDOW,
             * which is the other section about how the app itself behaves. */
            self.updates(ui, cx);
            /* FIGHT NOTES SITS HERE AND NOT AT THE TOP, because a person comes to this screen for
             * it deliberately (a note he cannot find any more) rather than sweeping the page. The
             * sections above are all about where the app READS from; this one and HOTKEYS are
             * about what he has written into it. */
            self.notes(ui, cx);
            self.hotkeys(ui, cx);
            self.file_line(ui);
        });
    }

    /// Where this person would rather watch, and the first section on the screen.
    ///
    /// A `CHANNEL` SECTION USED TO SIT ABOVE THIS ONE AND IS GONE.
    /// It had already lost its two handle editors, and what was left was a sentence saying the
    /// build follows one channel, the two handles, and their poll cadence: four true statements
    /// about something nobody can change. The owner read it and asked why it was there at all.
    /// A settings screen is where decisions are made, so facts that are fixed at compile time now
    /// live only in the code that uses them, next to the constants themselves
    /// ([`TWITCH_HANDLE`], [`YOUTUBE_HANDLE`], `watcher::POLL_EVERY`) and in this module's header.
    /// Nothing was moved down here to replace them: a fact needs a reader with a decision to make,
    /// and this section's reader has one.
    ///
    /// THIS SECTION IS UNCHANGED BY THAT. `Preferred` never lived under `CHANNEL`; it has always
    /// had its own heading, precisely because that section offered nothing and this one does.
    fn watch(&mut self, ui: &mut Ui, cx: &mut Cx) {
        heading(ui, "WATCH");
        ui.horizontal(|ui| {
            field_label(ui, "Preferred");
            for p in Platform::ALL {
                /* `selectable_value` and not `radio_value`: a radio button is a circle, and
                 * `theme` spends this app's one round shape on the live pill. */
                if ui
                    .selectable_value(&mut cx.settings.watch_on, p, p.label())
                    .changed()
                {
                    self.mark_dirty();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            /* The chosen platform is named in the sentence rather than left to the highlight, so
             * the line reads as a statement of what is happening and not as a caption under a
             * control the reader still has to decode. */
            ui.label(
                RichText::new(format!(
                    "The live pill reports {0}, and clicking it opens {0}. Both platforms are \
                     polled either way, and both marks stay in the main window's title bar.",
                    cx.settings.watch_on.label()
                ))
                .color(TEXT_3),
            );
        });
        ui.add_space(6.0);
    }

    fn data(&mut self, ui: &mut Ui, cx: &mut Cx) {
        heading(ui, "DATA");
        /* Reseed the buffer when the setting changed underneath us. */
        if self.data_root_seen.as_ref() != Some(&cx.settings.data_root) {
            self.data_root_text = path_text(&cx.settings.data_root);
            self.data_root_seen = Some(cx.settings.data_root.clone());
        }
        ui.horizontal(|ui| {
            field_label(ui, "Data root");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.data_root_text)
                    .desired_width(420.0)
                    .font(FontId::monospace(12.0))
                    .hint_text("blank: use what is detected"),
            );
            if r.lost_focus() {
                let v = text_path(&self.data_root_text);
                if v != cx.settings.data_root {
                    cx.settings.data_root = v.clone();
                    self.data_root_seen = Some(v);
                    self.changed.data_root = true;
                    self.mark_dirty();
                }
            }
            if ui.button("Use detected").clicked() {
                self.detected_data = Some(crate::data::Snapshot::locate());
                if let Some(Some(p)) = &self.detected_data {
                    cx.settings.data_root = Some(p.clone());
                    self.data_root_text = path_text(&cx.settings.data_root);
                    self.data_root_seen = Some(cx.settings.data_root.clone());
                    self.changed.data_root = true;
                    self.mark_dirty();
                }
            }
        });
        let detected = self
            .detected_data
            .get_or_insert_with(crate::data::Snapshot::locate)
            .clone();
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            match detected {
                Some(p) => {
                    ui.label(RichText::new("detected").color(TEXT_3));
                    mono(ui, &p.display().to_string(), TEXT_2);
                }
                None => {
                    /* The loader's own probe list, in its order, so this cannot drift from what
                     * `Snapshot::locate` actually tried. */
                    let tried: Vec<String> = crate::data::candidates()
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect();
                    ui.label(
                        RichText::new(format!(
                            "nothing detected: put the data folder at one of {}",
                            tried.join("; ")
                        ))
                        .color(TEXT_2),
                    );
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            match (cx.data, cx.data_err) {
                (Some(d), _) => {
                    let r = d.report();
                    mark(
                        ui,
                        State::Settled,
                        &format!(
                            "loaded: {} items, {} zones, {} drops, {} sky items, {} quests",
                            r.items, r.zones, r.drops, r.sky_items, r.quests
                        ),
                    );
                    mono(ui, &r.root.display().to_string(), TEXT_3);
                }
                (None, Some(e)) => {
                    mark(ui, State::Wrong, e);
                }
                (None, None) => match &self.data_loading {
                    /* WORKING: a thread of ours is parsing it right now */
                    Some((root, for_)) => {
                        mark(
                            ui,
                            State::Working,
                            &format!("loading the snapshot, {:.1}s so far", for_.as_secs_f32()),
                        );
                        mono(ui, &root.display().to_string(), TEXT_3);
                        ui.ctx().request_repaint_after(Duration::from_millis(100));
                    }
                    None => {
                        mark(ui, State::Idle, "no snapshot loaded");
                    }
                },
            }
        });
        ui.add_space(6.0);
    }

    fn log(&mut self, ui: &mut Ui, cx: &mut Cx) {
        heading(ui, "LOG FOLDER");
        if self.log_dir_seen.as_ref() != Some(&cx.settings.log_dir) {
            self.log_dir_text = path_text(&cx.settings.log_dir);
            self.log_dir_seen = Some(cx.settings.log_dir.clone());
        }
        ui.horizontal(|ui| {
            field_label(ui, "Logs");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.log_dir_text)
                    .desired_width(420.0)
                    .font(FontId::monospace(12.0))
                    .hint_text("the EverQuest Legends Logs folder"),
            );
            if r.lost_focus() {
                let v = text_path(&self.log_dir_text);
                if v != cx.settings.log_dir {
                    cx.settings.log_dir = v.clone();
                    self.log_dir_seen = Some(v);
                    self.changed.log_dir = true;
                    self.mark_dirty();
                    cx.ingest.rescan();
                }
            }
            if ui.button("Use detected").clicked() {
                self.detected_log = Some(detect_log_dir());
                if let Some(Some(p)) = &self.detected_log {
                    cx.settings.log_dir = Some(p.clone());
                    self.log_dir_text = path_text(&cx.settings.log_dir);
                    self.log_dir_seen = Some(cx.settings.log_dir.clone());
                    self.changed.log_dir = true;
                    self.mark_dirty();
                    cx.ingest.rescan();
                }
            }
            if let Some(p) = &cx.settings.log_dir {
                if ui.button("Open folder").clicked() {
                    /* THE THIRD COPY OF THIS CALL STOOD HERE and said "cannot open" where the
                     * other two said "could not open", so one failure had two sentences
                     * depending on where you met it. */
                    if let Err(e) = crate::shell::open_dir(p) {
                        log::warn!("{e}");
                    }
                }
            }
        });
        let detected = self.detected_log.get_or_insert_with(detect_log_dir).clone();
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            match (detected, default_log_dir()) {
                (Some(p), _) => {
                    ui.label(RichText::new("detected").color(TEXT_3));
                    mono(ui, &p.display().to_string(), TEXT_2);
                }
                (None, Some(d)) => {
                    ui.label(RichText::new("not at the default install path").color(TEXT_3));
                    mono(ui, &d.display().to_string(), TEXT_3);
                }
                (None, None) => {
                    ui.label(
                        RichText::new("no default install path on this platform").color(TEXT_3),
                    );
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            let (state, words) =
                log_folder_mark(cx.settings.log_dir.as_deref(), &cx.ingest.sources());
            mark(ui, state, &words);
        });
        ui.add_space(6.0);
    }

    /// The ingest's ledger, and what came out of it. Decision D7.
    ///
    /// One ingest reads the inventory dumps, the achievements dump, the hunting logs and the EQ
    /// log tail, and this section is its account: what files, when each was last read, how many
    /// records came out, and what went wrong. Nothing here is computed; every number is the
    /// ingest's own count (and, for the snapshot, the data module's own load time), and a source
    /// with a problem shows the problem in the WRONG colour on its own row rather than vanishing
    /// from the table.
    ///
    /// IT WAS A RAIL ROW UNDER CRAFT AND THE OWNER WAS RIGHT THAT IT DOES NOT BELONG THERE. CRAFT
    /// is Commission, Work orders, Workshop and Standing: making a thing to order. Which files on
    /// this machine were read, and when, is not that work. It is the app's account of its own
    /// plumbing, its one control is a maintenance button, and every question it answers is
    /// answered about the folder the section above this one sets.
    ///
    /// THE AUTHORITY DOES NOT PUT IT THERE EITHER, WHICH IS WHAT THE ROW WAS LEANING ON. `web/app.html`
    /// does carry a `Sources` row under Tradesman, and it is a DIFFERENT SCREEN: from line 2338,
    /// "Where a component comes from", vendor, drop and forage rows keyed by a component name.
    /// Not one word of it is about ingest files. So the rail row was a web row's NAME over a
    /// desktop screen that shares nothing with it but the word, and the placement came with the
    /// name.
    ///
    /// WHAT THE OLD SCREEN SAID TWICE IS NOW SAID ONCE. It opened with a log folder block of its
    /// own: the folder's path, "no log folder set; point Settings at the EverQuest Legends Logs
    /// folder", and "that folder does not exist; fix it in Settings". LOG FOLDER, immediately
    /// above, says all three and carries the field that fixes them, so the block is gone rather
    /// than repeated four lines under itself. The one thing only that block said, the ingest's
    /// `log_dir_problem` naming every place it looked, is kept in the empty state below, which is
    /// the state it explains.
    fn sources(&mut self, ui: &mut Ui, cx: &mut Cx) {
        heading(ui, "SOURCES");
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            ui.label(
                RichText::new("Inventory and achievements dumps (/outputfile), hunting logs and the EQ log tail, read by one ingest.")
                    .color(TEXT_2),
            );
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            if ui.button("Re-read").clicked() {
                cx.ingest.rescan();
                self.last_reread = Some(Instant::now());
            }
            if cx.ingest.scanning() {
                mark(ui, State::Working, "scanning");
            }
            if let Some(t) = self.last_reread {
                ui.label(
                    RichText::new(format!(
                        "re-read asked {} ago",
                        age_text(t.elapsed().as_secs() as i64)
                    ))
                    .color(TEXT_3),
                );
            }
            /* The ingest's own stamp of its last bootstrap, not this section's button: a scan the
             * folder watcher started on its own (a new log taking over) shows here too. */
            let now = chrono::Utc::now();
            match cx.ingest.scanned_at() {
                Some(t) => ui.label(
                    RichText::new(format!("folder scanned {}", last_read_text(Some(t), now)))
                        .color(TEXT_3),
                ),
                None => ui.label(RichText::new("folder not scanned yet").color(TEXT_3)),
            };
            /* keep the ages ticking while the screen is up */
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        });
        ui.add_space(4.0);

        let sources = cx.ingest.sources();
        if sources.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                match &cx.settings.log_dir {
                    Some(dir) => mark(
                        ui,
                        State::Idle,
                        &format!("nothing found under {}", dir.display()),
                    ),
                    None => mark(ui, State::Idle, "nothing to read"),
                };
            });
            /* WHY THE LEDGER IS EMPTY, in the ingest's own words: every place it looked, or the
             * reader thread that would not start. The rows below are what it looks FOR; this is
             * what happened when it went looking. */
            if let Some(p) = cx.ingest.log_dir_problem() {
                ui.horizontal(|ui| {
                    ui.add_space(FIELD_W + 14.0);
                    ui.label(RichText::new(p).color(TEXT_3));
                });
            }
            for line in LOOKS_FOR {
                ui.horizontal(|ui| {
                    ui.add_space(FIELD_W + 14.0);
                    ui.label(RichText::new(*line).color(TEXT_3));
                });
            }
        } else {
            let w = sources
                .iter()
                .map(|s| s.records.to_string().len())
                .max()
                .unwrap_or(1);
            let now = chrono::Utc::now();
            /* THE ONLY THING ON THIS SCREEN THAT SCROLLS SIDEWAYS, AND IT IS MEASURED RATHER THAN
             * PREFERRED. Five columns, one of them a full path repeated on every row, inside a
             * screen whose own ScrollArea is VERTICAL. A cell laid out past the right edge is not
             * clipped, it is NOT DRAWN: `Label` asks `is_rect_visible` first. So on the owner's
             * own machine, a 2240px window at 175% (1280 logical, about 1055 of it body), the
             * RECORDS and PROBLEM columns simply were not there. A problem is the thing a person
             * opens this ledger to read.
             *
             * IT IS NOT INDENTED TO THE LABEL COLUMN EITHER, which every other section is, and
             * that is the same 92 pixels of the same argument. The retired Sources screen drew
             * this table at its own margin and so does this.
             *
             * THE HOUSE ALREADY ANSWERS THIS QUESTION THIS WAY: `screens::inventory` puts its wide
             * tables in `ScrollArea::both`. Sideways is the answer that drops nothing; eliding the
             * path or reordering the columns both decide for the reader which half matters. */
            egui::ScrollArea::horizontal()
                .id_salt("sources-table-scroll")
                .show(ui, |ui| {
                    egui::Grid::new("sources-table")
                        .num_columns(5)
                        .spacing([18.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("kind").color(TEXT_3));
                            ui.label(RichText::new("path").color(TEXT_3));
                            ui.label(RichText::new("last read").color(TEXT_3));
                            ui.label(
                                RichText::new(pad_left("records", w.max("records".len())))
                                    .font(FontId::monospace(11.5))
                                    .color(TEXT_3),
                            );
                            ui.label(RichText::new("problem").color(TEXT_3));
                            ui.end_row();
                            for s in &sources {
                                /* The ingest's own label for its kind: it owns that enum and a hand
                                 * written table of its variants here would drift from it. */
                                ui.label(RichText::new(s.kind.label()).color(TEXT_2));
                                mono(ui, &s.path.display().to_string(), TEXT);
                                ui.label(
                                    RichText::new(last_read_text(s.last_read, now)).color(TEXT_2),
                                );
                                /* Right aligned by padding in the monospace face, which is what tabular
                                 * figures are for: the column lines up without a layout trick. */
                                ui.label(
                                    RichText::new(pad_left(
                                        &s.records.to_string(),
                                        w.max("records".len()),
                                    ))
                                    .font(FontId::monospace(11.5))
                                    .color(TEXT),
                                );
                                match &s.problem {
                                    /* The ingest tails only the newest log and annotates every other one
                                     * with "not tailed: ...". That is an explanation, not a fault, and a
                                     * red square on every other character's log would spend WRONG on
                                     * decoration. It is printed in plain words; everything else the
                                     * ingest calls a problem is drawn as one. */
                                    Some(p) if p.starts_with(crate::nav::NOT_TAILED_PREFIX) => {
                                        ui.label(RichText::new(p).color(TEXT_3));
                                    }
                                    Some(p) => {
                                        mark(ui, State::Wrong, p);
                                    }
                                    None => {
                                        ui.label("");
                                    }
                                }
                                ui.end_row();
                            }
                        });
                });
        }

        heading(ui, "WHAT CAME OUT");
        let kills = cx.ingest.kills().len();
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            if kills == 0 {
                mark(ui, State::Idle, "no kill events yet");
            } else {
                mark(
                    ui,
                    State::Settled,
                    &format!("{kills} kill event{}", if kills == 1 { "" } else { "s" }),
                );
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            match (cx.ingest.roster(), cx.ingest.roster_problem()) {
                (Some(r), _) => mark(
                    ui,
                    State::Settled,
                    &format!(
                        "mob roster: {} zones from {}",
                        r.zones.len(),
                        r.path.display()
                    ),
                ),
                (None, Some(p)) => mark(ui, State::Idle, &format!("mob roster: {p}")),
                (None, None) => mark(ui, State::Idle, "mob roster not loaded yet"),
            };
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            match cx.ingest.inventory() {
                Some(d) => {
                    /* The dump by section, the ingest's own reading of the location column, and
                     * the worn count through the same per-slot lookup the gear screens use. Both
                     * are the ingest's numbers, printed, not this section's. */
                    let worn: usize = crate::ingest::WORN_SLOTS
                        .iter()
                        .map(|s| d.worn_in(s).len())
                        .sum();
                    let by_section: Vec<String> = d
                        .section_counts()
                        .iter()
                        .map(|(s, n)| format!("{} {n}", s.label()))
                        .collect();
                    let whose = match (&d.character, &d.server) {
                        (Some(c), Some(s)) => format!("{c} on {s}: "),
                        (Some(c), None) => format!("{c}: "),
                        _ => String::new(),
                    };
                    mark(
                        ui,
                        State::Settled,
                        &format!(
                            "inventory dump read: {whose}{} rows, {worn} worn ({})",
                            d.rows().count(),
                            by_section.join(", ")
                        ),
                    );
                }
                None => {
                    mark(
                        ui,
                        State::Idle,
                        cx.ingest
                            .inventory_problem()
                            .unwrap_or("no inventory dump read"),
                    );
                }
            };
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            match cx.ingest.achievements() {
                Some(d) => {
                    let who = match (&d.character, &d.server) {
                        (Some(c), Some(s)) => format!(", {c}'s on {s}"),
                        (Some(c), None) => format!(", {c}'s"),
                        _ => String::new(),
                    };
                    mark(
                        ui,
                        State::Settled,
                        &format!(
                            "achievements dump read: {} rows{who}; SKY / Keys reads it",
                            d.lines
                        ),
                    );
                }
                None => {
                    mark(
                        ui,
                        State::Idle,
                        cx.ingest
                            .achievements_problem()
                            .unwrap_or("no achievements dump read"),
                    );
                }
            };
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            match (cx.data, cx.data_err) {
                (Some(s), _) => {
                    let r = s.report();
                    mark(
                        ui,
                        State::Settled,
                        &format!(
                            "snapshot: {} items, {} tooltips, {} zones, {} drops, {} sky items, {} quests, {} spells, {} factions, {} merchants from {} in {} ms",
                            r.items,
                            s.tooltips.len(),
                            r.zones,
                            r.drops,
                            r.sky_items,
                            r.quests,
                            s.spells.len(),
                            r.factions,
                            r.merchants,
                            r.root.display(),
                            s.load_time.as_millis()
                        ),
                    );
                }
                (None, Some(e)) => {
                    mark(ui, State::Wrong, e);
                }
                (None, None) => {
                    mark(ui, State::Idle, "snapshot not loaded yet");
                }
            }
        });
        /* WHERE THE MERCHANTS THAT LAND NOWHERE ARE COUNTED, AND WHY IT IS HERE.
         *
         * Zone detail is the only home merchants.json got, so a merchant whose `zone` string
         * matches no zone in this snapshot is a record the app holds and never draws. That is the
         * exact defect this whole lane exists to remove, at a smaller scale, and it would be very
         * easy to leave silent. It is not silent: the ledger says how many and names them, and
         * fixing it means an alias table this build refuses to invent, or a corrected wiki page.
         *
         * Measured on the shipped file: nine, over seven zone spellings. */
        if let Some(s) = cx.data {
            if !s.merchants.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(FIELD_W);
                    mark(
                        ui,
                        State::Settled,
                        &format!(
                            "who sells it: {} item names over {} stock lines, indexed at load",
                            s.sellers.items(),
                            s.sellers.lines()
                        ),
                    );
                });
            }
            let orphans = s.merchants_without_a_zone_page();
            if !orphans.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(FIELD_W);
                    let mut zones: Vec<&str> = orphans.iter().map(|m| m.zone.as_str()).collect();
                    zones.sort_unstable();
                    zones.dedup();
                    mark(
                        ui,
                        State::Idle,
                        &format!(
                            "{} merchant{} name a zone this snapshot has no page for, so they are on no zone screen: {}",
                            orphans.len(),
                            if orphans.len() == 1 { "" } else { "s" },
                            zones.join(", ")
                        ),
                    );
                });
            }
        }
        ui.add_space(6.0);
    }

    fn window(&mut self, ui: &mut Ui, cx: &mut Cx) {
        heading(ui, "WINDOW");
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            /* The registry applies this on its next pass (`Windows::show` compares the setting
             * with the level it last pushed), so the only thing to do here is save it. */
            if ui
                .checkbox(
                    &mut cx.settings.always_on_top,
                    "main window starts always on top",
                )
                .changed()
            {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            ui.label(
                RichText::new("A tool window with a title bar has its own pin in it; a window with no title bar is in OVERLAYS below. This is only the main window's starting state.")
                    .color(TEXT_3),
            );
        });
        ui.add_space(6.0);
    }

    /// THE UPDATES SECTION: the two controls, the six facts, and the four buttons.
    ///
    /// # WHERE IT SITS AND WHY
    ///
    /// After WINDOW and before FIGHT NOTES. The ordering comment a few lines up in `ui` states the
    /// rule: the sections above FIGHT NOTES are about where the app READS from, and FIGHT NOTES
    /// and HOTKEYS are about what the owner has written into it. Updates is neither. WINDOW is the
    /// other section about how the app itself behaves, so this belongs beside it, and FILE stays
    /// last because it is the file this whole screen writes.
    ///
    /// # EVERY FIGURE HERE COMES OUT OF REAL STATE, AND AN EMPTY ONE SAYS SO
    ///
    /// The running version is `titlebar::version()`, which is `env!("CARGO_PKG_VERSION")` and
    /// cannot disagree with `Cargo.toml`. The last check is an `Option` and prints "never" through
    /// the same `last_read_text` the SOURCES ledger uses, rather than a zero or a stamp nothing
    /// produced. The download's two numbers are the bytes actually written and the length the
    /// SIGNED manifest states, so the percentage is against a figure a key vouched for rather than
    /// against a `Content-Length` header anybody can write. There is no placeholder anywhere in
    /// this section: a build with no updater at all says that in words.
    ///
    /// # THE ONE STATE THIS SECTION MAY NOT INVENT
    ///
    /// `self.updater` is `None` until `App::ui` has written a view into it, and it stays `None`
    /// forever on a build with nowhere to put the files (`Layout::platform` answers `None`) and in
    /// a preflight run, where `App::new` deliberately constructs no `Updater` at all. Those are
    /// three different silences and only the last two are permanent, so the line says what is
    /// true of all three and claims nothing about which.
    fn updates(&mut self, ui: &mut Ui, cx: &mut Cx) {
        use crate::updater::run::{self, Ask, Phase};

        heading(ui, "UPDATES");

        let running = crate::titlebar::version();
        let view = self.updater.clone();

        ui.horizontal(|ui| {
            field_label(ui, "Running");
            mono(ui, running, TEXT);
            /* NAMED ONLY WHEN THE UPDATER PUT IT THERE. On a build nobody has updated there is no
             * pointer, and printing "installed by the updater" beside a version the installer
             * wrote would be a claim about where the file came from that is simply false. */
            if let Some(v) = view.as_ref().and_then(|v| v.installed.clone()) {
                ui.label(
                    RichText::new(if v == running {
                        "installed by the updater".to_owned()
                    } else {
                        format!("the updater has installed {v}, which starts at the next launch")
                    })
                    .color(TEXT_3),
                );
            }
        });

        ui.horizontal(|ui| {
            field_label(ui, "Channel");
            for c in run::CHANNELS {
                /* `selectable_value` and not `radio_value`, the same choice `watch` above makes and
                 * for the same reason: `theme` spends this app's one round shape on the live pill.
                 *
                 * A LIST AND NOT A TEXT BOX. The channel is half of the manifest URL, and a typo in
                 * a text box would point the client at a path that publishes nothing and leave it
                 * saying it could not check, forever, with no way to tell that from an outage. */
                if ui
                    .selectable_label(cx.settings.updater.channel == c, c)
                    .clicked()
                    && cx.settings.updater.channel != c
                {
                    cx.settings.updater.channel = c.to_owned();
                    self.mark_dirty();
                }
            }
        });

        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            if ui
                .checkbox(&mut cx.settings.updater.enabled, "check for updates")
                .changed()
            {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            if ui
                .checkbox(
                    &mut cx.settings.updater.auto_download,
                    "download one as soon as it is found",
                )
                .changed()
            {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            if ui
                .checkbox(
                    &mut cx.settings.updater.auto_install,
                    "install it without asking",
                )
                .changed()
            {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            /* WHAT AUTO-DOWNLOAD DOES NOT DO IS THE HALF WORTH SAYING. A person reading a checkbox
             * called "download automatically" wants to know whether the app is going to change
             * itself while they are playing, and the answer is no: the bytes land in a staging
             * folder, the moment is chosen, and both of the steps that change what runs are
             * presses. */
            ui.label(
                RichText::new(
                    "A download waits for the encounter to close, and lands in a staging folder \
                     with its signature checked. Nothing is installed and nothing restarts until \
                     you press for it.",
                )
                .color(TEXT_3),
            );
        });

        let Some(view) = view else {
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                mark(
                    ui,
                    State::Idle,
                    "no updater is running in this window, so nothing here has been checked",
                );
            });
            ui.add_space(6.0);
            return;
        };

        /* THE THREAD WOULD NOT START. Shown ahead of the phase, because every line below it would
         * otherwise describe a check that is never coming as one that has not landed yet. */
        if let Some(p) = &view.problem {
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                mark(ui, State::Wrong, p);
            });
        }

        /* THE POINTER IS IN A SHAPE THIS BUILD CANNOT USE. `launch::plan` answers RunHere for it,
         * which is safe and silent, and silent is how an app that reinstalls the same version
         * every session goes unreported for months: the only symptom is a version number that
         * never changes. */
        if let Some(p) = &view.pointer_note {
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                mark(ui, State::Wrong, p);
            });
        }

        /* THIS BUILD TRUSTS A DEVELOPMENT SIGNING KEY, WHICH IS A FACT AND NOT A WARNING NOBODY
         * CAN ACT ON. A release-profile build carrying it does not compile and the release
         * workflow refuses to publish on it, so the only way to see this line is to be running a
         * development build, which is exactly who should see it. */
        if crate::updater::verify::trust_root_is_development() {
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                mark(
                    ui,
                    State::Idle,
                    "this build trusts the development signing key, so it would accept an update \
                     signed by whoever holds the development secret. Release builds cannot be \
                     made on it.",
                );
            });
        }

        let pulse = cx.ingest.pulse();
        ui.horizontal(|ui| {
            field_label(ui, "State");
            let (st, words) = phase_line(&view.phase);
            mark(ui, st, &words);
        });

        ui.horizontal(|ui| {
            field_label(ui, "Checked");
            /* THE SAME HELPER THE SOURCES LEDGER PRINTS ITS LAST READ WITH, so "never" and "12s
             * ago" are spelled the same way in both places. `last_check_before` comes out of
             * `state.json` and is what stops this line resetting to "never" on every launch for
             * somebody who checked an hour ago. */
            let when = view.last_check.or(view.last_check_before);
            mono(ui, &last_read_text(when, chrono::Utc::now()), TEXT_2);
            if view.last_check.is_none() && view.last_check_before.is_some() {
                ui.label(RichText::new("in an earlier session").color(TEXT_3));
            }
        });

        if let Some(f) = &view.failure {
            ui.horizontal(|ui| {
                field_label(ui, "Last error");
                /* THE STABLE WORD BESIDE THE SENTENCE. `Refusal::code` exists so a person pastes a
                 * word into a bug report that will not be reworded next week, and so a corrupt
                 * download and a tampered one are never the same string. */
                mono(ui, &f.code, WRONG);
            });
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                mark(ui, State::Wrong, &f.sentence);
            });
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                let about = match &f.version {
                    Some(v) => format!("{v}, {}", last_read_text(Some(f.when), chrono::Utc::now())),
                    None => last_read_text(Some(f.when), chrono::Utc::now()),
                };
                ui.label(RichText::new(about).color(TEXT_3));
            });
        }

        /* ------------------------------------------------------------- the buttons -- */

        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            /* CHECK NOW IS ONLY OFFERED WHEN CHECKING IS ON. The worker refuses it either way
             * (`Worker::tick` returns on `!enabled` whatever brought it there), so this is the
             * screen agreeing with the switch rather than the thing that enforces it. */
            /* AND NOT ONCE A RESTART IS ALREADY WAITING. `Worker::tick` returns early on
             * `Phase::Installed`, deliberately: checking on would find the same manifest and
             * either overwrite the one line the reader needs or offer the version already
             * installed. The button stayed drawn and enabled over that, so pressing it did
             * nothing at all and gave no feedback for the press, which is the shape of button a
             * person presses repeatedly. The line below the buttons is what says why. */
            let done = matches!(view.phase, Phase::Installed { .. });
            if cx.settings.updater.enabled && !done {
                let busy = matches!(view.phase, Phase::Checking | Phase::Downloading { .. });
                if ui
                    .add_enabled(!busy, egui::Button::new("Check now"))
                    .clicked()
                {
                    self.changed.updater = Some(Ask::CheckNow);
                }
            }

            /* DOWNLOAD IS OFFERED ONLY WHEN THERE IS SOMETHING KNOWN AND NOT YET FETCHED. While an
             * encounter is open the button is drawn and disabled rather than hidden, because a
             * control that disappears reads as a feature that is missing and a control that is
             * greyed reads as a moment that is wrong, and the second one is true. */
            if let Phase::Known { .. } = &view.phase {
                let quiet = crate::updater::may_start_download(pulse);
                if ui
                    .add_enabled(quiet, egui::Button::new("Download now"))
                    .clicked()
                {
                    self.changed.updater = Some(Ask::Download);
                }
            }

            if let Phase::Downloaded { version } = &view.phase {
                /* THE INSTALL IS AS MUCH A MOMENT AS THE DOWNLOAD IS, AND WAS THE ONE CONTROL HERE
                 * WITH NO GATE ON IT.
                 *
                 * The first thing `install_app` does after checking the bytes is SPAWN them, and a
                 * preflight is built with the same `NativeOptions` as any launch: a real,
                 * undecorated, visible window for `data::LOAD_BUDGET`, which also registers global
                 * hotkeys. Pressing this mid-raid put a second Grimoire window over a fullscreen
                 * game and took focus for three seconds. `install_app` asks the gate itself as
                 * well, so the rule does not live only at this call site. */
                let quiet = crate::updater::may_start_download(pulse);
                if ui
                    .add_enabled(quiet, egui::Button::new(format!("Install {version}")))
                    .on_hover_text(
                        "Runs the downloaded program once to check it starts on this machine, \
                         which briefly opens a second window, then verifies it again from disk and \
                         points the launcher at it. Nothing restarts.",
                    )
                    .clicked()
                {
                    self.changed.updater = Some(Ask::Install);
                }
            }

            /* TRY AGAIN, FOR THE FAILURES THAT WERE ABOUT THIS MACHINE AND NOT ABOUT THE RELEASE.
             *
             * A disk that filled, an antivirus that held the exe open for a second, a preflight
             * that failed against a driver: the worker retries these on a widening ladder, and the
             * reader is the one who actually knows the moment has passed, because the reader is
             * the one who freed the disk. It is drawn only when there is something to retry, so it
             * never appears beside a refusal nothing can change. */
            if view.retry_available && matches!(view.phase, Phase::Refused { .. }) {
                let quiet = crate::updater::may_start_download(pulse);
                if ui
                    .add_enabled(quiet, egui::Button::new("Try again"))
                    .on_hover_text(
                        "The last attempt failed for a reason about this machine rather than about \
                         the download, so it can simply be tried again.",
                    )
                    .clicked()
                {
                    self.changed.updater = Some(Ask::Download);
                }
            }

            if matches!(view.phase, Phase::Installed { .. }) {
                let quiet = run::may_restart(pulse, &view.phase);
                if ui
                    .add_enabled(quiet, egui::Button::new("Restart now"))
                    .clicked()
                {
                    self.changed.restart = true;
                }
            }

            /* THE DATA BUTTON IS A BUTTON AND NOT AN AUTOMATIC ACTION, which
             * `install::restore_previous_data` argues at length: rolling the snapshot back on its
             * own would fight the reader's own `data_root` override, which the loader treats as an
             * instruction rather than a guess. */
            if view.data_previous
                && ui
                    .button("Put the previous data back")
                    .on_hover_text(
                        "Swaps the installed snapshot with the one it replaced. Pressing it again \
                         puts this one back.",
                    )
                    .clicked()
            {
                self.changed.updater = Some(Ask::RestorePreviousData);
            }
        });

        /* THE RELEASE NOTES, WHEN THE SIGNED MANIFEST NAMED ONE. Below the buttons and not beside
         * them: it is the only control here that leaves the app. */
        if let Phase::Known {
            notes_url: Some(url),
            version,
            ..
        } = &view.phase
        {
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                ui.label(RichText::new(format!("what changed in {version}:")).color(TEXT_3));
                mono(ui, url, TEXT_2);
            });
        }

        if matches!(view.phase, Phase::Installed { .. }) {
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                /* WHY THERE IS NO CHECK NOW BUTTON HERE. It is not hidden to tidy the row: the
                 * worker stops checking once the pointer has flipped, so the button would have
                 * been one that did nothing and said nothing. */
                ui.label(
                    RichText::new(
                        "Nothing further is checked until this one has been started, because the \
                         answer would be about the version that is already waiting.",
                    )
                    .color(TEXT_3),
                );
            });
        }

        if matches!(view.phase, Phase::Installed { .. })
            && !crate::updater::may_start_download(pulse)
        {
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                ui.label(
                    RichText::new(
                        "The restart is held back until the encounter closes. Nothing is waiting \
                         on it: the new version starts whenever you next open the app.",
                    )
                    .color(TEXT_3),
                );
            });
        }

        ui.add_space(6.0);
    }

    fn hotkeys(&mut self, ui: &mut Ui, cx: &mut Cx) {
        heading(ui, "HOTKEYS");
        if self.bindings.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(FIELD_W);
                mark(ui, State::Idle, "no bindings reported: the hotkey manager has not installed yet, or its bindings were not handed to this screen");
            });
            ui.add_space(6.0);
            return;
        }
        let failed = self.bindings.iter().filter(|b| !b.registered).count();
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            if failed == 0 {
                mark(ui, State::Settled, &format!("all {} registered", self.bindings.len()));
            } else {
                /* The gold "act" state, the same word the persona footer's gear uses: a chord
                 * another program owns is something a person resolves, by rebinding it here or
                 * closing the other program. Not WRONG: nothing of ours failed. */
                mark(ui, State::You, &format!("{failed} of {} not registered; rebind the row or close the program that owns it", self.bindings.len()));
            }
            if ui.button("Copy bindings").clicked() {
                ui.ctx().copy_text(bindings_text(&self.bindings));
                self.copied_at = Some(Instant::now());
            }
            if let Some(t) = self.copied_at {
                if t.elapsed() < COPIED_FOR {
                    ui.label(RichText::new("copied").color(TEXT_3));
                    ui.ctx().request_repaint_after(COPIED_FOR - t.elapsed());
                } else {
                    self.copied_at = None;
                }
            }
        });
        ui.add_space(4.0);
        /* Edits are collected under the grid and applied after it: the grid borrows the
         * bindings, and applying one changes settings, which the integrator turns into a new
         * bindings list on its next frame. */
        let mut apply: Option<(&'static str, String)> = None;
        let mut reset: Option<&'static str> = None;
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            egui::Grid::new("hotkey-table")
                .num_columns(4)
                .spacing([18.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("chord").color(TEXT_3));
                    ui.label(RichText::new("opens").color(TEXT_3));
                    ui.label(RichText::new("state").color(TEXT_3));
                    ui.label(RichText::new("").color(TEXT_3));
                    ui.end_row();
                    for b in &self.bindings {
                        let buf = self
                            .chord_text
                            .entry(b.id)
                            .or_insert_with(|| b.chord.clone());
                        let r = ui.add(
                            egui::TextEdit::singleline(buf)
                                .desired_width(170.0)
                                .font(FontId::monospace(11.5)),
                        );
                        if r.changed() {
                            self.chord_problem.remove(b.id);
                        }
                        let submitted =
                            r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.label(RichText::new(tool_text(b)).color(TEXT_2));
                        match (self.chord_problem.get(b.id), b.registered, &b.conflict) {
                            (Some(p), _, _) => mark(ui, State::Wrong, p),
                            (None, true, _) if b.overridden => {
                                mark(ui, State::Settled, "registered, rebound")
                            }
                            (None, true, _) => mark(ui, State::Settled, "registered"),
                            (None, false, Some(c)) => mark(ui, State::You, c),
                            (None, false, None) => {
                                mark(ui, State::You, "not registered, no reason reported")
                            }
                        };
                        ui.horizontal(|ui| {
                            let dirty = buf.trim() != b.chord;
                            if (ui.add_enabled(dirty, egui::Button::new("Apply")).clicked()
                                || (submitted && dirty))
                                && apply.is_none()
                            {
                                apply = Some((b.id, buf.clone()));
                            }
                            if b.overridden && ui.button("Default").clicked() {
                                reset = Some(b.id);
                            }
                        });
                        ui.end_row();
                    }
                });
        });
        if let Some((id, text)) = apply {
            match crate::hotkeys::parse_chord(&text) {
                Ok(spec) => {
                    let canonical = spec.text();
                    let default = self
                        .bindings
                        .iter()
                        .find(|b| b.id == id)
                        .map(|b| b.default)
                        .unwrap_or("");
                    if canonical == default {
                        cx.settings.hotkeys.remove(id);
                    } else {
                        cx.settings.hotkeys.insert(id.to_owned(), canonical.clone());
                    }
                    self.chord_text.insert(id, canonical);
                    self.chord_problem.remove(id);
                    self.changed.hotkeys = true;
                    self.mark_dirty();
                }
                Err(e) => {
                    self.chord_problem.insert(id, format!("not applied: {e}"));
                }
            }
        }
        if let Some(id) = reset {
            cx.settings.hotkeys.remove(id);
            if let Some(b) = self.bindings.iter().find(|b| b.id == id) {
                self.chord_text.insert(id, b.default.to_owned());
            }
            self.chord_problem.remove(id);
            self.changed.hotkeys = true;
            self.mark_dirty();
        }
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            /* The window length is the hotkeys module's constant, printed rather than restated,
             * so this line cannot drift from the machine it describes. */
            let secs = crate::hotkeys::LEADER_WINDOW.as_secs_f32();
            ui.label(
                RichText::new(format!(
                    "Type a chord as Ctrl+Alt+S then K (modifiers joined by +, one plain key after then) and press Apply or Enter. A leader chord waits {secs:.1} seconds for its second key, and takes that key from every program for exactly that long so the chord completes with the game in front. Default puts a row back to the built-in chord."
                ))
                .color(TEXT_3),
            );
        });
        ui.add_space(6.0);
    }

    /// WHAT A PERSON WROTE ABOUT A FIGHT, AND THE ONLY PLACE HE CAN SEE ALL OF IT.
    ///
    /// # THE DEFECT THIS SECTION EXISTS FOR
    ///
    /// `Settings::fight_notes` is a map from a fight's own start stamp to a person's words about
    /// it, and the stamp is the right key for the reason its own doc gives: the fights list is
    /// rebuilt on every rescan, and an index would slide a note onto a different pull. What that
    /// key costs is a way BACK to the note. The one surface that reads the map is the Analysis
    /// page, and it reads exactly one entry, the one belonging to the fight it is currently
    /// showing. Analysis can only show a fight the app is holding, and the app holds the last
    /// 40MB of the log. So the day a noted fight falls off the back of that tail, the note is
    /// still in `settings.json`, still saved and reloaded on every launch, and there is no screen
    /// in the app that can print it or delete it. It is not lost, which would at least be honest.
    /// It is invisible.
    ///
    /// THE HALF OF THAT FINDING THAT IS ALREADY GONE, recorded because a reader will otherwise
    /// wonder why this is not larger. It used to be true that NOTHING in the main window could
    /// open the Analysis page at all (see the note in `nav::SECTIONS`), so the map was a setting
    /// no main-window reader could even produce. Analysis is a section of Log Parser now and is
    /// routed. Writing a note is reachable. Finding one again was not, and that is what this fixes.
    ///
    /// # WHY IT IS A LIST AND A DELETE AND NOT AN EDITOR
    ///
    /// A note belongs to a fight and is written where the fight is, with the fight on screen next
    /// to it. Editing one here, against a stamp and nothing else, would be typing about a pull
    /// nobody can see. What a person needs from this screen is the two things Analysis cannot
    /// give him: every note at once, and a way to throw one away.
    ///
    /// AND IT NEVER SAYS WHETHER A FIGHT IS STILL REACHABLE. It would take a store read per frame
    /// to know, and the answer would be a claim about a fight this screen is not holding. The
    /// stamp is printed as the log wrote it, which is a fact, and the reader can tell.
    fn notes(&mut self, ui: &mut Ui, cx: &mut Cx) {
        heading(ui, "FIGHT NOTES");
        /* The delete is collected and applied after the rows are gone: the rows borrow the map
         * that removing an entry mutates, and the count is taken while the borrow is still live. */
        let mut drop_key: Option<String> = None;
        let n = {
            let rows = notes_newest_first(&cx.settings.fight_notes);
            if rows.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(FIELD_W);
                    mark(
                        ui,
                        State::Idle,
                        "none written. A note is written on a fight, on Log Parser's Analysis \
                         page, and every one you write shows up here",
                    );
                });
                ui.add_space(6.0);
                return;
            }
            for (stamp, text) in &rows {
                ui.horizontal_top(|ui| {
                    ui.add_space(FIELD_W);
                    /* THE STAMP AS THE LOG WROTE IT, in the mono face this screen gives every
                     * other piece of text that came out of a file rather than out of this app. It
                     * is the key, so it is also the only thing that identifies the fight. */
                    mono(ui, stamp, TEXT_3);
                    if ui
                        .button("Delete")
                        .on_hover_text(
                            "Remove this note from settings.json. The fight it was written on is \
                             not touched: a note is this app's, the fight is the log's.",
                        )
                        .clicked()
                    {
                        drop_key = Some((*stamp).to_owned());
                    }
                });
                ui.horizontal_top(|ui| {
                    ui.add_space(FIELD_W);
                    /* WRAPPED AND NOT TRUNCATED. A note is a person's own sentence and the whole
                     * point of this section is that it can be read; an ellipsis here would leave
                     * a note as unreachable as it was, only visibly so. */
                    ui.label(RichText::new(*text).color(TEXT_2));
                });
                ui.add_space(4.0);
            }
            rows.len()
        };
        if let Some(k) = drop_key {
            cx.settings.fight_notes.remove(&k);
            self.mark_dirty();
        }
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            let s = if n == 1 { "" } else { "s" };
            ui.label(RichText::new(format!("{n} note{s} stored")).color(TEXT_3));
        });
        ui.add_space(6.0);
    }

    fn file_line(&mut self, ui: &mut Ui) {
        heading(ui, "FILE");
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            match Settings::path() {
                Some(p) => mono(ui, &p.display().to_string(), TEXT_3),
                None => mark(
                    ui,
                    State::Wrong,
                    "no config directory on this platform; nothing persists",
                ),
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(FIELD_W);
            if self.dirty_since.is_some() {
                ui.label(RichText::new("saving").color(TEXT_3));
            } else if let Some((at, r)) = &self.last_save {
                match r {
                    Ok(_) => mark(
                        ui,
                        State::Settled,
                        &format!("saved {} ago", age_text(at.elapsed().as_secs() as i64)),
                    ),
                    Err(e) => mark(ui, State::Wrong, e),
                };
            }
        });
    }
}

/* ------------------------------------------------------------------ helpers -- */

/// What the ingest looks for, in the words the SOURCES empty state prints. The file names are the
/// EQ client's, not this app's invention.
const LOOKS_FOR: &[&str] = &[
    "eqlog_<character>_<server>.txt in the Logs folder; the game writes it only while logging is on (/log on)",
    "<character>_<server>-Inventory.txt from /outputfile inventory, beside the game executable, one folder above Logs",
    "<character>_<server>-Achievements.txt from /outputfile achievements, same place",
];

/// WHERE THE UPDATER IS, AS A SIGNAL COLOUR AND A SENTENCE.
///
/// # A FREE FUNCTION BECAUSE A BRANCH INSIDE A DRAW IS A BRANCH NOTHING CAN TEST
///
/// `App::ui` cannot be called from a test at all (`main.rs:2537`: `eframe::Frame` has no public
/// constructor), and the same is true of every `fn` on `SettingsScreen` that takes a `Ui`. This is
/// the whole of what the UPDATES section decides, lifted out so it can be driven, exactly as
/// `rail_plan`, `persona_pending` and `parse_smoke` were lifted out of `App::ui` for the same
/// reason.
///
/// # WHY EACH STATE GETS THE COLOUR IT GETS
///
/// `Working` is the Gnomish signal for something in flight, and it covers the three states that
/// are waiting on something: a check, a download, and a payload sitting staged for a press.
/// `Settled` is for the two endings that need nothing from anybody. `Idle` is the hollow ring and
/// belongs to the two states where nothing is happening AND nothing is wrong, which is a switched
/// off updater and a session that has not checked yet. `Wrong` is a refusal and nothing else, so
/// that a red square on this screen always means a decision was made against the update rather
/// than that a download is slow.
fn phase_line(p: &crate::updater::run::Phase) -> (State, String) {
    use crate::updater::run::Phase;
    match p {
        Phase::NeverChecked => (
            State::Idle,
            "nothing has been checked yet in this session".to_owned(),
        ),
        Phase::Off => (
            State::Idle,
            "checking is switched off, so nothing is being looked for".to_owned(),
        ),
        Phase::Checking => (State::Working, "asking the update channel".to_owned()),
        Phase::UpToDate => (
            State::Settled,
            "this is the newest version published on this channel".to_owned(),
        ),
        Phase::Known { version, why, .. } => (
            State::Working,
            format!("{version} is available, and is not being fetched: {why}"),
        ),
        Phase::Downloading {
            version,
            done,
            size,
        } => (
            State::Working,
            /* THE TWO REAL COUNTS AND A PERCENTAGE DERIVED FROM THEM, never a bar with no number
             * beside it. `size` is the length the SIGNED manifest states and `done` is what has
             * actually been written, so the percentage is against a figure a key vouched for.
             *
             * A SIZE OF ZERO PRINTS NO PERCENTAGE RATHER THAN A DIVIDE. A signed manifest should
             * never carry one, and `copy_sealed` would refuse the transfer on the first byte if it
             * did; printing "100%" for it would be a figure nothing measured. */
            if *size == 0 {
                format!("fetching {version}: {done} bytes so far, of a length the manifest gave as zero")
            } else {
                format!(
                    "fetching {version}: {}%, {done} of {size} bytes",
                    done.saturating_mul(100) / size
                )
            },
        ),
        Phase::Downloaded { version } => (
            State::Working,
            format!(
                "{version} has been downloaded and its signature checked. Nothing on disk has \
                 been replaced yet."
            ),
        ),
        Phase::Installed { version } => (
            State::Settled,
            format!("{version} is installed and starts the next time the app opens"),
        ),
        /* THE SENTENCE IS THE REFUSAL'S OWN. `Refusal`'s `Display` names both what was wrong and
         * what was expected, deliberately, because "signature check failed" sends a person to the
         * wrong half of the pipeline. Rewording it here would undo that. */
        Phase::Refused { why, .. } => (State::Wrong, why.clone()),
        /* AND AN OUTAGE IS NOT A REFUSAL, WHICH IS THE INVARIANT THIS WHOLE COMMENT BLOCK CLAIMS.
         *
         * A dropped connection used to arrive here as `Phase::Refused` and was drawn with the red
         * square, so a reader playing offline, behind a captive portal, or during a two minute
         * outage at the host was shown the same signal the screen reserves for a signature that
         * did not check out, for the rest of the session. `Working` is the honest one: something is
         * in flight and has not landed. The Last error line still carries the code word, because
         * that is what a bug report wants. */
        Phase::Unreachable { since, why } => (
            State::Working,
            format!(
                "{why}. It has not answered since {}, and it is asked again on a widening ladder \
                 rather than continuously.",
                since.format("%H:%M")
            ),
        ),
    }
}

/// "12s ago", or "never" when the ingest has not read the file.
fn last_read_text(
    last_read: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    match last_read {
        Some(t) => format!("{} ago", age_text((now - t).num_seconds())),
        None => "never".to_owned(),
    }
}

/// Left pad to `w` so a column of numbers lines up on its right edge in a monospace face.
fn pad_left(s: &str, w: usize) -> String {
    format!("{s:>w$}")
}

/// WHEN A NOTE'S FIGHT STARTED, read off the key.
///
/// THE KEY IS A LOG STAMP AND NOT A SORTABLE STRING, which is the whole reason this exists.
/// `Settings::fight_notes` is a `BTreeMap`, so its own order is the byte order of
/// `Wed Jul 15 23:16:50 2026`, and that begins with the WEEKDAY: every Friday in a person's
/// history sorts before every Monday, and July sorts before June. Printing the map in its own
/// order would be printing a person's notes in an order that looks deliberate and is nonsense.
///
/// PARSED BY `screens::sky::log_ts` AND NOT BY A SECOND READER OF ITS OWN. That function already
/// reads exactly this format, already drops the weekday before parsing (chrono checks a weekday
/// against its date and a client that wrote a wrong one would otherwise make the whole stamp
/// unreadable), and is tested where it lives. A stamp parser written here would be a second
/// chance to disagree with it.
///
/// `None` IS A REAL ANSWER AND THE CALLER MUST NOT HIDE IT. A key can be anything a hand edit or
/// an older build put in the file, and a note whose stamp cannot be read is exactly the note this
/// section was written to make reachable.
fn note_when(stamp: &str) -> Option<chrono::NaiveDateTime> {
    crate::screens::sky::log_ts(stamp.trim().trim_start_matches('[').trim_end_matches(']'))
}

/// EVERY STORED NOTE, NEWEST FIRST, WITH THE UNREADABLE STAMPS KEPT AT THE END.
///
/// NEWEST FIRST BECAUSE THE NOTE A PERSON IS LOOKING FOR IS ALMOST ALWAYS THE ONE HE JUST WROTE.
/// This is the same ordering the loot feed and the fights list use and for the same reason.
///
/// A STAMP THAT WILL NOT PARSE STILL GETS A ROW. It goes last, in key order, because there is no
/// honest place to put it among the dated ones; dropping it would leave the one note this whole
/// section exists for as invisible as it was before.
pub fn notes_newest_first(notes: &BTreeMap<String, String>) -> Vec<(&str, &str)> {
    let mut rows: Vec<(&str, &str)> = notes
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    /* `Option` orders `None` below every `Some`, so a descending sort on the parsed stamp puts
     * the newest first and sweeps the unreadable ones to the bottom in one comparison. */
    rows.sort_by(|a, b| {
        note_when(b.0)
            .cmp(&note_when(a.0))
            .then_with(|| a.0.cmp(b.0))
    });
    rows
}

/// Width of the label column, so every field's control starts on the same vertical line.
const FIELD_W: f32 = 92.0;

fn heading(ui: &mut Ui, s: &str) {
    ui.add_space(10.0);
    ui.label(
        RichText::new(s)
            .font(crate::fonts::display(12.0))
            .color(GOLD),
    );
    ui.add_space(4.0);
}

fn field_label(ui: &mut Ui, s: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(FIELD_W, 18.0), egui::Sense::hover());
    ui.painter().text(
        egui::Pos2::new(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        s,
        FontId::proportional(12.5),
        TEXT_2,
    );
}

fn mono(ui: &mut Ui, s: &str, col: egui::Color32) -> egui::Response {
    ui.label(RichText::new(s).font(FontId::monospace(11.5)).color(col))
}

/// A leading square in a state colour and a line of text. The square is the Gnomish signal, drawn
/// the way `chrome::nav_row` draws it: idle is a hollow ring, everything else is filled.
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

fn path_text(p: &Option<PathBuf>) -> String {
    p.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

fn text_path(s: &str) -> Option<PathBuf> {
    let t = s.trim().trim_matches('"');
    if t.is_empty() {
        None
    } else {
        Some(PathBuf::from(t))
    }
}

/// "12s", "3m", "2h", "5d": how long ago, in the coarsest unit that is not zero.
pub fn age_text(secs: i64) -> String {
    let s = secs.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::SourceKind;
    use serde_json::json;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grimoire-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("nested").join(FILE)
    }

    #[test]
    fn defaults_carry_nothing_invented() {
        let s = Settings::default();
        assert!(s.data_root.is_none());
        assert!(s.log_dir.is_none());
        assert!(!s.always_on_top);
        assert!(s.extra.is_empty());
    }

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

    /// Paint the WHOLE settings screen for two real frames and report every string it drew and
    /// whether Tab found anything on it to take the caret.
    ///
    /// THE WHOLE SCREEN AND NOT ONE SECTION. What is asserted over this is now an ABSENCE, and an
    /// absence is only worth asserting over everything: a helper that ran one section could go
    /// green while the retired rows sat under the next heading down.
    ///
    /// TWO PASSES, BECAUSE FOCUS IS ALWAYS ONE PASS BEHIND. egui decides where Tab sends focus
    /// from the widget list the PREVIOUS pass registered, so a single pass would report "nothing
    /// focusable" over a screen full of text boxes. The first pass registers, the second presses
    /// Tab, and `Memory::focused` is read after it. The strings come from the first pass, before
    /// anything is focused, so they are what the screen paints when it is simply looked at.
    ///
    /// THE RECT IS TALL ENOUGH FOR EVERY SECTION AT ONCE. An egui widget paints nothing for a
    /// rect the pass cannot see, so a short viewport inside this screen's `ScrollArea` would hand
    /// back the first heading or two, and an assertion that a word is absent from THAT is an
    /// assertion about a viewport rather than about the screen.
    ///
    /// The faces are installed because `heading` sets its word in Cinzel and a default `Context`
    /// carries egui's own definitions with no family under that name, so the lookup panics inside
    /// epaint.
    ///
    /// NOTHING HERE CAN REACH THE OPERATOR'S settings.json. No event is fed that changes a value,
    /// so `dirty_since` stays `None` and the debounce in `ui` never calls `write`; and `write`
    /// goes to `save_replacing_unreadable`, which refuses outright under `cfg(test)`.
    fn settings_screen_frame() -> (Vec<String>, bool) {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let mut screen = SettingsScreen::default();
        let mut settings = Settings::default();
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(YOUTUBE_HANDLE),
        };
        let input = |events: Vec<egui::Event>| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 3000.0),
            )),
            events,
            ..Default::default()
        };
        let mut pass = |events: Vec<egui::Event>,
                        screen: &mut SettingsScreen,
                        settings: &mut Settings|
         -> Vec<String> {
            let mut cx = crate::screens::Cx {
                data: None,
                railed: false,
                data_err: None,
                live: &live,
                settings,
                ingest: &mut ingest,
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
            let mut out = ctx.run_ui(input(events), |ui| screen.ui(ui, &mut cx));
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
        };

        let words = pass(Vec::new(), &mut screen, &mut settings);
        let tab = egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        pass(vec![tab], &mut screen, &mut settings);
        let focused = ctx.memory(|m| m.focused()).is_some();
        (words, focused)
    }

    /// THE SETTINGS SCREEN SAYS NOTHING ABOUT THE CHANNEL, AND THIS DRIVES REAL FRAMES TO SAY SO.
    ///
    /// A `CHANNEL` section used to open this screen. Its two handle editors went first, when the
    /// owner said Settings should have no spot to swap `Broken_Stoic` out; what was left was a
    /// heading, a sentence saying the build follows one channel and cannot be pointed at another,
    /// and the two handles with their poll cadence. The owner looked at that and asked why any of
    /// it was there. It is all true and none of it is a decision, so the section is gone.
    ///
    /// WHY THIS IS NOT THE VACUOUS TEST THE OLD ONE WOULD HAVE DECAYED INTO. The test this
    /// replaces pressed Tab at the section and asserted nothing in it took the caret. Delete the
    /// section and that assertion passes over an empty frame, which proves nothing whatsoever. So
    /// the absence is asserted over the WHOLE screen, and it is fenced on both sides:
    ///
    /// FENCE ONE, the screen really painted. Every remaining heading and the WATCH section's own
    /// sentence are asserted PRESENT first. If `ui` painted nothing, or half of itself, the
    /// absence would be free and these fail before it is ever reached.
    ///
    /// FENCE TWO, the screen really registered controls. Tab must land on something. A frame that
    /// draws text but registers no widget is the other way an absence goes green for free, and it
    /// is the state the old helper's `!focused` was silently satisfied by.
    ///
    /// WHAT STILL HOLDS THE ORIGINAL CLAIM, that the handles are not settable. Not this test on
    /// its own: an empty editor with a hint would paint no handle. It is that there is no field
    /// left to bind one to. `twitch_handle` and `youtube_handle` are not on `Settings`, they are
    /// in `LEGACY_KEYS`, and `an_old_settings_file_still_loads_and_keeps_every_other_field` holds
    /// them out of both `extra` and the next write. This test holds the weaker and now more
    /// useful line: the screen does not talk about the channel at all.
    #[test]
    fn the_settings_screen_carries_nothing_about_the_channel() {
        let (words, focused) = settings_screen_frame();
        let joined = words.join("\u{1F}").to_lowercase();

        /* FENCE ONE */
        for want in [
            "WATCH",
            "Preferred",
            "The live pill reports",
            "DATA",
            "LOG FOLDER",
            "WINDOW",
            "HOTKEYS",
        ] {
            assert!(
                joined.contains(&want.to_lowercase()),
                "the settings screen never painted {want:?}, so an absence proves nothing here; \
                 it painted {words:?}"
            );
        }

        /* FENCE TWO */
        assert!(
            focused,
            "Tab landed on nothing, so this frame registered no widgets and the absence below is \
             free; it painted {words:?}"
        );

        /* THE HEADING, MATCHED EXACTLY AND NO LONGER AS A SUBSTRING.
         *
         * IT WAS A CASE-INSENSITIVE SUBSTRING SEARCH FOR "channel" OVER THE WHOLE SCREEN, and that
         * stopped being the right question when the UPDATES section arrived, because there are now
         * two unrelated things in this app called a channel: the one this build follows on Twitch,
         * which is what this test is about, and the release channel an update is published on,
         * which is a genuine decision with two values and belongs on a settings screen. A
         * substring search cannot tell them apart and would have forced the update control to be
         * named something nobody calls it.
         *
         * THIS IS NOT THE WEAKER TEST IT LOOKS LIKE. Every heading on this screen is painted by
         * `heading`, which paints the literal constant as one string, so an exact match on a
         * painted word catches the section coming back exactly as well as the substring did. The
         * five phrases below are still substring searches and they are the ones that carry the
         * actual claim: the handles, the cadence, and the sentence about not being repointable. */
        assert!(
            !words.iter().any(|w| w == "CHANNEL"),
            "the settings screen painted a CHANNEL heading again; that section is meant to be gone \
             from it. It painted {words:?}"
        );

        /* the section itself */
        for gone in [
            "cannot be pointed at another",
            TWITCH_HANDLE,
            "@broken_stoic",
            "polled every",
            "polled with",
        ] {
            assert!(
                !joined.contains(&gone.to_lowercase()),
                "the settings screen still says {gone:?}; the CHANNEL section is meant to be gone \
                 from it. It painted {words:?}"
            );
        }
    }

    /// A settings.json WRITTEN BY A BUILD THAT HAD THE FIELDS MUST NOT COST THE USER ANYTHING.
    ///
    /// The file on James's machine already carries `twitch_handle` and `youtube_handle`. Removing
    /// the typed fields is only safe because unknown keys are collected by `#[serde(flatten)]`
    /// rather than refused, and "only safe because" is a claim, so this is the file itself: every
    /// real setting comes back, another lane's key comes back, and the two retired keys are gone
    /// from `extra` and stay out of the next write instead of being copied forward for ever.
    ///
    /// It goes through `load_from` and `save_to` on a scratch path, never `load`/`save`, for the
    /// reason the module header gives.
    #[test]
    fn an_old_settings_file_still_loads_and_keeps_every_other_field() {
        let p = scratch("legacy-handles");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            r#"{
              "twitch_handle": "SomebodyElse",
              "youtube_handle": "@somebodyelse",
              "data_root": "C:/eq/data",
              "log_dir": "C:/eq/Logs",
              "always_on_top": true,
              "hotkeys": {"companion": "Ctrl+Alt+G"},
              "lfg_board": [{"who": "Stoic"}]
            }"#,
        )
        .unwrap();

        let s = Settings::load_from(&p).expect("an old file must load, not wipe the real settings");
        assert_eq!(s.data_root, Some(PathBuf::from("C:/eq/data")));
        assert_eq!(s.log_dir, Some(PathBuf::from("C:/eq/Logs")));
        assert!(s.always_on_top);
        assert_eq!(
            s.hotkeys.get("companion").map(String::as_str),
            Some("Ctrl+Alt+G")
        );
        assert_eq!(s.extra["lfg_board"][0]["who"], "Stoic");
        for k in LEGACY_KEYS {
            assert!(
                !s.extra.contains_key(k),
                "{k} was adopted by extra instead of being dropped"
            );
        }

        s.save_to(&p).expect("save");
        let written = std::fs::read_to_string(&p).unwrap();
        for k in LEGACY_KEYS {
            assert!(
                !written.contains(k),
                "{k} was written back out; it is a setting nothing reads"
            );
        }
        assert!(
            written.contains("lfg_board"),
            "another lane's key must survive the same save"
        );

        /* THE OPERATOR'S ACTUAL FILE, copied byte for byte out of
         * %APPDATA%\eql-grimoire\settings.json on 2026-09-02 while this unit was written. The
         * fixture above is a superset and could have been generous in a way the real file is not:
         * this one carries `"data_root": null` and an EMPTY `youtube_handle`, and losing that log
         * folder is the whole cost this test exists to refuse. */
        let real = scratch("operators-file");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(
            &real,
            "{\n  \"twitch_handle\": \"Broken_Stoic\",\n  \"youtube_handle\": \"\",\n  \
             \"data_root\": null,\n  \"log_dir\": \"C:\\\\Users\\\\Public\\\\Daybreak Game \
             Company\\\\Installed Games\\\\EverQuest Legends\\\\Logs\",\n  \
             \"always_on_top\": false\n}\n",
        )
        .unwrap();
        let s = Settings::load_from(&real).expect("the file on the operator's machine must load");
        assert_eq!(s.data_root, None);
        assert_eq!(
            s.log_dir,
            Some(PathBuf::from(
                r"C:\Users\Public\Daybreak Game Company\Installed Games\EverQuest Legends\Logs"
            )),
            "the log folder is the setting a failed load would cost"
        );
        assert!(!s.always_on_top);
        assert!(
            s.extra.is_empty(),
            "the two retired keys were its only extras"
        );

        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
        let _ = std::fs::remove_dir_all(real.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn round_trip_preserves_extra() {
        let mut s = Settings {
            always_on_top: true,
            log_dir: Some(PathBuf::from("C:/eq/Logs")),
            ..Default::default()
        };
        s.extra.insert(
            "lfg_board".into(),
            json!([{"who": "Stoic", "want": "raid"}]),
        );
        s.extra
            .insert("priorities".into(), json!({"str": 3, "sta": 2}));
        let p = scratch("roundtrip");
        s.save_to(&p).expect("save");
        let back = Settings::load_from(&p).expect("load");
        assert_eq!(back, s);
        assert_eq!(back.extra["lfg_board"][0]["who"], "Stoic");
        assert_eq!(back.extra["priorities"]["str"], 3);
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn unknown_keys_land_in_extra_and_survive_a_save() {
        /* Another lane wrote a key this struct does not name. It must come back out. */
        let text = r#"{"always_on_top":true,"lfg_board":[1,2,3],"someday":{"a":1}}"#;
        let s: Settings = serde_json::from_str(text).unwrap();
        assert_eq!(s.extra["lfg_board"], json!([1, 2, 3]));
        assert_eq!(s.extra["someday"]["a"], 1);
        let out = serde_json::to_string(&s).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["lfg_board"], json!([1, 2, 3]));
        assert_eq!(v["someday"]["a"], 1);
        assert!(
            v.get("load_problem").is_none(),
            "load_problem is never written"
        );
    }

    #[test]
    fn partial_file_fills_defaults() {
        let s: Settings = serde_json::from_str(r#"{"always_on_top": true}"#).unwrap();
        assert!(s.always_on_top);
        assert!(s.log_dir.is_none());
    }

    /// DEFECT: THE OWNER'S OWN SETTINGS FILE COULD NOT RECEIVE A NEW DEFAULT.
    ///
    /// # THE FILE THIS IS BUILT FROM IS REAL
    ///
    /// These are the bytes that were in `%APPDATA%`eql-grimoire`settings.json` on the owner's
    /// machine on 2026-09-08, copied out while working out why the damage meter he had asked for
    /// was built, tested, shipped, running, and not on his screen. He has never opened the
    /// overlay builder. An older build wrote this list for him the first time he dragged an
    /// overlay window: see the defect on `overlay::Overlay::widgets`.
    ///
    /// # WHY A FIXTURE OF REAL BYTES AND NOT A CONSTRUCTED ONE
    ///
    /// Everything else about this defect is covered by unit tests over `forget_unchosen` and
    /// `apply_edits`, and all of them build their input by calling the same constructors the
    /// production code calls. That is exactly the shape of test that stayed green while the
    /// owner looked at the wrong widget: it can only prove the pieces agree with each other.
    /// This one starts from JSON that a DIFFERENT BUILD wrote and asserts what a reader ends up
    /// looking at, which is the only claim that was ever in doubt.
    ///
    /// READ ONLY, ON A SCRATCH PATH. Nothing here goes near the real file; see the module doc
    /// for the day a test overwrote it.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the `forget_unchosen` call out of `load_from`,
    /// or `Overlay::panels` preferring a stored list over the shipped one.
    #[test]
    fn a_real_settings_file_from_before_the_meter_still_gets_the_meter() {
        let path = scratch("owner-fossil");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"overlays":[{"id":"dps","name":"DPS","open":false,"pinned":true,
               "chips":false,"w":460,"h":200,"widgets":[{"kind":"ranked",
               "metric":"dealt","rate":true,"cols":{"rank":false,"value":true,
               "share":true,"bar":true,"head":false},"cap":12,"headline":true}]}]}"#,
        )
        .expect("writes the fixture");

        let s = Settings::load_from(&path).expect("the owner's file loads");
        let list = crate::overlay::or_default(&s.overlays);
        assert_eq!(list.len(), 1, "his one overlay did not survive the load");
        assert_eq!(
            list[0].id, "dps",
            "the overlay he had was replaced rather than repaired"
        );
        /* HIS GEOMETRY IS UNTOUCHED. Only the list nobody chose is handed back. */
        assert_eq!(list[0].w, 460.0, "the window size he had was thrown away");
        assert!(list[0].pinned, "his pin was thrown away");

        assert_eq!(
            list[0].widgets, None,
            "the panel list an older build wrote for him is still being read as his choice, so \
             his overlay can never receive a new default"
        );
        assert_eq!(
            list[0].panels(),
            crate::overlay::Overlay::shipped_panels(),
            "his overlay is not drawing what this build ships"
        );
        assert!(
            matches!(
                list[0].panels().first(),
                Some(crate::overlay::Widget::Meter(_))
            ),
            "the overlay he opens is still not the damage meter he asked for: {:?}",
            list[0].panels()
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    /// THE OWNER'S OWN SETTINGS FILE, AS IT WAS ON THE NIGHT HIS QUARTER SCREEN DREW THE FULL PAGE.
    ///
    /// Thirteen placements, every one the shipped cell, copied byte for byte out of his file. The
    /// window could not choose his layout while this was read as his choice.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the dashboard's `forget_unchosen` call out of
    /// `load_from`.
    #[test]
    fn a_real_dashboard_nobody_arranged_follows_the_window_again() {
        let path = scratch("owner-dashboard");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let fossil = r#"{"dashboard": [ { "tile": "live", "col": 8, "span": 5, "row": 31, "rows": 12 }, { "tile": "damage", "col": 1, "span": 7, "row": 31, "rows": 31 }, { "tile": "healing", "col": 5, "span": 4, "row": 62, "rows": 22 }, { "tile": "taken", "col": 1, "span": 4, "row": 62, "rows": 22 }, { "tile": "timeline", "col": 1, "span": 12, "row": 1, "rows": 30 }, { "tile": "progression", "col": 8, "span": 5, "row": 43, "rows": 19 }, { "tile": "yours", "col": 1, "span": 6, "row": 105, "rows": 22 }, { "tile": "fights", "col": 9, "span": 4, "row": 62, "rows": 22 }, { "tile": "night", "col": 12, "span": 1, "row": 94, "rows": 11 }, { "tile": "mobs", "col": 12, "span": 1, "row": 84, "rows": 10 }, { "tile": "kills", "col": 1, "span": 5, "row": 84, "rows": 21 }, { "tile": "loot", "col": 6, "span": 6, "row": 84, "rows": 21 }, { "tile": "overlays", "col": 7, "span": 6, "row": 105, "rows": 22 } ]}"#;
        std::fs::write(&path, fossil).expect("writes the fixture");
        let s = Settings::load_from(&path).expect("the owner's file loads");
        assert_eq!(
            s.dashboard, None,
            "the shipped layout a click wrote for him is still read as his arrangement, so the \
             window can never choose how much dashboard he gets"
        );

        /* AND ONE CARD HE REALLY MOVED KEEPS THE WHOLE LIST HIS. */
        std::fs::write(
            &path,
            fossil.replace("\"row\": 31, \"rows\": 12", "\"row\": 31, \"rows\": 13"),
        )
        .expect("writes the moved fixture");
        let s = Settings::load_from(&path).expect("loads");
        assert_eq!(
            s.dashboard.as_ref().map(Vec::len),
            Some(13),
            "a dashboard the reader changed was thrown away"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn missing_file_is_default_not_error() {
        let p = scratch("missing");
        let s = Settings::load_from(&p).expect("a missing file is a first launch");
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn corrupt_file_is_an_error_naming_the_path() {
        let p = scratch("corrupt");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{ not json").unwrap();
        let e = Settings::load_from(&p).unwrap_err();
        assert!(e.contains(&p.display().to_string()), "{e}");
        assert!(e.contains("not valid JSON"), "{e}");
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn save_replaces_atomically_and_leaves_no_temp() {
        let p = scratch("atomic");
        let s = Settings::default();
        s.save_to(&p).unwrap();
        s.save_to(&p).unwrap();
        assert!(p.is_file());
        assert!(!p.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn detected_log_dir_fills_only_when_unset() {
        let mut s = Settings::default();
        s.fill_detected_log_dir(None);
        assert!(s.log_dir.is_none());
        s.fill_detected_log_dir(Some(PathBuf::from("D:/detected")));
        assert_eq!(s.log_dir.as_deref(), Some(Path::new("D:/detected")));
        s.fill_detected_log_dir(Some(PathBuf::from("D:/other")));
        assert_eq!(
            s.log_dir.as_deref(),
            Some(Path::new("D:/detected")),
            "a chosen folder is never overridden"
        );
    }

    #[test]
    fn default_log_dir_is_the_client_install_path() {
        if cfg!(windows) {
            assert_eq!(
                default_log_dir().unwrap(),
                PathBuf::from(
                    r"C:\Users\Public\Daybreak Game Company\Installed Games\EverQuest Legends\Logs"
                )
            );
        } else if cfg!(target_os = "macos") {
            let p = default_log_dir().unwrap();
            assert!(p.ends_with("osxEQL/prefix/drive_c/users/Public/Daybreak Game Company/Installed Games/EverQuest Legends/Logs"));
        } else {
            assert!(default_log_dir().is_none());
        }
    }

    fn binding(
        id: &'static str,
        chord: &'static str,
        tool: Tool,
        alias: bool,
        registered: bool,
        conflict: Option<&str>,
    ) -> Binding {
        Binding {
            id,
            chord: chord.into(),
            default: chord,
            tool,
            alias,
            overridden: false,
            registered,
            conflict: conflict.map(str::to_owned),
        }
    }

    #[test]
    fn bindings_text_is_a_padded_table() {
        let b = vec![
            binding("watch", "Ctrl+Alt+W", Tool::Watch, false, true, None),
            binding(
                "sky",
                "Ctrl+Alt+S then K",
                Tool::Sky,
                false,
                false,
                Some("owned by another app"),
            ),
            binding(
                "lfg-motes",
                "Ctrl+Alt+L then M",
                Tool::Lfg(LfgMode::Motes),
                false,
                false,
                None,
            ),
        ];
        let t = bindings_text(&b);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines[0],
            "Ctrl+Alt+W         Watch: Broken Stoic live  registered"
        );
        assert_eq!(
            lines[1],
            "Ctrl+Alt+S then K  Plane of Sky              not registered: owned by another app"
        );
        assert_eq!(
            lines[2],
            "Ctrl+Alt+L then M  LFG, looking for motes    not registered"
        );
    }

    #[test]
    fn alias_rows_say_so() {
        /* Ctrl+Alt+K is the direct alias for the Sky chord; Ctrl+Alt+S then K is the D4 row. The
         * table has to tell them apart or ten rows contradict a table of seven. */
        let alias = binding("sky-direct", "Ctrl+Alt+K", Tool::Sky, true, true, None);
        let d4 = binding("sky", "Ctrl+Alt+S then K", Tool::Sky, false, true, None);
        assert_eq!(tool_text(&alias), "Plane of Sky, direct alias");
        assert_eq!(tool_text(&d4), "Plane of Sky");
        let t = bindings_text(&[alias, d4]);
        assert!(t.lines().next().unwrap().contains("direct alias"), "{t}");
        assert!(!t.lines().nth(1).unwrap().contains("direct alias"), "{t}");
    }

    /// DEFECT: THE REBINDING LIST WAS THE LAST PLACE STILL CALLING THE LOG PARSER "Parser".
    ///
    /// `Tool::title` is what the window's own strip, its taskbar entry and the rail row print, and
    /// it says "Log Parser"; this table is the "opens" column of the hotkeys list on Settings, and
    /// it said something else about the same window. Nothing breaks, which is exactly why the copy
    /// survived a rename that reached everywhere else.
    ///
    /// ASSERTED AGAINST `Tool::title` AND NOT AGAINST THE STRING, so the next rename cannot leave
    /// this row behind either. The literal is spelled out too, because two functions that agree on
    /// the wrong answer would satisfy the comparison on its own.
    ///
    /// WHAT MUTATION MAKES THIS RED: `Tool::Parser => "Parser"` in `tool_label`.
    #[test]
    fn the_hotkey_rows_call_a_window_what_the_window_calls_itself() {
        assert_eq!(tool_label(&Tool::Parser), Tool::Parser.title());
        assert_eq!(tool_label(&Tool::Parser), "Log Parser");
    }

    /// DEFECT: A SHIPPED DEFAULT FLIPPED BY THE SPELLING OF ONE ATTRIBUTE.
    ///
    /// The kill tracker's counting filters moved onto `Settings` so they survive a restart and so
    /// both windows read one value. `TrackerSettings::default` has `ignore_cities: TRUE`, which is
    /// the behaviour every reader has had since the tracker was ported, and it is the one field of
    /// the five whose default is not the zero value. `#[serde(default)]` on the CONTAINER fills an
    /// absent key from that `Default`; the same attribute written on each FIELD fills it from
    /// `bool::default()`, which is false. The two spellings are a line apart in a diff and the
    /// second one silently starts counting every city kill for everybody who upgrades.
    ///
    /// THE FIXTURE IS A PARTIAL OBJECT ON PURPOSE. A file with no `tracker` key at all takes the
    /// whole struct from `Settings::default()` and passes either way, so it proves nothing about
    /// this; the interesting shape is the one a LATER build produces, where the key exists and one
    /// of its fields does not.
    ///
    /// WHAT MUTATION MAKES THIS RED: moving `#[serde(default)]` off the `TrackerSettings`
    /// container and onto its fields, or removing it (the partial object then fails to parse and
    /// takes the whole settings file down with it).
    #[test]
    fn an_absent_tracker_key_keeps_the_shipped_counting_rule() {
        /* No `tracker` key at all: the file every reader has on disk today. */
        let old: Settings = serde_json::from_str(
            r#"{"data_root":null,"log_dir":null,"always_on_top":false,"watch_on":"twitch"}"#,
        )
        .expect("a file written before the tracker moved here still loads");
        assert!(
            old.tracker.ignore_cities,
            "an upgrade started counting city kills for a reader who never asked for it"
        );
        assert!(!old.tracker.witnessed);
        assert!(!old.tracker.generic_everywhere);

        /* The key is there and one field of it is not: what a file written by a build with fewer
         * fields looks like to a build with more. */
        let partial: Settings = serde_json::from_str(r#"{"tracker":{"witnessed":true}}"#)
            .expect("a partial tracker object must not fail the whole file");
        assert!(partial.tracker.witnessed, "the one stated field was lost");
        assert!(
            partial.tracker.ignore_cities,
            "an absent field took bool::default() instead of TrackerSettings::default()"
        );
        assert!(partial.tracker.ignored_zones.is_empty());

        /* And it round trips, so what this build writes is what it reads back. */
        let mut s = Settings::default();
        s.tracker.witnessed = true;
        s.tracker.ignore_cities = false;
        s.tracker.ignored_zones.push("oggok".to_owned());
        let text = serde_json::to_string(&s).expect("serialises");
        let back: Settings = serde_json::from_str(&text).expect("round trips");
        assert_eq!(back.tracker, s.tracker, "{text}");
    }

    /// The guard behind the module doc: under test, `save()` never reaches the operator's file.
    /// This test is what makes the valet incident impossible to repeat by accident.
    #[test]
    fn save_refuses_under_test_and_save_to_still_works() {
        let s = Settings::default();
        let e = s.save().unwrap_err();
        assert!(e.contains("refused"), "{e}");
        let p = scratch("guard");
        s.save_to(&p)
            .expect("a scratch path is the honest way to test a write");
        assert!(p.is_file());
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn hotkey_overrides_persist_and_an_empty_map_is_not_written() {
        let mut s = Settings::default();
        let out = serde_json::to_string(&s).unwrap();
        assert!(
            !out.contains("hotkeys"),
            "an empty override map stays out of the file: {out}"
        );
        s.hotkeys.insert("watch".into(), "Ctrl+Shift+W".into());
        let p = scratch("hotkeys");
        s.save_to(&p).unwrap();
        let back = Settings::load_from(&p).unwrap();
        assert_eq!(
            back.hotkeys.get("watch").map(String::as_str),
            Some("Ctrl+Shift+W")
        );
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn age_text_picks_the_coarsest_unit() {
        assert_eq!(age_text(0), "0s");
        assert_eq!(age_text(59), "59s");
        assert_eq!(age_text(60), "1m");
        assert_eq!(age_text(3599), "59m");
        assert_eq!(age_text(3600), "1h");
        assert_eq!(age_text(86_400 * 3 + 5), "3d");
        assert_eq!(
            age_text(-5),
            "0s",
            "a clock that went backwards is not an age in the future"
        );
    }

    #[test]
    fn path_text_round_trips_and_strips_quotes() {
        assert_eq!(text_path(""), None);
        assert_eq!(text_path("   "), None);
        assert_eq!(
            text_path("\"C:/eq/Logs\""),
            Some(PathBuf::from("C:/eq/Logs"))
        );
        assert_eq!(path_text(&Some(PathBuf::from("C:/eq/Logs"))), "C:/eq/Logs");
        assert_eq!(path_text(&None), "");
    }

    /// THE NUMBER NOBODY COMPUTED, PINNED. This line printed `ingest.sources().len()`, the count
    /// of ROWS the ingest listed, and the ingest lists a placeholder row for every folder it
    /// looked in and found nothing in. A fresh install with a valid Logs folder and no game files
    /// listed three placeholders and this line read "3 sources found" in the SETTLED green while
    /// the rail's badge, which counts through `nav::source_files`, correctly said nothing.
    ///
    /// The fixture is exactly that machine: the three rows `Ingest::sources` pushes when nothing
    /// has been read, whose paths are FOLDERS. It fails on the old code with "3 sources found" and
    /// on any future rewrite that counts rows instead of files.
    #[test]
    fn the_log_folder_line_counts_files_found_not_folders_looked_in() {
        let logs = std::env::temp_dir();
        assert!(
            logs.is_dir(),
            "the temp dir is the stand in for a real Logs folder"
        );
        let game = logs.parent().unwrap_or(&logs).to_path_buf();
        let placeholders = vec![
            Source {
                kind: SourceKind::Log,
                path: logs.clone(),
                last_read: None,
                records: 0,
                problem: Some("No eqlog_*.txt files".to_owned()),
            },
            Source {
                kind: SourceKind::Inventory,
                path: game.clone(),
                last_read: None,
                records: 0,
                problem: None,
            },
            Source {
                kind: SourceKind::Achievements,
                path: game,
                last_read: None,
                records: 0,
                problem: None,
            },
        ];
        assert_eq!(
            placeholders.len(),
            3,
            "the fixture is the three row listing"
        );

        let (state, words) = log_folder_mark(Some(&logs), &placeholders);
        assert_eq!(
            state,
            State::Idle,
            "three folders looked in and nothing found is not a settled folder: {words}"
        );
        assert!(
            !words.contains('3'),
            "the line claims a count of rows: {words}"
        );
        assert!(words.contains("nothing found in it yet"), "{words}");
    }

    /// The other three branches, and the SETTLED one with a count that was actually computed.
    #[test]
    fn the_log_folder_line_says_each_of_its_four_states() {
        assert_eq!(log_folder_mark(None, &[]).0, State::You);

        let missing = std::env::temp_dir().join("grimoire-no-such-folder-ever");
        assert!(
            !missing.is_dir(),
            "the fixture must name a folder that is not there"
        );
        assert_eq!(log_folder_mark(Some(&missing), &[]).0, State::Wrong);

        let logs = std::env::temp_dir();
        let one_file = vec![Source {
            kind: SourceKind::Log,
            path: logs.join("eqlog_Grimtooth_server.txt"),
            last_read: None,
            records: 0,
            problem: None,
        }];
        let (state, words) = log_folder_mark(Some(&logs), &one_file);
        assert_eq!(state, State::Settled);
        assert_eq!(
            words, "1 source found; the SOURCES section below lists them",
            "singular, not `1 sources`"
        );

        let two_files = vec![
            one_file[0].clone(),
            Source {
                kind: SourceKind::Inventory,
                path: logs.join("Grimtooth_server-Inventory.txt"),
                last_read: None,
                records: 0,
                problem: None,
            },
        ];
        assert_eq!(
            log_folder_mark(Some(&logs), &two_files).1,
            "2 sources found; the SOURCES section below lists them"
        );
    }

    /* --------------------------------------------------------- the unreadable file -- */

    /// The hand edit that cost the file. `"true"` is quoted, which is what a person types when
    /// they open settings.json in Notepad, and it is enough to make `load_from` fail. Everything
    /// around it is what a real file carries and what a save of defaults would have destroyed: a
    /// log folder, a hotkey table, and another lane's `lfg_board`.
    const HAND_EDITED: &str = concat!(
        "{\n",
        "  \"log_dir\": \"C:/eq/Logs\",\n",
        "  \"always_on_top\": \"true\",\n",
        "  \"hotkeys\": {\"companion\": \"Ctrl+Alt+G\", \"sky\": \"Ctrl+Alt+S then K\"},\n",
        "  \"lfg_board\": [{\"who\": \"Stoic\", \"want\": \"raid\"}]\n",
        "}\n"
    );

    /// AN ORDINARY SAVE MUST NOT REPLACE A FILE NOBODY COULD READ, AND THE PROOF IS THE BYTES.
    ///
    /// This is the sequence the lens ran, in order: the file above is on disk, `load_from`
    /// refuses it, `load` answers that refusal with `Settings::default()`, and any of the five
    /// writers then saves those defaults through the ordinary path. Before the guard the file
    /// became `{"data_root":null,"log_dir":null,"always_on_top":false}` and the rename made that
    /// final. The log folder, the two chords and another lane's board were gone.
    ///
    /// IT ASSERTS THE FILE, NOT THE RETURN VALUE. An `Err` proves the call reported something; it
    /// says nothing about whether the bytes on disk survived, and the bytes are the whole subject.
    /// The temp file is checked too, since a half written `settings.json.tmp` left beside a file
    /// this call refused to touch is litter that the next reader has to explain.
    ///
    /// `save_to` and not `save` for the reason the module doc gives: `save` refuses under
    /// `cfg(test)` so no test can reach the operator's file. They are one function either way,
    /// `write_file`, which is the point of putting the guard there.
    /// REPAIRING THE FILE BY HAND, WITHOUT RESTARTING, MUST NOT LET THE STALE DEFAULTS EAT IT.
    ///
    /// This is the sequence the old error message actively recommended, and it destroyed the file
    /// at the end of it:
    ///   1. the file is malformed, so this session is holding `..default()` with `load_problem` set
    ///   2. a save refuses, correctly, and the file is untouched
    ///   3. the operator repairs the JSON by hand, as instructed, WITHOUT restarting
    ///   4. anything saves; the file parses now, so the file guard stands down, and the defaults
    ///      from step 1 are written over a whole file, atomically, returning Ok
    ///
    /// The assertion is on the BYTES on disk, not on the return value, because a guard that reports
    /// an error after the file is already gone is not a guard.
    /// STARTING FRESH KEEPS WHAT IT REPLACES, EVEN WHEN WHAT IT REPLACES IS PERFECTLY GOOD.
    ///
    /// The deliberate path was exempted from the session guard on the stated grounds that
    /// `keep_a_copy` "preserves the bytes first regardless". It did not. It ran off
    /// `unreadable_bytes`, which yields bytes ONLY for a file that fails to parse, so the copy was
    /// kept in exactly the case where the file was already worthless and skipped in the case where
    /// it mattered.
    ///
    /// The sequence that exposes it is the one the screen tells you to follow: the file is broken,
    /// a save refuses, you repair the JSON by hand without restarting, the file is now whole, and
    /// the screen is STILL showing the refusal and its button. You press it, and a session holding
    /// defaults replaces a good file with no backup anywhere.
    #[test]
    fn starting_fresh_keeps_a_copy_even_when_the_file_on_disk_parses() {
        let dir = std::env::temp_dir().join(format!(
            "grimoire-settings-{}-freshcopy",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("settings.json");

        /* A file that PARSES and holds real data. This is the repaired-by-hand state. */
        let good = r#"{"log_dir":"C:/eq/Logs","always_on_top":true,"hotkeys":{"t":"Ctrl+Alt+S"}}"#;
        std::fs::write(&path, good).expect("seed");
        assert!(
            Settings::load_from(&path).is_ok(),
            "the fixture must parse, or this tests the wrong branch entirely"
        );

        /* The deliberate replacement, which is what the screen's button performs. */
        let kept = Settings::default()
            .write_file(&path, OnUnreadable::KeepACopy)
            .expect("a deliberate replacement is allowed");

        let kept = kept
            .expect("no copy was kept: a good file was replaced and the bytes are gone for good");
        let saved = std::fs::read_to_string(&kept).expect("the kept copy exists on disk");
        assert_eq!(
            saved,
            good,
            "the kept copy does not hold what was replaced; it is at {}",
            kept.display()
        );
        assert!(
            saved.contains("C:/eq/Logs") && saved.contains("Ctrl+Alt+S"),
            "the kept copy is missing the data it exists to preserve: {saved}"
        );

        /* And the replacement itself still happened, or this passes by refusing to do the job. */
        let now = std::fs::read_to_string(&path).expect("the settings file still exists");
        assert!(
            !now.contains("C:/eq/Logs"),
            "start fresh did not actually replace anything: {now}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn a_file_repaired_by_hand_is_not_eaten_by_a_session_that_never_read_it() {
        let dir =
            std::env::temp_dir().join(format!("grimoire-settings-{}-repaired", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("settings.json");

        /* 1. malformed: a quoted boolean, the classic hand edit, alongside real data. */
        std::fs::write(
            &path,
            r#"{"log_dir":"C:/eq/Logs","always_on_top":"true","hotkeys":{"toggle":"Ctrl+Alt+S"}}"#,
        )
        .expect("seed");
        let problem = Settings::load_from(&path).expect_err("a quoted boolean is not valid");

        /* what `load` hands the app on that branch, and what it keeps holding all session */
        let stale = Settings {
            load_problem: Some(problem),
            ..Settings::default()
        };

        /* 2. a save refuses while the file is still broken */
        stale
            .write_file(&path, OnUnreadable::Refuse)
            .expect_err("a broken file must not be replaced by an ordinary save");

        /* 3. the operator repairs it by hand, WITHOUT restarting. The file is now valid and whole. */
        let repaired =
            r#"{"log_dir":"C:/eq/Logs","always_on_top":true,"hotkeys":{"toggle":"Ctrl+Alt+S"}}"#;
        std::fs::write(&path, repaired).expect("repair");
        assert!(
            Settings::load_from(&path).is_ok(),
            "the repaired file must be valid, or this test proves nothing about step 4"
        );

        /* 4. anything saves, using the SAME stale Settings this session has held since step 1 */
        let out = stale.write_file(&path, OnUnreadable::Refuse);

        let after = std::fs::read_to_string(&path).expect("the file still exists");
        assert!(
            after.contains("C:/eq/Logs") && after.contains("Ctrl+Alt+S"),
            "the repaired file was eaten by a session running on defaults. On disk now: {after}"
        );
        assert_eq!(
            after, repaired,
            "the repaired file was rewritten at all; a session that never read it has nothing to say"
        );
        out.expect_err("the save must report that it refused, not silently succeed");

        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn an_ordinary_save_refuses_to_replace_a_file_it_could_not_read() {
        let p = scratch("unreadable");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, HAND_EDITED).unwrap();

        let why = Settings::load_from(&p).unwrap_err();
        assert!(why.contains("not valid JSON"), "{why}");
        /* What `load` hands the app after that refusal, which is what a save would write. */
        let running_on = Settings::default();

        let refused = running_on
            .save_to(&p)
            .expect_err("defaults must not silently replace a file nobody could read");
        assert!(
            refused.contains(&p.display().to_string()),
            "the refusal must name the file: {refused}"
        );

        let after = std::fs::read_to_string(&p).expect("the file is still there");
        assert_eq!(after, HAND_EDITED, "the user's bytes were modified");
        for want in [
            "C:/eq/Logs",
            "Ctrl+Alt+G",
            "Ctrl+Alt+S then K",
            "lfg_board",
            "Stoic",
        ] {
            assert!(
                after.contains(want),
                "{want:?} was destroyed by the save; the file now reads {after}"
            );
        }
        assert!(
            !p.with_extension("json.tmp").exists(),
            "a refused save left its temp file behind"
        );

        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    /// THE DELIBERATE REPLACEMENT STILL WORKS, AND THE OLD BYTES ARE SOMEWHERE TO GO BACK TO.
    ///
    /// The Settings screen is allowed to write a fresh file, because it is the one surface that
    /// says so before it happens. The cost of that permission is the copy, so this asserts the
    /// copy holds the original file byte for byte and that the new file is the settings that were
    /// written, not a mixture.
    ///
    /// THEN IT BREAKS THE FILE AGAIN, which is the half a single pass would miss. Two failures in
    /// one second name the same timestamp, and a plain write would have put the second copy on
    /// top of the first: the same total loss, one file over. `create_new` is what stops that, so
    /// the first copy is read back after the second is made.
    #[test]
    fn the_deliberate_replacement_keeps_the_old_bytes_and_never_clobbers_a_kept_copy() {
        let p = scratch("deliberate");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, HAND_EDITED).unwrap();

        let fresh = Settings {
            log_dir: Some(PathBuf::from("D:/chosen/Logs")),
            ..Settings::default()
        };
        let first = fresh
            .save_to_replacing_unreadable(&p)
            .expect("the deliberate path writes")
            .expect("an unreadable file must be kept, not dropped");
        assert_eq!(
            std::fs::read_to_string(&first).unwrap(),
            HAND_EDITED,
            "the kept copy is not the file that was there"
        );
        let now = Settings::load_from(&p).expect("the replacement file parses");
        assert_eq!(now.log_dir, Some(PathBuf::from("D:/chosen/Logs")));

        const BROKEN_AGAIN: &str = "{ still not json, and different";
        std::fs::write(&p, BROKEN_AGAIN).unwrap();
        let second = fresh
            .save_to_replacing_unreadable(&p)
            .expect("the second deliberate save writes")
            .expect("the second unreadable file is kept too");
        assert_ne!(first, second, "the second copy took the first copy's name");
        assert_eq!(
            std::fs::read_to_string(&first).unwrap(),
            HAND_EDITED,
            "the first kept copy was written over by the second"
        );
        assert_eq!(std::fs::read_to_string(&second).unwrap(), BROKEN_AGAIN);

        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    /// THE GUARD MUST NOT WEDGE THE ORDINARY CASE, and there are two ordinary cases.
    ///
    /// A file that parses is replaced exactly as before, with nothing kept beside it, because
    /// everything it held is in the `Settings` being written: `extra` carries every key this
    /// struct does not name. An EMPTY file is replaced too. Refusing on one would be the guard
    /// eating itself: nothing is lost by overwriting no bytes, and a truncated file left by some
    /// other program's crash would otherwise lock the app out of saving for ever.
    #[test]
    fn a_readable_or_empty_file_is_still_replaced_and_nothing_is_kept_beside_it() {
        let p = scratch("ordinary");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();

        let mut before = Settings {
            always_on_top: true,
            ..Settings::default()
        };
        before
            .extra
            .insert("lfg_board".into(), json!([{"who": "Stoic"}]));
        before
            .save_to(&p)
            .expect("a first write has no file to lose");

        let mut after = Settings::load_from(&p).expect("it parses");
        after.log_dir = Some(PathBuf::from("D:/chosen/Logs"));
        after
            .save_to(&p)
            .expect("a readable file is replaced as before");
        let back = Settings::load_from(&p).unwrap();
        assert_eq!(back.log_dir, Some(PathBuf::from("D:/chosen/Logs")));
        assert_eq!(back.extra["lfg_board"][0]["who"], "Stoic");

        std::fs::write(&p, "").unwrap();
        Settings::default()
            .save_to(&p)
            .expect("an empty file holds nothing to lose");
        std::fs::write(&p, "   \n\t\n").unwrap();
        Settings::default()
            .save_to(&p)
            .expect("whitespace holds nothing to lose either");

        let kept: Vec<PathBuf> = std::fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|q| q.to_string_lossy().contains(".unreadable-"))
            .collect();
        assert!(
            kept.is_empty(),
            "a copy was kept of a file worth nothing: {kept:?}"
        );

        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    /// WHAT THE SETTINGS SCREEN SAYS ABOUT ALL THIS, PROVED BY PAINTING IT.
    ///
    /// This is the reachability check, and it is here because this codebase keeps shipping wiring
    /// that reaches no pixel. The two sentences below are the only place a person is told that
    /// their file is being protected and where the copy of it went, so a test that asserted the
    /// field rather than the paint would prove nothing at all. It runs a real frame through
    /// `SettingsScreen::ui` and reads the strings out of the shapes egui produced.
    ///
    /// The screen is NOT dirty, so no write is attempted: `ui` writes only when the debounce is
    /// pending, and the debounce is set only by `mark_dirty`, which only an edit on this screen
    /// calls.
    #[test]
    fn the_settings_screen_paints_the_warning_and_where_the_kept_copy_went() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);

        let mut settings = Settings {
            load_problem: Some("C:/x/settings.json: settings are not valid JSON".to_owned()),
            ..Settings::default()
        };
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(YOUTUBE_HANDLE),
        };
        let mut screen = SettingsScreen {
            kept: Some(PathBuf::from(
                "C:/x/settings.json.unreadable-20260902-101500",
            )),
            ..Default::default()
        };
        let mut cx = crate::screens::Cx {
            data: None,
            railed: false,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: &mut ingest,
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
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 1400.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();

        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.shape, &mut flat);
        }
        let words: Vec<String> = flat
            .iter()
            .filter_map(|sh| match sh {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        let joined = words.join("\u{1F}");
        for want in [
            "settings are not valid JSON",
            "Every other screen now refuses to save",
            "keeps the unreadable file beside it first",
            "Kept as",
            "settings.json.unreadable-20260902-101500",
        ] {
            assert!(
                joined.contains(want),
                "the screen never painted {want:?}; it painted {words:?}"
            );
        }
    }

    /* ------------------------------------------------- the preferred platform -- */

    /// THE STORED KEY IS WHAT SERDE ACTUALLY WRITES, held rather than assumed.
    ///
    /// `Platform::key` is hand written and `Serialize` is derived with `rename_all = "lowercase"`,
    /// so there are two opinions about what goes in the file. That is deliberate: the derive is the
    /// authority and `key` is what `from_key` reads back with. If they ever disagreed, the file
    /// would be written under one spelling and read under another, every load would silently fall
    /// back to the default, and nothing would look broken. This runs both through serde_json.
    #[test]
    fn the_platform_key_is_what_serde_writes_and_what_from_key_reads() {
        for p in Platform::ALL {
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(json, format!("\"{}\"", p.key()), "{p:?}");
            assert_eq!(Platform::from_key(p.key()), Some(p));
        }
        assert_eq!(Platform::from_key("kick"), None);
        assert_eq!(Platform::default(), Platform::Twitch);
        /* the labels are the platforms' own spellings, and they are not the keys */
        assert_eq!(Platform::Twitch.label(), "Twitch");
        assert_eq!(Platform::YouTube.label(), "YouTube");
    }

    /// A SETTINGS FILE FROM BEFORE THIS BUILD LOADS, KEEPS EVERYTHING, AND TAKES THE DEFAULT.
    ///
    /// Every settings.json on disk today has no `watch_on` key, the operator's included. A new
    /// typed field is only safe if a file missing it still loads with every other setting intact,
    /// and "only safe if" is a claim, so this is a real file on disk through `load_from`: the two
    /// paths, the pin, the hotkey table and another lane's `lfg_board` all come back, and the
    /// preference is Twitch, which is what this app has always shown.
    ///
    /// AND THE ROUND TRIP DOES NOT LOSE THE OTHER LANE. The file is written back and reloaded,
    /// because a field that loads cleanly and then drops `extra` on the next save is the same data
    /// loss arriving one save later.
    #[test]
    fn a_settings_file_without_the_preference_loads_and_keeps_every_other_field() {
        let p = scratch("no-watch-on");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            r#"{
              "data_root": "C:/eq/data",
              "log_dir": "C:/eq/Logs",
              "always_on_top": true,
              "hotkeys": {"companion": "Ctrl+Alt+G"},
              "lfg_board": [{"who": "Stoic"}]
            }"#,
        )
        .unwrap();

        let s = Settings::load_from(&p).expect("a file without the new key still loads");
        assert_eq!(s.watch_on, Platform::Twitch, "a missing key is the default");
        assert_eq!(s.data_root, Some(PathBuf::from("C:/eq/data")));
        assert_eq!(s.log_dir, Some(PathBuf::from("C:/eq/Logs")));
        assert!(s.always_on_top);
        assert_eq!(
            s.hotkeys.get("companion").map(String::as_str),
            Some("Ctrl+Alt+G")
        );
        assert!(
            s.extra.contains_key("lfg_board"),
            "another lane's state: {:?}",
            s.extra
        );

        let mut round = s.clone();
        round.watch_on = Platform::YouTube;
        round.save_to(&p).expect("a readable file is replaced");
        let back = Settings::load_from(&p).expect("what we just wrote must load");
        assert_eq!(back.watch_on, Platform::YouTube, "the preference persists");
        assert_eq!(back.log_dir, s.log_dir);
        assert!(back.extra.contains_key("lfg_board"));
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .contains("\"watch_on\": \"youtube\""),
            "the file has to carry the key from_key reads back"
        );

        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    /// A VALUE THIS BUILD DOES NOT KNOW COSTS THE PREFERENCE AND NOTHING ELSE.
    ///
    /// A derived `Deserialize` refuses an unknown string, and refusing here fails the WHOLE file:
    /// `load` hands back defaults with a `load_problem`, and every writer in the app is then
    /// refused by the guard `save_to` carries, over a preference. That is exactly what a later
    /// build writing a third platform, then rolled back, would do to this one. Four shapes are
    /// tried, because `serde_json::Value` is what absorbs them and a string-only tolerance would
    /// still wedge on a number.
    #[test]
    fn an_unknown_preference_does_not_cost_the_rest_of_the_file() {
        for (name, raw) in [
            ("a later platform", r#""kick""#),
            ("a number", "7"),
            ("null", "null"),
            ("an object", r#"{"platform":"twitch"}"#),
        ] {
            let p = scratch(&format!("odd-watch-on-{}", raw.len()));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(
                &p,
                format!(r#"{{"log_dir": "C:/eq/Logs", "watch_on": {raw}}}"#),
            )
            .unwrap();
            let s = Settings::load_from(&p)
                .unwrap_or_else(|e| panic!("{name} wedged the whole file: {e}"));
            assert_eq!(s.watch_on, Platform::Twitch, "{name}");
            assert_eq!(
                s.log_dir,
                Some(PathBuf::from("C:/eq/Logs")),
                "{name} cost a setting that had nothing to do with it"
            );
            let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
        }
    }

    /// THE PREFERENCE DOES NOT REOPEN THE DATA LOSS THE PREVIOUS UNIT CLOSED.
    ///
    /// The closed defect was: an unparsable file loads as DEFAULTS, and a writer that never looks
    /// at `load_problem` writes those defaults over it, atomically. The Settings screen writes on a
    /// debounce whenever `mark_dirty` fires, and choosing a platform is now one of the things that
    /// fires it, so this is a NEW FINGER ON THAT TRIGGER and has to meet the same rule as the
    /// others: an ordinary save carrying a changed preference over an unreadable file is refused
    /// and the bytes survive, and only the deliberate path replaces it, keeping a copy first.
    #[test]
    fn changing_the_preference_cannot_overwrite_an_unreadable_file() {
        let p = scratch("watch-on-guard");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, HAND_EDITED).unwrap();

        let running_on = Settings {
            watch_on: Platform::YouTube,
            ..Settings::default()
        };

        let refused = running_on
            .save_to(&p)
            .expect_err("a preference is not a licence to replace a file nobody could read");
        assert!(refused.contains(&p.display().to_string()), "{refused}");
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            HAND_EDITED,
            "a preference change destroyed the user's bytes"
        );
        assert!(
            !p.with_extension("json.tmp").exists(),
            "a refused save left its temp file behind"
        );

        /* the deliberate path still works, and still keeps the old bytes beside the file */
        let kept = running_on
            .save_to_replacing_unreadable(&p)
            .expect("the Settings screen may replace it")
            .expect("and must say where the old bytes went");
        assert_eq!(std::fs::read(&kept).unwrap(), HAND_EDITED.as_bytes());
        assert_eq!(Settings::load_from(&p).unwrap().watch_on, Platform::YouTube);

        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    /// DEFECT: a per-window preference that agrees with the default and is written down anyway.
    ///
    /// A key recording agreement is indistinguishable from a key recording a CHOICE, so it freezes
    /// the old default in place for ever: change `Slot::pin_default` in a later build and everybody
    /// who ever toggled the checkbox twice keeps the old behaviour, with nothing on screen to
    /// explain why. `hotkeys` has stored only rows that differ since it was written, for exactly
    /// this reason, and this is the same rule on the same file.
    ///
    /// WHAT MUTATION MAKES THIS RED: `*field(e) = Some(on)` in `win_set`.
    #[test]
    fn a_window_preference_that_matches_the_default_is_not_written_down() {
        let mut s = Settings::default();
        assert!(s.windows.is_empty());

        /* Agreeing with a default writes nothing at all. */
        s.set_win_pinned("dps", true, true);
        assert!(
            s.windows.is_empty(),
            "agreement was recorded: {:?}",
            s.windows
        );

        /* Disagreeing is recorded, and only the field that disagrees. */
        s.set_win_pinned("dps", true, false);
        assert_eq!(s.windows["dps"].pinned, Some(false));
        assert_eq!(
            s.windows["dps"].open, None,
            "the open state was never mentioned"
        );
        assert_eq!(s.windows["dps"].rect, None, "and neither was the rectangle");
        assert!(!s.win_pinned("dps", true));

        /* And going back to the default REMOVES the whole entry, so a file toggled twice is byte
         * for byte the file that was never touched. */
        s.set_win_pinned("dps", true, true);
        assert!(s.windows.is_empty(), "the entry survived: {:?}", s.windows);
        assert!(s.win_pinned("dps", true), "and it follows the code again");
    }

    /// DEFECT: the preferences sharing one entry and erasing each other.
    ///
    /// They live in one `WindowPrefs`, so a setter that removed the entry whenever ITS OWN field
    /// matched the default would take the other fields down with it.
    ///
    /// `set_win_rect` IS IN HERE BECAUSE IT IS THE ONE SETTER THAT COULD NOT GO THROUGH `win_set`:
    /// a rectangle is not an `Option<bool>` and has no code default to erase against, so it writes
    /// its own field and then calls `win_prune`. That is exactly the shape of thing that ends up
    /// with a second copy of the erasure rule, and a second copy is a copy that drifts. If the
    /// rectangle ever stops sharing the rule, the pin and the open state below go with it.
    ///
    /// WHAT MUTATION MAKES THIS RED: inlining `win_prune` into `win_set` and leaving
    /// `set_win_rect` without it, or the reverse; either way one of the three writers stops
    /// erasing and the entry outlives its contents.
    #[test]
    fn the_window_preferences_do_not_erase_each_other() {
        let mut s = Settings::default();
        s.set_win_pinned("dps", true, false);
        s.set_win_open("dps", true);
        s.set_win_rect("dps", Some([10.0, 20.0, 300.0, 400.0]));
        assert_eq!(s.windows["dps"].pinned, Some(false));
        assert_eq!(s.windows["dps"].open, Some(true));
        assert_eq!(s.win_rect("dps"), Some([10.0, 20.0, 300.0, 400.0]));

        /* The rectangle forgotten leaves the other two alone. */
        s.set_win_rect("dps", None);
        assert_eq!(s.windows["dps"].pinned, Some(false));
        assert_eq!(s.windows["dps"].open, Some(true));
        assert_eq!(s.win_rect("dps"), None);

        /* The window closed leaves the pin alone. */
        s.set_win_open("dps", false);
        assert_eq!(s.windows["dps"].pinned, Some(false));
        assert!(!s.win_open("dps"));

        /* And the pin back to the default now clears the entry outright. */
        s.set_win_pinned("dps", true, true);
        assert!(s.windows.is_empty(), "the entry survived: {:?}", s.windows);

        /* THE RECTANGLE ON ITS OWN, WHICH IS THE ONLY WAY TO SEE ITS ERASURE. Above it was never
         * the last writer, so a `set_win_rect` that skipped the prune passed every line of it: the
         * pin or the open state was always still there to keep the entry alive. A window that was
         * dragged once and then forgotten is the real case, and it must leave the file exactly as
         * it found it. */
        s.set_win_rect("chat", Some([0.0, 0.0, 100.0, 100.0]));
        assert_eq!(s.windows["chat"].rect, Some([0.0, 0.0, 100.0, 100.0]));
        s.set_win_rect("chat", None);
        assert!(
            s.windows.is_empty(),
            "an entry holding nothing but a forgotten rectangle survived: {:?}",
            s.windows
        );
    }

    /// DEFECT: A POP-OUT'S SIZE AND POSITION DID NOT SURVIVE A RELAUNCH, AND NEITHER DID THE FACT
    /// THAT IT WAS OPEN.
    ///
    /// `WindowPrefs` carried the pin and nothing else, so the owner arranged his overlays over the
    /// game once per session, every session. This is the storage half: the round trip through the
    /// file, which is where a `[f32; 4]` is most likely to go wrong, since JSON has one number type
    /// and serde is free to hand a whole-numbered float back as an integer.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping `rect` or `open` from `WindowPrefs`, or giving
    /// either a `#[serde(skip)]`.
    #[test]
    fn a_windows_place_and_its_open_state_survive_the_file() {
        let mut s = Settings::default();
        s.set_win_open("dps", true);
        s.set_win_rect("dps", Some([-1920.5, 12.0, 480.0, 260.75]));

        let text = serde_json::to_string(&s).expect("serialises");
        let back: Settings = serde_json::from_str(&text).expect("round trips");
        assert!(back.win_open("dps"), "the window came back closed: {text}");
        assert_eq!(
            back.win_rect("dps"),
            Some([-1920.5, 12.0, 480.0, 260.75]),
            "a negative x is a second monitor to the left and is not a corruption: {text}"
        );

        /* A window nobody has touched has neither, and the map has no entry for it at all. */
        assert!(!back.win_open("chat"));
        assert_eq!(back.win_rect("chat"), None);
        assert_eq!(back.windows.len(), 1, "{:?}", back.windows);
    }

    /// DEFECT: a settings file written by an older build losing its overlay preferences, or one
    /// written by this build being unreadable by a screen that does not know the key.
    ///
    /// The map is `skip_serializing_if` empty, so a person who has changed nothing has no `windows`
    /// key at all, and `#[serde(default)]` is what lets every file written before today load.
    #[test]
    fn the_overlay_preferences_survive_a_round_trip_and_an_older_file_still_loads() {
        let mut s = Settings::default();
        let bare = serde_json::to_string(&s).expect("a default serialises");
        assert!(
            !bare.contains("windows"),
            "an untouched settings file must not grow a key: {bare}"
        );

        s.set_win_pinned("dps", true, false);
        s.set_win_open("dps", true);
        let text = serde_json::to_string(&s).expect("serialises");
        let back: Settings = serde_json::from_str(&text).expect("round trips");
        assert_eq!(back.windows, s.windows);

        /* The shape a file written before today has. */
        let old: Settings = serde_json::from_str(
            r#"{"data_root":null,"log_dir":null,"always_on_top":false,"watch_on":"twitch"}"#,
        )
        .expect("a file with no windows key still loads");
        assert!(old.windows.is_empty());
        assert!(
            old.win_pinned("dps", true),
            "and every window follows the code"
        );
    }

    /// THE WATCH SECTION IS A REAL CONTROL AND IT REACHES THE SETTING, proven on painted frames.
    ///
    /// Wiring that reaches no pixel is this codebase's defining defect and it had just struck
    /// again, so this asserts nothing about a function existing. It runs a frame, reads the strings
    /// out of the shapes the section actually painted, finds where the word `YouTube` was drawn,
    /// CLICKS there on the next frame, and then asserts two things: the settings struct changed,
    /// and the screen went dirty, which is what schedules the write. Without the second, the choice
    /// would be correct on screen and die with the process.
    ///
    /// TWO PASSES, for the reason `settings_screen_frame` gives: egui resolves a click against the
    /// widget list the PREVIOUS pass registered, so a click on the first frame lands on nothing.
    #[test]
    fn the_watch_section_paints_the_choice_and_a_click_reaches_the_setting() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let mut screen = SettingsScreen::default();
        let mut settings = Settings::default();
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(YOUTUBE_HANDLE),
        };

        let input = |events: Vec<egui::Event>| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(720.0, 400.0),
            )),
            events,
            ..Default::default()
        };

        let mut frame = |events: Vec<egui::Event>,
                         screen: &mut SettingsScreen,
                         settings: &mut Settings|
         -> Vec<(String, egui::Pos2)> {
            let mut cx = crate::screens::Cx {
                data: None,
                railed: false,
                data_err: None,
                live: &live,
                settings,
                ingest: &mut ingest,
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
            let mut out = ctx.run_ui(input(events), |ui| screen.watch(ui, &mut cx));
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();
            let mut flat = Vec::new();
            for cs in shapes {
                flatten(cs.shape, &mut flat);
            }
            flat.iter()
                .filter_map(|sh| match sh {
                    egui::Shape::Text(t) => Some((t.galley.text().to_owned(), t.pos)),
                    _ => None,
                })
                .collect()
        };

        let words = frame(Vec::new(), &mut screen, &mut settings);
        let painted: Vec<String> = words.iter().map(|(s, _)| s.clone()).collect();
        for want in [
            "WATCH",
            "Preferred",
            "Twitch",
            "YouTube",
            "The live pill reports Twitch",
        ] {
            assert!(
                painted.iter().any(|s| s.contains(want)),
                "the WATCH section never painted {want:?}; it painted {painted:?}"
            );
        }

        let at = words
            .iter()
            .find(|(s, _)| s == "YouTube")
            .map(|(_, p)| *p + Vec2::new(4.0, 6.0))
            .expect("the YouTube option has to be on screen to be clickable");

        assert_eq!(settings.watch_on, Platform::Twitch, "the starting value");
        assert!(screen.dirty_since.is_none(), "nothing has been chosen yet");

        let click = vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        let after = frame(click, &mut screen, &mut settings);

        assert_eq!(
            settings.watch_on,
            Platform::YouTube,
            "clicking the YouTube option reached no setting; the control paints and does nothing"
        );
        assert!(
            screen.dirty_since.is_some(),
            "the choice changed and nothing scheduled a save, so it dies with the process"
        );
        /* and the sentence under the control follows the choice rather than being a fixed caption */
        let painted: Vec<String> = after.iter().map(|(s, _)| s.clone()).collect();
        assert!(
            painted
                .iter()
                .any(|s| s.contains("The live pill reports YouTube")),
            "the explanation still names the old platform: {painted:?}"
        );
    }

    /* ------------------------------------------------------------ the SOURCES section -- */

    /// A game folder with an empty `Logs` inside it, laid out the way a real install is, removed
    /// on drop.
    ///
    /// IT CAME FROM `nav.rs` WITH THE TEST UNDER IT. The rail's Sources badge was the surface that
    /// counted source files; that row is gone and this line is, so the fixture followed the
    /// question. Hand rolled because this crate has no `tempfile` dev dependency and Cargo.toml is
    /// deliberately not edited from a lane.
    ///
    /// NESTED ON PURPOSE. The ingest looks for the dumps in the log folder's PARENT, so a `Logs`
    /// placed straight in the system temp folder would make the whole temp folder the game folder
    /// and let any stray `*-Inventory.txt` another program left there change what this counts.
    struct TempTree {
        root: PathBuf,
    }

    impl TempTree {
        fn new(tag: &str) -> TempTree {
            let nanos = std::time::SystemTime::now()
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

    /// Pump the ingest until it has adopted its scan, or give up. The scan runs on a worker and
    /// lands on a `tail()`; `drain_scan` is the first thing `tail` does, ahead of its own poll
    /// interval, so pumping faster than `TAIL_POLL` still adopts.
    fn pump_until(
        ingest: &mut crate::ingest::Ingest,
        done: fn(&crate::ingest::Ingest) -> bool,
    ) -> bool {
        for _ in 0..600 {
            ingest.tail();
            if done(ingest) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// THE LEDGER'S TABLE, DRIVEN OVER A REAL SCAN, WHICH IS THE BRANCH NOTHING ELSE ENTERS.
    ///
    /// `settings_screen_frame` runs this screen with a default `Settings`, so the ingest resolves
    /// no folder, `sources()` comes back empty and only the EMPTY state is ever painted. The grid
    /// under the other arm is the half a person with a Logs folder actually looks at, which is
    /// nearly everybody, and it had no coverage at all: it was drawn by a screen no test drove
    /// with a scanned folder.
    ///
    /// AND IT CAUGHT A DEFECT THIS FOLD PUT THERE, WHICH IS WHY THE WIDTH IS A REAL ONE. The first
    /// cut wrapped the grid in `ui.horizontal(|ui| { ui.add_space(FIELD_W); .. })` so the table
    /// would line up with every other section's label column. That pushed five columns 92px right
    /// in a screen whose own `ScrollArea` is VERTICAL, and a cell laid out past the right edge is
    /// not clipped, it is NOT DRAWN: `Label` asks `is_rect_visible` first. The PROBLEM column,
    /// which is the reason a person opens this ledger, silently stopped existing. Measured, not
    /// guessed: this test failed on exactly that at 1100px and passed at 3000px.
    ///
    /// 1100 IS THEREFORE THE ASSERTION AND NOT AN ARBITRARY NUMBER. It is a real window's body
    /// width, and it sits above the threshold at the margin (measured over the planted path, a 110
    /// character one: readable at 1050, the last columns gone by 1000) and below it with the
    /// indent. Put the horizontal wrapper back and this goes red.
    ///
    /// WHAT IT DOES NOT CLAIM, AND WHAT THE APP DOES ABOUT IT. That the table is readable at ANY
    /// width. It is not: at 1000px the last columns are laid out past the edge whatever the indent
    /// does, and that was not a hypothesis, it was the owner's own window (2240px at 175%, about
    /// 1055 of body) with RECORDS and PROBLEM missing from a screenshot of this very fold. So the
    /// grid sits in a `ScrollArea::horizontal` and the columns are REACHABLE at any width even
    /// when they are not all visible at once. This test cannot assert that half: a cell scrolled
    /// out of view is not painted, so there is nothing in the shape list to read. What holds it is
    /// the screenshot in the report and `screens::inventory`, which answers the same question the
    /// same way for the same reason.
    ///
    /// THE COLUMN HEADS ARE THE ASSERTION because they only exist on that arm: the empty state
    /// paints none of them. `records` is padded to the width of the widest number, so it is
    /// matched by trimming rather than by equality.
    #[test]
    fn the_ledger_paints_its_table_once_the_ingest_has_scanned() {
        let tree = TempTree::new("grimoire-settings-table");
        std::fs::write(
            tree.logs().join("eqlog_Stoic_legends.txt"),
            "[Sun Sep 01 23:58:00 2026] Welcome to EverQuest!\n",
        )
        .expect("plant a log");
        let mut settings = Settings {
            log_dir: Some(tree.logs()),
            data_root: Some(tree.root.clone()),
            ..Default::default()
        };
        let mut ingest = crate::ingest::Ingest::new(&settings);
        assert!(
            pump_until(&mut ingest, |ig| {
                ig.sources()
                    .iter()
                    .any(|s| crate::nav::looks_like_eq_log(&s.path))
            }),
            "the ingest never listed the log planted in {}",
            tree.logs().display()
        );
        let planted = ingest.sources().len();
        assert!(planted > 0, "the ledger's table arm needs rows to draw");

        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(YOUTUBE_HANDLE),
        };
        let mut screen = SettingsScreen::default();
        let planted_path = tree.logs().join("eqlog_Stoic_legends.txt");
        /* One real window width. See the note above for why it is this one and what it holds. */
        for width in [1100.0_f32] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, 3000.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let mut cx = crate::screens::Cx {
                        data: None,
                        railed: false,
                        data_err: None,
                        live: &live,
                        settings: &mut settings,
                        ingest: &mut ingest,
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
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();
            let mut flat = Vec::new();
            for cs in shapes {
                flatten(cs.shape, &mut flat);
            }
            let words: Vec<String> = flat
                .iter()
                .filter_map(|sh| match sh {
                    egui::Shape::Text(t) => Some(t.galley.text().trim().to_owned()),
                    _ => None,
                })
                .collect();

            for head in ["kind", "path", "last read", "records", "problem"] {
                assert!(
                    words.iter().any(|w| w == head),
                    "at {width}px the ledger's table never painted its {head:?} column: {words:?}"
                );
            }
            /* And the row itself, so this is the table and not five loose words: the planted log's
             * own path is one of the cells. */
            assert!(
                words
                    .iter()
                    .any(|w| w == &planted_path.display().to_string()),
                "at {width}px the table drew its heads and not the row under them: {words:?}"
            );
            /* The empty state must NOT be painted beside a table that has rows. */
            assert!(
                !words.iter().any(|w| w == "nothing to read"),
                "at {width}px the ledger painted its empty state over a folder it had just scanned"
            );
        }
    }

    /// THE SAME CLAIM AS THE TEST ABOVE, OVER ROWS THE INGEST REALLY PRODUCED. That one plants
    /// three placeholder rows by hand and asserts they are what a fresh install lists; this one
    /// makes the ingest list them, which is what holds the two halves of that claim together.
    ///
    /// WHY THIS ONE TOUCHES THE DISK when every other test of this line plants its rows instead.
    /// The two answers are identical until the ingest has scanned: a fresh `Ingest` lists nothing
    /// at all, so `sources().len()` and `source_files()` are both 0 and agree. The case that tells
    /// them apart only exists AFTER a scan has landed, which needs a real folder and the reader
    /// thread, so no pure seam can reach it.
    ///
    /// IT USED TO ASK `nav::count_of` AND NOW IT ASKS THIS SCREEN, because this screen is the last
    /// surface that asks the question. The rail's Sources badge was the other one and it went with
    /// the row. The defect is unchanged and it is this line's own history: printing
    /// `ingest.sources().len()` here read "3 sources found" on a machine holding one log and two
    /// folders the ingest had merely looked in.
    #[test]
    fn the_log_folder_line_counts_files_found_over_a_real_scan() {
        let tree = TempTree::new("grimoire-settings-sources");
        std::fs::write(
            tree.logs().join("eqlog_Stoic_legends.txt"),
            "[Sun Sep 01 23:58:00 2026] Welcome to EverQuest!\n",
        )
        .expect("plant a log");
        let settings = Settings {
            log_dir: Some(tree.logs()),
            /* Hermetic: an empty data root, so a question about counting files does not go and
             * read the real snapshot to answer it. */
            data_root: Some(tree.root.clone()),
            ..Default::default()
        };
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let settled = pump_until(&mut ingest, |ig| {
            ig.sources()
                .iter()
                .any(|s| crate::nav::looks_like_eq_log(&s.path))
        });
        /* A test that gives up quietly is a test that proves nothing. */
        assert!(
            settled,
            "the ingest never listed the log planted in {}",
            tree.logs().display()
        );

        let listed = ingest.sources();
        /* THE VALIDITY CHECK, BEFORE THE RESULT. This case only discriminates while the ingest is
         * listing at least one row that is not a file: the dump placeholders beside the game
         * folder. If it ever stops listing those, the assertion below would hold against a plain
         * row count too and would be proving nothing while still going green. Say so instead. */
        assert!(
            listed.len() > crate::nav::source_files(&listed),
            "not a discriminating case: every row listed is a file, so this cannot tell a count of \
             finds from a count of rows"
        );

        let (state, words) = log_folder_mark(settings.log_dir.as_deref(), &listed);
        assert_eq!(state, State::Settled);
        assert_eq!(
            words, "1 source found; the SOURCES section below lists them",
            "one log was planted; the rest of what the ingest listed is folders it looked in"
        );
    }

    /// THE SOURCES LEDGER IS ON THE SETTINGS SCREEN, AND THIS DRIVES A REAL FRAME TO SAY SO.
    ///
    /// It was a rail row under CRAFT (`ScreenId::Sources`, `screens::sources`), which the owner
    /// said had no reason to be there. The row and the screen are both gone, so "it moved" is a
    /// claim about paint, and paint is what this reads: the two headings and the words that only
    /// that screen ever carried have to come out of a frame this screen really drew.
    ///
    /// THE FENCE IS THE SECTION ABOVE IT. `LOG FOLDER` is asserted present first. Without that a
    /// screen that painted nothing, or stopped halfway, would satisfy nothing below and this would
    /// fail for the wrong reason or, worse, an absence test would pass for free.
    ///
    /// AND THE ORDER IS PART OF THE CLAIM. The ledger answers questions about the folder the
    /// section above it sets, and the fold is only an improvement if the two are adjacent, so the
    /// index of SOURCES is asserted to fall between LOG FOLDER's and WINDOW's.
    #[test]
    fn the_sources_ledger_is_painted_by_the_settings_screen() {
        let (words, _) = settings_screen_frame();
        let at = |want: &str| {
            words
                .iter()
                .position(|w| w == want)
                .unwrap_or_else(|| panic!("the settings screen never painted {want:?}: {words:?}"))
        };
        let log = at("LOG FOLDER");
        let sources = at("SOURCES");
        let came_out = at("WHAT CAME OUT");
        let window = at("WINDOW");
        assert!(
            log < sources && sources < came_out && came_out < window,
            "the ledger has to sit under the field it is about: LOG FOLDER at {log}, SOURCES at \
             {sources}, WHAT CAME OUT at {came_out}, WINDOW at {window}"
        );

        /* The words that only the retired screen carried. Not headings: sentences, so a heading
         * moved without its body would not satisfy this. */
        for want in [
            "Inventory and achievements dumps (/outputfile), hunting logs and the EQ log tail, read by one ingest.",
            "Re-read",
            "folder not scanned yet",
            "no kill events yet",
            "snapshot not loaded yet",
        ] {
            assert!(
                words.iter().any(|w| w == want),
                "the SOURCES section never painted {want:?}: {words:?}"
            );
        }

        /* The empty state names every file the ingest looks for, which is the only thing on this
         * screen that tells a reader what to go and make. */
        for line in LOOKS_FOR {
            assert!(
                words.iter().any(|w| w == line),
                "the empty state dropped {line:?}"
            );
        }
    }

    /// D7: the empty state names every source kind the ingest lists. Came from the retired screen
    /// with the constant it asserts on.
    #[test]
    fn looks_for_names_every_source_kind_the_ingest_lists() {
        for (kind, word) in [
            (SourceKind::Log, "eqlog_"),
            (SourceKind::Inventory, "-Inventory.txt"),
            (SourceKind::Achievements, "-Achievements.txt"),
        ] {
            assert!(
                LOOKS_FOR.iter().any(|l| l.contains(word)),
                "{} ({word}) is not in the empty state",
                kind.label()
            );
        }
    }

    /// DEFECT THIS PREVENTS: THE UPDATES SECTION EXISTING AND NEVER BEING DRAWN.
    ///
    /// `fn updates` is one method and `SettingsScreen::ui` is the one production caller of it. A
    /// section that is written, tested at the level of `phase_line`, and not listed in `ui` is
    /// this tree's signature defect wearing a settings screen: every other test here would stay
    /// green, and the reader would have an updater with no controls and no way to see what it was
    /// doing. A mutation run found exactly that gap, because the test standing nearest to it
    /// asserts an ABSENCE and an absence is free when nothing is painted.
    ///
    /// THE DEFAULT STATE IS THE ONE ASSERTED, and it is the honest one to assert here:
    /// `settings_screen_frame` builds a screen nobody has written a view into, which is what the
    /// first frames of every launch look like and what a preflight run looks like for ever. So the
    /// line the section paints in that state is part of the claim: it says there is no updater
    /// running rather than drawing an empty row that reads as "nothing found".
    ///
    /// WHAT MUTATION MAKES THIS RED: drop `self.updates(ui, cx)` from `SettingsScreen::ui`; or
    /// have the section return early before its heading; or print a placeholder version instead of
    /// `titlebar::version()`.
    #[test]
    fn the_updates_section_is_drawn_and_says_what_this_build_is() {
        let (words, _) = settings_screen_frame();

        for want in [
            "UPDATES",
            "Running",
            "Channel",
            "check for updates",
            "download one as soon as it is found",
        ] {
            assert!(
                words.iter().any(|w| w == want),
                "the settings screen never painted {want:?}, so the UPDATES section is not on it. \
                 It painted {words:?}"
            );
        }

        /* THE VERSION IS THE MANIFEST'S OWN AND NOT A STRING. `titlebar::version()` is
         * `env!("CARGO_PKG_VERSION")`, so this is the number in `Cargo.toml` reaching the screen;
         * a placeholder would pass a `contains` against a literal and fails against this. */
        assert!(
            words.iter().any(|w| w == crate::titlebar::version()),
            "the UPDATES section does not paint the running version, which is the one fact a \
             person checks this section for first. It painted {words:?}"
        );

        /* BOTH CHANNELS ARE OFFERED. One button is a control that cannot be used. */
        for c in crate::updater::run::CHANNELS {
            assert!(
                words.iter().any(|w| w == c),
                "the channel {c:?} is not offered, so the choice is not a choice"
            );
        }

        /* AND WITH NO UPDATER IN THE PROCESS IT SAYS SO. `settings_screen_frame` writes no view,
         * which is the state of every launch until the first frame has run and the permanent state
         * of a preflight. */
        assert!(
            words.iter().any(|w| w.contains("no updater is running")),
            "with no updater in this process the section drew no explanation, so an empty section \
             would read as an update check that found nothing. It painted {words:?}"
        );

        /* NOTHING IN IT CLAIMS A CHECK HAPPENED. The one way this section can lie is by printing a
         * stamp or a count for something that never ran. */
        for lie in ["ago", "up to date", "%"] {
            assert!(
                !words.iter().any(|w| w.to_lowercase().contains(lie)),
                "the section printed {lie:?} with no updater in the process, which is a figure \
                 nothing measured. It painted {words:?}"
            );
        }
    }

    /// DEFECT THIS PREVENTS: A SECOND GRIMOIRE WINDOW OPENING OVER A FULLSCREEN GAME, MID-RAID,
    /// BECAUSE THE READER PRESSED INSTALL.
    ///
    /// # WHY THE INSTALL BUTTON IS NOT LIKE THE OTHER THREE
    ///
    /// Download is `add_enabled(quiet, ..)`, Restart now is `add_enabled(may_restart(..), ..)`, and
    /// `install::stage` asks the gate itself for defence in depth. Install had no gate at all: the
    /// press went straight to `Worker::install` and `install_app`, whose preflight SPAWNS the
    /// staged binary, and that binary is built with the same `eframe::NativeOptions` as any launch.
    /// So a real, undecorated, visible window appears for `data::LOAD_BUDGET`, takes focus, and
    /// registers global hotkeys, on top of whatever the reader is doing. The hover text said
    /// "Runs the downloaded program once to check it starts on this machine... Nothing restarts",
    /// which describes a background check, which is not what happens.
    ///
    /// # A SOURCE-TEXT FLOOR, AND WHY
    ///
    /// The gate that actually bites is in `install_app` and is tested there against a real
    /// `Pulse` (`a_payload_that_will_not_start_never_becomes_the_pointer`). What cannot be reached
    /// from a test is the ENABLED state of a widget inside `fn updates`: `settings_screen_frame`
    /// paints words, not interaction state, and driving a live fight through this harness would be
    /// a test of the harness. The floor is the same instrument the two `heartbeat` tests in
    /// `main.rs` use, and it is a floor rather than a proof, which is worth saying out loud.
    ///
    /// WHAT MUTATION MAKES THIS RED: put `ui.button(format!("Install {version}"))` back; drop the
    /// sentence about the window from the hover text; or drop the `!done` term that stops Check
    /// now being drawn once a restart is already waiting.
    #[test]
    fn the_install_button_waits_for_the_encounter_and_says_what_it_does() {
        let src = include_str!("settings.rs");
        let at = src
            .find("fn updates(&mut self, ui: &mut Ui, cx: &mut Cx) {")
            .expect("the UPDATES section is gone");
        let section = &src[at..];
        let section = &section[..section
            .find("\n    /// \"12s ago\"")
            .unwrap_or(section.len().min(20_000))];

        let install = section
            .split("Phase::Downloaded { version }")
            .nth(1)
            .expect("the Install button is gone");
        let install = &install[..install.len().min(2_000)];
        assert!(
            install.contains("may_start_download(pulse)") && install.contains("add_enabled(quiet"),
            "the Install button has no fight gate, so pressing it mid-pull opens a second window \
             over the game and takes focus for the length of the preflight"
        );
        assert!(
            install.contains("opens a second window"),
            "the hover text does not say that the check opens a window, which is the fact the \
             reader needs in order to decide when to press it"
        );

        let check_now = section
            .split("Check now")
            .next()
            .expect("the Check now button is gone");
        assert!(
            check_now.contains("Phase::Installed { .. });")
                && check_now.contains("cx.settings.updater.enabled && !done"),
            "Check now is still drawn once a restart is waiting, and `Worker::tick` returns early \
             in that state, so the press is silently a no-op and gives no feedback at all"
        );
    }

    /// DEFECT THIS PREVENTS: THE UPDATES SECTION PRINTING A FIGURE NOBODY MEASURED.
    ///
    /// Every state this section can be in prints a sentence, and three of them are the ones that
    /// go wrong. A session that has not checked must say so rather than showing a zero or an old
    /// stamp; a download must print the two real counts rather than a bar with nothing beside it;
    /// and a refusal must print the refusal's OWN words, because `Refusal`'s `Display` names both
    /// what was wrong and what was expected on purpose, and rewording it here would undo that.
    ///
    /// THE SIGNAL COLOUR IS PART OF THE CLAIM AND NOT DECORATION. `State::Wrong` paints the square
    /// red, and a red square on this screen has to mean that a decision was made AGAINST the update
    /// rather than that a download is slow, or the one signal a person actually reacts to becomes
    /// noise. So the two waiting states are `Working`, the two endings are `Settled`, and nothing
    /// but a refusal is `Wrong`.
    ///
    /// WHAT MUTATION MAKES THIS RED: give `Phase::Downloading` the colour `State::Wrong`; make
    /// `NeverChecked` print "checked just now" or any stamp at all; compute the percentage as
    /// `done / size * 100` in integers, which is zero until the download finishes; or drop the
    /// `size == 0` arm, which divides by zero on a manifest that claims a length of nothing.
    #[test]
    fn the_updates_section_prints_only_what_it_was_actually_told() {
        use crate::updater::run::Phase;

        let (st, words) = phase_line(&Phase::NeverChecked);
        assert_eq!(st, State::Idle);
        assert!(
            words.contains("nothing has been checked yet"),
            "a session that has not checked says {words:?}, which reads like a result"
        );

        let (st, words) = phase_line(&Phase::Off);
        assert_eq!(
            st,
            State::Idle,
            "a switched-off updater paints a signal other than the hollow ring, so it looks like \
             something is happening or something is wrong"
        );
        assert!(words.contains("switched off"), "{words}");

        /* THE TWO REAL COUNTS AND A PERCENTAGE DERIVED FROM THEM. The figures here are the measured
         * release binary (10,874,880 bytes, this machine, 2026-09-11) and a quarter of it, so the
         * expected percentage is arithmetic on real numbers rather than a round figure chosen to
         * make the assertion easy. */
        let (st, words) = phase_line(&Phase::Downloading {
            version: "0.2.0".to_owned(),
            done: 2_718_720,
            size: 10_874_880,
        });
        assert_eq!(
            st,
            State::Working,
            "a download in flight paints something other than WORKING"
        );
        assert!(
            words.contains("25%") && words.contains("2718720") && words.contains("10874880"),
            "the download line does not carry both real counts and the percentage between them: \
             {words}"
        );

        /* A LENGTH OF ZERO PRINTS NO PERCENTAGE RATHER THAN DIVIDING BY IT. A signed manifest
         * should never carry one and `copy_sealed` would refuse the transfer on its first byte if
         * it did, so the only wrong answers here are a panic and a "100%" that measured nothing. */
        let (_, words) = phase_line(&Phase::Downloading {
            version: "0.2.0".to_owned(),
            done: 17,
            size: 0,
        });
        assert!(
            !words.contains('%') && words.contains("17"),
            "a manifest claiming a length of zero produced a percentage: {words}"
        );

        let (st, words) = phase_line(&Phase::Installed {
            version: "0.2.0".to_owned(),
        });
        assert_eq!(st, State::Settled);
        assert!(
            words.contains("0.2.0") && words.contains("next time"),
            "the installed line does not say which version, or that nothing happens until the app \
             is opened again: {words}"
        );

        /* THE REFUSAL'S OWN SENTENCE, WORD FOR WORD. `Refusal::Display` names the thing that was
         * wrong AND what was expected, because "signature check failed" sends a person to the
         * wrong half of the pipeline. */
        let said = crate::updater::Refusal::ArtifactHashMismatch {
            said: "aa".to_owned(),
            got: "bb".to_owned(),
        }
        .to_string();
        let (st, words) = phase_line(&Phase::Refused {
            version: Some("0.2.0".to_owned()),
            why: said.clone(),
        });
        assert_eq!(
            st,
            State::Wrong,
            "a refusal paints something other than the red square, so the one signal a person \
             reacts to no longer means a decision was made against the update"
        );
        assert_eq!(
            words, said,
            "the section reworded the refusal instead of printing it"
        );

        /* AND NOTHING ELSE IS RED. A slow download and a quiet updater are not failures, and a red
         * square that can mean either is a square nobody looks at twice. */
        for p in [
            Phase::NeverChecked,
            Phase::Off,
            Phase::Checking,
            Phase::UpToDate,
            Phase::Known {
                version: "0.2.0".to_owned(),
                notes_url: None,
                why: "waiting",
            },
            Phase::Downloading {
                version: "0.2.0".to_owned(),
                done: 1,
                size: 2,
            },
            Phase::Downloaded {
                version: "0.2.0".to_owned(),
            },
            Phase::Installed {
                version: "0.2.0".to_owned(),
            },
            /* AND AN OUTAGE MOST OF ALL, WHICH IS THE ONE THAT WAS WRONG.
             *
             * A dropped connection reached `phase_line` as `Phase::Refused` and was painted red,
             * so a reader playing offline, behind a captive portal, or during a two minute outage
             * at the host was shown the signal this screen reserves for a signature that did not
             * check out, for the rest of the session. `NETWORK_CODE`'s own doc argues at length
             * that a dropped connection is not a decision; the phase did not honour it. */
            Phase::Unreachable {
                since: chrono::Utc::now(),
                why: "could not reach the update channel: connection refused".to_owned(),
            },
        ] {
            let (st, words) = phase_line(&p);
            assert_ne!(
                st,
                State::Wrong,
                "{p:?} is painted as a failure, and it is not one: {words}"
            );
            assert!(
                !words.is_empty(),
                "{p:?} paints nothing at all, so the State row would be blank"
            );
        }

        /* THE OUTAGE STILL SAYS WHAT HAPPENED AND SINCE WHEN, which is what makes `Working` an
         * honest answer rather than a quieter lie: something is in flight, it has not landed, and
         * here is how long that has been true. */
        let (st, words) = phase_line(&Phase::Unreachable {
            since: chrono::Utc::now(),
            why: "could not reach the update channel: connection refused".to_owned(),
        });
        assert_eq!(st, State::Working);
        assert!(
            words.contains("could not reach") && words.contains("since"),
            "the outage line says neither what happened nor for how long: {words}"
        );
    }

    /// The two helpers the ledger prints its columns with, kept with the section that uses them.
    #[test]
    fn last_read_never_and_ago() {
        use chrono::TimeZone;
        let now = chrono::Utc.with_ymd_and_hms(2026, 9, 2, 12, 0, 0).unwrap();
        assert_eq!(last_read_text(None, now), "never");
        let t = chrono::Utc
            .with_ymd_and_hms(2026, 9, 2, 11, 59, 48)
            .unwrap();
        assert_eq!(last_read_text(Some(t), now), "12s ago");
        let t = chrono::Utc.with_ymd_and_hms(2026, 9, 2, 9, 0, 0).unwrap();
        assert_eq!(last_read_text(Some(t), now), "3h ago");
    }

    #[test]
    fn records_column_pads_on_the_left() {
        assert_eq!(pad_left("7", 4), "   7");
        assert_eq!(pad_left("1234", 4), "1234");
        assert_eq!(pad_left("12345", 4), "12345", "never truncates");
    }

    /* ------------------------------------------------------------- fight notes -- */

    /// DEFECT: A NOTE PRINTED IN THE ORDER OF THE WEEKDAY'S ALPHABET.
    ///
    /// `Settings::fight_notes` is a `BTreeMap` keyed on a log stamp, `Wed Jul 15 23:16:50 2026`,
    /// and the map's own order is the byte order of that text. The text opens with the WEEKDAY, so
    /// every Friday in a person's history sorts ahead of every Monday and June sorts after July.
    /// A list drawn straight out of the map would look deliberate and be nonsense.
    ///
    /// AND A STAMP THAT WILL NOT PARSE STILL GETS A ROW. That is not tidiness: the whole reason
    /// this list exists is that a note can become unreachable, so a sort that quietly dropped the
    /// one note with a hand edited key would rebuild the defect inside the fix.
    ///
    /// WHAT MUTATION MAKES THIS RED: iterating the map in its own order, sorting ascending, or
    /// filtering out the keys `note_when` cannot read.
    #[test]
    fn stored_notes_are_listed_newest_first_and_the_unreadable_one_is_kept() {
        let mut notes: BTreeMap<String, String> = BTreeMap::new();
        notes.insert(
            "Wed Jul 15 23:16:50 2026".to_owned(),
            "golem pull went long".to_owned(),
        );
        notes.insert(
            "Mon Aug 24 21:04:11 2026".to_owned(),
            "clean princess kill".to_owned(),
        );
        notes.insert(
            "Fri Jun 12 08:00:00 2026".to_owned(),
            "first night in sky".to_owned(),
        );
        /* A KEY NO STAMP READER CAN TAKE, and it is deliberately one the map sorts FIRST: '?' is
         * 63 and 'F' is 70, so a test whose odd key sorted last would pass on a filter that
         * dropped it. */
        notes.insert("??? hand edited".to_owned(), "who knows".to_owned());

        /* The map's own order, so the assertion below is against something and not against
         * itself. */
        let map_order: Vec<&str> = notes.keys().map(|k| k.as_str()).collect();
        assert_eq!(
            map_order,
            vec![
                "??? hand edited",
                "Fri Jun 12 08:00:00 2026",
                "Mon Aug 24 21:04:11 2026",
                "Wed Jul 15 23:16:50 2026",
            ],
            "this fixture no longer tells the map's order apart from the right one"
        );

        let rows = notes_newest_first(&notes);
        let got: Vec<&str> = rows.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            got,
            vec![
                "Mon Aug 24 21:04:11 2026",
                "Wed Jul 15 23:16:50 2026",
                "Fri Jun 12 08:00:00 2026",
                "??? hand edited",
            ],
            "the notes are not newest first, or the unreadable stamp was dropped"
        );
        assert_eq!(
            rows.len(),
            notes.len(),
            "a note went missing on the way out"
        );
        /* And the words travel with the key they belong to. */
        assert_eq!(rows[0].1, "clean princess kill");
        assert_eq!(rows[3].1, "who knows");

        /* THE STAMP READER IS THE ONE `screens::sky` ALREADY OWNS, so this pins the two ends
         * rather than a second parser written here. */
        assert!(note_when("Wed Jul 15 23:16:50 2026").is_some());
        assert!(
            note_when("[Wed Jul 15 23:16:50 2026]").is_some(),
            "brackets"
        );
        assert!(note_when("??? hand edited").is_none());
    }

    /// DEFECT: A NOTE THAT IS SAVED, RELOADED FOR EVER, AND VISIBLE ON NO SCREEN.
    ///
    /// # HOW A PERSON HIT IT
    ///
    /// Write a note on a fight, on the Analysis page. Keep playing. That fight leaves the 40MB
    /// tail the app holds, so Analysis can no longer show it, and Analysis reading exactly one
    /// entry (the one for the fight in front of it) was the only reader `fight_notes` had. The
    /// note is still in `settings.json` and there was no surface in the app that could print it
    /// or delete it.
    ///
    /// # WHY THIS IS A FRAME AND NOT A CALL TO `notes`
    ///
    /// This tree's signature defect is a function that compiles, is tested, and has no production
    /// caller, and a fix for an unreachable setting that was itself unreachable would be that
    /// defect twice. So this drives `SettingsScreen::ui`, the method the app calls, and reads the
    /// strings back out of the shapes egui produced. Calling `notes` directly would prove the
    /// section renders and say nothing about whether anybody can get to it.
    ///
    /// The screen is NOT dirty, so no write is attempted and the owner's real settings.json is
    /// never touched: `ui` writes only when the debounce is pending, and only `mark_dirty` sets
    /// it, which only an edit on this screen calls.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping `self.notes(ui, cx)` out of `ui`, printing a count
    /// without the notes themselves, or truncating a note to a preview.
    #[test]
    fn the_settings_screen_lists_every_stored_fight_note() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);

        let mut notes: BTreeMap<String, String> = BTreeMap::new();
        notes.insert(
            "Wed Jul 15 23:16:50 2026".to_owned(),
            "golem pull went long".to_owned(),
        );
        notes.insert(
            "Mon Aug 24 21:04:11 2026".to_owned(),
            "clean princess kill".to_owned(),
        );
        let mut settings = Settings {
            fight_notes: notes,
            ..Settings::default()
        };
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(YOUTUBE_HANDLE),
        };
        let mut screen = SettingsScreen::default();
        let mut cx = crate::screens::Cx {
            data: None,
            railed: false,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: &mut ingest,
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
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 6000.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.shape, &mut flat);
        }
        let words: Vec<String> = flat
            .iter()
            .filter_map(|sh| match sh {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        let joined = words.join("\u{1F}");

        for want in [
            "FIGHT NOTES",
            "Wed Jul 15 23:16:50 2026",
            "golem pull went long",
            "Mon Aug 24 21:04:11 2026",
            "clean princess kill",
            "Delete",
            "2 notes stored",
        ] {
            assert!(
                joined.contains(want),
                "the settings screen never painted {want:?}; it painted {words:?}"
            );
        }
    }

    /// AND WITH NO NOTES IT SAYS WHERE ONE COMES FROM RATHER THAN DRAWING AN EMPTY BOX.
    ///
    /// A blank rectangle cannot be told from a broken screen, which is the empty state rule this
    /// tree holds everywhere. This one has a second job: a person who has never written a note is
    /// exactly the person who does not know the Analysis page is where notes are written.
    ///
    /// WHAT MUTATION MAKES THIS RED: an empty state that draws nothing, or one that names no way
    /// to produce a note.
    #[test]
    fn with_no_notes_the_section_says_where_a_note_is_written() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);

        let mut settings = Settings::default();
        assert!(settings.fight_notes.is_empty());
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let live = crate::watcher::Status {
            twitch: crate::watcher::Channel::unchecked(TWITCH_HANDLE),
            youtube: crate::watcher::Channel::unchecked(YOUTUBE_HANDLE),
        };
        let mut screen = SettingsScreen::default();
        let mut cx = crate::screens::Cx {
            data: None,
            railed: false,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: &mut ingest,
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
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 6000.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| screen.ui(ui, &mut cx));
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.shape, &mut flat);
        }
        let joined: String = flat
            .iter()
            .filter_map(|sh| match sh {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<String>>()
            .join("\u{1F}");
        assert!(joined.contains("FIGHT NOTES"), "the heading is not drawn");
        assert!(
            joined.contains("Analysis"),
            "the empty state does not say where a note is written: {joined:?}"
        );
        assert!(
            !joined.contains("Delete"),
            "a delete control was drawn with nothing to delete"
        );
    }
}
