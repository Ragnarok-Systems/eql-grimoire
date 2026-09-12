//! The house faces, compiled into the binary.
//!
//! THEY ARE EMBEDDED, NOT LOADED FROM DISK, AND THAT IS THE POINT.
//! A desktop application that reads its typeface from a path has a state where the path is wrong
//! and the window comes up in whatever the system happened to offer. That is the same class of
//! failure as the web build's "engine is not built" banner: a product that renders, looks roughly
//! right, and is not the thing that was designed. `include_bytes!` makes the face a fact about the
//! binary rather than a fact about the machine it landed on.
//!
//! THE THREE ROLES, AND WHY IT IS THREE AND NOT ONE.
//!   Cinzel        display only. Inscriptional Roman capitals, and the reason the brand reads as
//!                 struck metal rather than set text. It has no lowercase rhythm built for running
//!                 copy and no monospace cut, so it stops at headings.
//!   IBM Plex Sans body and controls.
//!   IBM Plex Mono ids, paths, coin columns, anything a person compares character by character.
//!                 Tabular figures are the whole argument: a quote is a column of numbers and they
//!                 have to line up or the eye cannot subtract them.
//!
//! The TTFs were decompressed from the woff2 the Gnomish console already vendors, so both products
//! are running the same outlines rather than two downloads that drift.

use egui::{FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use std::sync::Arc;

/// The display family, registered under its own name so a caller has to ask for it deliberately.
pub const DISPLAY: &str = "cinzel";

pub fn install(ctx: &egui::Context) {
    let mut f = FontDefinitions::default();

    f.font_data.insert(
        DISPLAY.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/Cinzel-Variable.ttf"
        ))),
    );
    f.font_data.insert(
        "plex".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/IBMPlexSans-Regular.ttf"
        ))),
    );
    f.font_data.insert(
        "plexmono".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/IBMPlexMono-Regular.ttf"
        ))),
    );

    /* Plex first in Proportional, so ordinary text is Plex. Cinzel is NOT in this list: a display
     * face silently answering for body text is how a UI ends up unreadable at 12px. It is reachable
     * only through FontFamily::Name("cinzel"), which is a decision a caller has to make. */
    f.families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "plex".to_owned());
    f.families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "plexmono".to_owned());

    /* The display family, on its own. egui falls back through the list, so Plex is appended behind
     * Cinzel to cover any glyph Cinzel does not carry. Cinzel is caps-focused and will not have,
     * for example, the box-drawing characters a log line might contain. */
    f.families.insert(
        FontFamily::Name(DISPLAY.into()),
        vec![DISPLAY.to_owned(), "plex".to_owned()],
    );

    ctx.set_fonts(f);

    /* Text styles, so the sizes are decided once here rather than guessed at each call site. */
    /* egui 0.36 keeps a style per theme. This app is dark only, but writing both keeps the sizes
     * correct if anything ever flips the theme. */
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |s| {
            s.text_styles = [
                (
                    TextStyle::Heading,
                    FontId::new(19.0, FontFamily::Name(DISPLAY.into())),
                ),
                (TextStyle::Body, FontId::proportional(12.5)),
                (TextStyle::Button, FontId::proportional(12.5)),
                (TextStyle::Small, FontId::proportional(10.5)),
                (TextStyle::Monospace, FontId::monospace(11.5)),
            ]
            .into();
        });
    }
}

/// A display-face font id at `size`. The one way to reach Cinzel.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}
