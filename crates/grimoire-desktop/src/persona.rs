//! The persona footer: who you are, pinned to the foot of the rail, with Settings tucked into it.
//!
//! The whole block is ONE target that opens Settings, as in the original web build's `.pgfoot`:
//! the person is the last thing in the rail, and the way into settings is the person.
//!
//! WHO YOU ARE comes from the Ragnarok account you signed in with (`crate::account`). Under
//! EverQuest Legends this row read the character off the combat log's file name and drew a
//! faction-standing seal from the crafting engine; Monsters & Memories has no log and that engine
//! is gone, so both went. Nothing here is invented: with no sign-in the row says so, and the
//! hover says where to sign in.
//!
//! THE GEAR CARRIES THE SETTINGS STATE. At rest it takes the colour of `chrome::State` when
//! settings.json would not load or a global hotkey did not register; under the pointer it answers
//! the pointer, and the hover carries the same fact in words, so the warning is never lost.

use crate::chrome::State;
use crate::theme::*;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/// What the footer shows. Every field is filled by the App at its one construction site.
pub struct Persona {
    /// The signed-in Ragnarok account's display name, or None when nobody is signed in.
    pub name: Option<String>,
    /// What the Settings screen would report, from `main::settings_state`.
    pub settings: State,
}

/// What the row asks the app to do. The whole row is one target.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersonaAction {
    None,
    OpenSettings,
}

/// The footer holds no state. Build it as `PersonaFooter`, not `PersonaFooter::default()`
/// (`clippy::default_constructed_unit_structs` is deny under `-D warnings`).
#[derive(Default)]
pub struct PersonaFooter;

/* ----------------------------------------------------------------- the sizes -- */

const ROW_H: f32 = 30.0;
const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 7.0;
/// The gear glyph on the right, which opens Settings.
const GLYPH: f32 = 13.0;
/// Under this width the rail is collapsed and only the gear fits.
const COMPACT_UNDER: f32 = 120.0;
/// The breathing room between the name run and the gear.
const NAME_GAP: f32 = 8.0;

/// EVERY PIXEL THE FOOTER CONSUMES: the hairline, the padding above and below it, and the row.
/// The App reserves exactly this much at the foot of the rail. Held to the drawing by
/// `the_footer_consumes_exactly_height`.
pub const HEIGHT: f32 = 1.0 + PAD_Y + ROW_H + PAD_Y;

/// The height a scrolling list may take when this footer is pinned under it in `ui`.
///
/// The spacing is subtracted because egui charges `item_spacing.y` after the scroll area too; a
/// caller that reserved `HEIGHT` alone would be one spacing short once the list fills its room.
pub fn room_above(ui: &Ui) -> f32 {
    (ui.available_height() - HEIGHT - ui.spacing().item_spacing.y).max(0.0)
}

/// Drop the cursor to exactly `HEIGHT` above the floor, so the footer that follows sits on it.
pub fn push_to_floor(ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let gap = (ui.available_height() - HEIGHT).max(0.0);
    ui.add_space(gap);
}

/* ----------------------------------------------------------------- the words -- */

const NO_NAME: &str = "Not signed in";
const NO_NAME_WHY: &str = "Not signed in to a Ragnarok account. Sign in from the Settings screen \
with Twitch, Google, GitHub or Discord.";

fn who(p: &Persona) -> Option<&str> {
    p.name.as_deref().map(str::trim).filter(|n| !n.is_empty())
}

/// The name and the ink it is set in. The absent case is a statement in the dimmest text, never a
/// placeholder that could be mistaken for a name, and it stays quiet under the pointer.
fn name_line(p: &Persona, hot: bool) -> (&str, Color32) {
    match who(p) {
        Some(n) if hot => (n, GOLD_HI),
        Some(n) => (n, TEXT),
        None => (NO_NAME, TEXT_3),
    }
}

/// At rest the gear is the settings state; under the pointer it is the pointer.
fn gear_ink(settings: State, hot: bool) -> Color32 {
    match (hot, settings) {
        (true, _) => GOLD_HI,
        (false, State::Settled) => TEXT_3,
        (false, st) => st.color(),
    }
}

/// What a settings state that is not settled says, or None when there is nothing to say.
fn fault_words(settings: State) -> Option<&'static str> {
    match settings {
        State::Wrong => {
            Some("The settings file could not be read, so this is running on defaults.")
        }
        State::You => Some("A global hotkey did not register."),
        State::Idle | State::Working | State::Settled => None,
    }
}

/// The sentence every hover on this row ends with.
fn tail(settings: State) -> String {
    match fault_words(settings) {
        Some(w) => format!("{w} Click for Settings."),
        None => "Click for Settings.".to_owned(),
    }
}

/// Whether the rail is collapsed: then only the gear fits and the hover carries the name.
fn compact(width: f32) -> bool {
    width < COMPACT_UNDER
}

/// The row's one tooltip.
fn tip(p: &Persona) -> String {
    let end = tail(p.settings);
    match who(p) {
        Some(n) => format!("Signed in as {n}. {end}"),
        None => format!("{NO_NAME_WHY} {end}"),
    }
}

/* ---------------------------------------------------------------- the layout -- */

/// Every position on the row, computed once; the drawing only paints what this returns.
#[derive(Clone, Copy, Debug)]
struct Slots {
    /// The gear that opens Settings, on the right. Always drawn.
    gear_c: Pos2,
    /// The run the name is drawn in and clipped to. `None` when collapsed.
    name: Option<Rect>,
}

fn slots(rect: Rect, tight: bool) -> Slots {
    let pad = if tight { 8.0 } else { PAD_X };
    let gear_c = Pos2::new(rect.right() - pad - GLYPH * 0.5, rect.center().y);
    if tight {
        return Slots { gear_c, name: None };
    }
    /* Clipped horizontally only, over the row's full height, so a tall glyph keeps its tail. */
    let x0 = rect.left() + pad;
    let x1 = gear_c.x - GLYPH * 0.5 - NAME_GAP;
    let name = Some(Rect::from_min_max(
        Pos2::new(x0, rect.top()),
        Pos2::new(x1.max(x0), rect.bottom()),
    ));
    Slots { gear_c, name }
}

/* --------------------------------------------------------------- the drawing -- */

impl PersonaFooter {
    pub fn ui(&mut self, ui: &mut Ui, p: &Persona) -> PersonaAction {
        /* HEIGHT is a promise: egui charges item_spacing.y after every allocation, so it is
         * zeroed here, where the pieces are known. */
        ui.spacing_mut().item_spacing.y = 0.0;
        let w = ui.available_width();
        let tight = compact(w);

        let (rule_rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
        ui.painter().rect_filled(rule_rect, 0.0, RULE);
        ui.add_space(PAD_Y);

        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, ROW_H), Sense::click());
        let pt = ui.painter().clone();
        let hot = resp.contains_pointer();
        /* PANEL, the same fill a nav row uses under the pointer (PANEL_2 means selected). */
        if hot {
            pt.rect_filled(rect, 0.0, PANEL);
        }

        let s = slots(rect, tight);
        gear_glyph(&pt, s.gear_c, gear_ink(p.settings, hot));
        if let Some(run) = s.name {
            let (label, col) = name_line(p, hot);
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

/// Teeth on the cog. Six: at 13px across, eight teeth and gaps grey into a ring.
const COG_TEETH: usize = 6;

/// The cog that opens Settings: one closed outline whose edge steps in and out (a ring with
/// spokes reads as the brightness icon), and a hub.
fn gear_glyph(pt: &Painter, c: Pos2, col: Color32) {
    let stroke = Stroke::new(1.0, col);
    let r = GLYPH * 0.5;
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
    pt.circle_stroke(c, r * 0.30, stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nobody() -> Persona {
        Persona {
            name: None,
            settings: State::Settled,
        }
    }

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

    #[test]
    fn nobody_signed_in_says_so_and_says_where() {
        let p = nobody();
        assert_eq!(name_line(&p, false), (NO_NAME, TEXT_3));
        assert_eq!(
            name_line(&p, true).0,
            NO_NAME,
            "the absent case stays quiet under the pointer"
        );
        assert!(tip(&p).contains("Sign in from the Settings screen"));
        let blank = Persona {
            name: Some("   ".into()),
            settings: State::Settled,
        };
        assert_eq!(
            name_line(&blank, false).0,
            NO_NAME,
            "a blank name is no name"
        );
    }

    #[test]
    fn a_signed_in_account_is_named() {
        let p = Persona {
            name: Some("Grimtooth".into()),
            settings: State::Settled,
        };
        assert_eq!(name_line(&p, false), ("Grimtooth", TEXT));
        assert_eq!(name_line(&p, true), ("Grimtooth", GOLD_HI));
        assert_eq!(tip(&p), "Signed in as Grimtooth. Click for Settings.");
    }

    #[test]
    fn the_cog_carries_the_settings_state_and_still_answers_the_pointer() {
        assert_eq!(gear_ink(State::Settled, false), TEXT_3);
        assert_eq!(gear_ink(State::Wrong, false), State::Wrong.color());
        assert_eq!(gear_ink(State::You, false), State::You.color());
        for st in [State::Settled, State::Wrong, State::You] {
            assert_eq!(gear_ink(st, true), GOLD_HI);
        }
        let p = Persona {
            name: None,
            settings: State::Wrong,
        };
        assert!(
            tip(&p).contains("could not be read"),
            "the hover carries the fault in words"
        );
    }

    #[test]
    fn the_collapsed_rail_draws_the_gear_and_no_name() {
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(60.0, ROW_H));
        assert!(compact(60.0));
        assert!(slots(r, true).name.is_none());
        let wide = Rect::from_min_size(Pos2::ZERO, Vec2::new(220.0, ROW_H));
        let s = slots(wide, false);
        let run = s.name.expect("a name run when open");
        assert!(
            run.right() < s.gear_c.x - GLYPH * 0.5,
            "the name stops clear of the gear"
        );
    }
}
