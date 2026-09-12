//! The Broken Stoic palette, and the one rule it is built on.
//!
//! The reference is the Broken Stoic title card: a black ground, a wordmark cut from polished gold
//! that runs bright at the top edge and deep bronze in the shadow, and a warm white flare at the
//! centre. Two things follow from that and they are the whole theme.
//!
//! GOLD IS A RAMP, NOT A COLOUR. A single flat `#C89B3C` reads as mustard, because what makes metal
//! look like metal is the distance between its highlight and its shadow, not its midpoint. So gold
//! here is five stops and anything drawn in it picks the stop that matches how much light that
//! element should be catching. Brand catches the most, a resting nav row catches almost none.
//!
//! THE GROUND IS NEAR BLACK, NOT BLACK. `#000000` leaves nothing below the darkest surface, so a
//! sunken panel has no way to read as sunken and every border has to be drawn rather than felt.
//! `INK` is a hair above black precisely so `SUNK` can go under it.

use egui::Color32;

/* ---------------------------------------------------------------- the ground -- */

/// The page. A hair above black so `SUNK` has somewhere to go.
pub const INK: Color32 = Color32::from_rgb(0x07, 0x09, 0x0D);
/// A panel sitting on the page: the rail, a card.
pub const PANEL: Color32 = Color32::from_rgb(0x0E, 0x12, 0x18);
/// A panel one step up, hovered or selected.
pub const PANEL_2: Color32 = Color32::from_rgb(0x13, 0x18, 0x20);
/// Below the page. Wells, inputs, the log.
pub const SUNK: Color32 = Color32::from_rgb(0x05, 0x04, 0x07);
/// Hairlines. Never a full stop lighter, or the grid reads as a table.
pub const RULE: Color32 = Color32::from_rgb(0x2A, 0x24, 0x2E);

/* ------------------------------------------------------------------ the gold -- */

/// The flare at the centre of the wordmark. Reserve it: this is the brightest thing on screen.
pub const FLARE: Color32 = Color32::from_rgb(0xFF, 0xF6, 0xDC);
/// The top edge of a polished letter.
pub const GOLD_HI: Color32 = Color32::from_rgb(0xFF, 0xD7, 0x7F);
/// The body of the metal. Headings, active nav, anything that should read as gold at a glance.
pub const GOLD: Color32 = Color32::from_rgb(0xF2, 0xB9, 0x4F);
/// Turning away from the light. Secondary labels, section headers.
pub const GOLD_DIM: Color32 = Color32::from_rgb(0x8A, 0x65, 0x20);
/// In shadow. Rules and dividers that should belong to the metal rather than the ground.
pub const GOLD_DEEP: Color32 = Color32::from_rgb(0x3A, 0x2A, 0x0C);

/* ------------------------------------------------------------------ the text -- */

pub const TEXT: Color32 = Color32::from_rgb(0xEE, 0xF1, 0xF5);
pub const TEXT_2: Color32 = Color32::from_rgb(0x92, 0x9A, 0xA8);
pub const TEXT_3: Color32 = Color32::from_rgb(0x68, 0x71, 0x80);

/* ----------------------------------------------------------- the five states --
 * Ported from the Gnomish console, which is where this chrome comes from. Nothing else in the app
 * may carry these colours, because the moment a decorative element borrows `WRONG` red, a red row
 * stops meaning a refusal. */

pub const IDLE: Color32 = Color32::from_rgb(0x6A, 0x63, 0x5A);
pub const WORKING: Color32 = Color32::from_rgb(0x4C, 0x91, 0xFF);
pub const YOU: Color32 = Color32::from_rgb(0xD4, 0xA3, 0x3C);
pub const WRONG: Color32 = Color32::from_rgb(0xEF, 0x47, 0x6F);
pub const SETTLED: Color32 = Color32::from_rgb(0x3D, 0xD5, 0x98);

/* --------------------------------------------------------- the regard ladder --
 * The seven rungs of the EverQuest faction ladder, in the colours the WEB BUILD gives them:
 * `.g-ally` through `.g-dub` at web/app.html:923 and :924, which is the authority for anything
 * visual. Taken verbatim, one per rung, so the seal on the desktop rail and the badge on the web
 * page cannot drift apart. The rungs and their floors are `grimoire_core::Regard`, and the two
 * ladders already agree floor for floor and word for word.
 *
 * THESE ARE NOT STATE COLOURS AND A STATE COLOUR IS NOT A RUNG. The five above are semantic:
 * borrowing `WRONG` red for the bottom rung would make a poor reputation read as a refusal, and
 * lending a rung colour to a status mark would do the same in reverse. The ladder already runs
 * green through neutral to red on its own and needs nothing from that set.
 * `no_rung_borrows_a_state_colour` holds the two apart. */

/// Ally, the top rung, floor 4.85. From `.g-ally{color:#63d36e}`, web/app.html:923.
pub const REGARD_ALLY: Color32 = Color32::from_rgb(0x63, 0xD3, 0x6E);
/// Warmly, floor 4.55. From `.g-warm{color:#8fce6a}`, web/app.html:923.
pub const REGARD_WARMLY: Color32 = Color32::from_rgb(0x8F, 0xCE, 0x6A);
/// Kindly, floor 4.15. From `.g-kind{color:#cbb96a}`, web/app.html:923.
pub const REGARD_KINDLY: Color32 = Color32::from_rgb(0xCB, 0xB9, 0x6A);
/// Amiably, floor 3.70. From `.g-ami{color:#c39a5e}`, web/app.html:924.
pub const REGARD_AMIABLY: Color32 = Color32::from_rgb(0xC3, 0x9A, 0x5E);
/// Indifferently, floor 2.60. From `.g-ind{color:#8c8c8c}`, web/app.html:924.
///
/// THIS IS ALSO `grimoire_core::Regard::UNPROVEN`, WHERE EVERY NEW HAND STARTS, AND IT IS THE ONE
/// RUNG ON THE LADDER WITH NO COLOUR AT ALL. That is convenient rather than accidental, and worth
/// saying because the persona footer leans on it: the seal a machine draws before anybody has been
/// rated is already a true neutral, so nothing has to be suppressed to keep an unproven standing
/// from looking like an earned one.
pub const REGARD_INDIFFERENTLY: Color32 = Color32::from_rgb(0x8C, 0x8C, 0x8C);
/// Apprehensively, floor 1.60. From `.g-app{color:#c98a5e}`, web/app.html:924.
pub const REGARD_APPREHENSIVELY: Color32 = Color32::from_rgb(0xC9, 0x8A, 0x5E);
/// Dubiously, the bottom rung, floor 0.00. From `.g-dub{color:#c4726a}`, web/app.html:924.
pub const REGARD_DUBIOUSLY: Color32 = Color32::from_rgb(0xC4, 0x72, 0x6A);

/* ----------------------------------------------------------------- the metal --
 * A vertical gold ramp, sampled. `t` runs 0.0 at the top edge of a glyph to 1.0 at its foot.
 *
 * FIVE STOPS AND NOT TWO. Two stops plus a lerp gives a flat wash that reads as a gradient rather
 * than as metal: real polished gold turns over sharply near the top, sits broad through the middle,
 * and drops fast into shadow. The stop positions below are that curve, and they are why the brand
 * looks struck rather than tinted. */
pub fn metal(t: f32) -> Color32 {
    const STOPS: [(f32, Color32); 5] = [
        (0.00, GOLD_HI),
        (0.22, FLARE),
        (0.48, GOLD),
        (0.78, GOLD_DIM),
        (1.00, GOLD_DEEP),
    ];
    let t = t.clamp(0.0, 1.0);
    for w in STOPS.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let k = if (t1 - t0).abs() < f32::EPSILON {
                0.0
            } else {
                (t - t0) / (t1 - t0)
            };
            return lerp(c0, c1, k);
        }
    }
    GOLD_DEEP
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Apply the palette to egui's own widget visuals, so stock widgets stop looking stock.
pub fn install(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = INK;
    v.window_fill = PANEL;
    v.extreme_bg_color = SUNK;
    v.faint_bg_color = PANEL_2;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, RULE);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT_2);
    v.widgets.inactive.bg_fill = PANEL;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = PANEL_2;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, GOLD_DEEP);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, GOLD_HI);
    v.widgets.active.bg_fill = PANEL_2;
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, GOLD_DIM);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, FLARE);
    v.selection.bg_fill = GOLD_DEEP;
    v.selection.stroke = egui::Stroke::new(1.0, GOLD_HI);
    /* The open state: a combo box's button while its list is down. egui's default is a lighter
     * grey box; it takes the same palette as an active widget so the one moment a control is
     * open does not turn stock. */
    v.widgets.open.bg_fill = PANEL_2;
    v.widgets.open.weak_bg_fill = PANEL_2;
    v.widgets.open.bg_stroke = egui::Stroke::new(1.0, GOLD_DIM);
    v.widgets.open.fg_stroke = egui::Stroke::new(1.0, FLARE);
    /* Square, not rounded, EVERYWHERE. The reference wordmark is cut with hard angular facets and
     * a 6px radius on every panel fights that immediately. The pill in the title strip is the one
     * round shape on screen (titlebar.rs) and it earns that by being the only one. So every
     * surface egui rounds on its own is zeroed here: the four widget states, the open state, the
     * popup frame every tooltip and combo list draws through (`menu_corner_radius`), and window
     * frames (`window_corner_radius`). Those popups also take the palette's hairline instead of
     * egui's grey stroke, so a tooltip belongs to the same page as the thing it explains. */
    v.widgets.noninteractive.corner_radius = egui::CornerRadius::ZERO;
    v.widgets.inactive.corner_radius = egui::CornerRadius::ZERO;
    v.widgets.hovered.corner_radius = egui::CornerRadius::ZERO;
    v.widgets.active.corner_radius = egui::CornerRadius::ZERO;
    v.widgets.open.corner_radius = egui::CornerRadius::ZERO;
    v.menu_corner_radius = egui::CornerRadius::ZERO;
    v.window_corner_radius = egui::CornerRadius::ZERO;
    v.window_stroke = egui::Stroke::new(1.0, RULE);

    /* THE LIGHT COMES FROM STRAIGHT ABOVE, AND EGUI'S DOES NOT.
     *
     * This block is the one directly above's twin and was missed the first time. The corner radii
     * were zeroed everywhere including the popup frame, and the popups were given the palette's
     * hairline, and then egui's DEFAULT SHADOWS were left in place underneath all of it:
     *   popup_shadow  offset [6, 10]  blur 8
     *   window_shadow offset [10, 20] blur 15
     * Both are offset to the RIGHT, so every tooltip and every combo list dropped a soft black
     * shadow down and to the side of a hard square box. It only appeared while a popup was open,
     * which is why it read as an intermittent glitch rather than as a style.
     *
     * `web/app.html` carries 23 outer box-shadows and EVERY ONE OF THEM begins with a bare `0`.
     * Not one has a horizontal offset. The product is lit from directly overhead, so a shadow
     * falls straight down and never sideways. That is the rule, and it is measured, not guessed.
     *
     * The value is the original's own: `.roll` at app.html:143 floats on
     * `0 4px 12px rgba(0,0,0,.55)`, which is the closest thing the design has to a small element
     * hovering over the page. .55 * 255 = 140.
     *
     * NOT `Shadow::NONE`. The design does use shadows; what it does not do is throw them
     * sideways. Removing them outright would fix the symptom by deleting the feature.
     *
     * `window_shadow` IS DELIBERATELY NOT SET. It only applies to `egui::Window`, and this app
     * draws none: the overlays are real OS viewports (`windows.rs`, whose `Window` is a local
     * struct for viewport state, not egui's), and everything else is a Panel. Setting it would be
     * configuration nothing reads, which is the same defect as code nothing calls. If an
     * `egui::Window` is ever added, it needs a vertical shadow too and app.html:380
     * (`0 26px 60px -22px #000`) is the value to port. Note that egui cannot express that -22px:
     * `Shadow::spread` is `u8`, so a negative spread has to be approximated by pulling the blur in. */
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };

    ctx.set_visuals(v);
}

#[cfg(test)]
mod shadow_tests {
    use super::*;

    /// A POPUP SHADOW FALLS STRAIGHT DOWN, NEVER SIDEWAYS.
    ///
    /// egui ships `popup_shadow` at offset [6, 10], so every tooltip and combo list in this app
    /// dropped a soft black shadow down AND to the right of a hard square box. It only showed while
    /// a popup was open, which is why it read as an intermittent glitch rather than as a style.
    ///
    /// `web/app.html` carries 23 outer box-shadows and every one begins with a bare `0`. The product
    /// is lit from directly overhead. This asserts the horizontal offset is zero, which is the whole
    /// defect, and asserts the shadow still EXISTS, so a later fix that reaches for `Shadow::NONE`
    /// and deletes the feature fails here too.
    #[test]
    fn a_popup_shadow_falls_straight_down_and_still_exists() {
        let ctx = egui::Context::default();
        install(&ctx);
        let sh = ctx.global_style().visuals.popup_shadow;

        assert_eq!(
            sh.offset[0], 0,
            "the popup shadow is thrown {}px sideways; every one of app.html's 23 outer shadows \
             starts at 0 because the design is lit from directly above",
            sh.offset[0]
        );
        assert!(
            sh.offset[1] > 0,
            "the shadow does not fall downward at all, so nothing reads as floating"
        );
        assert!(
            sh.color.a() > 0 && sh.blur > 0,
            "the shadow was deleted rather than corrected; the design DOES use shadows, it just \
             does not throw them sideways"
        );
    }

    /// egui's default is what this file exists to overrule, so pin that it IS being overruled.
    /// Without this, deleting the assignment leaves the test above passing on egui's own value if
    /// egui ever changes its default to a vertical one, and the override would rot away unnoticed.
    #[test]
    fn the_popup_shadow_is_ours_and_not_whatever_egui_ships() {
        let stock = egui::Visuals::dark().popup_shadow;
        let ctx = egui::Context::default();
        install(&ctx);
        let ours = ctx.global_style().visuals.popup_shadow;
        assert_ne!(
            (ours.offset, ours.blur, ours.color),
            (stock.offset, stock.blur, stock.color),
            "the theme is not overriding popup_shadow at all; egui's stock value is reaching the \
             screen"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one rule the theme is built on, checked on egui's own struct rather than on this
    /// file: every radius egui exposes is zero after `install`, so a new egui default cannot
    /// sneak a rounded corner back in.
    #[test]
    fn every_corner_is_square_after_install() {
        let ctx = egui::Context::default();
        install(&ctx);
        let v = ctx.global_style().visuals.clone();
        let zero = egui::CornerRadius::ZERO;
        assert_eq!(v.widgets.noninteractive.corner_radius, zero);
        assert_eq!(v.widgets.inactive.corner_radius, zero);
        assert_eq!(v.widgets.hovered.corner_radius, zero);
        assert_eq!(v.widgets.active.corner_radius, zero);
        assert_eq!(
            v.widgets.open.corner_radius, zero,
            "the combo box's open button"
        );
        assert_eq!(
            v.menu_corner_radius, zero,
            "every tooltip and combo list frame"
        );
        assert_eq!(v.window_corner_radius, zero);
        assert_eq!(
            v.window_stroke.color, RULE,
            "popups take the palette's hairline"
        );
        assert_eq!(v.widgets.open.bg_fill, PANEL_2);
    }

    /// THE LADDER AND THE FIVE STATES ARE TWO VOCABULARIES AND MAY NOT SHARE A COLOUR.
    ///
    /// The rule is stated beside the states and beside the ladder, and a comment cannot enforce it.
    /// The failure it stops is quiet: a rung that borrowed `WRONG` would make a low reputation read
    /// as a refusal on a rail where red already means one, and nothing would go red in CI.
    #[test]
    fn no_rung_borrows_a_state_colour() {
        const RUNGS: [(&str, Color32); 7] = [
            ("REGARD_ALLY", REGARD_ALLY),
            ("REGARD_WARMLY", REGARD_WARMLY),
            ("REGARD_KINDLY", REGARD_KINDLY),
            ("REGARD_AMIABLY", REGARD_AMIABLY),
            ("REGARD_INDIFFERENTLY", REGARD_INDIFFERENTLY),
            ("REGARD_APPREHENSIVELY", REGARD_APPREHENSIVELY),
            ("REGARD_DUBIOUSLY", REGARD_DUBIOUSLY),
        ];
        const STATES: [(&str, Color32); 5] = [
            ("IDLE", IDLE),
            ("WORKING", WORKING),
            ("YOU", YOU),
            ("WRONG", WRONG),
            ("SETTLED", SETTLED),
        ];
        for (rn, rc) in RUNGS {
            for (sn, sc) in STATES {
                assert_ne!(
                    rc, sc,
                    "{rn} is {sn}, and the two vocabularies must not overlap"
                );
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for (rn, rc) in RUNGS {
            assert!(
                seen.insert(rc.to_array()),
                "{rn} repeats a rung colour already on the ladder, so two rungs read alike"
            );
        }
    }

    #[test]
    fn the_ramp_runs_bright_to_deep() {
        assert_eq!(metal(0.0), GOLD_HI);
        assert_eq!(metal(1.0), GOLD_DEEP);
        assert_eq!(
            metal(0.22),
            FLARE,
            "the flare sits near the top edge, not the middle"
        );
        assert_eq!(metal(-1.0), GOLD_HI, "clamped");
        assert_eq!(metal(2.0), GOLD_DEEP, "clamped");
    }
}

/// THE LINE AROUND A CARD, and the app had no such colour at all.
///
/// A panel drawn as a fill with no edge does not read as an OBJECT, it reads as a slightly
/// different patch of background, which is most of why this app looked flat beside the design it
/// was built from. The mock gives every card a one pixel `#28303c`, and that single line is the
/// difference between a surface and a stain.
pub const LINE: Color32 = Color32::from_rgb(0x28, 0x30, 0x3C);

/// The softer version, for a rule INSIDE a card where a full edge would cut it in two.
pub const LINE_SOFT: Color32 = Color32::from_rgb(0x1E, 0x24, 0x2E);

/// HOW ROUND A CARD IS. Ten, off the mock's `--radius`.
///
/// THE APP USED TWO AND THREE, which at this size is not a rounded corner, it is a corner that
/// failed to be square. Ten is visibly a card.
pub const RADIUS: u8 = 10;

/// HOW THICK A PANEL'S BORDER IS, in points.
///
/// # A NUMBER THE CALLER HAS TO KNOW, WHICH IS WHY IT IS NOT A LITERAL
///
/// An `egui::Frame` draws its stroke OUTSIDE the rect it lays its content in, so a frame whose
/// content is forced to exactly H points is H plus two strokes tall. `screens::dashboards`
/// paints each card into a cell and clips to that cell, so a card built that way lost its
/// BOTTOM BORDER entirely: measured on the owner's screen, cell `156..274` and frame
/// `156..276`. The top border survived only because the clip and the frame share a top edge.
///
/// A caller that pins a panel to an exact height has to take this off first.
pub const PANEL_STROKE: f32 = 1.0;

/// A CARD: the box every panel in the design sits in.
///
/// # WHY THIS EXISTS RATHER THAN EACH PAGE ROLLING ITS OWN FRAME
///
/// Every page in this app drew its own `egui::Frame` with its own fill, its own margin and a two
/// pixel radius, and none of them drew a border or a shadow. The result was a flat stack of
/// slightly-different greys where the design has objects sitting on a ground, and the owner's
/// verdict on the difference was not gentle.
///
/// THE THREE THINGS THAT MAKE IT AN OBJECT are all here and none of them were anywhere before: a
/// one pixel [`LINE`] edge, a [`RADIUS`] corner, and a shadow underneath. egui has no box-shadow,
/// so the shadow is a soft rectangle painted behind the frame; it is cheap and it is what stops
/// the card from looking painted ON the background instead of above it.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    card_tinted(ui, PANEL, add)
}

/// [`card`] in a chosen fill, for the one or two places that want the raised tone.
pub fn card_tinted<R>(
    ui: &mut egui::Ui,
    fill: Color32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    /* THE SHADOW IS THE FRAME'S OWN, which is the only way round that works: egui paints a
     * frame in one pass and a rectangle drawn afterwards lands ON TOP of the card, not under it.
     * `Frame::shadow` is drawn as part of the frame and behind it.
     *
     * OFF THE MOCK'S `--shadow`, `0 18px 48px rgba(0,0,0,.36)`, scaled down: that page is a
     * browser at 16px type and this is a dense desktop panel, so the same offset would read as a
     * card floating an inch off the screen. */
    egui::Frame::NONE
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, LINE))
        .corner_radius(RADIUS)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .shadow(egui::epaint::Shadow {
            offset: [0, 5],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(92),
        })
        .show(ui, add)
}

/// A CARD'S OWN HEADING: small, spaced, gold, above its contents.
///
/// SMALL AND WIDE-TRACKED rather than big and bold, because a dashboard is many cards and a
/// heading that shouts turns the page into a list of shouts. The mock does the same thing.
pub fn card_head(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .font(egui::FontId::proportional(10.5))
            .color(TEXT_3),
    );
    ui.add_space(6.0);
}

/* ================================================================== the mock's own kit ==
 *
 * EVERY NUMBER BELOW WAS READ OFF THE MOCK, not chosen here. The mock is the owner's own
 * dashboard design, published at `https://eql-grimoire-live-dashboard.reviir.chatgpt.site`, and
 * it is not in this tree; the grid it specifies is transcribed in `screens::dashgrid` and that
 * module is the authority here. Where the mock and this file disagree, the mock is right.
 *
 * WHY A KIT AND NOT A PAGE THAT DRAWS ITS OWN BOXES. The mock is ONE design applied to nine
 * panels: the same head bar, the same row grid, the same bar track, the same chip. A page that
 * drew each of those itself would be nine chances for one of them to drift, and the drift is
 * exactly what the owner saw and called garbage. There is one panel head in this app now.
 */

/// A VERTICAL GRADIENT, WHICH egui HAS NO PRIMITIVE FOR.
///
/// The mock's panel heads, bar fills and sigils are all `linear-gradient`s, and a flat fill in
/// their place is most of the difference between a page that looks designed and a page that looks
/// like a table with a border on it. `epaint::Mesh` takes a colour per VERTEX, so a two triangle
/// quad with the top pair one colour and the bottom pair another is a real gradient, interpolated
/// by the renderer, for four vertices and no texture.
pub fn vgrad(p: &egui::Painter, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::epaint::Mesh::default();
    let uv = egui::epaint::WHITE_UV;
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.left_top(),
        uv,
        color: top,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.right_top(),
        uv,
        color: top,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.right_bottom(),
        uv,
        color: bottom,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.left_bottom(),
        uv,
        color: bottom,
    });
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    p.add(egui::Shape::mesh(mesh));
}

/// THE SAME THING SIDEWAYS, for the mock's `linear-gradient(90deg, ...)` bar fills.
pub fn hgrad(p: &egui::Painter, rect: egui::Rect, left: Color32, right: Color32) {
    let mut mesh = egui::epaint::Mesh::default();
    let uv = egui::epaint::WHITE_UV;
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.left_top(),
        uv,
        color: left,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.right_top(),
        uv,
        color: right,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.right_bottom(),
        uv,
        color: right,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.left_bottom(),
        uv,
        color: left,
    });
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    p.add(egui::Shape::mesh(mesh));
}

/// CSS `color-mix(in srgb, a X%, b)`. The mock builds every rank badge out of one.
///
/// A FUNCTION AND NOT TWELVE LITERALS, because the mock derives the badge's border, fill and text
/// from ONE row colour: writing them out would be four colours per rank times four ranks, and the
/// day a rank colour changes, sixteen numbers have to change with it.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 * t + y as f32 * (1.0 - t)).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// The mock's `--red`, `--blue`, `--violet`, `--cyan` and `--orange`, for the rows and the marks.
///
/// `RED`, `GREEN` and `BLUE` ARE THE STATE COLOURS ALREADY: `WRONG`, `SETTLED` and `WORKING` are
/// the same three hex values off the same three tokens, so they are aliased rather than declared
/// twice. A second `#EF476F` in this file is a second place to change it.
pub const VIOLET: Color32 = Color32::from_rgb(0x9A, 0x72, 0xFF);
pub const CYAN: Color32 = Color32::from_rgb(0x4F, 0xD8, 0xE8);
pub const ORANGE: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x45);

/// THE PANEL HEAD'S GRADIENT, `rgba(30,36,46,.72)` to `rgba(17,21,28,.35)` composited over
/// [`PANEL`]. Flattened here rather than drawn with alpha because the head sits on ONE known
/// ground and a translucent bar over a card edge picks up the edge.
pub const HEAD_TOP: Color32 = Color32::from_rgb(0x1A, 0x1F, 0x28);
pub const HEAD_BOT: Color32 = Color32::from_rgb(0x0F, 0x13, 0x19);

/// The mock's `.bar-track` ground, `#0a0d12`.
pub const TRACK: Color32 = Color32::from_rgb(0x0A, 0x0D, 0x12);

/// The mock's control ground and edge: `.ghost-btn` / `.icon-btn` fill `#10151d`, and the
/// `.scope-picker` / `.encounter-picker` well `#0d1117`.
pub const CONTROL: Color32 = Color32::from_rgb(0x10, 0x15, 0x1D);
pub const WELL: Color32 = Color32::from_rgb(0x0D, 0x11, 0x17);

/// WHICH COLOUR A ROW OF THE ROSTER IS, BY RANK.
///
/// FOUR AND THEN GREY, exactly as the mock: gold, red, green, blue for the top four and a neutral
/// for everybody after them. That is not decoration. A damage meter is READ BY GLANCING, and the
/// thing a glance resolves fastest is hue: the owner knows which bar is his before he has read a
/// single name. Ranks past four are not glanced at, they are looked up, so colouring them would
/// spend the vocabulary without buying anything.
pub fn row_colour(rank: usize) -> Color32 {
    match rank {
        0 => GOLD,
        1 => WRONG,
        2 => SETTLED,
        3 => WORKING,
        _ => Color32::from_rgb(0x5A, 0x63, 0x72),
    }
}

/// A PANEL: the mock's `.panel`, and the head is drawn INSIDE it.
///
/// NO INNER MARGIN, WHICH IS THE DIFFERENCE FROM [`card`]. The mock's head bar spans the panel
/// edge to edge with its own bottom rule, and its column header row does too; a padded frame
/// would inset both and leave the rule floating short of the border on each side. The body pads
/// itself: see [`panel_body`].
pub fn panel<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::NONE
        .fill(PANEL)
        .stroke(egui::Stroke::new(PANEL_STROKE, LINE))
        .corner_radius(RADIUS)
        .inner_margin(egui::Margin::ZERO)
        .shadow(egui::epaint::Shadow {
            offset: [0, 5],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(92),
        })
        .show(ui, add)
}

/// A PANEL'S HEAD BAR: the mock's `.panel-head`.
///
/// `min-height 48px`, padding `8px 12px 8px 15px`, a `--line-soft` rule under it, and the
/// `rgba(30,36,46,.72)` to `rgba(17,21,28,.35)` gradient behind it. The title is `15px` weight
/// `700`; the sub is `12px --muted` beside it, not under it.
///
/// `right` IS WHERE THE PANEL'S OWN CONTROLS GO, `margin-left:auto` in the mock. It is a closure
/// rather than a list of labels because what goes there differs per panel: metric tabs on the
/// roster, a close cross on a widget, `Full detail >` on the drill-in.
pub fn panel_head(
    ui: &mut egui::Ui,
    title: &str,
    sub: Option<&str>,
    right: impl FnOnce(&mut egui::Ui),
) {
    let full = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(full, 40.0), egui::Sense::hover());
    /* THE HEAD TAKES THE PANEL'S OWN TOP CORNERS.
     *
     * DEFECT: EVERY CARD'S TOP CORNERS WERE SQUARE while its bottom two were round. This
     * painted the head as a plain quad across the full width, straight over the rounded
     * corners the panel had just drawn. The rounding was never missing: it was covered up, and
     * only at the top, which is why one card had two of each.
     *
     * A ROUNDED BAND, THEN THE GRADIENT UNDER IT. `vgrad` is a four point mesh and cannot round
     * anything, and egui cannot clip a mesh to a rounded rect. So the top `RADIUS` points are a
     * rounded rect filled flat and the gradient runs from just under them. What that costs is
     * the first quarter of the head being flat instead of shading by three parts in 255, which
     * is under what a screen can show; what it buys is a corner that is actually round rather
     * than a notch painted in a background colour this function has no business knowing. */
    let r = f32::from(RADIUS);
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius {
            nw: RADIUS,
            ne: RADIUS,
            sw: 0,
            se: 0,
        },
        HEAD_TOP,
    );
    vgrad(
        ui.painter(),
        egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + r), rect.max),
        HEAD_TOP,
        HEAD_BOT,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, LINE_SOFT),
    );
    /* THE CONTROLS ON THE RIGHT ARE PLACED FIRST, AND THE WORDS GET WHAT IS LEFT.
     *
     * DEFECT: A NARROW CARD'S CAPTION PRINTED THROUGH ITS OWN TABS. The words went down first
     * and the right hand side took whatever width they left, which on a four column card with a
     * long caption was less than none, so the group caption ran straight under the DEALT and
     * TAKEN tabs. A tab is a control and a caption is words: the control keeps its room, and the
     * caption is cut to what remains, with all of it on hover. */
    let inner = rect.shrink2(egui::vec2(12.0, 6.0));
    let mut side = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    right(&mut side);
    let taken = side.min_rect().width();
    let words = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(
            (inner.right() - taken - 8.0).max(inner.left()),
            inner.bottom(),
        ),
    );
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(words)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar.add_space(3.0);
    let head = bar.add(
        egui::Label::new(
            egui::RichText::new(title)
                .font(egui::FontId::proportional(14.0))
                .strong()
                .color(TEXT),
        )
        .truncate(),
    );
    if let Some(s) = sub {
        /* A CAPTION WITH NO ROOM IS NOT DRAWN AS AN ELLIPSIS PAST THE TABS. A truncated label
         * still draws its ellipsis when it has no width at all, and on a four column card with
         * four tabs that ellipsis landed on the first of them. Below [`SUB_MIN`] the caption says
         * nothing a reader could use, so it moves onto the title's hover, where all of it is. */
        if bar.available_width() - 6.0 >= SUB_MIN {
            bar.add_space(6.0);
            bar.add(
                egui::Label::new(
                    egui::RichText::new(s)
                        .font(egui::FontId::proportional(11.0))
                        .color(TEXT_2),
                )
                .truncate(),
            );
        } else {
            head.on_hover_text(s);
        }
    }
}

/// THE NARROWEST A CARD'S CAPTION IS DRAWN AT, in points. About six characters at eleven
/// points, which is `in sco...`: under that it is a mark, not words. See [`panel_head`].
pub const SUB_MIN: f32 = 48.0;

/// THE PADDED INSIDE OF A PANEL, under its head. The mock's bodies pad `14px 15px`.
pub fn panel_body<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(14, 11))
        .show(ui, add)
}

/// A METRIC TAB on a panel head: the mock's `.metric-tab`.
///
/// THE ACTIVE ONE CARRIES AN UNDERLINE AND NOT ONLY A TINT (`box-shadow: inset 0 -1px --gold`),
/// because a tint alone at this size is a difference somebody has to look for.
pub fn metric_tab(ui: &mut egui::Ui, label: &str, on: bool) -> egui::Response {
    let r = ui.add(
        egui::Button::new(
            egui::RichText::new(label.to_uppercase())
                .font(egui::FontId::proportional(10.5))
                .color(if on { GOLD_HI } else { TEXT_2 }),
        )
        .fill(if on {
            Color32::from_rgb(0x1E, 0x1E, 0x1C)
        } else {
            Color32::TRANSPARENT
        })
        .corner_radius(4)
        .stroke(egui::Stroke::NONE),
    );
    if on {
        ui.painter().hline(
            r.rect.x_range(),
            r.rect.bottom() - 0.5,
            egui::Stroke::new(1.0, GOLD),
        );
    }
    r
}

/// ONE CELL OF THE SUMMARY STRIP: the mock's `.stat`.
///
/// `min-height 77px`, an uppercase `12px --muted` label with `.08em` tracking, and a `24px` value
/// at weight `680` with an optional `12px --muted` suffix on its baseline. The suffix is a
/// separate argument and not part of the value string because it is a DIFFERENT SIZE and colour;
/// `230.9 DPS` set as one run is the unit shouting as loudly as the number.
pub fn stat(ui: &mut egui::Ui, label: &str, value: &str, small: Option<&str>, tint: Color32) {
    ui.vertical(|ui| {
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(label.to_uppercase())
                .font(egui::FontId::proportional(10.0))
                .color(TEXT_2),
        );
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(
                egui::RichText::new(value)
                    .font(egui::FontId::proportional(21.0))
                    .strong()
                    .color(tint),
            );
            if let Some(s) = small {
                ui.label(
                    egui::RichText::new(s)
                        .font(egui::FontId::proportional(10.5))
                        .color(TEXT_2),
                );
            }
        });
    });
}

/// THE RANK BADGE: the mock's `.rank`, every colour mixed off the row's own.
pub fn rank_badge(ui: &mut egui::Ui, n: usize, row: Color32) {
    rank_badge_at(ui, n, row, 24.0, 11.0);
}

/// A RANK BADGE AT A SIZE THE CALLER PICKS.
///
/// THE ROSTER SIZES ITS OWN CONTENTS NOW (see `dashboards::roster_plan`), so a badge that was
/// always 24 points square with 11 point type sat wrong in a row twice that tall. The fixed
/// size version above is the rest of the app, which has no such plan.
pub fn rank_badge_at(ui: &mut egui::Ui, n: usize, row: Color32, size: f32, pt: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(
        rect,
        5.0,
        mix(row, Color32::from_rgb(0x0B, 0x0E, 0x13), 0.18),
    );
    p.rect_stroke(
        rect,
        5.0,
        egui::Stroke::new(1.0, mix(row, Color32::from_rgb(0x39, 0x40, 0x4C), 0.70)),
        egui::StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        n.to_string(),
        egui::FontId::proportional(pt),
        mix(row, Color32::WHITE, 0.76),
    );
}

/// THE `YOU` PILL: gold ground, near-black text, `9px` weight `900`, tracked.
///
/// IT IS THE ONE THING ON A ROSTER ROW THAT IS ABOUT THE READER, and it is why a damage meter is
/// opened at all. The mock gives it the only solid gold fill on the row.
pub fn you_pill(ui: &mut egui::Ui) {
    let galley = ui.painter().layout_no_wrap(
        String::from("YOU"),
        egui::FontId::proportional(9.0),
        Color32::from_rgb(0x13, 0x10, 0x08),
    );
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 10.0, galley.size().y + 3.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 8.0, GOLD);
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        Color32::from_rgb(0x13, 0x10, 0x08),
    );
}

/// A CLASS CHIP: the mock's `.class-chip` plus its `.class-rune`.
///
/// A RUNE AND THEN THE WORD, because an EQL character is a TRIO and three full class names on one
/// row is wider than the name column. The rune is the class's initial on the class's own colour,
/// which is what a reader picks out sideways; the word is there so the rune never has to be
/// learned.
pub fn class_chip(ui: &mut egui::Ui, class: &str, colour: Color32) -> egui::Response {
    class_chip_at(ui, class, colour, 9.5)
}

/// A CLASS CHIP AT A SIZE THE CALLER PICKS.
///
/// THE ROSTER SIZES ITS OWN CONTENTS (see `dashboards::roster_plan`), so a chip that was always
/// 9.5 point type in an 18 point box looked stranded under a name twice that size. Everything
/// inside the chip is a fraction of the type, so the rune, the padding and the box scale
/// together rather than the word growing inside a fixed rail.
pub fn class_chip_at(ui: &mut egui::Ui, class: &str, colour: Color32, pt: f32) -> egui::Response {
    let word = ui.painter().layout_no_wrap(
        class.to_owned(),
        egui::FontId::proportional(pt),
        Color32::from_rgb(0xAB, 0xB3, 0xBF),
    );
    let rune_w = pt * 1.26;
    let pad = pt * 0.42;
    let h = pt * 1.9;
    let (rect, r) = ui.allocate_exact_size(
        egui::vec2(word.size().x + rune_w + pad * 3.0, h),
        egui::Sense::hover(),
    );
    let p = ui.painter();
    p.rect_filled(rect, 4.0, Color32::from_rgb(0x11, 0x17, 0x20));
    p.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, Color32::from_rgb(0x33, 0x3C, 0x49)),
        egui::StrokeKind::Inside,
    );
    let rune = egui::Rect::from_min_size(
        egui::pos2(rect.left() + pad, rect.center().y - rune_w / 2.0),
        egui::vec2(rune_w, rune_w),
    );
    p.rect_filled(rune, 3.0, colour);
    p.text(
        rune.center(),
        egui::Align2::CENTER_CENTER,
        class
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string(),
        egui::FontId::proportional(pt * 0.84),
        Color32::WHITE,
    );
    let at = egui::pos2(rune.right() + pad, rect.center().y - word.size().y / 2.0);
    p.galley(at, word, Color32::from_rgb(0xAB, 0xB3, 0xBF));
    r
}

/// THE RELATIVE BAR: the mock's `.bar-track` and `.bar-fill`.
///
/// A TRACK THAT IS ALWAYS DRAWN, EVEN AT ZERO, which is what makes a column of these readable: a
/// bar chart with no rail is a row of unrelated blocks and the eye has nothing to measure them
/// against. The fill runs from a dark mix of the row's colour to the colour itself, so a long bar
/// reads as ONE object rather than as a flat slab.
pub fn bar(ui: &mut egui::Ui, width: f32, frac: f32, row: Color32) {
    bar_at(ui, width, 22.0, frac, row);
}

/// A RELATIVE BAR AT A THICKNESS THE CALLER PICKS. See [`rank_badge_at`] for why.
pub fn bar_at(ui: &mut egui::Ui, width: f32, height: f32, frac: f32, row: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, TRACK);
    p.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, Color32::from_white_alpha(9)),
        egui::StrokeKind::Inside,
    );
    let w = (rect.width() * frac.clamp(0.0, 1.0)).max(0.0);
    if w < 1.0 {
        return;
    }
    let fill = egui::Rect::from_min_size(rect.min, egui::vec2(w, rect.height()));
    /* CLIPPED TO THE TRACK'S OWN ROUNDING. A gradient mesh has no corner radius of its own, so
     * without this a full bar has square corners inside a rounded rail. */
    let clipped = p.with_clip_rect(rect);
    clipped.rect_filled(
        fill,
        4.0,
        mix(row, Color32::from_rgb(0x11, 0x11, 0x11), 0.72),
    );
    hgrad(
        &clipped,
        fill.shrink2(egui::vec2(0.0, 0.0)),
        mix(row, Color32::from_rgb(0x11, 0x11, 0x11), 0.72),
        row,
    );
}
