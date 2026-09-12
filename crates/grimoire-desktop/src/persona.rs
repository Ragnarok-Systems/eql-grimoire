//! The persona footer: who you are, pinned to the foot of the rail, with Settings tucked into it.
//!
//! THE SHAPE IS THE WEB BUILD'S `.pgfoot`, PORTED. There, the whole block is ONE link to a profile
//! screen: a regard seal, the character's name, and a bell with a count, sitting under a hairline
//! at the bottom of the page rail. It is the pattern the owner named, and the one Discord, Slack
//! and VS Code all landed on independently: the person is the last thing in the rail, and the way
//! into settings is the person, not a nineteenth nav row.
//!
//! THE HARD PART OF THIS FILE IS NOT DRAWING IT. Most of what the original block displays has no
//! data source in this build, and the house rule is that nothing on screen may be invented, and
//! that a control for a feature that does not exist is not drawn. So each element was taken one at
//! a time and asked where its number comes from:
//!
//!   NAME       real, sometimes. `ingest::Ingest::active_character` derives it from the log file
//!              being tailed, `eqlog_<character>_<server>.txt`. When no log has been read there is
//!              no name, and the row says so in plain words and points at the Settings screen. It
//!              never prints the original author's character, which is what the web mock hard
//!              codes into its markup. THIS IS THE ONLY ELEMENT WITH A SOURCE, so it is the only
//!              thing on the row besides the gear that leads to Settings.
//!   STANDING   REAL, and the seal at the head of the row is it. `grimoire_core::Standing` is
//!              not an absence: `Standing::UNPROVEN` is a DEFINED value, 3.0 on zero ratings, and
//!              it is where the engine says every new hand starts. Printing it reports the engine
//!              rather than inventing a number, and that is the whole difference from round one,
//!              which read a `regard` field the App never wrote: "absent" was that seal's only
//!              reachable state and the paint behind every other one was code the binary could not
//!              run. The App now fills `standing` at its one construction site, so the field has a
//!              production writer and the value on screen is the engine's. The one thing the seal
//!              must not do is let an unproven standing read as an earned one; see `seal`.
//!   ALERTS     CUT, for the same reason and one more. There is no alert system, so the bell's
//!              count had no writer either; and the bell sat inside the row's single click target,
//!              so clicking the alert icon opened Settings. The web build hides the bell outright
//!              when the count is zero (`bl.style.display=n?'':'none'`), which on this build is
//!              always.
//!   THEMES     CUT ENTIRELY. The original footer offers four swatches: grimoire, guild, stone,
//!              system. This app has exactly one theme, `theme::install`, and no theme switching
//!              anywhere in it. Four swatches of which three do nothing is the clearest possible
//!              violation of the no invented UI rule.
//!
//! BOTH CUTS ARE THE SAME RULE APPLIED TWICE, and the rule is worth stating once more because the
//! bell was argued for at length in round one and shipped: an empty state belongs on a surface
//! that WILL hold data and does not hold it yet. Alerts have no source in this build and no code
//! that could ever produce one, and three of the four theme swatches have nothing to switch, so
//! those marks are not waiting on data, they are decoration with a tooltip apologising for itself.
//! They come back with the code that computes them, and `Persona` is where their fields go when it
//! does, alongside `every_persona_field_is_filled_by_the_app`, which fails the moment a field is
//! added that the App does not fill.
//!
//! THE SEAL IS THE COUNTER EXAMPLE AND IT IS WORTH KNOWING WHY IT IS NOT A THIRD CUT. The bell
//! would have to invent a count before it could draw anything at all. A standing does not: the
//! engine defines where an unrated hand stands, so there is a real number to print on the first
//! launch on a machine nobody has ever rated. What that leaves is not an absence to apologise for
//! but a distinction to draw, between a standing that was earned and one that is only where you
//! start, and the seal draws it.
//!
//! WHAT IS ON THE ROW. The seal, the name and the gear. The hairline above them, the reserved
//! height and the pinning to the floor are unchanged, because those are about the rail's shape and
//! not about what the row holds.
//!
//! THE GEAR NOW CARRIES THE SETTINGS STATE, AND THAT IS THE ONE THING THIS ROW GAINED. The rail
//! used to end with a Settings ROW under a hairline, a second door to the screen this row already
//! opens. The row is gone; what it CARRIED could not simply go with it. Its state came from
//! `main::settings_state`, which answers `State::Wrong` when settings.json would not load and
//! `State::You` when a global hotkey failed to register, and that was the only place on screen
//! either fact was reported. So the state moved to the one control that still opens Settings: see
//! [`gear_ink`] for the colour and [`fault_words`] for the sentence the hover adds. Deleting the
//! row and the signal together would have removed a warning quietly, which is the failure this
//! whole file is written against.

use crate::chrome::State;
use crate::theme::*;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/* ------------------------------------------------------------------ the data -- */

/// What this build actually knows about the person using it.
///
/// EVERY FIELD HAS A PRODUCTION WRITER, WHICH IS THE ONLY REASON THERE ARE FOUR OF THEM. Round
/// one carried three and the App's only construction site filled one, so two thirds of this
/// struct was `None` on every frame of every launch. The four below are all named at that site.
/// A field with no production writer is a drawing branch the shipped binary cannot enter; see the
/// module note. `every_persona_field_is_filled_by_the_app` holds this struct to the App's
/// construction site so a field cannot arrive ahead of its data again.
///
/// THERE IS NO `Default`, AND ITS ABSENCE IS THE POINT. A derived `Default` is what let round one
/// write `Persona { name: .., ..Persona::default() }` and leave two fields unwritten forever; the
/// test above reads the App's site for a rest pattern, and taking the derive away removes the
/// value that pattern was reaching for. Every construction of this struct, in the App and in the
/// tests alike, now names every field.
#[derive(Clone, Debug)]
pub struct Persona {
    /// The character on the log file being tailed, from `ingest::Ingest::active_character`.
    pub name: Option<String>,
    /// The server that log belongs to, from `ingest::LogFile::server`.
    ///
    /// SEPARATELY OPTIONAL FROM THE NAME, because the two are pulled out of the file name by
    /// different patterns and either can miss. PRINTED VERBATIM: the ingest yields it lower case
    /// and it may carry an instance suffix, "qeynos 1", so title casing it here would be this file
    /// inventing a proper noun. That is the house precedent: the Settings screen's SOURCES ledger
    /// (`settings::SettingsScreen::sources`) prints it the same way.
    pub server: Option<String>,
    /// Where you stand, `grimoire_core::Standing`: a score and the number of ratings behind it.
    ///
    /// NOT AN `Option`, AND THAT IS THE POINT. An unrated hand has a standing, and the engine says
    /// what it is: `Standing::UNPROVEN`, 3.0 on zero ratings. Nothing in this build writes a
    /// rating, so `ratings` is 0 on every launch and the seal draws its unproven face; the value
    /// is still the engine's rather than one this file made up.
    pub standing: grimoire_core::Standing,
    /// What the Settings screen would report, from `main::settings_state`: `State::Wrong` when the
    /// settings file could not be read, `State::You` when a global hotkey did not register,
    /// `State::Settled` when neither is true.
    ///
    /// IT IS HERE BECAUSE THE ROW THAT USED TO CARRY IT IS GONE. The rail ended with a Settings row
    /// under a hairline whose only job this state was; the row was a third door to a screen this
    /// footer and its gear already open, so it went, and the fact it reported came here rather
    /// than being deleted with it. The gear takes the colour ([`gear_ink`]) and the hover takes the
    /// words ([`fault_words`]), so the warning survives the row at both rail widths.
    pub settings: State,
}

/// What the row asks the app to do. The whole row is one target, as in the original.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersonaAction {
    None,
    OpenSettings,
}

/// The footer holds NO state, and that is a finding rather than an oversight. The one stateful
/// behaviour in the original is the bell's ring animation, which is driven by the alert count
/// CHANGING; there is no alert system and no bell, so there is nothing to animate and nothing to
/// remember. The type stays so the integrator has a place to put state the day this row grows
/// something that has any.
///
/// FOR THE INTEGRATOR: while it is a unit struct, build it as `PersonaFooter` and NOT as
/// `PersonaFooter::default()`. The derive is here so a `#[derive(Default)]` on the App struct that
/// holds one keeps working, but an explicit `::default()` call on a unit struct is
/// `clippy::default_constructed_unit_structs`, which is deny under `-D warnings`. That is a warning
/// this lane found by compiling, not by guessing.
#[derive(Default)]
pub struct PersonaFooter;

/* ----------------------------------------------------------------- the sizes -- */

const ROW_H: f32 = 30.0;
const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 7.0;
/// The gear glyph on the right, which opens Settings.
const GLYPH: f32 = 13.0;
/// The regard seal that leads the row. The web's literal, UNSCALED: `.rseal.sm{width:26px}` at
/// web/app.html:102.
///
/// IT IS NOT SCALED TO THE DESKTOP ROW, and that is a decision rather than an oversight. The web
/// profile row is 38px tall against this row's 30, so the proportional seal is 20.5, which is
/// where this constant used to sit. Scaling the seal scales what is inside it, and the number
/// would land at 7.9px of Cinzel; the smallest display face anywhere else in this crate is 10.0.
/// The number is the whole element, so the number is the binding constraint and not the row. At
/// 26.0 the wax reaches 13.2 including its stroke, which leaves 1.8px of air top and bottom inside
/// the row and clears the gear at the collapsed rail width by 9px. Both are held by
/// `nothing_the_row_paints_escapes_the_row`.
const SEAL_D: f32 = 26.0;

/// Between the seal and the name it introduces.
const SEAL_GAP: f32 = 9.0;
/// Under this width the rail is collapsed and only the seal fits. See `compact`.
const COMPACT_UNDER: f32 = 120.0;
/// The breathing room between the name run and the right edge of the row.
const NAME_GAP: f32 = 8.0;

/// EVERY PIXEL THE FOOTER CONSUMES: the hairline, the padding above and below it, and the row.
///
/// The App reserves exactly this much at the foot of the rail so the block is PINNED to the floor
/// rather than floating up under a nav list that does not fill the rail. It is a constant and not
/// a number the caller guesses because the caller cannot see `PAD_Y` or `ROW_H`, and a caller that
/// guesses 44 leaves a one pixel seam that only shows on the one machine nobody tested on. Held to
/// the drawing by `the_footer_consumes_exactly_height`, which measures a real frame.
pub const HEIGHT: f32 = 1.0 + PAD_Y + ROW_H + PAD_Y;

/* ------------------------------------------------------- pinning it to the floor --
 *
 * TWO FUNCTIONS AND NOT A COMMENT TELLING THE APP HOW TO DO IT. Pinning a fixed block under a
 * scrolling list is three steps that have to agree, and every one of them is a place to be three
 * pixels wrong; the App called them in the wrong order once already in review. They live here,
 * beside the height they are protecting and beside the test that drives all three in sequence. */

/// The height a scrolling list may take when this footer is pinned under it in `ui`.
///
/// THE SPACING IS SUBTRACTED AND THAT IS THE WHOLE POINT. egui charges `item_spacing.y` AFTER the
/// scroll area as well as after everything inside it, so a caller that reserves `HEIGHT` alone is
/// exactly one spacing short the moment the list is long enough to fill its room: the footer then
/// starts below the panel floor and loses its bottom padding to the clip. A list SHORTER than its
/// room hides the bug, which is why the test drives the overflowing case too.
pub fn room_above(ui: &Ui) -> f32 {
    (ui.available_height() - HEIGHT - ui.spacing().item_spacing.y).max(0.0)
}

/// Drop the cursor to exactly `HEIGHT` above the floor, so the footer that follows sits on it.
///
/// A scroll area that shrank to a short list leaves the cursor high, and a footer drawn there
/// floats in the middle of the rail attached to nothing. This allocates the difference as blank.
/// The spacing is zeroed first for the same reason [`PersonaFooter::ui`] zeroes it: an `add_space`
/// that is then charged 3px of spacing does not put the cursor where it was asked to.
pub fn push_to_floor(ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let gap = (ui.available_height() - HEIGHT).max(0.0);
    ui.add_space(gap);
}

/* ----------------------------------------------------------------- the words -- */

const NO_NAME: &str = "no character yet";
const NO_NAME_WHY: &str = "no EverQuest log file has been read, so nothing here knows which \
character you are playing. The log folder is set on the Settings screen; the name is taken from \
the file name, eqlog_<character>_<server>.txt.";

/// WHY THE SEAL PRINTS A NUMBER NOBODY GAVE YOU, said in the hover because the dashed ring can
/// only say THAT a standing is unproven and not why the figure under it is 3.0.
///
/// IT IS A CONST AND NOT A BACKSLASH CONTINUATION INSIDE THE `format!`, and that is a scar. It was
/// written as a continuation, indented to sit with the code around it; the backslash did not
/// survive the formatter and fourteen spaces were left sitting in the middle of the hover. Every
/// gate stayed green, because every assertion on that string looked for a phrase that fell wholly
/// on one side of the gap. `the_hover_never_carries_a_run_of_spaces` now guards the SHAPE of every
/// hover this row can produce, which is the assertion that would have caught it.
const UNPROVEN_WHY: &str = "unproven: nobody has rated your work, so this is where a new hand starts and not a score you earned.";

/* ------------------------------------------------------------ the pure rules -- */

/// What the name line prints, and in what colour. The fallback is a statement of absence in the
/// dimmest text, never a placeholder that could be mistaken for a character.
/// The name and the ink it is set in.
///
/// `hot` IS A PARAMETER BECAUSE THIS ROW WAS THE ONLY LABEL IN THE RAIL THAT IGNORED THE POINTER.
/// Every `chrome::nav_row` label goes GOLD_HI under the pointer; this one sat at TEXT no matter
/// what, so the block filled behind a name that never acknowledged it and the row read as
/// decoration rather than as a control. The original gives both the same `color:var(--ink2)` on
/// hover.
fn name_line(p: &Persona, hot: bool) -> (&str, Color32) {
    match who(p) {
        Some(n) if hot => (n, GOLD_HI),
        Some(n) => (n, TEXT),
        /* The absent case stays quiet under the pointer. It is a statement that something is
         * missing, not a label you are invited to read harder. */
        None => (NO_NAME, TEXT_3),
    }
}

/// What the gear is drawn in, given the settings state and whether the pointer is over the row.
///
/// AT REST IT IS THE STATE, UNDER THE POINTER IT IS THE POINTER, AND THAT IS TWO JOBS FOR ONE
/// COLOUR SHARED OUT IN TIME rather than fought over. The gear was `TEXT_3` at rest and `GOLD_HI`
/// hot, and it still is whenever there is no news: `State::Settled` gets no colour of its own,
/// because a mark that is always on is a mark nobody reads. When settings.json would not load or a
/// chord did not register, the resting gear takes that state's colour from `chrome::State::color`,
/// which is the same table the Settings screen paints its own rows from, so the rail and the
/// screen cannot disagree about one fact.
///
/// THE POINTER STILL WINS WHILE IT IS THERE, and the words are what stops that being a loss. A
/// gear that refused to answer the pointer would be the only dead control in the rail; a gear that
/// answered it by erasing a warning would be worse. So hovering swaps the colour for the sentence:
/// `fault_words` puts the same fact in the tooltip that opens under the pointer, and the colour
/// comes back the moment the pointer leaves. Held by
/// `the_cog_carries_the_settings_state_and_still_answers_the_pointer`.
fn gear_ink(settings: State, hot: bool) -> Color32 {
    match (hot, settings) {
        (true, _) => GOLD_HI,
        (false, State::Settled) => TEXT_3,
        (false, st) => st.color(),
    }
}

/// What a settings state that is not settled says, in words, or None when there is nothing to say.
///
/// THE COLOUR ALONE IS NOT ENOUGH AND NEVER WAS. The row this replaced put its state in a 3px bar
/// beside the word "Settings", which said THAT something wanted you and never what; and its
/// `State::Wrong` said nothing at all, because `chrome::nav_row` draws a trailing bar for
/// `State::You` and paints no mark whatsoever for the other four. A red gear with no sentence
/// behind it would repeat that. These are the two states `main::settings_state` can produce
/// besides settled, and both name the fix rather than the symptom.
fn fault_words(settings: State) -> Option<&'static str> {
    match settings {
        State::Wrong => {
            Some("The settings file could not be read, so this is running on defaults.")
        }
        State::You => Some("A global hotkey did not register."),
        State::Idle | State::Working | State::Settled => None,
    }
}

/// The sentence every hover on this row ends with: what is wrong, if anything is, and where the
/// click goes. Built in one place because there are three hover branches and a fault can happen
/// under any of them.
fn tail(settings: State) -> String {
    match fault_words(settings) {
        Some(w) => format!("{w} Click for Settings."),
        None => "Click for Settings.".to_owned(),
    }
}

/// Whether the rail is collapsed. The row's signature takes no width flag, so it reads the one it
/// was given: at `chrome::RAIL_NARROW` there is room for the gear and nothing else, so the name
/// drops out and the row's hover carries it instead.
fn compact(width: f32) -> bool {
    width < COMPACT_UNDER
}

/// What the row's one tooltip says.
///
/// ONE TOOLTIP FOR THE WHOLE ROW, AND THE REGARD SENTENCE IS FOLDED INTO IT rather than given to
/// the seal. The whole block is one click target and one hover; a second hit box over the seal
/// would be a second copy of the layout arithmetic, free to disagree with what was painted, which
/// is the defect the layout note above this file's `slots` was written about.
///
/// THE UNPROVEN WORDING IS THE HONESTY OF THIS ELEMENT. The seal prints 3.0 on a machine nobody
/// has ever rated, and 3.0 in wax reads as a score somebody gave you. The words say it is not:
/// they name the standing, say it is unproven, and say why the number is what it is. The dashed
/// ring says the same thing in the visible layer, because a tooltip nobody hovers has told nobody
/// anything.
///
/// THE SERVER IS PRINTED VERBATIM. The web's string is `${myServer} regards you ${w.toLowerCase()}`
/// (web/app.html:2001), and there `myServer` is a constant. Here it is real, off the log file
/// name, lower case and sometimes carrying an instance suffix. See `Persona::server`.
///
/// THE COUNT IS NOT DECORATION. `grimoire_core::regard` asks for it in so many words: the ratings
/// count is carried so the UI can say "on 3 orders" rather than implying a rung earned over fifty.
///
/// The empty branch says what is missing and where it would come from, which is the empty state
/// rule applied to a hover. With no log there is no you, so there is no standing to report and it
/// says nothing new.
fn tip(p: &Persona) -> String {
    let end = tail(p.settings);
    let Some(n) = who(p) else {
        return format!("{NO_NAME_WHY} {end}");
    };
    let st = p.standing;
    let score = format!("{:.1}", st.score);
    let on = p.server.as_deref().map(str::trim).filter(|s| !s.is_empty());

    if st.ratings == 0 {
        let where_ = match on {
            Some(s) => format!("Standing {score} on {s}"),
            None => format!("Standing {score}"),
        };
        return format!("{n}. {where_}, {UNPROVEN_WHY} {end}");
    }

    let rung = st.regard().as_str().to_lowercase();
    let count = if st.ratings == 1 {
        "1 rating".to_owned()
    } else {
        format!("{} ratings", st.ratings)
    };
    match on {
        Some(s) => format!("{n}. {s} regards you {rung}, on {count}. {end}"),
        None => format!("{n}. You are regarded {rung}, on {count}. {end}"),
    }
}

/* ---------------------------------------------------------------- the layout --
 *
 * EVERY HORIZONTAL POSITION ON THE ROW IS COMPUTED HERE, ONCE, AND THE DRAWING ONLY PAINTS WHAT
 * THIS RETURNS. The first cut of this file did the arithmetic inline inside the painting code, and
 * that shape produced two real defects that a green test suite hid, both found by measuring a real
 * frame rather than by reading: a badge that grew rightward out of the bell and painted through
 * the gear's teeth, and a hit box that did not cover what was painted, so the pointer could be on
 * a number while the tooltip answered for the row. Both pieces are gone now (see the module note),
 * and the shape that caught them stays: when the painting owns the arithmetic, the hit boxes are a
 * second copy of it, and a second copy is free to disagree. */

/// Every position on the row. Absent pieces are `None` rather than off screen rectangles, so a
/// caller cannot hit test something that was never painted.
#[derive(Clone, Copy, Debug)]
struct Slots {
    /// The identity seal that leads the row. Always drawn.
    seal_c: Pos2,
    /// The gear that opens Settings, on the right. Always drawn.
    ///
    /// IT IS A SECOND ROUTE TO ONE ACTION AND THAT IS DELIBERATE. The whole block is clickable
    /// and its hover says so, but a hover is not an affordance you can SEE, and a block whose
    /// only hint that it does anything is a background fill on pointer-over is a block most
    /// people never click. The gear is the visible half of the same button.
    ///
    /// It is also the one place the settings state is reported now that the rail's Settings row
    /// is gone. See [`gear_ink`].
    gear_c: Pos2,
    /// The run the name is drawn in and clipped to. `None` when collapsed.
    name: Option<Rect>,
}

/// Lay the row out.
fn slots(rect: Rect, tight: bool) -> Slots {
    let pad = if tight { 8.0 } else { PAD_X };
    let mid = rect.center().y;
    let seal_c = Pos2::new(rect.left() + pad + SEAL_D * 0.5, mid);
    let gear_c = Pos2::new(rect.right() - pad - GLYPH * 0.5, mid);

    if tight {
        return Slots {
            seal_c,
            gear_c,
            name: None,
        };
    }

    /* The name stops clear of the gear. Clipped HORIZONTALLY ONLY, over the row's full height: a
     * clip that also trimmed the box vertically is what once sliced the tail off Cinzel's Q and
     * made the wordmark read as a different word, because the em does not bound the ink. */
    let x0 = seal_c.x + SEAL_D * 0.5 + SEAL_GAP;
    let x1 = gear_c.x - GLYPH * 0.5 - NAME_GAP;
    let name = Some(Rect::from_min_max(
        Pos2::new(x0, rect.top()),
        Pos2::new(x1.max(x0), rect.bottom()),
    ));

    Slots {
        seal_c,
        gear_c,
        name,
    }
}

/* --------------------------------------------------------------- the drawing -- */

impl PersonaFooter {
    pub fn ui(&mut self, ui: &mut Ui, p: &Persona) -> PersonaAction {
        /* HEIGHT IS A PROMISE AND THIS IS WHAT KEEPS IT. The App reserves exactly `HEIGHT` at the
         * foot of the rail. egui charges `item_spacing.y` after every allocation, and with the
         * default 3px this block would consume 54 rather than 45, overhang the panel and lose its
         * bottom padding to the clip. It is zeroed here and not at the call site because a caller
         * cannot know how many pieces this row allocates. The footer is the last thing drawn in
         * the rail, so nothing after it pays for the change. */
        ui.spacing_mut().item_spacing.y = 0.0;
        let w = ui.available_width();
        let tight = compact(w);

        /* The hairline that closes the rail off above the footer. It runs the full width here and
         * not the width of its contents, which is the opposite of the rule under the wordmark: this
         * one IS a divider, between a list that scrolls and a block that is pinned. */
        let (rule_rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
        ui.painter().rect_filled(rule_rect, 0.0, RULE);
        ui.add_space(PAD_Y);

        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, ROW_H), Sense::click());
        /* cloned so the painter holds no borrow across the `add_space` below */
        let pt = ui.painter().clone();
        /* contains_pointer, not hovered: the fill answers the pointer being anywhere over the block,
         * and hovered can go false while a tooltip of ours is open above it */
        let hot = resp.contains_pointer();
        /* PANEL, THE SAME FILL A NAV ROW USES, AND NOT PANEL_2.
         *
         * This row used PANEL_2, which `chrome::nav_row` reserves for the SELECTED state, so the
         * one block at the foot of the rail lit up harder under the pointer than any row above it
         * and looked selected while merely hovered. The original makes them literally the same
         * rule: `.pg a:hover` and `.pgfoot a:hover` are both
         * `color:var(--ink2); background:rgba(255,255,255,.022)` at app.html:91 and :99. One
         * pointer, one response, wherever it is in the rail. */
        if hot {
            pt.rect_filled(rect, 0.0, PANEL);
        }

        let s = slots(rect, tight);
        seal(&pt, s.seal_c, p);
        gear_glyph(&pt, s.gear_c, gear_ink(p.settings, hot));

        if let Some(run) = s.name {
            let (label, col) = name_line(p, hot);
            /* A name too long for the run is cut at the gear, and the row's hover carries it
             * whole. */
            pt.with_clip_rect(run).text(
                Pos2::new(run.left(), rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(13.0),
                col,
            );
        }

        let resp = resp.on_hover_text(tip(p));
        ui.add_space(PAD_Y);

        if resp.clicked() {
            PersonaAction::OpenSettings
        } else {
            PersonaAction::None
        }
    }
}

/// The gear: a hub and eight teeth. Drawn in the same hand as the pin and the Twitch mark in the
/// title strip, which is why it is strokes on a circle and not a glyph from a font.
/// The gear that opens Settings. Drawn at both rail widths: at the collapsed width it is the only
/// thing with room, and at the open width it is the visible half of a button the whole row shares.
/// Teeth on the cog. Six, not eight: at 13px across, eight teeth and eight gaps land under two
/// pixels apart and grey into a ring.
const COG_TEETH: usize = 6;

/// The cog that opens Settings.
///
/// WHY THIS IS DRAWN TOOTH BY TOOTH AND NOT AS A CIRCLE WITH SPOKES.
///
/// The first cut was `circle_stroke` plus eight line segments radiating outward from it. That is
/// not a cog, it is the universal BRIGHTNESS icon, and the owner read it as one immediately. A sun
/// is rays leaving a disc; a cog is a ring whose EDGE steps in and out. The difference is entirely
/// in whether the outline is continuous, so the outline has to be built as one closed path.
///
/// There is no reference drawing for this. `web/app.html` has no gear, cog or settings icon
/// anywhere; its profile row carries a bell, and the bell was cut here because nothing in this
/// build computes an alert. The cog is ours, added because the owner asked for one, so it gets
/// held to the design's rules rather than to a source: hard edges, one hairline weight, and it
/// answers the pointer with the rest of the row.
fn gear_glyph(pt: &Painter, c: Pos2, col: Color32) {
    let stroke = Stroke::new(1.0, col);
    let r = GLYPH * 0.5;
    /* Tip half-angle narrower than the root half-angle, so a tooth is a trapezoid standing on the
     * ring rather than a rectangle, which is what stops it reading as a spoke. */
    let tip_half = 0.17_f32;
    let root_half = 0.31_f32;
    let r_root = r * 0.70;

    let mut pts: Vec<Pos2> = Vec::with_capacity(COG_TEETH * 4);
    for i in 0..COG_TEETH {
        let a = std::f32::consts::TAU * i as f32 / COG_TEETH as f32;
        for (ang, rad) in [
            (a - root_half, r_root),
            (a - tip_half, r),
            (a + tip_half, r),
            (a + root_half, r_root),
        ] {
            pts.push(Pos2::new(c.x + ang.cos() * rad, c.y + ang.sin() * rad));
        }
    }
    pt.add(egui::Shape::closed_line(pts, stroke));

    /* The hub. A cog without a hole in it is a flower. */
    pt.circle_stroke(c, r * 0.30, stroke);
}

/* --------------------------------------------------------------- the wax edge --
 *
 * WAX NEVER SETS A PERFECT CIRCLE, AND THE WEB SAYS EXACTLY HOW MUCH IT DOES NOT.
 * `.seal{border-radius:49% 51% 47% 53% / 52% 48% 52% 48%}` at web/app.html:291. Every edge pair
 * sums to exactly 100 percent, so no browser down scaling happens and the shape is four quarter
 * ellipse arcs meeting exactly at the midpoints of the box edges. The four rows below are those
 * arcs, in units of the box: centre x, centre y, radius x, radius y, first angle, last angle. The
 * ink spans the box exactly, so a seal on a box of D reaches D/2 and no further.
 *
 * IT REPLACES A SINE WOBBLE THIS FILE USED TO CARRY, which breathed the edge by up to 6.5 percent
 * and took its phase from an FNV hash of the character's name. app.html is the authority on
 * anything visual, and app.html says 3 percent, fixed, with no phase. */
const BLOB: [[f32; 6]; 4] = [
    [0.49, 0.52, 0.49, 0.52, 180.0, 270.0],
    [0.49, 0.48, 0.51, 0.48, 270.0, 360.0],
    [0.53, 0.48, 0.47, 0.52, 0.0, 90.0],
    [0.53, 0.52, 0.53, 0.48, 90.0, 180.0],
];

/// Points sampled per arc, so sixty around the whole edge. At this size that is a step of under
/// half a pixel, which is finer than the dashing below can show as a corner.
const BLOB_STEPS: usize = 15;

/// The wax edge on a box of `box_d`, centred on `c` and tilted `ang` radians about that centre.
///
/// THE BOX IS NOT THE SEAL'S DIAMETER, AND THAT IS THE ONE THING TO GET RIGHT HERE. A CSS border
/// is drawn INSIDE its box; an egui stroke straddles its path. So a caller that wants the web's
/// 1px border on the web's 26px box passes 25.0, and the stroke lands where the browser puts it.
///
/// The pivot is the centre of the BOX, which is what `transform-origin` defaults to, and not the
/// centroid of the blob: the blob's mean vertex sits about a third of a percent to the right of
/// the box centre, and rotating about that instead would shift the whole impression.
fn wax(c: Pos2, box_d: f32, ang: f32) -> Vec<Pos2> {
    let (sa, ca) = (ang.sin(), ang.cos());
    let mut out = Vec::with_capacity(BLOB.len() * BLOB_STEPS);
    for [qx, qy, rx, ry, a0, a1] in BLOB {
        for i in 0..BLOB_STEPS {
            let a = (a0 + (a1 - a0) * i as f32 / BLOB_STEPS as f32).to_radians();
            /* box units, then the offset from the box centre, then scaled and turned */
            let x = (qx + rx * a.cos() - 0.5) * box_d;
            let y = (qy + ry * a.sin() - 0.5) * box_d;
            out.push(Pos2::new(c.x + x * ca - y * sa, c.y + x * sa + y * ca));
        }
    }
    out
}

/// The length of the closed path through `pts`, including the segment that closes it.
///
/// MEASURED AND NOT ASSUMED, because the dash lengths below are absolute lengths: a period that
/// does not divide the path leaves a visible seam where the last gap meets the first dash.
fn perimeter(pts: &[Pos2]) -> f32 {
    (0..pts.len())
        .map(|i| (pts[(i + 1) % pts.len()] - pts[i]).length())
        .sum()
}

/* ------------------------------------------------------------ the regard seal -- */

/// The fixed tilt of the seal, in degrees. `.rseal.sm{transform:rotate(-6deg)}`, web/app.html:102.
///
/// CSS positive rotation is clockwise and egui's `with_angle` is clockwise in y down screen space,
/// so the sign carries across unchanged.
const TILT_DEG: f32 = -6.0;
/// The inner ring's inset from the seal's box. `.rseal.sm::after{inset:3px}`, web/app.html:103.
const RING_INSET: f32 = 3.0;
/// Every ring on this seal is a hairline: `.rseal.sm{border-width:1px}` (web/app.html:102) and
/// `.rseal::after{border:1px solid currentColor}` (web/app.html:576).
const HAIR: f32 = 1.0;
/// The fill: the rung's colour at 9 percent.
/// `background:color-mix(in srgb, currentColor 9%, transparent)` at web/app.html:574, and
/// 0.09 * 255 = 22.95. IT IS SEMI TRANSPARENT OVER THE ROW, not an opaque panel colour, or the
/// tint is lost.
const FILL_A: u8 = 23;
/// The inner ring's alpha. `.rseal.sm::after{opacity:.5}` (web/app.html:103) outranks the base
/// `.42` (web/app.html:576) on specificity, and 0.5 * 255 = 127.5.
const INNER_A: u8 = 128;
/// The score, in the display face. `.rseal.sm s{font:400 10px/1 var(--disp)}` at web/app.html:104,
/// with `--disp` the Cinzel stack at web/app.html:18.
const NUMBER_PT: f32 = 10.0;
/// Dashes around an unproven seal's outer ring, spread evenly over the measured perimeter.
const DASHES: f32 = 16.0;
/// How much of each dash period is ink.
const DASH_DUTY: f32 = 0.6;

/// The character on the row, if there is one worth drawing. A name of pure whitespace is not one.
///
/// ONE ANSWER TO "IS THERE A YOU", read by the name line, the hover and the seal. Three copies of
/// that test would be three chances for the row to press a seal for a character the name line is
/// calling absent in the same breath.
fn who(p: &Persona) -> Option<&str> {
    p.name.as_deref().map(str::trim).filter(|n| !n.is_empty())
}

/// Which colour a rung is drawn in: the web's `.g-*` classes, one per rung. See `theme`.
///
/// EXHAUSTIVE ON THE ENUM ON PURPOSE. Only one arm is reachable while nothing in this build writes
/// a rating, and that is acceptable where ninety lines of speculative paint were not: rendering
/// every variant of an enum is a lookup table. A `_ =>` arm would let a rung added to the engine
/// land silently on a colour that is not its own.
fn rung_colour(r: grimoire_core::Regard) -> Color32 {
    use grimoire_core::Regard as R;
    match r {
        R::Ally => REGARD_ALLY,
        R::Warmly => REGARD_WARMLY,
        R::Kindly => REGARD_KINDLY,
        R::Amiably => REGARD_AMIABLY,
        R::Indifferently => REGARD_INDIFFERENTLY,
        R::Apprehensively => REGARD_APPREHENSIVELY,
        R::Dubiously => REGARD_DUBIOUSLY,
    }
}

/// The regard seal that leads the row: what the server thinks of you, as a number pressed in wax.
///
/// THIS IS THE WEB BUILD'S `badge(ME.rep, "sm")`, PORTED. It is defined at web/app.html:1999 and
/// filled into the same nav slot at :2630 and :3460. Every number in it belongs to that element
/// and was read off the cascade rather than chosen here: a 26px box, a 1px outer ring in the
/// rung's colour, a second ring inset 3px at half alpha, a fill of the same colour at 9 percent,
/// the score to one decimal in the display face at 10px, and a tilt of exactly minus six degrees.
///
/// THE TILT IS FIXED, AND KILLING THE ALTERNATIVE IS PART OF THE JOB. An earlier cut of this file
/// tilted the seal by an FNV hash of the character's name over six angles, on the rule that COLOUR
/// IS STATE and ROTATION IS IDENTITY. That rule is real and the web states it, `.r1` through `.r6`
/// at web/app.html:298. It is not the rule THIS element follows. `.rseal.sm` carries a literal
/// `transform:rotate(-6deg)` at specificity 0,2,0, which outranks
/// `.seal{transform:rotate(var(--rot))}` at 0,1,0, so the `r2` in `badge`'s own class list is inert
/// and there is no per identity rotation on this element at all. A seal that tilted by identity AND
/// coloured by rung would be one mark making two claims, and a reader would have no way to know
/// which of them it was answering.
///
/// UNPROVEN IS DASHED, AND THE DASH IS NOT AN INVENTION EITHER. This design language already has a
/// mark for an act that has not happened: `.s-offer` at web/app.html:301 is commented "an offer is
/// not yet sealed, it's a proposal, so the wax is only outlined" and implements it as
/// `border-style:dashed`. A standing nobody has rated is exactly a standing that has not been
/// struck. NOTHING ELSE CHANGES WITH IT, and the two tempting extras were both refused. The inner
/// ring stays, because `.rseal::after` (web/app.html:575) applies to every regard seal at every
/// size in every state, so it is this seal's anatomy and not a state marker; the double ring that
/// DOES mark a formal act, `.s-done`, belongs to the order seal family and borrowing it across
/// families would invent a rule. The 9 percent fill stays for the same reason. Dimming was the
/// third candidate and it is the worst of them: dimming is this app's ABSENCE vocabulary, the
/// missing name in `TEXT_3` and the hollow idle ring, and 3.0 on zero ratings is not absent, it is
/// defined.
///
/// THE COLOUR NEEDS NO SUPPRESSING. `Regard::UNPROVEN` is `Indifferently`, which is the one rung on
/// the ladder whose colour is a true neutral. See `theme::REGARD_INDIFFERENTLY`.
///
/// THE SEAL DOES NOT ANSWER THE POINTER. `.pgfoot a:hover` (web/app.html:99) recolours the ANCHOR,
/// and the seal span carries its own `.g-*` colour, so the rung colour is untouched by hover. An
/// earlier cut brightened this seal to `GOLD_HI` on pointer over, which here would say the rung had
/// changed. The row's background fill still answers the pointer, and so does the gear.
///
/// WHAT IS DROPPED, SAID OUT LOUD RATHER THAN LEFT OUT. `.seal` also carries
/// `box-shadow:inset 0 1px 0 rgba(255,255,255,.05), inset 0 -3px 8px rgba(0,0,0,.4)` at
/// web/app.html:294. egui has no inset shadow primitive and this crate has no precedent for faking
/// one, so it is not drawn. That is the only thing on this element the port does not carry.
///
/// NO NAME YET IS A HOLLOW RING, already the house's idle mark on the nav rows. With no log read
/// there is no you to hold a standing, so there is nothing to report and nothing is pressed.
fn seal(pt: &Painter, c: Pos2, p: &Persona) {
    if who(p).is_none() {
        pt.circle_stroke(c, SEAL_D * 0.5 * 0.9, Stroke::new(HAIR, IDLE));
        return;
    }

    let st = p.standing;
    let col = rung_colour(st.regard());
    let ang = TILT_DEG.to_radians();
    let tint = |a: u8| Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), a);

    let outer = wax(c, SEAL_D - HAIR, ang);
    let inner = wax(c, SEAL_D - RING_INSET * 2.0 - HAIR, ang);

    /* The wax. Semi transparent, so the hovered row shows through it as it does on the web. */
    pt.add(egui::Shape::convex_polygon(
        outer.clone(),
        tint(FILL_A),
        Stroke::NONE,
    ));

    if st.ratings == 0 {
        /* THE DASH LENGTHS ARE COMPUTED, NOT LITERAL. `dashed_line` takes absolute lengths and does
         * not close the path, so the first point is repeated at the end and the period is divided
         * out of the measured perimeter. A hard coded period would leave a seam at whatever
         * diameter this seal is next drawn at. */
        let mut path = outer.clone();
        path.push(outer[0]);
        let period = perimeter(&outer) / DASHES;
        pt.extend(egui::Shape::dashed_line(
            &path,
            Stroke::new(HAIR, col),
            period * DASH_DUTY,
            period * (1.0 - DASH_DUTY),
        ));
    } else {
        pt.add(egui::Shape::closed_line(outer, Stroke::new(HAIR, col)));
    }

    /* The inner ring, always solid and always drawn: it is the regard seal's anatomy. */
    pt.add(egui::Shape::closed_line(
        inner,
        Stroke::new(HAIR, tint(INNER_A)),
    ));

    /* THE SCORE, WHICH IS THE ELEMENT. One decimal, always, trailing zero kept, from
     * `rep.toFixed(1)` at web/app.html:2002. Never a bare integer and never a dash: the house rule
     * for a badge with nothing in it is that it VANISHES, and this one is never empty.
     *
     * JS `toFixed` rounds half away from zero and Rust `{:.1}` rounds half to even, so the two
     * disagree on an exact .x5 score, 4.25 printing as 4.3 there and 4.2 here. Unreachable while
     * the only score this build holds is 3.0, and noted rather than papered over with a hand
     * rolled rounder.
     *
     * TABULAR FIGURES ARE A NO OP AND THE WEB DOES NOT GET THEM EITHER, so do not "restore" them.
     * `.rseal s{font-variant-numeric:tabular-nums}` (web/app.html:581) asks for a `tnum` feature
     * Cinzel does not ship: its GSUB carries only `locl` and its GPOS only `kern` and `mark`, and
     * its digit advances genuinely vary. egui applies no OpenType features at all, so nothing is
     * lost by not asking for one.
     *
     * Full alpha, because `.rseal.sm s{opacity:1}` (web/app.html:104) overrides the base `.85`
     * (web/app.html:580). Anchored on the box centre and turned with the seal, or the number swings
     * out of the wax as the tilt grows. */
    let galley = pt.layout_no_wrap(
        format!("{:.1}", st.score),
        crate::fonts::display(NUMBER_PT),
        col,
    );
    let at = c - galley.rect.size() * 0.5;
    pt.add(
        egui::epaint::TextShape::new(at, galley, col)
            .with_angle_and_anchor(ang, Align2::CENTER_CENTER),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    use grimoire_core::Standing;

    /* ------------------------------------------------------ driving a real frame --
     *
     * THE SEAL'S TESTS MEASURE PAINT AND NOT ARITHMETIC, and they have to: a seal can compute a
     * perfect rung and then draw the wrong thing, which is exactly how the element this one
     * replaced shipped a character's INITIAL where a score belonged. These helpers run the footer
     * through egui's own context, flatten what came out, and answer questions about it BY COLOUR.
     * Colour is the right key because every piece of the seal is a known tint of one rung colour
     * and nothing else on the row shares it: the gear is `TEXT_3`, `GOLD_HI` or the settings
     * state's own colour, the name is `TEXT`, the hover fill is `PANEL`. */

    /// The row with nothing known on it: no log read, and nothing wrong with the settings.
    ///
    /// THIS IS WHAT `Persona::default()` USED TO BE, and it is a function now because the struct
    /// no longer derives `Default`. That derive was the hole round one's unwritten fields hid in,
    /// so the fixtures name every field for the same reason the App does.
    fn nobody() -> Persona {
        Persona {
            name: None,
            server: None,
            standing: Standing::default(),
            settings: State::Settled,
        }
    }

    /// A persona with every field filled, so a test cannot pass on a default it did not mean.
    fn named(n: &str) -> Persona {
        Persona {
            name: Some(n.to_owned()),
            server: Some("qeynos".to_owned()),
            standing: Standing::UNPROVEN,
            settings: State::Settled,
        }
    }

    /// The same row with something wrong behind the gear.
    fn troubled(n: &str, settings: State) -> Persona {
        Persona {
            settings,
            ..named(n)
        }
    }

    fn rated(score: f64, ratings: u32) -> Standing {
        Standing { score, ratings }
    }

    fn with(n: &str, st: Standing) -> Persona {
        Persona {
            standing: st,
            ..named(n)
        }
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

    /// Every shape the footer paints for `p`, flattened, from a real frame at the open rail width,
    /// with the pointer where `pointer` puts it.
    ///
    /// NOT `__run_test_ui`. The seal presses a number in Cinzel, and a default `Context` carries
    /// egui's own font definitions with no family under that name, so the lookup panics inside
    /// epaint. The real faces have to be installed, for the same reason `chrome`'s frame helper
    /// installs them.
    fn frame(p: &Persona, pointer: Option<Pos2>) -> (Vec<egui::Shape>, Pos2) {
        let w = crate::chrome::RAIL_WIDE;
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        /* WHERE THE ROW LANDED, MEASURED RATHER THAN GUESSED. A test that wants the pointer on the
         * row cannot compute its y from the screen rect: the panel egui wraps this in has a margin
         * of its own. The cursor after `push_to_floor` is where the block starts, and the block is
         * a hairline, PAD_Y, then the row.
         *
         * TWO PASSES, AND THE SECOND IS THE ANSWER. egui decides whether a rect contains the
         * pointer against the layer rects it learned on the PREVIOUS frame, so on a fresh context
         * nothing is under the pointer no matter where the pointer is. One pass is what a hover
         * test silently measures nothing on. */
        let mut row_c = Pos2::ZERO;
        let mut all = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(w, 200.0))),
                events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                ui.set_max_width(w);
                push_to_floor(ui);
                row_c = Pos2::new(
                    ui.max_rect().center().x,
                    ui.cursor().top() + 1.0 + PAD_Y + ROW_H * 0.5,
                );
                let mut foot = PersonaFooter;
                foot.ui(ui, p);
            });
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();
            all = Vec::new();
            for cs in shapes {
                flatten(cs.shape, &mut all);
            }
        }
        (all, row_c)
    }

    fn painted(p: &Persona) -> Vec<egui::Shape> {
        frame(p, None).0
    }

    /// Every point of every STROKED shape painted in exactly `col`. Fills are excluded, so the
    /// wax's own 9 percent tint never answers a question about a ring.
    fn stroked_in(all: &[egui::Shape], col: Color32) -> Vec<Pos2> {
        let mut pts = Vec::new();
        for sh in all {
            match sh {
                egui::Shape::Path(pp) => {
                    if pp.stroke.width > 0.0
                        && matches!(pp.stroke.color, egui::epaint::ColorMode::Solid(c) if c == col)
                    {
                        pts.extend(pp.points.iter().copied());
                    }
                }
                egui::Shape::LineSegment { points, stroke }
                    if stroke.color == col && stroke.width > 0.0 =>
                {
                    pts.extend(points.iter().copied());
                }
                _ => {}
            }
        }
        pts
    }

    /// Stroked PATHS only, in `col`. The gear is line segments, so this asks about wax alone.
    /// The row's midpoint. Everything the SEAL draws sits left of it and everything the COG draws
    /// sits right of it, which is the only thing that still separates them now that both are
    /// stroked outlines: `seal` rings with `closed_line`, and so does `gear_glyph`. Fill cannot
    /// tell them apart because neither ring is filled.
    fn seal_side(width: f32) -> f32 {
        width * 0.5
    }

    /// Stroked outlines in `col`, counting only those on the SEAL's side of the row.
    ///
    /// The side filter is not tidying. The cog is legitimately gold and legitimately brightens
    /// under the pointer, so a caller asking "did the seal draw in gold" over the whole row gets
    /// the cog back and reads it as a seal that broke its own colour rule.
    fn stroked_paths_in(all: &[egui::Shape], col: Color32) -> usize {
        let mid = seal_side(crate::chrome::RAIL_WIDE);
        all.iter()
            .filter(|sh| {
                matches!(sh, egui::Shape::Path(pp)
                    if pp.stroke.width > 0.0
                        && matches!(pp.stroke.color, egui::epaint::ColorMode::Solid(c) if c == col)
                        && pp.points.iter().all(|q| q.x < mid))
            })
            .count()
    }

    /// Every ink the COG was drawn in: the stroke colour of each shape lying wholly on the settings
    /// side of the row. The cog is one closed outline and one hub circle, so a well drawn cog
    /// answers with two colours and they agree.
    ///
    /// The side filter is the same one `stroked_paths_in` uses and for the same reason: the seal is
    /// also an outline, and it is legitimately a colour of its own.
    fn cog_ink(all: &[egui::Shape]) -> Vec<Color32> {
        let mid = seal_side(crate::chrome::RAIL_WIDE);
        let mut out = Vec::new();
        for sh in all {
            match sh {
                egui::Shape::Path(pp)
                    if pp.stroke.width > 0.0 && pp.points.iter().all(|q| q.x > mid) =>
                {
                    if let egui::epaint::ColorMode::Solid(c) = pp.stroke.color {
                        out.push(c);
                    }
                }
                egui::Shape::Circle(cc) if cc.stroke.width > 0.0 && cc.center.x > mid => {
                    out.push(cc.stroke.color)
                }
                _ => {}
            }
        }
        out
    }

    /// A CLOSED stroked ring in `col`, which is what a solid outer ring is and a dash is not.
    fn closed_ring(all: &[egui::Shape], col: Color32) -> bool {
        all.iter().any(|sh| {
            matches!(sh, egui::Shape::Path(pp)
                if pp.closed
                    && pp.stroke.width > 0.0
                    && matches!(pp.stroke.color, egui::epaint::ColorMode::Solid(c) if c == col))
        })
    }

    fn dash_count(all: &[egui::Shape], col: Color32) -> usize {
        all.iter()
            .filter(
                |sh| matches!(sh, egui::Shape::LineSegment { stroke, .. } if stroke.color == col),
            )
            .count()
    }

    /// Every string the row painted, in paint order.
    fn texts(all: &[egui::Shape]) -> Vec<String> {
        all.iter()
            .filter_map(|sh| match sh {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// The angle every text run was turned by.
    fn text_angles(all: &[egui::Shape]) -> Vec<f32> {
        all.iter()
            .filter_map(|sh| match sh {
                egui::Shape::Text(t) => Some(t.angle),
                _ => None,
            })
            .collect()
    }

    /// The mean distance of a point cloud from the centre of its own bounding box.
    ///
    /// MEAN AND NOT MAX, because a dashed ring samples only 60 percent of its own edge and the
    /// vertex that happens to reach furthest can fall in a gap. Over an evenly dashed ring the mean
    /// is the mean of the whole ring, so two rings of the same shape compare by their sizes.
    fn reach(pts: &[Pos2]) -> f32 {
        let mut min = Pos2::new(f32::MAX, f32::MAX);
        let mut max = Pos2::new(f32::MIN, f32::MIN);
        for p in pts {
            min = Pos2::new(min.x.min(p.x), min.y.min(p.y));
            max = Pos2::new(max.x.max(p.x), max.y.max(p.y));
        }
        let c = min + (max - min) * 0.5;
        pts.iter().map(|p| (*p - c).length()).sum::<f32>() / pts.len() as f32
    }

    /// The seven rung colours, so a test can say a rung is painted in ITS colour and in no other.
    const RUNGS: [Color32; 7] = [
        REGARD_ALLY,
        REGARD_WARMLY,
        REGARD_KINDLY,
        REGARD_AMIABLY,
        REGARD_INDIFFERENTLY,
        REGARD_APPREHENSIVELY,
        REGARD_DUBIOUSLY,
    ];

    /* ----------------------------------------------------------- the regard seal -- */

    /// THE SEAL PAINTS THE SCORE, TO ONE DECIMAL, AND THE EXPECTED STRING IS IN THE ASSERTION.
    ///
    /// The defect this pins is the one that sent this lane back: the slot carried the character's
    /// INITIAL, a mark this app invented, where the web build carries `rep.toFixed(1)`. A test that
    /// only asked "is there text inside the wax" would have passed on the letter G. So the wanted
    /// string is spelled out and the failure prints everything the row did paint.
    ///
    /// The trailing zero cases are where a `{score}` that dropped its `:.1` would show, and nowhere
    /// else would.
    #[test]
    fn the_seal_paints_the_score_to_one_decimal() {
        for (st, want) in [
            (Standing::UNPROVEN, "3.0"),
            (rated(4.6, 12), "4.6"),
            (rated(5.0, 3), "5.0"),
            (rated(0.0, 1), "0.0"),
            (rated(1.96, 5), "2.0"),
            (rated(2.34, 8), "2.3"),
        ] {
            let all = painted(&with("Grimtooth", st));
            let got = texts(&all);
            assert!(
                got.iter().any(|t| t == want),
                "a standing of {st:?} should press {want:?} into the wax; the row painted {got:?}"
            );
        }
    }

    /// THE COLOUR IS THE RUNG, AND ONLY THE RUNG.
    ///
    /// One score cannot tell a ladder from a constant, so every rung is driven, and each must paint
    /// in its own colour AND in none of the other six. Without the second half a `rung_colour` that
    /// answered `REGARD_ALLY` for everything would pass the Ally row and be caught nowhere.
    #[test]
    fn the_seal_takes_its_colour_from_the_rung() {
        for (score, want) in [
            (5.00, REGARD_ALLY),
            (4.60, REGARD_WARMLY),
            (4.20, REGARD_KINDLY),
            (3.80, REGARD_AMIABLY),
            (3.00, REGARD_INDIFFERENTLY),
            (2.00, REGARD_APPREHENSIVELY),
            (0.50, REGARD_DUBIOUSLY),
        ] {
            let all = painted(&with("Grimtooth", rated(score, 9)));
            assert!(
                !stroked_in(&all, want).is_empty(),
                "a score of {score} is {:?} and painted nothing in {want:?}",
                grimoire_core::Regard::of(score)
            );
            for other in RUNGS {
                if other != want {
                    assert!(
                        stroked_in(&all, other).is_empty(),
                        "a score of {score} also painted in {other:?}, so the rung is not what \
                         chose the colour"
                    );
                }
            }
        }
    }

    /// THE TILT IS FIXED AT MINUS SIX DEGREES AND IS NOT THE CHARACTER'S.
    ///
    /// This is the test that stops the identity hash coming back, and it asserts twice because
    /// either half alone has a hole. The ANGLE catches a tilt that moved off the web's literal;
    /// none of the six angles the old `SEAL_ANGLES` offered is minus six, so a restored hash fails
    /// it. The identical GEOMETRY catches a hash applied to the rings while the number stayed put.
    #[test]
    fn the_seal_tilts_the_same_for_every_character() {
        let want = TILT_DEG.to_radians();
        let mut first: Option<Vec<Pos2>> = None;
        for n in [
            "Grimtooth",
            "Zek",
            "a",
            "Bristlebane",
            "Firiona",
            "Cazic",
            "Averyverylongcharactername",
        ] {
            let all = painted(&named(n));
            let angles = text_angles(&all);
            assert!(
                angles.iter().any(|a| (a - want).abs() < 1e-6),
                "{n} turned its text by {angles:?} and this seal is fixed at {want}"
            );
            let ring = stroked_in(&all, REGARD_INDIFFERENTLY);
            assert!(!ring.is_empty(), "{n} pressed no seal at all");
            match &first {
                None => first = Some(ring),
                Some(f) => assert_eq!(
                    *f, ring,
                    "{n} pressed a differently shaped seal, so something is reading the name again"
                ),
            }
        }
    }

    /// UNPROVEN IS LEGIBLE AS UNPROVEN, IN THE PAINT AND IN THE WORDS.
    ///
    /// BOTH PERSONAS HERE STAND AT THE SAME RUNG AND THE SAME SCORE, 3.0, so the colour and the
    /// number are identical between them and the ring is the only thing that can tell them apart.
    /// That is deliberate: a test that compared an unproven 3.0 against an earned 4.9 would pass on
    /// the colour alone and prove nothing whatever about the dash.
    #[test]
    fn an_unproven_standing_is_legible_as_unproven() {
        let col = REGARD_INDIFFERENTLY;

        let un = painted(&with("Grimtooth", Standing::UNPROVEN));
        assert!(
            dash_count(&un, col) >= 8,
            "an unproven seal drew {} dashes on its outer ring",
            dash_count(&un, col)
        );
        assert!(
            !closed_ring(&un, col),
            "an unproven seal drew a solid outer ring, so it reads as struck"
        );

        let earned = painted(&with("Grimtooth", rated(3.0, 4)));
        assert!(
            closed_ring(&earned, col),
            "a rated seal drew no solid outer ring"
        );
        assert_eq!(
            dash_count(&earned, col),
            0,
            "a rated seal drew a dashed outer ring, so an earned standing looks unproven"
        );

        let unsaid = tip(&with("Grimtooth", Standing::UNPROVEN));
        assert!(unsaid.contains("Standing 3.0 on qeynos"), "{unsaid}");
        assert!(unsaid.contains("unproven"), "{unsaid}");
        assert!(
            unsaid.contains("not a score you earned"),
            "the hover has to say the number was not earned: {unsaid}"
        );

        let said = tip(&with("Grimtooth", rated(3.0, 4)));
        assert!(
            !said.contains("unproven"),
            "a rated standing was called unproven: {said}"
        );
        assert!(
            said.contains("qeynos regards you indifferently"),
            "the hover has to name the server and the rung: {said}"
        );
        assert!(
            said.contains("on 4 ratings"),
            "the count is what stops a rung earned over four reading like one earned over fifty: \
             {said}"
        );
        assert!(
            tip(&with("Grimtooth", rated(3.0, 1))).contains("on 1 rating."),
            "one rating is not 1 ratings"
        );
    }

    /// NO HOVER THIS ROW CAN PRODUCE CARRIES A RUN OF SPACES.
    ///
    /// THIS IS A REAL DEFECT THIS LANE SHIPPED AND CAUGHT BY READING, NOT BY TESTING. The unproven
    /// sentence was written as a backslash continuation with the second line indented to match the
    /// code around it; rustfmt joined the two lines, dropped the backslash and left fourteen
    /// spaces sitting in the middle of the hover. Every gate stayed green, because every assertion
    /// on that string looked for a phrase that fell entirely on one side of the gap. So the guard
    /// is on the SHAPE of the string rather than on its words, and it covers every branch.
    #[test]
    fn the_hover_never_carries_a_run_of_spaces() {
        let mut rows = vec![nobody(), named("Grimtooth")];
        for st in [Standing::UNPROVEN, rated(3.0, 1), rated(4.9, 7)] {
            rows.push(with("Grimtooth", st));
            rows.push(Persona {
                server: None,
                ..with("Grimtooth", st)
            });
        }
        /* The fault sentence is glued onto the front of the click line, which is a join, and a
         * join is where the defect this test exists for happens. Every settings state a row can
         * hold goes through it, on a named row and on a nameless one, because the nameless branch
         * builds its hover from a different string. */
        for st in [State::Idle, State::Working, State::You, State::Wrong] {
            rows.push(troubled("Grimtooth", st));
            rows.push(Persona {
                settings: st,
                ..nobody()
            });
        }
        for p in rows {
            let t = tip(&p);
            assert!(
                !t.contains("  "),
                "{p:?} hovers with a run of spaces: {t:?}"
            );
            assert!(
                !t.contains(" ,"),
                "{p:?} hovers with a space before a comma: {t:?}"
            );
            assert!(
                !t.contains(
                    "
"
                ),
                "{p:?} hovers across two lines: {t:?}"
            );
        }
    }

    /// A SERVERLESS ROW STILL REPORTS ITS STANDING, and never prints an empty server.
    ///
    /// `server` is `Option` independently of `name`, because the two are pulled out of the log file
    /// name by different patterns and either can miss. A hover reading "regards you" with a hole
    /// where the server was is the invention rule's other failure mode.
    #[test]
    fn the_hover_drops_the_server_rather_than_leaving_a_hole() {
        let no_server = Persona {
            server: None,
            ..named("Grimtooth")
        };
        let t = tip(&no_server);
        assert!(t.contains("Standing 3.0,"), "{t}");
        assert!(!t.contains(" on ,"), "{t}");
        let blank = Persona {
            server: Some("  ".to_owned()),
            ..named("Grimtooth")
        };
        assert_eq!(tip(&blank), t, "a blank server is not a server");

        let earned = Persona {
            server: None,
            ..with("Grimtooth", rated(4.9, 7))
        };
        assert!(
            tip(&earned).contains("You are regarded ally, on 7 ratings."),
            "{}",
            tip(&earned)
        );
    }

    /// TWO RINGS, NOT ONE, IN BOTH STATES.
    ///
    /// `.rseal::after` is the regard seal's own anatomy and applies at every size in every state, so
    /// the inner ring must not vanish with the dash. The RATIO is what stops a second ring drawn at
    /// the same size as the first, which would read as one thick ring and would pass a bare "there
    /// are two paths here" assertion.
    #[test]
    fn the_seal_is_double_ringed_in_both_states() {
        let col = REGARD_INDIFFERENTLY;
        let dim = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), INNER_A);
        let want = (SEAL_D - RING_INSET * 2.0 - HAIR) / (SEAL_D - HAIR);
        for st in [Standing::UNPROVEN, rated(3.0, 4)] {
            let all = painted(&with("Grimtooth", st));
            let outer = stroked_in(&all, col);
            let inner = stroked_in(&all, dim);
            assert!(!outer.is_empty(), "no outer ring at {st:?}");
            assert!(
                !inner.is_empty(),
                "no inner ring at {st:?}; the seal is single ringed"
            );
            let (ro, ri) = (reach(&outer), reach(&inner));
            assert!(
                (ri / ro - want).abs() < 0.03,
                "at {st:?} the rings reach {ri} and {ro}, a ratio of {}, and the two boxes say \
                 {want}",
                ri / ro
            );
        }
    }

    /// THE POINTER DOES NOT CHANGE THE RUNG.
    ///
    /// The seal this replaced brightened from `GOLD` to `GOLD_HI` under the pointer. On a seal
    /// whose colour IS the standing that would say the standing had changed. On the web the hover
    /// recolours the anchor and the seal span carries its own class, so the rung is untouched.
    ///
    /// The first assertion is not decoration: without it a pointer that missed the row entirely
    /// would make every other line here pass while testing nothing.
    #[test]
    fn hovering_the_row_does_not_recolour_the_seal() {
        let p = named("Grimtooth");
        let (cold, row_c) = frame(&p, None);
        let (hot, _) = frame(&p, Some(row_c));

        assert!(
            hot.len() > cold.len(),
            "the hovered frame painted no more than the cold one, so the pointer missed the row"
        );
        assert_eq!(
            stroked_in(&hot, REGARD_INDIFFERENTLY),
            stroked_in(&cold, REGARD_INDIFFERENTLY),
            "hovering moved or recoloured the seal's outer ring"
        );
        for gold in [GOLD, GOLD_HI, FLARE] {
            assert_eq!(
                stroked_paths_in(&hot, gold),
                0,
                "the hovered row drew wax in gold; the rung is the only thing that colours a seal"
            );
        }
    }

    /// THE ROW HOVERS LIKE EVERY OTHER ROW IN THE RAIL, AND NOT LIKE A SELECTED ONE.
    ///
    /// This row filled with PANEL_2 under the pointer. `chrome::nav_row` uses PANEL_2 for SELECTED
    /// and PANEL for hovered, so the one block at the foot of the rail lit up harder than anything
    /// above it and read as selected while merely hovered. The owner noticed before any test did,
    /// which is why this exists: three mutations were run against this file and the hover fill was
    /// the one nothing caught.
    ///
    /// The original makes them the same rule outright: `.pg a:hover` at app.html:91 and
    /// `.pgfoot a:hover` at app.html:99 are both `background:rgba(255,255,255,.022)`.
    #[test]
    fn the_row_hovers_in_the_same_ink_a_nav_row_hovers_in() {
        assert_ne!(
            PANEL, PANEL_2,
            "PANEL and PANEL_2 are the same colour, so this test cannot tell hover from selected"
        );

        let p = named("Grimtooth");
        let (cold, row_c) = frame(&p, None);
        let (hot, _) = frame(&p, Some(row_c));

        let full_row_fill = |all: &[egui::Shape]| -> Option<Color32> {
            all.iter().find_map(|sh| match sh {
                egui::Shape::Rect(r)
                    if r.rect.width() > crate::chrome::RAIL_WIDE * 0.9 && r.rect.height() > 2.0 =>
                {
                    Some(r.fill)
                }
                _ => None,
            })
        };

        let got = full_row_fill(&hot).expect("a hovered row fills its background");
        assert_eq!(
            got, PANEL,
            "the hovered row filled with {got:?}; PANEL_2 is what a SELECTED nav row uses, and this \
             row is only hovered"
        );
        assert!(
            full_row_fill(&cold).is_none_or(|c| c != PANEL),
            "the row paints its hover fill even when the pointer is nowhere near it"
        );
    }

    /// THE COG IS A COG AND NOT A SUN, WHICH IS WHAT IT WAS.
    ///
    /// The first drawing was `circle_stroke` plus eight `line_segment` rays leaving it, and the owner
    /// read it as a brightness icon on sight. The difference between the two is not decoration: a sun
    /// is a disc with SEPARATE strokes flying off it, a cog is ONE CONTINUOUS OUTLINE whose edge steps
    /// in and out. So this asserts the topology rather than the look.
    ///
    /// Scoped to the right half of the row, because the seal on the left is also a closed outline.
    #[test]
    fn the_settings_glyph_is_a_closed_outline_and_not_a_disc_with_rays() {
        let all = painted(&named("Grimtooth"));
        let mid = seal_side(crate::chrome::RAIL_WIDE);

        let rays = all
            .iter()
            .filter(|sh| matches!(sh, egui::Shape::LineSegment { points, .. } if points[0].x > mid))
            .count();
        assert_eq!(
            rays, 0,
            "the settings glyph drew {rays} loose line segments; rays leaving a disc is a BRIGHTNESS \
             icon, which is exactly what this looked like before"
        );

        let outlines: Vec<usize> = all
            .iter()
            .filter_map(|sh| match sh {
                egui::Shape::Path(pp) if pp.points.iter().all(|q| q.x > mid) => {
                    Some(pp.points.len())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            outlines,
            vec![COG_TEETH * 4],
            "expected exactly one closed outline of {} points on the settings side, got {outlines:?}",
            COG_TEETH * 4
        );
    }

    /// THE COG CARRIES THE SETTINGS STATE, WHICH IS WHAT THE DELETED RAIL ROW USED TO CARRY.
    ///
    /// The rail ended with a Settings row under a hairline whose only reason to exist was
    /// `main::settings_state`: red when settings.json would not load, gold when a global hotkey did
    /// not register. The row was a third door to a screen this footer and its gear already open, so
    /// it went. Deleting a row and its signal in one move is how a warning disappears quietly, and
    /// this is the test that says the signal landed somewhere.
    ///
    /// FOUR THINGS ARE ASSERTED AND EACH ONE FAILS A DIFFERENT MISTAKE. That a settled state gets
    /// NO colour (a mark that is always on is a mark nobody reads, and it would also mean the
    /// resting gear had simply been repainted green). That both faults get their own state colour,
    /// so `Wrong` and `You` cannot be confused for each other. That the pointer still answers, on a
    /// broken row as well as a sound one, because a control that stops responding while something
    /// is wrong is the least helpful moment to go dead. And that the row is discriminating at all:
    /// the two frames must differ, or a pointer that missed the row would pass every line here.
    #[test]
    fn the_cog_carries_the_settings_state_and_still_answers_the_pointer() {
        /* The fixture is only discriminating while these four are four colours. */
        for (an, a) in [("TEXT_3", TEXT_3), ("GOLD_HI", GOLD_HI)] {
            for (bn, b) in [("WRONG", WRONG), ("YOU", YOU), ("SETTLED", SETTLED)] {
                assert_ne!(
                    a, b,
                    "{an} and {bn} are the same colour, so this proves nothing"
                );
            }
        }
        assert_ne!(WRONG, YOU, "the two faults must not read alike");

        let fine = named("Grimtooth");
        let (cold, row_c) = frame(&fine, None);
        assert_eq!(
            cog_ink(&cold),
            vec![TEXT_3, TEXT_3],
            "a settled row painted its cog in something other than its resting ink"
        );

        let (hot, _) = frame(&fine, Some(row_c));
        assert!(
            hot.len() > cold.len(),
            "the hovered frame painted no more than the cold one, so the pointer missed the row"
        );
        assert_eq!(
            cog_ink(&hot),
            vec![GOLD_HI, GOLD_HI],
            "the cog stopped answering the pointer"
        );

        for (st, want) in [(State::Wrong, WRONG), (State::You, YOU)] {
            let p = troubled("Grimtooth", st);
            let (cold, row_c) = frame(&p, None);
            assert_eq!(
                cog_ink(&cold),
                vec![want, want],
                "{st:?} settings did not reach the cog; the deleted rail row was the only other \
                 thing that reported it"
            );
            let (hot, _) = frame(&p, Some(row_c));
            assert_eq!(
                cog_ink(&hot),
                vec![GOLD_HI, GOLD_HI],
                "{st:?} settings left the cog dead under the pointer"
            );
            /* The colour is gone while the pointer is there, so the words have to be under it. */
            let words = fault_words(st).expect("a fault this cog paints has words to explain it");
            assert!(
                tip(&p).contains(words),
                "the hover does not say what the cog's colour meant: {}",
                tip(&p)
            );
            assert!(
                !tip(&fine).contains(words),
                "a settled row hovers with a fault it does not have"
            );
        }
    }

    #[test]
    fn the_name_falls_back_to_a_statement_of_absence() {
        let none = nobody();
        assert_eq!(name_line(&none, false), (NO_NAME, TEXT_3));
        let real = named("Grimtooth");
        assert_eq!(name_line(&real, false), ("Grimtooth", TEXT));
        /* a log named `eqlog__server.txt` yields an empty capture, which is not a character */
        let blank = named("   ");
        assert_eq!(name_line(&blank, false), (NO_NAME, TEXT_3));

        /* UNDER THE POINTER, and this half is the point of the parameter. A real name comes up to
         * GOLD_HI exactly as every `chrome::nav_row` label does, and the ABSENT case deliberately
         * does not: it is a statement that something is missing, not a control inviting a click. */
        assert_eq!(name_line(&real, true), ("Grimtooth", GOLD_HI));
        assert_ne!(
            name_line(&real, true).1,
            name_line(&real, false).1,
            "the name is the only label in the rail that ignored the pointer; it must not go back"
        );
        assert_eq!(
            name_line(&none, true),
            (NO_NAME, TEXT_3),
            "an absent name must stay quiet under the pointer"
        );
    }

    /// The web mock hard codes the original author's character into its markup. Porting that string
    /// would put one person's name on the rail of every machine that runs this, which is the
    /// invention rule's worst case: it looks correct and it is a lie. The guard covers the whole
    /// file, comments included, so the name cannot arrive as an example in a doc comment either.
    ///
    /// The needle is spelled with `concat!` so the guard does not trip over itself: written whole
    /// it would be in the file it scans, and the test would fail on its own evidence.
    #[test]
    fn the_mock_character_is_nowhere_in_this_lane() {
        let src = include_str!("persona.rs");
        let mock = concat!("Rev", "iir");
        assert!(
            !src.contains(mock),
            "the web mock's character name is in this file"
        );
    }

    /// EVERY FIELD ON `Persona` HAS A PRODUCTION WRITER, AND THIS IS WHAT SAYS SO.
    ///
    /// The defect this pins, exactly as it shipped: round one's `Persona` carried `regard` and
    /// `alerts` beside `name`, and the App's ONE construction site read
    /// `Persona { name: ..., ..Persona::default() }`, so both were `None` on every frame of every
    /// launch. Everything behind them, the struck seal, the seven rung metal ladder, the alert
    /// badge, the bell's hit box and two of the three tooltip arms, was reachable from this file's
    /// own tests and from nothing else. Neither rustc nor `reach.rs` could see it: `Persona`
    /// derives only `Clone`, `Debug` and `Default`, none of them the masking traits `reach.rs`
    /// walks, and a `pub` field in a library is API by definition.
    ///
    /// So this reads the struct's own field list out of this file and holds the App's construction
    /// site to it: every field must be named there, and the site may not fall back on a rest
    /// pattern. Add a field before the code that fills it and this fails, which is the order the
    /// round one lane got wrong.
    #[test]
    fn every_persona_field_is_filled_by_the_app() {
        let src = include_str!("persona.rs");
        let decl = src
            .split_once("pub struct Persona {")
            .expect("this file declares Persona")
            .1
            .split_once('}')
            .expect("the declaration closes")
            .0;
        let fields: Vec<&str> = decl
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//") && l.contains("pub "))
            .filter_map(|l| l.trim_start_matches("pub ").split(':').next())
            .map(str::trim)
            .collect();
        assert!(!fields.is_empty(), "found no fields to check");

        let app = include_str!("main.rs");
        let site = app
            .split_once("let who = Persona {")
            .expect("the App builds exactly one Persona, and this test needs to find it")
            .1
            .split_once("};")
            .expect("the construction closes")
            .0;
        for f in &fields {
            assert!(
                site.contains(&format!("{f}:")),
                "the App does not fill Persona::{f}; a field with no writer is a drawing branch \
                 the binary cannot enter"
            );
        }
        assert!(
            !site.contains(".."),
            "the App fills Persona with a rest pattern, which hides an unwritten field: {site}"
        );
    }

    /// A NAMELESS ROW DRAWS A HOLLOW RING AND NOTHING ELSE.
    ///
    /// This is the honesty rule made testable, and it survived the seal being replaced. With no log
    /// read there is no you, so there is no standing to report: an impression pressed on an empty
    /// persona would be a score for a character that does not exist.
    #[test]
    fn a_row_with_no_character_presses_no_seal() {
        for (p, want_ring) in [
            (nobody(), true),
            (named("   "), true),
            (named("Grimtooth"), false),
        ] {
            let all = painted(&p);
            /* THE WAX IS THE FILLED BODY, and that is now the only safe way to find it.
             *
             * This used to look for any many sided path, on the stated grounds that "the gear is
             * circles and line segments". That stopped being true the moment the gear became an
             * actual cog: a cog is one closed 24 point outline, so it answered to the old test as
             * wax and both of this row's seal tests went red on a change that never touched the
             * seal.
             *
             * The distinction that actually holds is FILL. `seal` lays its body down with
             * `convex_polygon` and a tier tint; its two rings and the cog are all `closed_line`
             * with a transparent fill. A filled many sided path on this row is wax and nothing
             * else. */
            let wax = all.iter().any(|sh| {
                matches!(sh, egui::Shape::Path(pp)
                    if pp.points.len() > 12 && pp.fill != Color32::TRANSPARENT)
            });
            assert_eq!(
                wax, !want_ring,
                "persona {:?} drew wax={wax} when it should have been {}",
                p.name, !want_ring
            );
        }
    }

    #[test]
    fn the_rail_widths_fall_on_opposite_sides_of_the_compact_threshold() {
        assert!(
            compact(crate::chrome::RAIL_NARROW),
            "the collapsed rail must drop the name"
        );
        assert!(
            !compact(crate::chrome::RAIL_WIDE),
            "the open rail must show it"
        );
    }

    /* ------------------------------------------------------------- the layout --
     *
     * The first cut of this file had one drawing test, "it does not panic", and a row can paint
     * one element straight through another without panicking. These measure the rectangles. */

    /// A row at the open rail width.
    fn open_row() -> (Rect, Slots) {
        let rect = Rect::from_min_size(
            Pos2::new(0.0, 0.0),
            Vec2::new(crate::chrome::RAIL_WIDE, ROW_H),
        );
        (rect, slots(rect, false))
    }

    /// The name run starts clear of the SEAL and stays inside the row.
    ///
    /// It used to stop clear of a trailing GEAR, and the gear is gone: it was a second button for
    /// the one action the whole block already performs, so at the open width the person IS the
    /// button and the row keeps the width the gear was taking. The collision that mattered moved to
    /// the other end, where a name overrunning leftward would be painted through the wax.
    #[test]
    fn the_name_run_sits_between_the_seal_and_the_gear() {
        let (_, s) = open_row();
        let run = s.name.expect("an open rail draws a name");
        let seal_right = s.seal_c.x + SEAL_D * 0.5;
        assert!(
            run.left() >= seal_right,
            "the name starts at {} and the seal ends at {seal_right}",
            run.left()
        );
        let gear_left = s.gear_c.x - GLYPH * 0.5;
        assert!(
            run.right() <= gear_left,
            "the name runs to {} and the gear starts at {gear_left}",
            run.right()
        );
        assert!(
            run.width() > 0.0,
            "the name run collapsed to nothing at the open rail width"
        );
    }

    /// The collapsed rail draws nothing it has no room for, and keeps the one thing it does draw
    /// inside the rail.
    #[test]
    fn the_collapsed_row_drops_the_name_and_keeps_both_glyphs_on_the_rail() {
        let rect = Rect::from_min_size(
            Pos2::new(0.0, 0.0),
            Vec2::new(crate::chrome::RAIL_NARROW, ROW_H),
        );
        let s = slots(rect, true);
        assert!(
            s.name.is_none(),
            "the collapsed rail has no room for a name"
        );
        assert!(
            s.seal_c.x - SEAL_D * 0.5 >= rect.left(),
            "the seal hangs off the left"
        );
        assert!(
            s.seal_c.x + SEAL_D * 0.5 <= rect.right(),
            "the seal hangs off the right"
        );
        assert!(
            s.gear_c.x + GLYPH * 0.5 <= rect.right(),
            "the gear hangs off the right"
        );
        /* The collapsed rail is 64px and now carries two glyphs. They must not touch. */
        assert!(
            s.gear_c.x - GLYPH * 0.5 > s.seal_c.x + SEAL_D * 0.5,
            "the gear at {} overlaps the seal ending at {}",
            s.gear_c.x - GLYPH * 0.5,
            s.seal_c.x + SEAL_D * 0.5
        );
    }

    /// Everything the row paints stays inside the row it was given, at both widths. A piece that
    /// escapes vertically is clipped by the panel and reads as a bug.
    #[test]
    fn nothing_the_row_paints_escapes_the_row() {
        for (w, tight) in [
            (crate::chrome::RAIL_NARROW, true),
            (crate::chrome::RAIL_WIDE, false),
        ] {
            let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(w, ROW_H));
            let s = slots(rect, tight);
            if let Some(b) = s.name {
                assert!(
                    rect.contains_rect(b),
                    "{b:?} escapes the row {rect:?} at width {w}"
                );
            }
            /* THE WHOLE DISC, NOT JUST THE CENTRE. A centre inside a 30px row says nothing about a
             * 26px seal drawn around it. The wax edge spans its box exactly, so the true reach is
             * SEAL_D/2 less the half hairline the stroke is inset by, 12.65 plus 0.5; the 1.07 here
             * was sized for a sine wobble that is gone and is now plain headroom. It is kept
             * because it is the tighter bound, and a seal that grew past it would be caught here
             * rather than by eye. */
            let reach = SEAL_D * 0.5 * 1.07;
            assert!(
                s.seal_c.y - reach >= rect.top() && s.seal_c.y + reach <= rect.bottom(),
                "the seal escapes the row vertically at width {w}"
            );
            assert!(
                s.seal_c.x - reach >= rect.left() && s.seal_c.x + reach <= rect.right(),
                "the seal escapes the row horizontally at width {w}"
            );
            assert!(
                rect.contains(s.gear_c),
                "the gear centre is outside the row at width {w}"
            );
        }
    }

    #[test]
    fn the_empty_hover_says_what_is_missing_and_where_it_would_come_from() {
        let name = tip(&nobody());
        assert!(name.contains("log"), "{name}");
        assert!(name.contains("folder"), "{name}");
        assert!(name.contains("Settings"), "{name}");
    }

    #[test]
    fn a_known_persona_hovers_with_its_own_facts() {
        assert!(tip(&named("Grimtooth")).starts_with("Grimtooth"));
    }

    /// The house forbids both dashes in code, comments and UI strings alike, and a hover string is
    /// a UI string. Checked over the whole file so a comment cannot carry one in either.
    #[test]
    fn no_dashes_anywhere_in_this_file() {
        let src = include_str!("persona.rs");
        for (i, line) in src.lines().enumerate() {
            assert!(!line.contains('\u{2014}'), "em dash on line {}", i + 1);
            assert!(!line.contains('\u{2013}'), "en dash on line {}", i + 1);
        }
    }

    /// The drawing itself, run through egui's test context at both rail widths and in every state
    /// the row has. It proves the row lays out, clips and paints without panicking, and that a row
    /// nobody clicked asks for nothing.
    #[test]
    fn the_row_draws_at_both_widths_and_asks_for_nothing_unclicked() {
        for w in [crate::chrome::RAIL_NARROW, crate::chrome::RAIL_WIDE] {
            for p in [
                nobody(),
                named("Grimtooth"),
                named("Averyverylongcharactername"),
                named("   "),
                with("Grimtooth", rated(4.9, 31)),
                Persona {
                    server: None,
                    ..named("Grimtooth")
                },
                troubled("Grimtooth", State::Wrong),
                troubled("Grimtooth", State::You),
            ] {
                let mut foot = PersonaFooter;
                let mut got = PersonaAction::OpenSettings;
                /* NOT `__run_test_ui`. The seal presses the character's initial in Cinzel, and a
                 * default Context carries egui's own font definitions with no family under that
                 * name, so the lookup panics inside epaint. The real faces have to be installed
                 * for the same reason `chrome`'s frame helper installs them. */
                let ctx = egui::Context::default();
                crate::fonts::install(&ctx);
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(w, 200.0))),
                    ..Default::default()
                };
                let out = ctx.run_ui(input, |ui| {
                    ui.set_max_width(w);
                    got = foot.ui(ui, &p);
                });
                out.drop_without_applying_deltas();
                assert_eq!(got, PersonaAction::None, "at width {w}");
            }
        }
    }

    /// HEIGHT IS WHAT THE APP RESERVES, SO IT IS MEASURED AND NOT ASSERTED FROM ARITHMETIC.
    ///
    /// The App takes `HEIGHT` off the rail before it sizes the scrolling nav list, so if the row
    /// consumes one pixel more than the constant the footer hangs past the panel floor and the
    /// bottom padding is clipped away. This walks a real frame and measures the cursor, which is
    /// the only thing that knows what egui charged: the first cut of this footer paid
    /// `item_spacing.y` after each of its four allocations and consumed 54 against a constant of
    /// 45. Run at both rail widths because the compact branch allocates a different set of pieces.
    #[test]
    fn the_footer_consumes_exactly_height() {
        for w in [crate::chrome::RAIL_NARROW, crate::chrome::RAIL_WIDE] {
            let mut used = 0.0f32;
            egui::__run_test_ui(|ui| {
                ui.set_max_width(w);
                let before = ui.cursor().top();
                let mut foot = PersonaFooter;
                foot.ui(ui, &nobody());
                used = ui.cursor().top() - before;
            });
            assert!(
                (used - HEIGHT).abs() < 0.01,
                "the footer consumed {used} at width {w}, and the App reserves {HEIGHT}"
            );
        }
    }

    /// THE PINNING, DRIVEN AS THE APP DRIVES IT: `room_above`, then the list, then
    /// `push_to_floor`, then the footer. The three calls in that order ARE the contract, so the
    /// test makes them rather than re-deriving the arithmetic beside them, which is how a test
    /// ends up agreeing with itself instead of with the code.
    ///
    /// BOTH CASES, AND THE SHORT ONE IS NOT THE INTERESTING ONE. A list that overflows its room is
    /// what exposes the spacing egui charges after the scroll area; a list that shrinks is what
    /// exposes a missing `push_to_floor`. Each case passes with the other's bug still in.
    #[test]
    fn the_footer_lands_on_the_floor_whether_the_list_overflows_or_not() {
        for rows in [2usize, 400] {
            let ctx = egui::Context::default();
            crate::fonts::install(&ctx);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(crate::chrome::RAIL_WIDE, 480.0),
                )),
                ..Default::default()
            };
            let mut floor = 0.0f32;
            let mut top = 0.0f32;
            let out = ctx.run_ui(input, |ui| {
                floor = ui.max_rect().bottom();
                let room = room_above(ui);
                egui::ScrollArea::vertical()
                    .max_height(room)
                    .show(ui, |ui| {
                        for _ in 0..rows {
                            ui.allocate_exact_size(Vec2::new(10.0, 20.0), Sense::hover());
                        }
                    });
                push_to_floor(ui);
                top = ui.cursor().top();
                let mut foot = PersonaFooter;
                foot.ui(ui, &nobody());
            });
            out.drop_without_applying_deltas();
            assert!(
                (top + HEIGHT - floor).abs() < 0.01,
                "{rows} rows: the footer runs {top} to {}, and the rail floor is {floor}",
                top + HEIGHT
            );
        }
    }
}
