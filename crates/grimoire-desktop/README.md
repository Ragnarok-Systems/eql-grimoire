# grimoire-desktop

A native Rust desktop app for EverQuest Legends, in the Gnomish console's chrome and the Broken
Stoic palette. It carries a parser, kill tracker, loot feed, inventory, gear score, valet,
exaltations, poSky checklist, unlocks and quests, plus item, zone and drop lists, can be summoned
always-on-top by global hotkey either whole or one tool at a time, and points at Broken Stoic on
both Twitch and YouTube from the title strip and the Watch screen, with a preferred platform in
Settings. The goal document every decision in this crate resolves against is not checked in;
decisions are cited here by their D-number (D1..D11), and each one is restated where it is used.

Toolkit: egui 0.36 / eframe 0.36, painted, no webview (see "Embedded playback" below for the one
permitted exception and why it is not built). The crafting maths is `grimoire_wasm::call`, the same
function the wasm module wraps, called in process from the Commission screen.

## Build and run

From the workspace root, `C:/Repos/Everquest Legends/eql-grimoire`:

```text
cargo build -p grimoire-desktop
cargo run -p grimoire-desktop
cargo clippy -p grimoire-desktop --all-targets -- -D warnings
cargo test -p grimoire-desktop
```

The crate is a library (`src/lib.rs`, every module) plus a thin binary (`src/main.rs`, the App).
`RUST_LOG=info` on the command line shows the watcher's polls and the snapshot's load time; the
modules log at `warn` for anything that failed.

`GRIMOIRE_SMOKE_MS=<ms>` makes the app close itself after that many milliseconds and print
`smoke: N frames` to stdout first. An exit code of 0 with N above zero means every panel was
built, the hotkeys registered (or recorded why not), the watcher and the ingest started, the
snapshot load and the corpus read were kicked off, and N frames drew without a panic. Only the
debug binary has a console to print to: the release build is a `windows_subsystem = "windows"`
executable.

Settings live at `%APPDATA%\eql-grimoire\settings.json` (the platform config dir on other
systems). A malformed file never stops the app; the Settings screen shows the reason in red and
runs on defaults until saved over. Under `cargo test`, `Settings::save` REFUSES to write that file
(a test once overwrote the operator's saved folders with a fixture); tests round trip through
`save_to` on a scratch path.

The CHANNEL this build follows is NOT a setting, and the Settings screen no longer mentions it.
`settings::TWITCH_HANDLE` and `settings::YOUTUBE_HANDLE` are constants, each documented with the
account id that makes it checkable years from now. They were two text boxes over two `Settings`
fields; a box that repoints an app built for one channel can only ever be wrong, so the
boxes, the fields and the "unconfigured" states that hung off them are gone. A settings.json
written by a build that had those fields still loads, and the two retired keys are dropped from it
rather than copied forward.

Losing the boxes left a read only `CHANNEL` section, and that has gone too: a heading, a sentence
saying the build follows one channel, and the two handles with their poll cadence. All of it true,
none of it a decision, on a screen whose job is decisions. The facts kept their homes elsewhere:
the handles are the constants above, documented where a reader of the code meets them, and the
poll interval (`watcher::POLL_EVERY`, 90 seconds) is on the Watch screen's `Check now` hover,
which is where somebody is actually choosing between waiting and asking now.

WHICH PLATFORM YOU WATCH ON IS a setting, and it is the only part of this that is. `watch_on`
(`settings::Platform`, Twitch or YouTube) decides which channel the live pill reports and where a
click on the pill or on the maker mark goes; both platforms are polled either way, and each keeps
its own mark in the main window title strip as a link to itself. The pill NAMES the platform it is
reading, because `LIVE` on its own is ambiguous once there are two. A settings.json without
`watch_on` loads and takes Twitch; a `watch_on` this build does not recognise, of any JSON type,
also takes Twitch rather than failing the whole file.

## The window

The main window is frameless. Its title strip (Cinzel title, the live pill, pin, minimise,
maximise, close) is painted by the app: drag the empty part to move the window, double click it
to maximise, and use the grip in the bottom right corner to resize. The pin glyph shows the window
level last pushed to the OS, which mirrors `always_on_top` in settings once the registry has run a
pass; a click saves the new value. Every tool window (Watch, Parser, Plane of Sky, LFG) is a
separate OS window with the same strip and its own pin. A row that has a tool window (PLAY,
SKY, GROUP and STOIC rows) carries a "Pop out" control in the context bar, which reads "Focus
window" while that window is open.

The rail on the left is decision D5's seven sections, two open at a time (opening a third closes
the least recently opened). Clicking the brand block collapses the rail to its squares; the
collapsed rail keeps each section's marker (with the section's name on hover) and shows only the
open sections' rows, so the two-open cap holds at both widths. The square before each
row is that row's state: settled (has its data), a hollow ring (nothing to draw from yet), a gold
bar (a person has to act, usually by pointing Settings at the Logs folder or rebinding a hotkey),
red (something it depends on failed), or blue while the snapshot or the crafting corpus is still
being read. Blue is those two threads and nothing else: a quest with some of its pieces in hand,
an unlock with some of its steps done, is idle with the count beside it, never "working". CRAFT /
Commission follows the corpus read (`web/corpus.grim`), so its square and its body agree when the
file is absent. SKY / Keys settles on either of its two faces: the island ladder (sky.json) or the
achievements dump. Below the sections, pinned to the floor, is the persona footer, and it is the
whole way into Settings (see "One door to Settings" below).

The footer reads `v0.1.0 · AGPL-3.0-only`, both from the manifest. Decision D8 adds `· Source on GitHub` as
a link, and the footer draws that the moment `package.repository` is declared in Cargo.toml
(worded "Source" when the URL is not on github.com). Until then it draws nothing in that place:
there is no repository to point at (see NOT DONE), and the words without the link would be the
one invented fact on the page.

### The rail's marks

Four marks and no more, and each says one thing.

**The section marker is a chevron**, pointing right when the section is shut and down when it is
open, in the header's own colour so the mark and the word brighten together under the pointer. It
was a bar that stood up and lay flat, and the argument for the bar was internal consistency: a
chevron is a third shape in a rail that already has a leading square and a trailing bar. That
argument lost. The chevron is THE disclosure convention, already in the reader's head before this
app is opened, and a rail that beats a convention that strong charges every reader the price of
learning a local dialect to save itself one shape. The mark states where the rows ARE, not where a
click would send them: down means they are below you already. Both rail widths turn the same
chevron; the collapsed rail drops the words, never the signals.

**The section dot** is a 4px gold circle to the left of the chevron, meaning something under this
section needs a person. A shut section cannot show its rows' trailing bars, so the header carries
the one fact those bars would have carried. It is `nav::section_wants_you`, and it follows the ONE
row in this build that is genuinely waiting on someone (CRAFT / Sources, when no Logs folder has
been pointed at) rather than lighting every section that holds such a row, which would be five
dots for one cause and not one of them saying where to go.

**The count badge** is small monospace at the right of a row, inside the attention bar's lane, and
it is a count of something the app is holding at that moment: Items, Zones, Drops and Quests are
the snapshot lengths those screens print as the denominator of "showing N of M", and Sources is
how many source FILES the ingest found (not `sources().len()`, which includes the placeholder row
for a folder that was looked in and was empty, and would put a 2 beside Sources on a machine with
no sources at all). Every other row has no badge, and `nav::COUNTED` lists the five in one place so
the omissions can be argued with; that list is the GATE and not a comment, because `count_of`
returns None for anything outside it before it reaches its own match, so a new match arm cannot put
a number on the rail without the row being argued into the list first. The Settings screen's LOG
FOLDER line counts through the same `nav::source_files`: it used to print `sources().len()`, so a
fresh install with a valid Logs folder and no game files read "3 sources found" beside a filled
SETTLED square while the rail's badge, correctly, said nothing at all. THE EMPTY RULE: absent and
zero both draw NOTHING, no zero, no
dash, no held space, which is the original's `.cnt:empty{display:none}` and its `f?f:''` together.
A rail full of zeroes is a rail that has taught the reader to stop reading it. The badge's lane is
reserved on every row whether or not a badge is drawn, so a count does not shuffle sideways at the
moment its row starts wanting you.

THE EMPTY RULE IS NOT ONLY THE RAIL'S, and PLAY / Kill tracker was breaking it in the biggest type
on that screen. Its head printed `done / total` as a percentage, and with total 0 it printed the
undefined ratio as a literal `0%` at 20pt over "0 of 0 mobs", "0/0 zones cleared" and an empty bar,
with "No zones with a roster." underneath. Nothing computed that zero, and a headline is the worst
place to invent one: `0%` reads as "you have killed none of them", a claim about the player. Two
ordinary things reach it. `Roster::load` accepts a valid kills-data.json that lists no zone with
mobs in it, and the tracker settings can exclude every zone the roster does list, which the default
`ignore cities` box does on a city-only roster with nothing wrong at all. `parser::head_pct` now
returns None rather than a number, which takes the whole head with it, and `parser::nothing_counted`
says which of the two it is, because one sends the reader to the data and the other to a checkbox.

**The bar survives in the body, not the rail.** Every combo box and every folding list in a body
screen takes it as an open marker (`chrome::combo_icon`, `chrome::fold_icon`) rather than egui's
filled triangle, and that argument is unchanged: a third shape earns nothing there. A fold inside
a table is already sitting on the thing it discloses; it needs a mark that takes no room, not one
that teaches a convention.

### The persona footer

Pinned to the foot of the rail, under a hairline, below the scrolling nav list and never inside
it: the regard seal, the character's name and a gear. It is the web build's `.pgfoot`, and the pattern Discord,
Slack and VS Code all landed on independently, that the person is the last thing in the rail and
the way into settings is the person. The whole row is one click target and it opens the Settings
screen.

Most of what the original block displays has no source here, so each element was taken one at a
time and asked where its number comes from:

- **The name** is real when there is one: `ingest::active_character`, taken from the log file
  being tailed (`eqlog_<character>_<server>.txt`). With no log read it says "no character yet" in
  the dimmest text and its hover says where the log folder is set. It never prints the character
  the web mock hard codes into its markup. **It is the only element on this row with a source**,
  which is why it is the only one drawn.
- **Standing is real**, and it is the regard seal leading the row: `grimoire_core::Standing`, the
  web build's `badge(ME.rep,'sm')` ported number for number. `Standing::UNPROVEN` is a DEFINED
  value, 3.0 on zero ratings, so printing it reports the engine rather than inventing a score;
  nothing in this build writes a rating yet, so the seal draws its unproven face, dashed like the
  original's unsealed offer, and says why in the hover. Round one is the counter example and it is
  why the field test exists: it drew this seal off a `regard` field the App never wrote, so
  "absent" was the only state the binary could reach.
- **The gear carries the settings state.** It is `TEXT_3` at rest and `GOLD_HI` under the pointer,
  and when `main::settings_state` is not settled the resting gear takes that state's colour
  instead. See "One door to Settings" below.
- **Alerts are cut**, for the same reason and one more: the bell sat inside the row's single click
  target, so clicking the alert icon opened Settings. The web build hides the bell outright when
  the count is zero, which on this build is always.
- **Theme swatches are cut entirely.** The original offers four (grimoire, guild, stone, system).
  This app has one theme and no theme switching, and four swatches of which three do nothing is
  the clearest possible violation of the no-invented-UI rule.

Both cuts are one rule applied twice: an empty state belongs on a surface that WILL hold data and
does not hold it yet, and a control for a feature that does not exist is not drawn. `Persona` has
four fields and every one of them has a production writer, and
`every_persona_field_is_filled_by_the_app` reads the App's construction site and fails if a field
is added before the code that fills it, or if that site falls back on a rest pattern. `Persona` no
longer derives `Default` at all, because that derive is the value such a rest pattern reaches for.

The App reserves `persona::HEIGHT` off the rail before it sizes the scrolling list, and
`persona::room_above` / `persona::push_to_floor` own that arithmetic rather than the App, because
it has to know what egui charges in `item_spacing` and the App does not.

### One door to Settings

The persona footer is the way in: its name and its gear both open the screen, as the web build's
`.pgfoot` does and as Discord, Slack and VS Code all landed on independently.

The rail used to end with a Settings ROW under a hairline as well, which made three doors to one
screen stacked on top of each other. It was argued for on one ground, that it carried the state
`main::settings_state` computes (red when settings.json could not be read, gold when a global
hotkey failed to register, D4) and the footer could not. The argument was answered rather than
overruled: the state moved onto the footer's gear, which is the control that still opens the
screen, so a broken settings file or an unregistered chord shows on the rail at both widths. The
row's version of that signal was in any case the weaker one, because `chrome::nav_row` paints a
trailing bar for the gold state and no mark at all for red, so a settings file that would not load
was reported by that row in no visible way whatsoever. The gear also carries the words: hovering
the footer says which fault it is, because a colour alone says that something is wrong and never
what.

The context bar carries the find box (decision D6): one box over every list, with `i:` `z:` `d:`
`q:` `s:` prefixes (items, zones, drops, quests, sky pieces), plain substring, case folded, no
fuzzy matching. Picking a hit opens the FIND screen that owns the record (a sky piece opens on
Items). It reads `data::Snapshot::search`, the same rule the tests pin.

## Hotkeys

All global (`global-hotkey`), all `Ctrl+Alt`, registered on the main thread at startup. The table
below is `hotkeys::DEFAULTS` and is what the Settings screen lists.

| chord | opens |
|---|---|
| `Ctrl+Alt+G` | the whole app, toggled (minimise / restore, never hide) |
| `Ctrl+Alt+W` | Watch: Broken Stoic live |
| `Ctrl+Alt+P` | Parser |
| `Ctrl+Alt+S` then `K` | poSky checklist |
| `Ctrl+Alt+L` | LFG, generic (when no second key arrives within 1.5 s) |
| `Ctrl+Alt+L` then `R` | LFG, looking for raid |
| `Ctrl+Alt+L` then `M` | LFG, looking for motes |
| `Ctrl+Alt+K` | poSky checklist, direct alias |
| `Ctrl+Alt+R` | LFG raid, direct alias |
| `Ctrl+Alt+M` | LFG motes, direct alias |

A leader (`Ctrl+Alt+S`, `Ctrl+Alt+L`) arms a 1.5 second window and shows a small `S: K = Sky`
hint in the bottom right corner of every Grimoire window. FOR THAT WINDOW THE SECOND KEYS ARE
REGISTERED AS BARE GLOBALS (`K`, or `R` and `M`) and released the moment the chord completes,
lapses or is cancelled, so the chord completes with the game in front, which is the whole point of
a global hotkey. egui's own input is read as a fallback for a bare key another program owns. The
second keys are probed once at startup (registered and released) so Settings can say now whether
a chord will complete from the game. `Escape` in a Grimoire window or a wrong key cancels a
leader; `Ctrl+Alt+L` lapsing with no second key opens generic LFG exactly once.

Every binding can be changed on the Settings screen (decision D4): type a chord as
`Ctrl+Alt+S then K` (modifiers joined by `+`, one plain key after `then`), press Apply or Enter,
and the globals are released and registered again on the same manager. Only rows that differ
from the table are saved (`Settings::hotkeys`, row id to chord text); Default puts a row back. A
saved chord that does not parse keeps the default and says so on its row.

A chord another program already owns registers as a failure on that one binding and the rest still
register; the Settings screen shows each row's state and the reason (Windows reports that a hotkey
is taken and never by whom, and the text says exactly that). An unregistered binding is the gold
"act" state on the persona footer's gear in the rail AND beside its row on the Settings screen:
one fact, one word. A release that fails is not dropped either: the key stays in the registered list so
the next release and `Drop` retry, its row says the bare key is held by this program, and a
second key that will not register at the moment a leader arms reaches the Settings screen on the
next frame (`Hotkeys::take_dirty`), not after the next rebind. A key this process still holds
from a failed release is not reported as "owned by another program" on the next rebind; it is
this program's and the binding works.

## The data path rule

The snapshot (gear-data.json 7.4 MB, item-tooltips.json 3.1 MB, quest-items.json 4.2 MB,
kills-data.json 0.5 MB, sky.json 1.4 MB, and the `atlas-wiki/` directory of 122 zone pages; all
CC BY-SA 4.0, source eqlwiki.com) is loaded at runtime on its own thread, never compiled in. The
root is chosen in this order, first directory holding `gear-data.json` wins:

1. `data_root` on the Settings screen, when set (used as given, and a wrong path is reported as a
   failure naming it rather than silently falling through).
2. `data/` beside the executable, which is the shipped layout.
3. `data/` in the per-user folder this app already writes `settings.json` into
   (`%APPDATA%\eql-grimoire\data` on Windows), because an installed binary can sit somewhere the
   reader cannot write and the snapshot is content he refreshes.
4. `crates/grimoire-desktop/data/` in the source tree this binary was built from, so a `cargo run`
   or a `cargo test` out of a checkout finds the checkout's own data. **Development builds only:**
   it is reached through `CARGO_MANIFEST_DIR` and is compiled out behind
   `#[cfg(any(test, debug_assertions))]`, so a release binary never probes, and never prints, a
   directory belonging to the machine that compiled it. `web/corpus.grim` is gated the same way in
   `screens::commission::corpus_candidates`.

Rows 2 and 3 are worked out when the loader runs, from `current_exe()` and from the platform's
config directory. No absolute path is written down anywhere in the list, and
`no_release_candidate_is_a_compile_time_absolute_path` fails if one is added again.

Every "put the data here" sentence in the app prints that list from `data::candidates()`, in the
loader's order, so no screen can drift from what the loader tried; the Items empty state takes the
WORDS from `data::candidates_labelled()` in the same call, so it cannot label a directory
something the loader does not call it either. While the snapshot parses,
EVERY screen that reads it (the FIND rows and the CHARACTER rows alike, and the Sky tool window's
own copy) draws the loading notice, never "no snapshot loaded". The FIND screens print the root
they read and the record count (6891 items, 122 zones, 1637 drop entries, 3057 sky items, 924
quests on the committed copy); the Items detail pane shows the wiki's stat block from
item-tooltips.json and the quest-items droppers for the same name; the Sources screen prints the
load time. Every record keeps unknown fields in a `#[serde(flatten)] extra` map; positional arrays
in the files stay positional with a comment naming what each index was observed to hold.

Tests that read the real files fail loudly when the files are absent. `GRIMOIRE_NO_DATA=1` opts
out and prints `SKIPPED ON PURPOSE` per test (46 lines out of the 457 tests) rather than passing on
nothing. The marker is written to the process's stderr handle directly, not through `eprintln!`,
because the test harness captures a passing test's print macros and shows them only under
`--nocapture`; under the plain `GRIMOIRE_NO_DATA=1 cargo test -p grimoire-desktop` the lines are on
the console (counted on 2026-09-02: 46 without `--nocapture`). A skip nobody can see is the silent
skip the goal forbids.

The Logs folder: `log_dir` in settings, else the default install path
(`%PUBLIC%\Daybreak Game Company\Installed Games\EverQuest Legends\Logs`) when it exists. The
newest `eqlog_*.txt` is tailed (`ingest::TAIL_CAP`, 40 MB, printed from the constant wherever a
screen mentions it; first partial line dropped only when the cap cut the file; cursor advanced by
bytes actually read); the newest `*-Inventory.txt` and the newest `*-Achievements.txt` in the
game folder or the Logs folder are the two dumps, both read by the one ingest (decision D7) and
both listed on the Sources screen with when they were read and how many rows. With NO Logs folder
resolved, every empty state says to set one in Settings; the `/log` and `/outputfile` advice
appears only once there is a folder to look in.

## Live status

`watcher.rs` polls Twitch every 90 seconds (`watcher::POLL_EVERY`, printed wherever a screen
mentions it) on its own thread: the public GQL endpoint first, then decapi.me as plain text, then
an empty Helix rung with slots for keys. A failed poll keeps the last known state and its age; the
pill's tooltip starts with the pill's own label and names which source answered. The Watch screen
has a "Check now" button that asks for one poll early. YouTube is polled on the same cycle, by
fetching the channel's `/live` page and reading the markers its player JSON carries. The YouTube
page reader says "offline" only for a body that is both
a YouTube page (`ytInitialData`) AND names that channel as its own (`canonicalBaseUrl`,
`vanityChannelUrl` or `ownerProfileUrl` ending in `/@handle`); a bot check, sign-in wall or
regional block that still embeds the page shell is a failed poll, which keeps the last known
state, never a confirmed offline.

## Embedded playback

There is none, in any build, and no feature flag claims otherwise. The Watch window shows live
status with its age, an Open on Twitch button, an Open chat button and the videos links, and says
"Playback opens in your browser." Decision D1 permits a `wry` WebView2 surface in that window and
only there, and its fallback clause calls the browser path an acceptable v1. Why the surface is
not built, verified in the egui 0.36.1 and eframe 0.36.1 sources: `show_viewport_deferred` hands
its callback only `(&mut Ui, ViewportClass)`, and eframe implements `HasWindowHandle` only for
`CreationContext` and `Frame`, both of which carry the ROOT window. There is no sanctioned way to
get the deferred Watch viewport's native handle, and finding it by window title through the OS
is forbidden by the goal. Round one shipped a `watch-embed` feature and an optional `wry`
dependency that compiled the crate in and hosted nothing, while the default build told the user
embedded playback was "a build option"; the feature, the dependency and the sentence are gone.

## The seals, and why there are none

`src/seal.rs` is gone. It was the wax seal of the original, ported whole, 1,211 lines and 33
passing tests, and it was NOT DRAWN ANYWHERE: no screen, no module, nothing outside `pub mod seal;`
in `lib.rs` ever named it. The linker dropped it from the shipping binary, which two independent
checks confirmed at the binary level. Round one recorded that fact in this file and kept the code
anyway, on the argument that an order store would want it one day. That argument does not survive
the rule the rest of the crate is built on: a module the binary cannot reach proves its rules about
a product that does not have them, and every one of its 33 green tests was evidence about nothing
a user can see.

**It is not recoverable work.** The design it ported is in `web/app.html`, which is authoritative,
and this is what a re-port has to know. A seal is a disc of wax with a word struck into it, and in
the web build it carries one fact about one commission. Its border style says WHETHER THE ACT
HAPPENED: dashed is a proposal, so there is no wax and no depth, only its outline; solid is struck;
solid with a second ring inset is a formal act. Its tilt says WHICH THING it belongs to,
`hash(order_id) % 6` over six fixed angles (`.r1` to `.r6`), because a seal that jitters reads as
broken rather than handmade, and six angles is enough that no two adjacent seals match while the
set still reads as one hand. A certification of standing overrides the tilt and sits at -2 degrees
(`.s-cert{--rot:-2deg}`), almost straight, because it is the most formal seal there is. The edge
wobbles off a circle from the same id hash, the inset shadow pools dark at the bottom and lays a
hairline of light across the top, and the whole thing turns together, wax and letters and shadow,
the way a CSS `transform` carries a `box-shadow` with it. THE SEAL OWNS NO PALETTE: the colour is
the state colour its caller passes. In egui none of that is free: there is no elliptical corner
primitive and a `Painter` has no canvas transform, so the edge is a generated ring of points and
every point, wax, rings, depth and each letter, is rotated about the centre before it is painted.

**What has to exist before it comes back.** An order store. Every seal in the web build hangs off
an order: `SEAL` is keyed by order phase and the tilt is `rot(o.id)`. This build has no order
store, which is the same absence that leaves Work orders, Workshop and Standing routed to
`main::unbuilt`, and CRAFT / Commission prices one quote through the in-process engine and keeps
nothing, so there is no id to tilt by. A fabricated id would make every seal identical and defeat
the one rule the whole design is built around. Write the store first, then the seal.

## Tests

`cargo test -p grimoire-desktop`: 457 tests, all passing on 2026-09-02, under a second. 445 on the
library target and 12 on the binary. Every rule (kill grammar, tail
reader edge cases, inventory columns, gear score, valet walk, exaltation sockets, poSky ledger,
quest steps, achievements trust, LFG copy line, nav table, settings round trip, watcher parsing,
hotkey chord machine and chord parsing, viewport registry, theme corners) carries a test that fails
without it. Real-data tests use `data::testdata::snapshot()` and the opt-out above.

**The count went DOWN from round one's 468 and that is the point.** 33 of those tests were
`seal.rs`, a module the binary could not reach (see "The seals" above), and 9 more were the persona
footer's regard ladder and alert badge, whose fields the App never wrote. Tests over code no
launch executes are not coverage. What replaced them is smaller and touches the shipped paths: the
binary target went from ZERO tests to 12, which is the first coverage the rail's own wiring has
ever had.

**`src/main.rs` used to carry no tests at all.** `cargo test` printed "Running unittests
src\main.rs ... 0 passed" beside a library suite of several hundred, so the lines that JOIN the
modules, which row the badge is counted for, which section carries the attention dot, where the
persona footer's click goes, were asserted by nothing. `App::ui` still cannot be called from a
test, because it takes an `&mut eframe::Frame` and eframe exposes no constructor for one, so the
joins were lifted out of the closure instead: `rail_plan` decides the entire rail as a value and
the drawing loop paints that value and decides nothing, `persona_pending` is the footer's route to
the body, and `parse_smoke` is the smoke switch with the environment lifted out. Each has a test
that fails when the join is mis-wired, including a fixture where a section standing open and a
section wanting a person are DIFFERENT sections, which is what makes transposing them detectable.

## Reachability

A green `clippy -D warnings` on the LIBRARY target is not evidence that the binary reaches a
function. Three checks stand in for it, and each has a blind spot the others cover.

**Check 1, rustc.** A bin-only copy of the crate (no `lib.rs`, modules declared in `main.rs`,
every `allow(dead_code)` stripped) under rustc's own dead-code lint. On 2026-09-02, after the
second fix round, it reports ZERO unused functions, methods, structs or constants and 25 "field
is never read" warnings, listed below. THAT RUN PREDATES `persona.rs`, AND IT HAS NOT BEEN REPEATED
SINCE, so the list below is a record of that run and not a claim about this tree. What is known
about the modules that landed after it is under "What round one's reachability findings were"
below, and it is known by check 3 and by grep, not by check 1. Its blind spot: rustc ignores only
derived `Clone` and
`Debug`; a struct that derives `PartialEq`, `Eq`, `Hash`, `PartialOrd` or `Ord` gets an impl that
reads every field, so every field on it counts as read. Round one certified "zero unused" on this
check alone, and an adversarial pass found fourteen fields hiding behind exactly that (computed
rules with test-only readers: `Fit::narrows_cls`, `Landing::toward`, `Crit::goal`,
`AuditSummary::swaps`, `Rank::better`, `Board::version` and the rest).

**Check 2, `src/reach.rs`.** A test that walks `src/`, cuts every `#[cfg(test)]` item, finds
every struct (100 of them) whose derive list carries one of those five traits, and fails unless
every field of every one has a `.field` read or a destructuring read in the production text that
remains. It found the same fourteen the adversarial pass found and nothing else; each was wired
into a screen (`narrows_text` after every home a stone is offered on Exaltations; `Standing::better`
in the rank tooltip; the merge arithmetic in the Copies hover; `swaps` on the Valet's audit line;
the "770/5000" after a counting achievement step; the LFG board's version checked on load and a
newer one refused) or removed (`CopyGroup::worn_rows`, consumed locally; `Tooltip::ic` and
`QuestStep::npct`, now in their records' `extra` maps; `Changed::always_on_top`, a flag nothing
read because `Windows::show` reads the setting itself). Its blind spot: it is a text floor, not a
type check. A field that shares its name with a read field on another struct passes (that is how
`Changed::always_on_top` hid behind `Settings::always_on_top`), and a `#[cfg(test)]` block the
cutter fails to recognise would count as production. The next adversarial pass starts from these
two floors, not from zero.

Every item the first adversarial pass listed was either wired into a production path (the D6 search engine and
its record `matches`/`hit`/`detail` rules behind the find box; `Snapshot::tooltip`, `drop_sources`,
`zone` and `load_time` on the Items, Zones and Sources screens; `Zone::drop_list` and
`AtlasMob` drops in the roster merge; `Sky::island`, `item`, `alts` and `SkyItem::drop_rows`
behind the `s:` search and the boss-view hover; `held` and `reconcile` under the Sky held counts;
`ScreenId::ordinal` and `ALL` as the App's per-screen tab memory; `parse_log` in the ingest's
bootstrap; `Section::label`, `worn_in`, `section_counts`, `scanned_at`, `Roster::path` and the
dump `server` fields on the Sources ledger; `Kind::what`, `SocketType::at`/`sort`,
`expected_sockets` and readDump `sources` on the Exaltations screen; `Walk::ctx` and the
exalt-audit `Home`/`Idle` fields as the Valet's idle-stone list; `norm_mob` marking wiki droppers
the tailed log has seen die; `Watcher::refresh` behind Check now; `Windows::is_open` and
`is_pinned` behind Pop out and the pin glyph; `pill_label` as the tooltip's first line; `Hotkeys`
rebinding behind the Settings editor; the achievements dump through the ingest) or cut as a
duplicate of a rule that is reached (`Channel::age_text`/`age_secs`, a second wording of the
pill's age; `ingest::count_of` and `sky::parse_dump_text`, second parsers of rows the ingest
already parses; `Drop::mobs_in`, the Drops screen's `by_zone`; `screens::inventory::parse_inventory`
and `ParserScreen::view`, wrappers with no caller; `Parsed::by_name`, `Windows::any_open`,
`player_url`, `Hotkeys::chord_mut`, `Sky::isle`, trivial accessors). No `allow(dead_code)` remains
anywhere in `src/`.

What check 1 still reports, by name, so the next pass can hold this list to account: 25 "field is
never read" warnings on structs without a masking derive. Twelve are the `#[serde(flatten)]
extra` maps the D6 contract requires on every record (`screens/quests.rs` 73, 148, 167, 215,
225, 235, 270, 279; `ingest.rs` 158, 172; `data/sky.rs` 234; and the quest step at
`screens/quests.rs` 187). The rest are typed fields deserialised so the shape is measured and
kept, or filled by a rule and not printed by a screen: `screens/quests.rs` 73 (`items`,
`zones`), 148 (`roles`), 187 (`npct`, `mob`, `fac`, `gold`), 215 (`t`), 225 (`alias`), 235
(`adj`, `oe`), 628 (`next`); `ingest.rs` 158 (`t`), 1198 (`server`); `screens/commission.rs` 121
(`item`, `disposition`), 133 (`item`); `screens/exalt.rs` 402 (`item`), 429 (`items_read`), 668
(`yields`); `screens/valet.rs` 451 (`slot`, `base_ac`), 462 (`slot`), 477 (`slot`, `name`,
`rec`, `tier`), 973 (`slot`, `vs`, `worn_ac`), 1071 (`slot`), 1156 (`kind`). None is a computed
verdict a screen withholds; the valet and exalt ones are the slot names and record indexes the
rows carry, read by the tests that pin those rules. Line numbers are as of 2026-09-02.

**Check 3, the module floor, `src/reach.rs`.** Every `pub mod` in `lib.rs` must be NAMED, as
`name::`, by production code in some OTHER file. Its own file cannot vouch for it and neither can
`lib.rs`'s declaration of it. This is the check the crate did not have, and it was not hypothetical:
`seal.rs` was 1,211 lines and 33 green tests with zero callers, and neither of the two checks above
could have said so. Check 1 runs on the BIN target and `seal` was `pub` in the LIBRARY, where a
`pub` item is the API by definition; check 2 walks FIELDS on structs behind a masking derive and
has no notion of a whole module having no caller. Its own blind spots, said plainly: it skips
whole-line comments so a doc comment cannot vouch for a module (round one's `persona.rs` described
`seal::draw` at length in prose, which would have passed a naive substring scan), but a trailing
comment on a line of code would still vouch for one; it proves a module is REACHED, not that every
item in it is; and it reads `lib.rs`, so a module declared only in `main.rs` is out of scope.

### What round one's reachability findings were, and what happened to them

`src/seal.rs` had ZERO production callers and is now DELETED, with the design it ported written
down under "The seals" above so the next port starts from the rule and not from an archaeology dig.
Check 3 exists so this class of finding is caught by a gate rather than by a reviewer's grep.

`nav::wants_you` was reached only from tests and is DELETED. It was the `Cx`-taking wrapper around
`nav::section_wants_you`, and it rebuilt the facts on every call, which re-lists the ingest's
sources: seven directory listings a frame to answer one question about one row. The App therefore
used the facts-taking form, the wrapper's only callers left were two tests in its own file, and a
second slower route to one answer reachable from nothing the binary runs is a route a future caller
picks by mistake. Its tests now ask the way the App asks.

`nav::COUNTED` was reached only from tests and is now PRODUCTION CODE: `count_of` returns None for
any row outside the list before it consults its own match, so the list is the gate rather than a
comment with a test attached. Nothing can be badged without being argued into it first.

`titlebar::maker_glyph` was reached from NOTHING, including tests: the identifier occurred exactly
once in the whole repository, at its own definition. It is DELETED. Worse than unused, its doc
comment asserted a behaviour the shipped window did not have ("the maker's mark when the rail is
collapsed"), while `chrome::brand`'s narrow branch said in so many words that the glyph had been
removed and why. A note where it stood says so, so it is not written a third time.

`Persona::regard` and `Persona::alerts` had no production WRITER, which is the same defect one
level down: the App's only construction site filled `name` and defaulted the rest, so the struck
seal, the rung ladder, the alert badge and two of three tooltip arms could not be entered by any
launch. Both fields and everything behind them are DELETED, and
`every_persona_field_is_filled_by_the_app` reads the App's construction site so a field cannot
arrive ahead of its data again.

None of these was caught by `clippy -D warnings`, because a `pub` item in a library IS the API.
That is the whole reason the three checks above exist.

## NOT DONE

Blunt, and in the order a player would notice.

- The combat engine is not built. PLAY / Fights says so in three lines and nothing else, no fake
  meter: what is missing, that the grammar for it is written in `docs/COMBAT-PARSER.md` and nothing
  here implements it, and that the log it would read IS being read by Kill tracker and Loot. Round
  one said only "Combat engine not ported yet. Kills and loot are live.", which is the first half
  of the empty state rule and names no file, no screen and no next step. The source it names is
  checked against this repository by a test, because a path a reader cannot open reads as fact and
  is the invention rule with extra steps.
- Embedded Twitch playback (see "Embedded playback"). Playback opens in the browser, in every
  build, and the screen says only that.
- YouTube live status is read from the channel page rather than an API, so it is best effort and
  says so through its source name, `youtube-page`.
- The Helix rung has slots for keys and no request; with keys filled it says "keys are set but
  the helix request is not implemented yet".
- Eight nav rows have no data source and say so on screen with the reason: Work orders,
  Workshop, Spells, Trio, AA, Levelling, Spawn timers, Standing. Their rail squares are hollow.
  The rows AND their words are one list, `nav::UNBUILT`: the App's unbuilt page reads it instead
  of keeping a second copy, and a test holds the list to the App's routing. That sentence about
  hollow squares was false when it was written. Spawn timers drew a FILLED square off the ingest's
  kills, while the page it opens ends with "The rail draws this row as a hollow ring for the same
  reason", so on any machine that had ever logged a kill the screen sent the reader to look at a
  ring that was a square. `nav::square` now answers Idle for every row on the list and a test says
  so against the fullest install the `Facts` can describe.
- **Standing's page named the wrong absence.** It said Standing was "read from a thread of offers
  and deliveries" and that this build "has no thread, so there is nothing to stand in", while
  `nav::square` said a rung is read off a TRADING PARTNER. A rung is a property of a hand and
  needs no thread, so the screen was inventing its own reason. It now names what is actually
  missing: this build keeps no trading partners, and `grimoire_core::Regard` is a type the engine
  takes as an argument rather than a record the app stores.
- Quests: geo-distance ordering and the log ledger (proven hand-ins) are not built; tracked
  quests are per run, not persisted.
- Kill tracker buckets are per run.
- poSky: the skip recommendation (needs the catalogue ranker) is not built; the achievements
  witness is not read by the Sky model, so a test finished before the log with no reward in the
  dump shows "ready", never "done".
- Race is not a settings field, so every gear derivation runs the no-race path.
- Fires (exaltation) only follow the tailed log, not every character.
- The trio. Until three classes are chosen on CHARACTER / Gear, every score, rank and verdict on
  Gear, Valet and Inventory is priced for the default trio (WAR/CLR/WIZ at 50), and
  each of those screens now says so on the line beside the trio rather than calling it yours.
  There is no first-run prompt for it.
- The footer's source link. D8 words the footer as `v{version} · <licence> · Source on GitHub`; this
  build draws `v0.1.0 · AGPL-3.0-only` and nothing after it, because no `package.repository` is set, the
  checkout's origin is a local path, and on 2026-09-02 neither the James-McMenamin account nor
  the Ragnarok-Systems org has an `eql-grimoire` repository to point at. Round one drew the
  words dimmed with an explaining hover; that was still "GitHub" on a page with no GitHub, and
  the house rule forbids it. Once the repository exists, set `repository` under
  `[workspace.package]` and `repository.workspace = true` in the crate manifest and the D8
  footer appears as a link with no code change (`titlebar::source_link`).
- **Theme switching.** The original's persona footer offers four themes (grimoire, guild, stone,
  system) as swatches. This app has one theme, `theme::install`, and no switch anywhere in it, so
  the swatches are not drawn. They come back with the second theme and `persona.rs` is where they
  belong when they do.
- **Standing.** Nothing in this build computes a regard score, so nothing on the rail claims one:
  the persona footer's seal is CUT, not drawn empty. A rung would come from ratings on completed
  commissions, and nothing here records one. `grimoire_core::Regard` knows the seven rungs and
  their floors and is where the ladder comes from when a score exists; this crate no longer
  restates it or depends on it. CRAFT / Standing says the same thing in words on its own screen.
- **Alerts.** There is no alert system, so the footer's bell is CUT. Round one drew a dim one on
  the argument that an element which vanishes teaches the reader nothing; the answer to that is
  that a bell for a subsystem with no code is not an empty state, and it sat inside the row's one
  click target, so clicking the alert icon opened Settings.
- **The wax seals** are not in this build at all. `src/seal.rs` was deleted: 1,211 lines with zero
  production callers, dead-stripped out of the shipping binary. The design and what has to exist
  before it comes back are under "The seals, and why there are none".
- The `screens::inventory::Section` enum duplicates `ingest::Section` (both reached, both
  labelled the same); one should go.
- A settings edit in a tool window and one in the main window inside the same 100 ms: the tool
  window's wins. Two human edit surfaces; not worth a merge.
- OS-level viewport behaviour (always-on-top actually taking effect, Alt+F4 raising
  `close_requested` on a tool window, the bare second-key globals arriving with the game in
  front) is verified by reading the egui, winit and global-hotkey APIs, not by an interactive
  session. The launches proved: exit 0 and zero panics. The last two, on the fix-round tree at
  16:51, were `GRIMOIRE_SMOKE_MS=4000` at 296 frames and `GRIMOIRE_NO_DATA=1 GRIMOIRE_SMOKE_MS=3000`
  at 127, the second being the path where every screen draws its empty state instead of its data.
  Both ran a binary built immediately before them, checked newer than every file under `src/`.
  Nobody has looked at the screen.
- **The rail's wiring is now covered, and here is what is still not.** Round one shipped with 0
  tests in `main.rs`, so the three lines JOINING the modules (the section dot from
  `nav::section_wants_you`, the badge from `nav::count_of`, the footer's
  `PersonaAction::OpenSettings` mapped to `Body::Settings`) were asserted by nothing, and
  `chrome::section` / `chrome::section_narrow` both ended in TWO ADJACENT BOOLS, so transposing a
  section's open state with its attention dot compiled, linted clean and passed the whole suite.
  Both are fixed: `chrome::SectionHead` makes the swap a thing you have to write down, and
  `rail_plan` decides the rail as a value that 6 tests read. What is STILL true is that the plan is
  tested and the PAINTING of it is not: `App::ui` cannot be constructed in a test, the smoke run
  clicks nothing, so no test and no launch has ever exercised a click on a nav row, a section
  header or the persona footer. The plan being correct and the loop drawing it faithfully are two
  claims, and only the first has evidence.
