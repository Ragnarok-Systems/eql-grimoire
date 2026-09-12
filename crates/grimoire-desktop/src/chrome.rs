//! The outer chrome, ported from the Gnomish console: a rail on the left, a context header, and a
//! body that a view fills. Same shape, same vocabulary, different palette.
//!
//! WHY THE BRAND BLOCK IS PAINTED RATHER THAN LAID OUT WITH LABELS.
//! `EQL GRIMOIRE` is the one place the gold ramp has to run down the glyphs, and no text widget can
//! do that: egui paints a run of text in one colour. So the wordmark is drawn a line at a time with
//! the ramp sampled per band, which is also why it has to know its own height before it draws.

use crate::theme::*;
use egui::{Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

/* 176, WHICH IS 252 LESS THIRTY PERCENT. The rail was sized when its widest row was `poSky
 * checklist`; the owner asked for the thirty back for the body, and `every_word_the_rail_paints
_fits_inside_it` below is what says the words still fit rather than a reading of this number. */
pub const RAIL_WIDE: f32 = 176.0;

/// THE SIZE THIS APPLICATION WANTS TO BE: ONE QUADRANT OF THE SCREEN IT IS ON.
///
/// # THE OWNER ASKED FOR HALF THE WIDTH AND HALF THE HEIGHT, SO IT DROPS INTO A 2x2 SNAP GRID
///
/// He runs the app in one quarter of a 4K panel and wants that to be what it opens
/// at, so that everything is designed at the size it is actually used at and grows from there.
///
/// # AND IT CANNOT BE A CONSTANT, WHICH IS WHAT THE FIRST ATTEMPT GOT WRONG
///
/// This was `[1920.0, 1080.0]`, being half of 3840 by 2160. That is right in PHYSICAL PIXELS and
/// wrong in the units the window is actually built in: `ViewportBuilder::with_inner_size` takes
/// egui POINTS, and a point is a pixel only at 100% scaling.
///
/// THE OWNER'S DESKTOP IS AT 175%, measured rather than assumed (`GetDpiForWindow` answers 168).
/// So his 3840 by 2160 panel is 2194 by 1234 POINTS, a quadrant of it is 1097 by 593, and
/// `[1920, 1080]` asked for a window 3360 by 1890 pixels: most of the screen. The matching
/// minimum was worse than wrong, it was larger than his entire logical desktop, so the window
/// could not have been resized down to a quadrant at all.
///
/// A GUESS AT SOMEBODY'S SCALING IS NOT A DESIGN SIZE. The rule is `half the screen`, and the
/// screen is a runtime fact, so this is a function of it and the App asks for it on its first
/// frame. See `main::App::fit_to_quadrant`.
///
/// # THE WORK AREA AND NOT THE MONITOR, BECAUSE THAT IS WHAT SNAP USES
///
/// Windows' own snap fits a window to the work area, which is the monitor less the taskbar. On
/// the owner's machine that is 1234 points tall against a 1186 point work area, so a quadrant
/// measured off the MONITOR would be 617 tall and sit twenty four points underneath the taskbar,
/// while a snapped one is 593. Measuring the same rectangle Windows measures is what makes the
/// window land exactly in the grid rather than near it.
/// # ONE FUNCTION, BECAUSE THE HALVING WAS NEVER THE HARD PART
///
/// This was `quadrant(work_area: Vec2)` and the CALLER converted the units before handing them
/// over. That is how the conversion came to be wrong twice in a row while a test of the halving
/// stayed green both times: the test composed the division the way it believed production did,
/// production composed it another way, and nothing compared the two. A test that proves a
/// function and not that the function is CALLED CORRECTLY is this tree's signature defect.
///
/// So the conversion and the halving are one call now, and the test drives this.
///
/// # THE UNITS, MEASURED AT LAST
///
/// `work_px` IS PHYSICAL PIXELS. `SPI_GETWORKAREA` answers in the coordinate space of the
/// calling process's DPI awareness, and this app is per-monitor aware, so it gets the real
/// pixels: 3840 by 2112 on the owner's machine.
///
/// `pixels_per_point` IS WHAT TURNS THOSE INTO THE UNITS THE WINDOW IS BUILT IN, and it is the
/// native scale times the app's own zoom, which is exactly the pair that has to divide out.
/// 3840 physical / 1.75 = 2194 points; half of that is 1097 points; 1097 points at 1.75 is 1920
/// real pixels, which is half of a 4K panel and what Windows' 2x2 snap grid gives you.
///
/// # HOW BOTH WRONG VERSIONS SURVIVED BEING CHECKED
///
/// The first shipped `[1920.0, 1080.0]` as a constant: right in pixels, read as points, so 1.75
/// times too big. The second removed the scale division entirely on the strength of a
/// measurement taken through a DPI-UNAWARE process, which reports every rectangle already
/// divided by the scale factor. That harness cannot tell a correct window from one 1.75 times
/// too big, because it divides both by 1.75 before showing them.
///
/// A MEASUREMENT IS ONLY EVIDENCE IF THE INSTRUMENT IS IN THE UNITS YOU THINK IT IS. Anything
/// checking this by hand must call `SetProcessDpiAwarenessContext(-4)` first.
pub fn quadrant_for(work_px: Vec2, pixels_per_point: f32) -> Vec2 {
    /* A ZERO OR NEGATIVE SCALE IS NOT A DIVISION, IT IS AN INFINITY. egui has no reason to
     * hand one over and a window sized from one would be unrecoverable, so it degrades to
     * `no scaling` rather than propagating. */
    let ppp = if pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    let points = work_px / ppp;
    Vec2::new((points.x / 2.0).floor(), (points.y / 2.0).floor())
}

/// HOW MUCH UNDER A QUADRANT THE WINDOW IS ALLOWED TO GO, AND WHY IT IS NOT ZERO.
///
/// The owner's rule is that the design size is also the floor: if nothing can be smaller than the
/// size everything was laid out at, no page in this app ever needs a second, narrower answer, and
/// the whole class of `it looks wrong when narrow` defects cannot happen.
///
/// SO THE FLOOR IS THE QUADRANT, LESS FOUR POINTS. Those four are not a second layout target and
/// nothing may be designed to them: they are rounding tolerance. The work area can be an odd
/// number of points (2194 is, here), the app floors its half and the window manager may round the
/// other way, and a minimum equal to the quadrant to the pixel is a minimum that can refuse the
/// very snap it was computed for.
pub const SNAP_SLACK: f32 = 4.0;

/// The smallest this window may be dragged to, given the quadrant it was sized for.
pub fn floor_for(quadrant: Vec2) -> Vec2 {
    Vec2::new(quadrant.x - SNAP_SLACK, quadrant.y - SNAP_SLACK)
}

/// WHAT THE WINDOW OPENS AT BEFORE THE SCREEN HAS ANSWERED, and it is deliberately modest.
///
/// The real size needs the work area, which is not known until a frame has run. This is the size
/// of the window for that one frame, and on a machine where the work area cannot be read at all
/// it is the size for good. Small enough to fit inside a quadrant of any display this app is
/// plausibly run on, because too small is a window a person resizes and too big is a window whose
/// controls are off the screen.
pub const FALLBACK: [f32; 2] = [1100.0, 700.0];

/// THE SMALLEST THIS WINDOW MAY EVER BE, whatever the screen turns out to be.
///
/// # IT IS NOT THE DESIGN FLOOR AND IT IS NOT MEANT TO BE COMFORTABLE
///
/// The design floor is a quadrant of the work area (see [`floor_for`]), and the App raises the
/// window's minimum to it on the first frame. This is the value in force BEFORE that, and on any
/// machine whose work area cannot be read at all.
///
/// SO IT ANSWERS A DIFFERENT QUESTION: not `how small may this be laid out` but `how small may a
/// window get before the person holding it can no longer move it`. A window smaller than its own
/// title strip is one whose close button and drag area are gone, and no page in this app is laid
/// out for 640 by 480. It is a safety rail, not a target.
pub const HARD_FLOOR: [f32; 2] = [640.0, 480.0];

/// THE PRIMARY MONITOR'S WORK AREA IN PHYSICAL PIXELS: the monitor less the taskbar.
///
/// THE SAME RECTANGLE WINDOWS' OWN SNAP USES, which is the whole reason it is asked for rather
/// than derived from egui's `monitor_size`. That value is the full monitor and would put a
/// quadrant under the taskbar by whatever the taskbar is tall.
///
/// # IT ANSWERS IN PHYSICAL PIXELS, MEASURED FROM A DPI-AWARE PROCESS
///
/// `SPI_GETWORKAREA` answers in the coordinate space of the CALLING PROCESS's DPI awareness.
/// This app is per-monitor aware, so it gets REAL PIXELS: 3840 by 2112 on the owner's machine,
/// a 4K panel less a 48 pixel taskbar. [`quadrant_for`] is what turns that into points.
///
/// A PASS AT THIS BRIEFLY BELIEVED THE OPPOSITE, on the strength of a reading taken through a
/// DPI-UNAWARE process. Such a process is shown every rectangle pre-divided by the scale factor,
/// so it reported this call returning 2194 by 1186 and reported a window 1.75 times too big as
/// correct, in one consistent set of wrong units. Verified since with
/// `SetProcessDpiAwarenessContext(-4)`: work area 3840 by 2112, quadrant 1920 by 1056.
///
/// `None` RATHER THAN A GUESS when the call fails: the caller keeps [`FALLBACK`], which is a
/// window a person can see and move, where an invented work area is a window that might not be.
#[cfg(windows)]
pub fn work_area_px() -> Option<Vec2> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut r = RECT::default();
    // SAFETY: `SPI_GETWORKAREA` writes a `RECT` through `pvparam`, which is what is passed, and
    // the call is read only: no update flags, so nothing is broadcast or persisted.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(std::ptr::from_mut(&mut r).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if ok.is_err() {
        return None;
    }
    let w = (r.right - r.left) as f32;
    let h = (r.bottom - r.top) as f32;
    (w > 1.0 && h > 1.0).then_some(Vec2::new(w, h))
}

/// Every other platform builds this app but nobody runs it there; the fallback is the answer.
#[cfg(not(windows))]
pub fn work_area_px() -> Option<Vec2> {
    None
}
pub const RAIL_NARROW: f32 = 64.0;

/// The five states, as the Gnomish console defines them.
///
/// THE RAIL NO LONGER PAINTS A SQUARE FOR THESE. That mark was an invention and was cut; only
/// `You` reaches the screen here, as the trailing 3px attention bar. The other four are still
/// computed by `nav::square` and are still read by its own tests, so anything that wants to show
/// them has to earn its own place on the rail rather than inherit this one.
///
/// `Debug` so a test that asserts a screen picked the right state can NAME the one it got. A
/// failure that reads "assertion failed: left != right" with no words is a failure someone has to
/// go and reproduce by hand; this is a fieldless enum, so the derive costs nothing and hides
/// nothing (rustc's dead code lint ignores `Debug`, and `reach.rs` masks on the comparison traits,
/// which this already carried).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    Working,
    You,
    Wrong,
    Settled,
}

impl State {
    pub fn color(self) -> Color32 {
        match self {
            State::Idle => IDLE,
            State::Working => WORKING,
            State::You => YOU,
            State::Wrong => WRONG,
            State::Settled => SETTLED,
        }
    }
}

/// The brand block: the wordmark and its underline, and nothing else.
///
/// ```text
///   EQL GRIMOIRE   <- the wordmark, gold ramp running top to bottom
///   ───────────    <- a hairline the exact width of the wordmark, not the panel
/// ```
///
/// THE MAKER'S LINE MOVED TO THE TITLE STRIP. It used to sit under the rule here, a second run of
/// tracked caps immediately below the wordmark. It now follows the window's name on
/// `titlebar::strip`, where it sits beside the live pill: the dedication and the channel's state
/// are the same subject, and a strip that is always on screen at any rail width is a better home
/// for the link than a block a collapsed rail cuts down to `EQL`.
///
/// WHAT IT DOES NOT SAY, since this note twice claimed otherwise. The strip's run is `BROKEN
/// STOIC`, not `BUILT FOR BROKEN STOIC`: `titlebar::maker_block` cut the connective tissue and
/// kept the full phrase on the hover, and it records why. And moving the line did not stop the
/// strip repeating the product's name, because the strip never stopped: `titlebar::strip` draws
/// `EQL Grimoire` first and has to, or the taskbar entry and the bar disagree. The wordmark below
/// and the strip above say the same words on purpose, at different sizes, for different reasons.
///
/// The rule stays. It was measured to the wordmark so it would read as an underline belonging to
/// the title rather than a divider between two unrelated things, and with the maker's line gone it
/// does both jobs: it still underlines the word, and it now closes the block against the first
/// section header. A full width rule here would go back to reading as a divider.
pub fn brand(ui: &mut Ui, narrow: bool) -> Response {
    /* Sized to what is actually in it, again. 84 once fitted a "brought to you by" line, 66 fitted
     * the maker's line under the rule, and both are gone; a height kept past its contents is the
     * dead gutter that makes a UI feel unconsidered even when every element in it is right. */
    let h = if narrow { 38.0 } else { 46.0 };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let p = ui.painter();

    if narrow {
        /* Collapsed: the mark alone, centred, and STILL CUT FROM THE SAME METAL: the ramp runs
         * down "EQL" through the same bands as the wordmark, not one flat stop of it (the first
         * cut painted it in GOLD_HI alone, which is the top edge of the metal and reads as a
         * different, yellower brand).
         *
         * The Twitch glyph that used to sit under it is gone. It was there so collapsing the rail
         * lost the words and not the link, which was right while the maker's line lived in this
         * block. The line now leads the title strip, which is always on screen at any rail width,
         * so a glyph here would be the second link to one channel on one screen. The rule is
         * dropped too: at this width it would be as wide as the mark and read as a divider. */
        let f = crate::fonts::display(17.0);
        let galley = p.layout_no_wrap("EQL".to_owned(), f.clone(), GOLD);
        let ink_h = galley.rect.height();
        let at = Pos2::new(
            rect.center().x - galley.rect.width() * 0.5,
            rect.top() + 6.0,
        );
        ramp_text(p, rect, at, "EQL", &f, ink_h);
        return resp;
    }

    let left = rect.left() + 14.0;
    let mut y = rect.top() + 6.0;

    /* The wordmark, drawn in horizontal bands so the ramp runs DOWN the letters (`ramp_text`). */

    /* 20 AND NOT 22, BECAUSE THE RAIL NARROWED UNDER IT. Measured rather than eyeballed: at 22 the
     * word inks 159pt from a 14pt inset and ends at 173 of a 176pt rail, which does not clip but
     * leaves a 3pt right margin against a 14pt left one, and a wordmark pressed against one wall of
     * its own panel reads as a layout that ran out of room. At 20 it ends near 159 and the two
     * margins are of a size. The rule below is measured to this word, so it follows on its own.
     * `every_word_the_rail_paints_fits_inside_it` is what holds this to the rail width. */
    let size = 20.0;
    let word = "EQL GRIMOIRE";
    let f = crate::fonts::display(size);
    let galley = p.layout_no_wrap(word.to_owned(), f.clone(), GOLD);
    let ink_h = galley.rect.height();
    ramp_text(p, rect, Pos2::new(left, y), word, &f, ink_h);
    let word_w = galley.rect.width();
    y += ink_h + 6.0;

    /* THE RULE, AND WHY IT IS MEASURED TO THE WORDMARK.
     *
     * This is the last thing the block draws, and the only thing under the wordmark. Two lines
     * that used to be here are gone: "brought to you by", four words in the register of an
     * infomercial, cut because POSITION already stated the relationship; and the maker's line
     * itself, which moved to `titlebar::strip` (see the doc above). What is left is a hairline
     * exactly as wide as the word above it.
     *
     * The width is the whole trick. A full width rule reads as a divider between two unrelated
     * things. A rule that stops where the word stops reads as an underline belonging to it, which
     * is why it is measured off the galley (`word_w`) and not off the panel. With the maker's line
     * gone it does both jobs at once: it underlines the wordmark, and it closes the block against
     * the first section header.
     *
     * GOLD_DEEP AND NOT THE RAMP. The wordmark runs the whole ramp, flare to shadow; a rule that
     * also ran it would compete with the letters it belongs to. One flat stop at the bottom of the
     * metal reads as the word's own shadow settling. */
    p.line_segment(
        [Pos2::new(left, y), Pos2::new(left + word_w, y)],
        Stroke::new(1.0, GOLD_DEEP),
    );

    resp
}

/// Draw text with manual letter spacing, returning the width consumed.
pub fn tracked(ui: &Ui, at: Pos2, s: &str, size: f32, track: f32, col: Color32) -> f32 {
    let p = ui.painter();
    let f = crate::fonts::display(size);
    let mut x = at.x;
    for ch in s.chars() {
        let g = ch.to_string();
        let w = ui
            .painter()
            .layout_no_wrap(g.clone(), f.clone(), col)
            .size()
            .x;
        p.text(Pos2::new(x, at.y), Align2::LEFT_TOP, g, f.clone(), col);
        x += w + track;
    }
    x - at.x
}

/// Text with the gold ramp running DOWN the glyphs: drawn six times, each pass clipped to one
/// horizontal band and coloured from `metal` at that band's height. `ink_h` is the galley's
/// measured height, which the caller already has from laying the string out.
///
/// THE BANDS MUST COVER THE GALLEY, NOT THE FONT SIZE, AND THAT COST A Q.
/// The first version clipped each band inside a total height of `size`, which is the em, not the
/// drawn height. A glyph with a descender falls below the em, so Cinzel's Q had its tail sliced
/// off by the bottom of the last band and the wordmark read "EOL GRIMOIRE". The bug looked like
/// a broken font and was a clip rectangle. So the height is MEASURED: layout_no_wrap returns the
/// galley egui is about to paint, and its rect is the truth about how tall this string is in this
/// face at this size, including anything hanging below the baseline. Never assume the em bounds
/// the ink.
fn ramp_text(p: &egui::Painter, within: Rect, at: Pos2, s: &str, f: &FontId, ink_h: f32) {
    let bands = 6;
    for i in 0..bands {
        let t0 = i as f32 / bands as f32;
        let clip = Rect::from_min_size(
            Pos2::new(within.left(), at.y + t0 * ink_h),
            /* +1.0 rather than +0.5: adjacent bands must OVERLAP by a subpixel or antialiasing
             * leaves a hairline seam between them on fractional scaling. */
            Vec2::new(within.width(), ink_h / bands as f32 + 1.0),
        );
        p.with_clip_rect(clip).text(
            at,
            Align2::LEFT_TOP,
            s,
            f.clone(),
            metal(t0 + 0.5 / bands as f32),
        );
    }
}

/* ---------------------------------------------------------- the section marker --
 *
 * THE MARKER IS A CHEVRON: POINTING RIGHT WHEN SHUT, DOWN WHEN OPEN.
 *
 * It used to be a bar that lay flat when the section was open and stood up when it was shut, and
 * the argument for the bar was internal consistency: a chevron is a THIRD shape vocabulary in a
 * rail that already has a trailing bar for attention, and every
 * extra shape costs the reader something.
 *
 * That argument was wrong the way internal consistency is usually wrong. The chevron is THE
 * disclosure convention. It is already in the reader's head before this app is opened, carried in
 * from every file tree, every accordion and every settings pane they have ever used, and a rail
 * that beats a convention that strong charges every reader the price of learning a local dialect
 * in order to save itself one shape. The shape is cheaper than the lesson. The bar had to be
 * taught here and nowhere else, and nothing that has to be taught in the rail is worth the rail.
 *
 * So this is settled, and it was asked for by name. Do not re-litigate it.
 *
 * The chevron sweeps the same 7px optical cell the bar swept, in the same place, in the header's
 * own colour, so nothing else in the row had to move to make room for it. */

/// Arm to arm: the chevron's long axis, matched to the 7px bar it replaces so the marker occupies
/// the cell the header already reserved.
const CHEV_SPAN: f32 = 7.0;
/// Arm line to apex. Shallower than the 45 degrees a 7x7 square would force, because at 9.5pt a
/// 45 degree chevron reads as an arrowhead and a shallow one reads as a fold.
const CHEV_DEPTH: f32 = 4.0;
/// A hair over a hairline, so the two arms read as ONE mark rather than as two scratches.
const CHEV_STROKE: f32 = 1.3;

/// The chevron's three points, arm to apex to arm, centred on `c`. It points DOWN when the section
/// is open and RIGHT when it is shut. The mark states WHERE THE ROWS ARE, not where a click would
/// send them: down means they are below you already.
fn chevron(c: Pos2, open: bool) -> [Pos2; 3] {
    let s = CHEV_SPAN * 0.5;
    let d = CHEV_DEPTH * 0.5;
    if open {
        [
            Pos2::new(c.x - s, c.y - d),
            Pos2::new(c.x, c.y + d),
            Pos2::new(c.x + s, c.y - d),
        ]
    } else {
        [
            Pos2::new(c.x - d, c.y - s),
            Pos2::new(c.x + d, c.y),
            Pos2::new(c.x - d, c.y + s),
        ]
    }
}

/// What a section header is saying this frame, as one named argument instead of a bare bool.
///
/// IT CARRIED TWO FACTS AND THE SECOND ONE IS GONE. `dot` said whether anything under the section
/// wanted a person, and `nav::section_wants_you` answered it off the one row that named the
/// missing Logs folder. That row moved into the Settings screen, so nothing can answer the
/// question from a section any more and the field went with the answer; the argument, and what
/// still reports that fact, is written out in `nav.rs` where the function stood.
///
/// THE STRUCT STAYS FOR THE REASON IT ARRIVED, WHICH IS NOT COUNTING. `section(ui, head, open,
/// dot)` and `section_narrow(ui, head, open, dot)` both ended in `(bool, bool)`, so transposing a
/// section's open state with its attention dot compiled, passed clippy and passed every test in
/// the suite. One field cannot be transposed with itself, so this is no longer load bearing today;
/// it is the header's marks, it is where the next one goes, and unwrapping it to a bare `bool`
/// would be a churn of every call site to save a word.
#[derive(Clone, Copy, Debug)]
pub struct SectionHead {
    /// Whether the section stands open. Turns the chevron and lights the label.
    pub open: bool,
    /// Whether this heading has anything to open at all.
    ///
    /// FALSE FOR A SECTION WITH NO ROWS, which today is PLAY: it is where the launcher will go
    /// and it holds nothing yet. A heading like that draws NO CHEVRON and takes NO CLICK, and
    /// both halves matter for the same reason. A chevron is a promise that something is under
    /// there; a label that brightens under the pointer is a promise that pressing it does
    /// something. This heading can keep neither promise, so it makes neither.
    ///
    /// THE FIELD IS NOT `rows.is_empty()` READ AT THE PAINT SITE, because `chrome` never sees the
    /// rows: it is handed a label and a mark and paints them. The decision is `rail_plan`'s, which
    /// is where every other rail fact is decided, and this crosses as a mark like the other one.
    pub foldable: bool,
    /// Drawn at full gold whatever else is true of it.
    ///
    /// TRUE FOR THE LAUNCHER AND NOTHING ELSE. Every other heading in this rail says its state
    /// with its colour: gold when open, dim under the pointer, in shadow when shut. PLAY has no
    /// state to say, because it does not open and it does not shut; it is the primary act of the
    /// app, so it is lit the way the one open section is lit and it stays there.
    ///
    /// IT IS NOT `!foldable`, AND THE DIFFERENCE IS VISIBLE ON SCREEN TODAY. ADVENTURE is empty
    /// too, so it is not foldable either, and it must NOT be gold: it is a placeholder waiting
    /// for rows, and lighting it would make an empty section look like a control. Two facts,
    /// two fields.
    pub lit: bool,
}

/// The mark on a heading that is a BUTTON rather than a fold: a play triangle, pointing right.
///
/// A CHEVRON TURNS AND THIS DOES NOT, which is the whole of what the two shapes say. A chevron
/// is about a state, open or shut, and it moves when that state does. This is about an ACT: it
/// points the way a play control has pointed since tape, it means the same thing every time you
/// look at it, and there is nothing for it to turn into.
///
/// IT SITS IN THE GUTTER, LEFT OF THE WORD, and the word does not move to make room. The rail's
/// headings and its rows share one left column (`the_header_and_a_row_share_one_left_column`),
/// and a heading that indented itself for a marker would break the one alignment the rail has.
fn action_mark(p: &egui::Painter, at: Pos2, col: Color32) {
    let h = ACTION_MARK;
    let w = h * 0.86;
    p.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(at.x - w * 0.5, at.y - h * 0.5),
            Pos2::new(at.x + w * 0.5, at.y),
            Pos2::new(at.x - w * 0.5, at.y + h * 0.5),
        ],
        col,
        Stroke::NONE,
    ));
}

/// The play triangle's height, in points. Sized under `TEXT_X` so it clears the word's column.
const ACTION_MARK: f32 = 8.0;

/// A section header in the rail: ADVENTURER, TRADESMAN, CHARACTER. Clickable, it says whether its
/// section stands open.
///
/// IT CARRIED AN ATTENTION DOT AND NO LONGER DOES. See [`SectionHead`] for why, and `nav.rs` for
/// what still reports the one fact it carried.
pub fn section(ui: &mut Ui, label: &str, head: SectionHead) -> Response {
    let SectionHead {
        open,
        foldable,
        lit,
    } = head;
    ui.add_space(11.0);
    /* A HEADING WITH NOTHING UNDER IT IS NOT CLICKABLE, and that is enforced here rather than by
     * the caller ignoring the answer. `Sense::hover` means `clicked()` can never come back true,
     * so there is no arm anywhere that has to remember to check, and no hover highlight below
     * either, because `resp.hovered()` is what lights the label. */
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 16.0),
        if foldable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let p = ui.painter();

    /* LIT FIRST, because it is not a state among the others: a heading that is always gold is
     * always gold, and the arms below are the ordinary heading saying whether it is open. */
    let col = if lit || (open && foldable) {
        GOLD
    } else if resp.hovered() && foldable {
        GOLD_DIM
    } else {
        GOLD_DEEP
    };
    /* THE ACTION MARK, for a heading that is a button. It goes in the gutter so the word keeps
     * the rail's one left column. */
    if lit {
        action_mark(
            p,
            Pos2::new(rect.left() + TEXT_X * 0.5, rect.center().y),
            col,
        );
    }
    tracked(
        ui,
        Pos2::new(rect.left() + TEXT_X, rect.top()),
        label,
        9.5,
        1.8,
        col,
    );

    /* The chevron, right aligned where the bar was, in the header's own colour so the marker and
     * the word brighten together under the pointer instead of the marker sitting at one fixed
     * value while the label answers the hover.
     *
     * AND NONE AT ALL WHEN THERE IS NOTHING TO OPEN. A chevron pointing at an empty section is
     * the app telling the reader to press it. */
    if foldable {
        let cx = rect.right() - 14.0;
        let cy = rect.center().y;
        let [a, apex, b] = chevron(Pos2::new(cx, cy), open);
        p.add(egui::Shape::line(
            vec![a, apex, b],
            Stroke::new(CHEV_STROKE, col),
        ));
    }

    ui.add_space(2.0);
    resp
}

/// The section marker alone, for the collapsed rail: THE SAME CHEVRON, clickable, with the
/// section's name on hover. With the words gone a section can still be opened and closed here,
/// which is what lets the two-open cap hold in both widths: the first cut drew every row of every
/// section in the narrow rail, 29 squares and the cap bypassed, because it had no heading to click.
///
/// This drew a rotating bar until the rail's headers turned a chevron, at which point the wide
/// rail and the narrow rail were showing two different shapes for one act. A marker that changes
/// shape when the rail is resized teaches the reader that the two are different things.
pub fn section_narrow(ui: &mut Ui, label: &str, head: SectionHead) -> Response {
    let SectionHead {
        open,
        foldable,
        lit,
    } = head;
    ui.add_space(8.0);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 12.0),
        if foldable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    /* The narrow marker takes the same colour the word would have, so the rail says one thing
     * about a heading at both widths. */
    let col = if lit || (open && foldable) {
        GOLD
    } else if resp.hovered() && foldable {
        GOLD_DIM
    } else {
        GOLD_DEEP
    };
    let c = rect.center();
    let p = ui.painter();
    if foldable {
        let [a, apex, b] = chevron(c, open);
        p.add(egui::Shape::line(
            vec![a, apex, b],
            Stroke::new(CHEV_STROKE, col),
        ));
    } else {
        /* THE SAME MARK AS THE WIDE RAIL, AND IT USED TO BE A DOT.
         *
         * The narrow rail is markers only, so a heading that drew its chevron conditionally and
         * nothing else would VANISH at one width and come back at the other, which reads as the
         * app losing a section. The dot held that place while the only non-folding headings were
         * unnamed placeholders. The launcher is the only one now, and it has a mark of its own,
         * so drawing something ELSE here would be the rail saying two shapes for one thing
         * depending on how wide it is, which is the exact mistake the chevron was introduced to
         * fix (see this function's own note). */
        action_mark(p, c, col);
    }
    ui.add_space(2.0);
    resp.on_hover_text(label)
}

/// THE BAR, and it is the BODY's marker now rather than the rail's: lying flat when open, standing
/// when closed, turning between the two as egui's `openness` runs 0 to 1. Passed as
/// `.icon(chrome::fold_icon)`.
///
/// The rail's section headers turn a chevron and these deliberately do not follow, because a body
/// list is not the rail. What this replaces is egui's default filled triangle, and that argument
/// is unchanged: a third shape earns nothing here. A fold inside a table is already sitting on the
/// thing it discloses, so it needs a mark that takes no room, not one that teaches a convention.
/// A SECTION OF THE DESTINATION ABOVE IT: tier 3, nested in the one rail.
///
/// A SECOND PANEL WAS THE FIRST ANSWER AND IT WAS THE WRONG ONE. Five of forty-four
/// destinations have sections, so a panel for them was absent seven times out of eight and the
/// whole body jumped 132 points sideways whenever it appeared or left. It also had to caption
/// itself with the destination's name, because a list of three words in a panel of its own says
/// nothing about what it belongs to. Nesting answers both: the rows sit under the row they
/// belong to, the rail keeps its width, and nothing moves when you navigate.
///
/// THE SPINE IS WHAT SAYS THEY BELONG. Each of these paints one point of hairline at the same x,
/// so a run of them draws a single unbroken rule down the left of the group, and the selected
/// one thickens its own piece of that rule to gold. That is deliberately NOT the marker a
/// destination uses: `nav_row` lights the rail's outer edge at x=0 for the destination you are
/// in, and both are true at once (you are in Log Parser, and in its Fights section), so the two
/// marks have to be legible at the same time and cannot be the same mark.
///
/// SMALLER AND QUIETER THAN A DESTINATION, because it is subordinate to it and a rail where
/// every row shouts equally is a rail with no shape.
pub fn sub_row(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let h = 22.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let p = ui.painter();

    if selected {
        p.rect_filled(rect, 0.0, PANEL_2);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, PANEL);
    }

    let x = rect.left() + SPINE_X;
    p.rect_filled(
        Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(1.0, h)),
        0.0,
        RULE,
    );
    if selected {
        p.rect_filled(
            Rect::from_min_size(Pos2::new(x, rect.top() + 3.0), Vec2::new(2.0, h - 6.0)),
            0.0,
            GOLD,
        );
    }

    let col = if selected {
        FLARE
    } else if resp.hovered() {
        GOLD_HI
    } else {
        TEXT_3
    };
    let area = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() - h),
        Pos2::new(rect.right() - BAR_LANE, rect.bottom() + h),
    );
    p.with_clip_rect(area).text(
        Pos2::new(rect.left() + SUB_X, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        col,
    );
    resp
}

pub fn fold_icon(ui: &mut Ui, openness: f32, response: &Response) {
    let rect = response.rect;
    let c = rect.center();
    let col = if response.hovered() {
        GOLD_DIM
    } else {
        GOLD_DEEP
    };
    /* 0 = closed = standing; 1 = open = flat. Width and height swap through the turn. */
    let w = 1.0 + 6.0 * openness;
    let h = 7.0 - 6.0 * openness;
    ui.painter()
        .rect_filled(Rect::from_center_size(c, Vec2::new(w, h)), 0.0, col);
}

/// The same bar again, as the open marker of an `egui::ComboBox`: `.icon(chrome::combo_icon)`.
/// egui's default is a filled triangle, a third shape after the rail's chevron and this bar, so
/// every combo in the app passes this instead and the body keeps to one marker.
pub fn combo_icon(ui: &Ui, rect: Rect, visuals: &egui::style::WidgetVisuals, is_open: bool) {
    let c = rect.center();
    let bar = if is_open {
        Vec2::new(7.0, 1.0)
    } else {
        Vec2::new(1.0, 7.0)
    };
    /* the widget's own foreground, so a hovered combo's marker brightens with its text */
    ui.painter()
        .rect_filled(Rect::from_center_size(c, bar), 0.0, visuals.fg_stroke.color);
}

/* -------------------------------------------------------- THE COUNT BADGE, GONE --
 *
 * `BADGE_SIZE`, `BADGE_H`, `BADGE_MIN_W`, `BADGE_PAD_X`, `BADGE_GAP`, `badge_text`, `badge_w`,
 * `badge_span` and the gilt pill `nav_row` painted between the label and the bar's lane all stood
 * here. They are deleted. The rule they drew, and the one thing that would have to be true before
 * any of it comes back, are written where the numbers were decided, in `nav.rs`.
 *
 * ONE PART OF IT WAS ALWAYS RIGHT AND IS WORTH SAVING FROM THE DELETION, because it is the rule
 * that will be got wrong if a badge ever returns. The original's CSS is `.cnt:empty{display:none}`
 * and the script that fills it writes `f?f:''`, so a count that has fallen to zero writes the
 * empty string. An absent badge is not a zero. It is silence, and it takes no width: the label
 * gets back every pixel, which is why the clip below subtracts nothing but `BAR_LANE`.
 *
 * WHAT KILLED IT was not the drawing but the numbers. The five rows that carried one were carrying
 * the length of a shipped JSON file, the same digits on every launch forever, and a badge that
 * never moves is furniture the label has to compete with. `.cnt` in the original is on In flight
 * and Work orders, which are queues. This build has no queue. */

/// The lane the trailing attention bar owns at the right edge: the 3px bar and its clearance.
/// RESERVED ON EVERY ROW, bar or no bar, so a label's last glyph does not sit against the bar on
/// the one row that has one and out in the margin on every row that does not.
const BAR_LANE: f32 = 8.0;

/// THE ONE TEXT COLUMN. Section headers and nav rows start their words at the same x, and this is
/// that x.
///
/// It used to be two numbers, 14.0 in `section` and 22.0 in `nav_row`, and with a status square
/// sitting in a gutter near +5 that made THREE left edges down a 252px rail, none of which agreed.
/// The eye reads a ragged left margin as a mistake even when it cannot say which element moved.
///
/// 16 IS THE ORIGINAL'S NUMBER, not a number picked to look right. `.pg a{padding:6px 16px}` and
/// `.pg em{padding:0 16px 8px}` set the rows and the headers on one column at 16, and with the
/// invented leading square gone there is nothing left to hold a gutter open for.
///
/// A header is not indented relative to its rows because the chevron already marks it as the
/// group; indenting the rows as well would say the same thing twice.
const TEXT_X: f32 = 16.0;

/// Where a nested section's spine runs, and where its word starts. Both measured from the
/// destination's own text column so the indent reads as one step in rather than a new margin.
const SPINE_X: f32 = TEXT_X + 5.0;
const SUB_X: f32 = TEXT_X + 15.0;

/// One navigation row.
///
/// THE SIGNAL VOCABULARY, and it is the Gnomish console's, unchanged:
///   (no leading mark: the original has none, and the one that was here was invented)
///   trailing 3px bar = attention, and it is TRAILING, never leading
///   a hollow ring    = idle, which is why idle is drawn as a stroke and never a fill
///
/// A count badge was the third of those and it is gone; see the block above. The bar survives the
/// collapsed rail, because collapsing is meant to drop the WORDS and not the signals, so a narrow
/// row now paints the bar and nothing else at all.
pub fn nav_row(
    ui: &mut Ui,
    label: &str,
    selected: bool,
    st: Option<State>,
    narrow: bool,
) -> Response {
    let h = 26.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let p = ui.painter();

    if selected {
        p.rect_filled(rect, 0.0, PANEL_2);
        /* the selected marker is a LEFT edge, so it cannot be confused with the trailing
         * attention bar on the right */
        p.rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(2.0, h)),
            0.0,
            GOLD,
        );
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, PANEL);
    }

    /* NO LEADING SQUARE. It was an invention and it is gone.
     *
     * The rail carried a 6px status square in a left gutter, ported from the Gnomish console's
     * vocabulary where a row's leading square IS its status. This is not that console. In
     * `web/app.html` a nav row is `.pg a{padding:6px 16px;border-left:2px solid transparent}`
     * with no leading mark of any kind: selection is the left border turning gilt, and the only
     * thing on the right is `.cnt`, a count. Three marks down one 252px rail, two of them
     * invented here, is how a ported design stops being the design.
     *
     * `st` STAYS, and is not now vestigial: the trailing attention bar below still reads it. */

    if !narrow {
        let col = if selected {
            FLARE
        } else if resp.hovered() {
            GOLD_HI
        } else {
            TEXT_2
        };
        /* The label gets the row minus the bar's lane, and nothing else. That subtraction used to
         * have a second term, the badge's span, and with the badge gone the label owns everything
         * up to the lane on every row: the empty rule, arrived at by deletion.
         *
         * Only the RIGHT edge of this clip does any work. It runs well past the top and bottom of
         * the row on purpose, because a clip rectangle that cuts ink is how the wordmark once read
         * "EOL GRIMOIRE" (see `ramp_text`): a clip meant to bound a layout must never be the thing
         * that decides how tall a glyph is allowed to be. */
        let area = Rect::from_min_max(
            Pos2::new(rect.left(), rect.top() - h),
            Pos2::new(rect.right() - BAR_LANE, rect.bottom() + h),
        );
        p.with_clip_rect(area).text(
            Pos2::new(rect.left() + TEXT_X, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.5),
            col,
        );
    }

    /* the trailing attention bar, 3px, right edge, and only when something wants you */
    if matches!(st, Some(State::You)) {
        p.rect_filled(
            Rect::from_min_size(Pos2::new(rect.right() - 3.0, rect.top()), Vec2::new(3.0, h)),
            0.0,
            YOU,
        );
    }
    resp
}

/// THE TABS OF THE SECTION YOU ARE IN. The rest of the context header is [`crumb`], [`pip`],
/// [`lock`] and [`header_button`], which are the controls the owner laid that row out with.
///
/// # THIS FUNCTION HAD A TWIN AND THE TWIN IS GONE
///
/// `context_bar_why` took a closure so a tab could carry an `on_hover_text`, and it existed for
/// exactly one caller: the Dashboards role tabs, which had four paragraphs of explanation moved
/// off the page and onto a `role_hover` function.
///
/// THE ROLES ARE GONE, SO THE CLOSURE IS GONE. Keeping it would have left a public function
/// whose one caller passed `|_| None` on every frame, which is a parameter that cannot do
/// anything and a reader having to prove that for himself. A hook kept for a caller that might
/// arrive is this codebase's signature defect written on purpose; the day a tab needs a reason
/// again, the closure comes back with the tab that needs it.
pub fn context_bar(ui: &mut Ui, tabs: &[&str], active: &mut usize) {
    ui.horizontal(|ui| {
        for (i, t) in tabs.iter().enumerate() {
            let on = *active == i;
            let txt = egui::RichText::new(*t)
                .font(FontId::proportional(12.5))
                .color(if on { FLARE } else { TEXT_2 });
            let hit = ui.add(
                egui::Button::new(txt)
                    .fill(if on { PANEL_2 } else { Color32::TRANSPARENT })
                    .stroke(Stroke::NONE),
            );
            if hit.clicked() {
                *active = i;
            }
        }
    });
}

/* ------------------------------------------------- the context header`s own controls -- */

/// THE BREADCRUMB: where you are, in the rail`s own words.
///
/// # THIS WAS CUT ONCE AND THE OWNER ASKED FOR IT BACK
///
/// `context_bar` used to lead with `the-tavern/guild/attendance` and the crumb was removed on the
/// grounds that the rail says it three times over: the heading is lit, the row is lit, and the
/// section is lit on its own spine. That argument was sound for a header whose left hand side was
/// already full of tabs. It is not sound now: Dashboards carries no tabs at all, so without this
/// the left of that row is empty and the one line with controls on it says nothing about where
/// the controls are.
///
/// THE WORDS ARE NEVER TYPED HERE. `nav::name_of` and `nav::sections_of` are where they live, so
/// a crumb cannot spell a destination differently from the rail six inches to its left.
///
/// THE SEPARATOR IS DIM AND THE LAST PART IS LIT, because a crumb is read backwards: what you are
/// looking at is the end of it, and the parts before it are how you got there.
///
/// THE SEPARATOR IS `>` AND THE LAST PART IS BOLD, off the mock's `.top-sep` (`#505966`, margin
/// `0 8px`) and its `.crumb strong` (`--ink`, weight 600). The parts before it are `--muted`. A
/// crumb set in one weight is a path; a crumb whose last part is bold is a PLACE with a path in
/// front of it, which is what a reader is actually looking for on that row.
pub fn crumb(ui: &mut Ui, parts: &[&str]) {
    ui.spacing_mut().item_spacing.x = 6.0;
    for (i, p) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        if i > 0 {
            ui.label(
                egui::RichText::new("\u{203a}")
                    .font(FontId::proportional(13.0))
                    .color(Color32::from_rgb(0x50, 0x59, 0x66)),
            );
        }
        let mut t = egui::RichText::new(*p)
            .font(FontId::proportional(12.5))
            .color(if last { TEXT } else { TEXT_2 });
        if last {
            t = t.strong();
        }
        ui.label(t);
    }
}

/// HOW BIG A HEADER ICON IS. One number, so the lock and the picture-in-picture cannot drift.
const ICON: f32 = 18.0;

/// THE PICTURE IN PICTURE CONTROL: put this page in its own window.
///
/// # IT WAS THE WORDS `Pop out` AND THE OWNER ASKED FOR THE ICON
///
/// Same control, same door: `windows::Slot`, `main::pop_out`. What changed is that a text button
/// on the left of a header is a thing a reader has to read on every frame to ignore, where the
/// picture-in-picture glyph is the one shape every video player, browser and phone in the world
/// already uses for exactly this.
///
/// PAINTED AND NOT TYPED, because the glyph is not in this app`s font atlas and a character the
/// atlas lacks comes out as an empty box on the owner`s machine and as nothing at all in a test.
/// It is an outlined frame with a smaller solid frame in its lower right corner, which is the
/// shape itself and not an approximation of it.
///
/// LIT WHEN THE WINDOW IS ALREADY OPEN, because the control does something different then: it
/// raises that window rather than making one. A control whose meaning changes must look different
/// while it means the other thing.
pub fn pip(ui: &mut Ui, is_open: bool) -> Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(ICON), Sense::click());
    let col = if is_open {
        GOLD
    } else if r.hovered() {
        TEXT
    } else {
        TEXT_2
    };
    let p = ui.painter();
    let outer = Rect::from_center_size(rect.center(), Vec2::new(14.0, 11.0));
    p.rect_stroke(outer, 2.0, Stroke::new(1.2, col), egui::StrokeKind::Inside);
    let inner = Rect::from_min_max(
        Pos2::new(outer.right() - 7.0, outer.bottom() - 5.5),
        Pos2::new(outer.right() - 1.5, outer.bottom() - 1.5),
    );
    p.rect_filled(inner, 1.0, col);
    r
}

/// THE LOCK: is the page's layout pinned?
///
/// # A COMBINATION LOCK, AND NOT A PADLOCK
///
/// This drew a padlock first, then a padlock with the words `Layout locked` beside it, and the
/// owner cut both: no words, and the shape he asked for is the dial lock, the one off a school
/// locker with the numbered wheel on its face.
///
/// HE IS RIGHT AND THE REASON IS NOT TASTE. A padlock is the most overloaded glyph in software:
/// it means HTTPS in a browser, a protected field in a form, a private repository, a paywall. On
/// a header that also carries a picture-in-picture control it reads as `this is secured`, which
/// is a claim this app is not making. A dial lock is not used for security anywhere in a user
/// interface, so it carries no second meaning to be confused with, and the DIAL is a picture of
/// the thing it actually does: something you turn to set, and turn again to change.
///
/// AND THE WORDS WENT because the row already says what it is about. The Widgets button is
/// immediately to its left and the breadcrumb reads `Log Parser / Dashboards`; a lock on that row
/// can only be the dashboard's. Two words on a header are two words on every frame forever.
///
/// # THE TWO STATES ARE TWO SHAPES AND NOT ONE TINT
///
/// A control whose only difference is its colour reads identically to somebody who cannot tell
/// gold from grey at a glance, and identically in a screenshot. Locked draws the shackle down and
/// closed against the body with the dial's mark at twelve; unlocked lifts the shackle clear and
/// turns the mark, which is what a lock that has been dialled open looks like.
pub fn lock(ui: &mut Ui, locked: bool) -> Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(ICON + 8.0), Sense::click());
    dial_lock(ui, rect, locked, r.hovered());
    r
}

/// THE COMBINATION LOCK ITSELF, painted into a rect somebody else allocated.
///
/// PAINTED AND NOT TYPED, for the reason [`pip`] is: this app ships one font and that font has no
/// dial lock in it. A character the atlas lacks comes out as an empty box on the owner's machine
/// and as nothing at all in a headless test, which is the worst of both.
///
/// THE PROPORTIONS ARE THE OBJECT'S. A real dial lock is a rounded body a little taller than it
/// is wide, a shackle that is a touch narrower than the body, and a dial that fills most of the
/// face. Drawn any other way it reads as a padlock with a dot on it.
pub fn dial_lock(ui: &mut Ui, rect: Rect, locked: bool, hot: bool) {
    let col = if locked {
        if hot {
            GOLD_HI
        } else {
            GOLD
        }
    } else if hot {
        TEXT
    } else {
        TEXT_2
    };
    let p = ui.painter();
    let c = rect.center();

    /* THE BODY, which is the same in both states: a reader tracking the control across a click
     * needs one fixed part to track it by. */
    let body = Rect::from_center_size(c + Vec2::new(0.0, 3.5), Vec2::new(13.0, 12.0));
    p.rect_filled(body, 3.0, col);

    /* THE SHACKLE. Closed sits down on the body; open lifts clear of it and hinges right, which
     * is the way a dial lock actually opens. */
    let lift = if locked { 0.0 } else { 3.0 };
    let hinge = if locked { 0.0 } else { 2.5 };
    let top = body.top() - 4.0 - lift;
    let left = c.x - 4.0 + hinge;
    let right = c.x + 4.0 + hinge;
    let st = Stroke::new(1.6, col);
    p.line_segment(
        [Pos2::new(left, body.top()), Pos2::new(left, top + 1.2)],
        st,
    );
    p.line_segment(
        [Pos2::new(right, body.top()), Pos2::new(right, top + 1.2)],
        st,
    );
    p.line_segment(
        [Pos2::new(left, top + 1.2), Pos2::new(c.x + hinge, top)],
        st,
    );
    p.line_segment(
        [Pos2::new(c.x + hinge, top), Pos2::new(right, top + 1.2)],
        st,
    );

    /* THE DIAL, which is the whole point of the shape. A ring punched out of the body in the
     * GROUND's colour rather than a stroked circle, because at thirteen points a one pixel ring
     * on a filled body is mud; a hole reads as a face. */
    let face = body.center();
    p.circle_filled(face, 4.2, INK);

    /* AND THE MARK ON IT, which is what says which way the dial is turned. Twelve o'clock when
     * locked, turned away when not: the state is in the geometry and not only in the colour. */
    let turn = if locked {
        -std::f32::consts::FRAC_PI_2
    } else {
        -std::f32::consts::FRAC_PI_2 + 2.2
    };
    let mark = Pos2::new(face.x + turn.cos() * 2.6, face.y + turn.sin() * 2.6);
    p.line_segment([face, mark], Stroke::new(1.3, col));
}
/// THE MOCK'S `.ghost-btn`: a control on the context header.
///
/// `1px solid --line`, radius `6px`, fill `#10151d`, padding `0 12px`. Every button on that row
/// in the design is this one shape, which is why it is one function: the owner's row has `Jump to
/// end`, `+ Widgets` and `Layout locked` on it and they must not be three slightly different
/// buttons.
///
/// LIT WHEN ITS THING IS OPEN OR ON, for the reason [`pip`] lights when its window is: pressing it
/// again closes the thing, and a control that toggles must say which way it is pointing.
pub fn ghost_btn(ui: &mut Ui, label: &str, on: bool) -> Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .font(FontId::proportional(11.5))
                .color(if on { GOLD_HI } else { TEXT_2 }),
        )
        .min_size(Vec2::new(0.0, 30.0))
        .corner_radius(6)
        .fill(if on { PANEL_2 } else { CONTROL })
        .stroke(Stroke::new(1.0, if on { GOLD_DIM } else { LINE })),
    )
}

/// THE MOCK'S `.icon-btn`: the square version of [`ghost_btn`], `38x38` there and `30x30` here.
///
/// SMALLER THAN THE DESIGN ON PURPOSE AND THIS IS THE ONE PLACE THAT DEVIATES. The mock is a web
/// page at 16px body type; this is a desktop app that has to sit beside a full screen game
/// without eating it, and the whole app is drawn at roughly four fifths of the mock's scale. The
/// SHAPE, the edge, the radius and the fill are the design's; the size is the app's.
pub fn icon_btn(ui: &mut Ui, glyph: &str, on: bool) -> Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(glyph)
                .font(FontId::proportional(12.5))
                .color(if on { GOLD_HI } else { TEXT_2 }),
        )
        .min_size(Vec2::splat(30.0))
        .corner_radius(6)
        .fill(if on { PANEL_2 } else { CONTROL })
        .stroke(Stroke::new(1.0, if on { GOLD_DIM } else { LINE })),
    )
}

#[cfg(test)]
mod quadrant_tests {
    use super::*;

    /// DEFECT: THE WINDOW OPENED 1.75 TIMES TOO BIG FOR A 2x2 SNAP QUADRANT, TWICE.
    ///
    /// # TWO WRONG ANSWERS, AND A GREEN TEST THROUGH BOTH OF THEM
    ///
    /// The owner runs the app in one quarter of a 4K panel and asked for that to be what it
    /// opens at. It shipped wrong twice:
    ///
    ///   1. `[1920.0, 1080.0]` as a constant. Half of 3840 by 2160 in PIXELS, handed to
    ///      `with_inner_size`, which reads POINTS. At his 175% that asked for 3360 real pixels.
    ///   2. The scale division removed entirely, on the strength of a measurement taken through a
    ///      DPI-UNAWARE process. Such a process is shown every rectangle pre-divided by the scale
    ///      factor, so it reports a window 1.75 times too big and a correct one identically.
    ///
    /// AND THE TEST THAT SAT HERE WAS GREEN FOR BOTH. It called the halving with the division
    /// composed the way it BELIEVED production composed it. Production composed it another way
    /// and nothing compared the two, so the test proved the arithmetic and nothing whatever about
    /// the window. That is this tree's signature defect wearing a green tick, for a second time in
    /// one file.
    ///
    /// SO THERE IS ONE FUNCTION NOW, [`quadrant_for`], and this drives exactly what
    /// `main::App::fit_to_quadrant` drives, with nothing composed on either side of it.
    ///
    /// # THE NUMBERS, MEASURED FROM A DPI-AWARE PROCESS
    ///
    /// `SetProcessDpiAwarenessContext(-4)` then `SPI_GETWORKAREA` on the owner's machine: work
    /// area 3840 by 2112 real pixels, so a snap quadrant is 1920 by 1056 real pixels. At 175% that
    /// is 1097 by 603 points, and 1097 by 603 points is what this must return.
    ///
    /// WHAT MUTATION MAKES THIS RED: dropping the scale division (the second defect), dividing by
    /// the zoom alone, or treating the work area as points.
    #[test]
    fn the_window_is_one_snap_quadrant_in_the_units_the_window_is_built_in() {
        /* THE OWNER'S MACHINE, read off a DPI-aware process rather than assumed. */
        let work_px = Vec2::new(3840.0, 2112.0);
        let ppp = 168.0 / 96.0; // 175%

        let q = quadrant_for(work_px, ppp);
        assert_eq!(
            q,
            Vec2::new(1097.0, 603.0),
            "the window is not one quadrant of the owner's screen"
        );

        /* AND IN REAL PIXELS IT IS THE SNAP TARGET. This is the assertion the owner would make
         * with his eyes, and it is the one both shipped defects failed. */
        let real = q * ppp;
        assert!(
            (real.x - 1920.0).abs() <= 1.0 && (real.y - 1056.0).abs() <= 1.0,
            "the window lands at {real:?} real pixels; a 2x2 snap quadrant of this screen is \
             1920 by 1056"
        );

        /* THE SECOND DEFECT, NAMED. Skipping the scale division returns the work area halved in
         * PIXELS, which is 1.75 times too big once egui multiplies it back up. */
        let unscaled = quadrant_for(work_px, 1.0);
        assert_ne!(
            q, unscaled,
            "the scale is not being divided out, so the window will open 1.75 times too big"
        );
        assert!(
            unscaled.x * ppp > 3000.0,
            "this is the shipped defect: it would ask for {} real pixels on a 3840 wide panel",
            unscaled.x * ppp
        );

        /* AT 100% THE TWO READINGS AGREE, which is why both defects looked right to anybody
         * checking on an unscaled display. It is the control, not the proof. */
        assert_eq!(quadrant_for(work_px, 1.0), Vec2::new(1920.0, 1056.0));

        /* THE FLOOR NEVER FORBIDS THE QUADRANT IT WAS MADE FOR. A minimum equal to the quadrant
         * to the pixel is a minimum that can refuse the very snap it was computed for, because
         * the work area can be an odd number of points and the two sides round differently. */
        for w in [q, Vec2::new(1920.0, 1056.0), Vec2::new(801.0, 601.0)] {
            let f = floor_for(w);
            assert!(
                f.x < w.x && f.y < w.y,
                "the floor {f:?} is not under the quadrant {w:?} it was computed from"
            );
            assert!(
                w.x - f.x <= 8.0 && w.y - f.y <= 8.0,
                "the floor {f:?} is far enough under {w:?} to be a second layout target, which \
                 is the thing the owner's rule exists to prevent"
            );
        }

        /* AND NOTHING DIVIDES BY ZERO. egui has no reason to hand over a scale of nought and a
         * window sized from an infinity would be unrecoverable. */
        assert_eq!(quadrant_for(work_px, 0.0), Vec2::new(1920.0, 1056.0));
        assert_eq!(quadrant_for(Vec2::new(2.0, 2.0), 1.0), Vec2::new(1.0, 1.0));
    }
}

/* ------------------------------------------------------------------- the tests --
 * They run a REAL headless frame and read the shapes that came out of it, rather than asserting
 * on the helpers alone. That distinction was the whole value here and it is now the ONLY value:
 * the helper tests that sat beside them all belonged to the count badge, which is gone, so every
 * rule left in this file is one that only ink can prove. A frame is drawn, its shapes are read,
 * and the assertions are about where the ink landed and what colour it was. */
#[cfg(test)]
mod brand_doc_tests {
    /// The brand block's doc told two lies about a strip it does not draw, and this holds it to
    /// what `titlebar.rs` actually paints.
    ///
    /// BOTH CLAIMS WERE LOAD BEARING, which is why prose gets a test here at all. It said the
    /// strip leads with `BUILT FOR BROKEN STOIC` (the run is `BROKEN STOIC`; `BUILT FOR` was cut
    /// and survives only on the hover), and it said moving the maker's line let the strip "stop
    /// repeating a name the wordmark already says louder" (the strip has always drawn `EQL
    /// Grimoire` first, and has to, or an alt tab lands on an unlabelled window). The second was
    /// the stated RATIONALE for the layout, so a reader trusting it would have removed the window
    /// title to finish a job that was never started. The same stale sentence had spread to two
    /// docs in `titlebar.rs`, which is the argument for pinning it rather than just fixing it.
    ///
    /// It reads source text because a doc comment leaves nothing in the binary to assert on. The
    /// window is the `brand` doc alone; the block comment inside the function is free to discuss
    /// what was cut, and does.
    #[test]
    fn the_brand_doc_does_not_describe_a_title_strip_that_is_not_drawn() {
        let src = include_str!("chrome.rs");
        let from = src.find("/// The brand block:").expect("the brand doc");
        let to = src[from..]
            .find("pub fn brand(")
            .map(|i| from + i)
            .expect("brand itself");
        let doc = &src[from..to];
        assert!(
            !doc.contains("`BUILT FOR BROKEN STOIC` now"),
            "the strip's maker run is BROKEN STOIC; BUILT FOR lives on the hover"
        );
        assert!(
            !doc.contains("stop repeating a name"),
            "the strip never stopped: titlebar::strip draws EQL Grimoire first, by necessity"
        );
        assert!(
            doc.contains("titlebar::maker_block") && doc.contains("titlebar::strip"),
            "the doc has to point at the two things that decide what the strip really says"
        );

        /* The claim is about another file, so it is checked against that file and not restated. */
        let strip = include_str!("titlebar.rs");
        assert!(
            strip.contains("const TITLE: &str = \"EQL Grimoire\";"),
            "if the strip ever does drop the window's name, this doc becomes true and must change"
        );
        /* THE MAKER RUN IS NOW DERIVED, so this checks the derivation and not a literal. It used
         * to assert `const NAME: &str = "BROKEN STOIC"` was still in that file. That literal was
         * the strip's own second spelling of a name the live pill was printing as the LOGIN
         * (`Broken_Stoic`), five pixels away on the same row, so pinning it in place pinned half
         * of a disagreement. `settings::DISPLAY_NAME` is the one spelling and the strip
         * upper-cases it. */
        assert!(
            strip.contains("DISPLAY_NAME.to_uppercase()"),
            "the strip's maker run has to come off settings::DISPLAY_NAME"
        );
    }
}

#[cfg(test)]
mod marker_and_row_tests {
    use super::*;
    use egui::epaint::ColorMode;
    use std::cell::Cell;

    /// One headless frame at the wide rail's width, returning every shape it painted together with
    /// the clip it was painted under. `Shape::Vec` is flattened: egui nests, and a test that only
    /// read the top level would find nothing and pass.
    /// Does this heading report a click when one lands on it?
    ///
    /// TWO PASSES, because egui resolves interaction against the widget rects registered on the
    /// PREVIOUS pass: a press and a release in the first frame a widget exists reaches nothing.
    /// A version of this that ran one pass would answer false for every heading and would call a
    /// clickable section unclickable.
    fn takes_a_click(mut head: impl FnMut(&mut Ui) -> Response) -> bool {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let at = Pos2::new(RAIL_WIDE / 2.0, 16.0);
        let pass = |events: Vec<egui::Event>, head: &mut dyn FnMut(&mut Ui) -> Response| -> bool {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(RAIL_WIDE, 600.0))),
                events,
                ..Default::default()
            };
            let mut hit = false;
            let out = ctx.run_ui(input, |ui| hit = head(ui).clicked());
            out.drop_without_applying_deltas();
            hit
        };
        pass(vec![egui::Event::PointerMoved(at)], &mut head);
        pass(
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            &mut head,
        )
    }

    fn painted(add: impl FnMut(&mut Ui)) -> Vec<(Rect, egui::Shape)> {
        let ctx = egui::Context::default();
        /* The real faces: `tracked` asks for Cinzel by name, and a Context carrying egui's own
         * definitions has no family under that name. */
        crate::fonts::install(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(RAIL_WIDE, 600.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, add);
        let shapes = std::mem::take(&mut out.shapes);
        /* a TexturesDelta panics if it is dropped with the font atlas still unapplied */
        out.drop_without_applying_deltas();
        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.clip_rect, cs.shape, &mut flat);
        }
        flat
    }

    fn flatten(clip: Rect, s: egui::Shape, out: &mut Vec<(Rect, egui::Shape)>) {
        match s {
            egui::Shape::Vec(v) => {
                for x in v {
                    flatten(clip, x, out);
                }
            }
            other => out.push((clip, other)),
        }
    }

    /// Every string the frame actually painted, in paint order.
    fn texts(shapes: &[(Rect, egui::Shape)]) -> Vec<String> {
        shapes
            .iter()
            .filter_map(|(_, s)| match s {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// The ink and the face a painted string actually carries, read off the GALLEY and never off
    /// `TextShape::fallback_color`.
    ///
    /// THE FALLBACK IS NOT THE COLOUR. epaint consults `fallback_color` only where the galley holds
    /// `Color32::PLACEHOLDER`; any real colour baked in at layout time wins over it. So a string
    /// laid out in one colour and handed to the painter with another paints the galley's, and a test
    /// that read the fallback would report the colour that lost. `job.sections[0].format` is what
    /// the glyphs are actually made of.
    fn ink(t: &egui::epaint::TextShape) -> (Color32, FontId) {
        let f = &t.galley.job.sections[0].format;
        (f.color, f.font_id.clone())
    }

    /// The one colour every glyph of a header's label was painted in. `tracked` lays a caps run out
    /// a character at a time, so insisting the characters agree also proves the run is one flat
    /// colour rather than something that drifts across the word.
    fn label_ink(shapes: &[(Rect, egui::Shape)]) -> Color32 {
        let mut found: Option<Color32> = None;
        for (_, s) in shapes {
            let egui::Shape::Text(t) = s else { continue };
            let (c, _) = ink(t);
            match found {
                None => found = Some(c),
                Some(prev) => assert_eq!(prev, c, "a caps run is painted in ONE colour"),
            }
        }
        found.expect("the header paints its label")
    }

    /// The header marker: the one three point polyline in the frame, and its colour.
    /// The colour the heading painted its WORD in.
    ///
    /// `tracked` lays one glyph at a time, each its own `TextShape`, all in the same colour, so
    /// this reads the first and asserts the rest agree rather than trusting one letter to speak
    /// for the word.
    fn word_colour(shapes: &[(Rect, egui::Shape)]) -> Color32 {
        let cols: Vec<Color32> = shapes
            .iter()
            .filter_map(|(_, sh)| match sh {
                egui::Shape::Text(t) => t
                    .override_text_color
                    .or_else(|| t.galley.job.sections.first().map(|s| s.format.color)),
                _ => None,
            })
            .collect();
        let first = *cols.first().expect("the heading painted no word at all");
        assert!(
            cols.iter().all(|c| *c == first),
            "the heading painted its letters in more than one colour: {cols:?}"
        );
        first
    }

    /// THE LAUNCHER IS GOLD IN EVERY STATE, AND AN EMPTY PLACEHOLDER IS NOT.
    ///
    /// Both are headings with no rows, so a colour derived from foldability would light them the
    /// same and an empty ADVENTURE would look like the primary control of the app. Reading the
    /// paint is what tells the two apart; reading the mark would only restate the input.
    #[test]
    fn the_launcher_is_gold_whatever_its_state_and_an_empty_placeholder_is_not() {
        for open in [false, true] {
            let play = painted(|ui| {
                section(
                    ui,
                    "PLAY",
                    SectionHead {
                        open,
                        foldable: false,
                        lit: true,
                    },
                );
            });
            assert_eq!(
                word_colour(&play),
                GOLD,
                "open={open}: the launcher is not gold"
            );
            /* AND THE MARK, at the wide width, which the colour alone does not cover: a launcher
             * that had stopped drawing its triangle would still have a gold word. A mutation
             * removing the wide mark survived this test until this assertion existed. */
            assert_eq!(
                action_marks(&play),
                [GOLD],
                "open={open}: the launcher paints no action mark, or paints it in the wrong gold"
            );

            let placeholder = painted(|ui| {
                section(
                    ui,
                    "ADVENTURE",
                    SectionHead {
                        open,
                        foldable: false,
                        lit: false,
                    },
                );
            });
            assert_eq!(
                word_colour(&placeholder),
                GOLD_DEEP,
                "open={open}: an empty placeholder is lit like the launcher"
            );
            /* A heading that is not the launcher carries no action mark at the wide width either,
             * so the triangle cannot become decoration on an ordinary empty section. */
            assert!(
                action_marks(&placeholder).is_empty(),
                "open={open}: a heading that is not a button drew an action mark"
            );
        }

        /* AND THE NARROW MARKER AGREES, so the rail says one thing about a heading at both widths. */
        let narrow = painted(|ui| {
            section_narrow(
                ui,
                "PLAY",
                SectionHead {
                    open: false,
                    foldable: false,
                    lit: true,
                },
            );
        });
        /* THE SAME MARK AS THE WIDE RAIL, in the same gold. A different shape at a different
         * width would be the rail saying two things about one heading. */
        assert_eq!(
            action_marks(&narrow),
            [GOLD],
            "the narrow launcher does not paint its action mark in gold"
        );
        assert!(
            chevrons(&narrow).is_empty(),
            "the narrow launcher drew a chevron beside its action mark"
        );
    }

    /// The CHEVRON: a three point path that is STROKED and not filled.
    ///
    /// THE ACTION MARK IS ALSO THREE POINTS, which is why this asks about the paint and not just
    /// the shape. A triangle counted as a chevron would make "the launcher draws no chevron" true
    /// and unprovable in the same breath: the assertion would go red on the very mark it is
    /// supposed to allow.
    fn chevrons(shapes: &[(Rect, egui::Shape)]) -> Vec<Color32> {
        shapes
            .iter()
            .filter_map(|(_, sh)| match sh {
                egui::Shape::Path(p) if p.points.len() == 3 && p.fill == Color32::TRANSPARENT => {
                    match p.stroke.color {
                        ColorMode::Solid(c) => Some(c),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect()
    }

    /// The ACTION MARK: a three point path that is FILLED and not stroked.
    fn action_marks(shapes: &[(Rect, egui::Shape)]) -> Vec<Color32> {
        shapes
            .iter()
            .filter_map(|(_, sh)| match sh {
                egui::Shape::Path(p) if p.points.len() == 3 && p.fill != Color32::TRANSPARENT => {
                    Some(p.fill)
                }
                _ => None,
            })
            .collect()
    }

    fn marker(shapes: &[(Rect, egui::Shape)]) -> ([Pos2; 3], Color32) {
        let mut found: Option<([Pos2; 3], Color32)> = None;
        for (_, s) in shapes {
            let egui::Shape::Path(path) = s else { continue };
            if path.points.len() != 3 {
                continue;
            }
            assert!(found.is_none(), "a header paints ONE marker");
            let ColorMode::Solid(c) = path.stroke.color else {
                panic!("the marker takes a solid colour, not a gradient");
            };
            found = Some(([path.points[0], path.points[1], path.points[2]], c));
        }
        found.expect("the header paints a three point marker: arm, apex, arm")
    }

    #[test]
    fn the_chevron_points_right_when_the_section_is_shut() {
        let c = Pos2::new(100.0, 50.0);
        let [a, apex, b] = chevron(c, false);
        assert!(apex.x > a.x && apex.x > b.x, "the apex leads to the RIGHT");
        assert_eq!(apex.y, c.y, "and it sits level with the centre of the cell");
        assert!(a.y < c.y && b.y > c.y, "the arms fall back, up and down");
        assert_eq!(
            b.y - a.y,
            CHEV_SPAN,
            "arm to arm is the 7px cell the bar used to sweep"
        );
    }

    #[test]
    fn the_chevron_points_down_when_the_section_is_open() {
        let c = Pos2::new(100.0, 50.0);
        let [a, apex, b] = chevron(c, true);
        assert!(
            apex.y > a.y && apex.y > b.y,
            "the apex leads DOWN, at the rows it has revealed"
        );
        assert_eq!(apex.x, c.x);
        assert!(a.x < c.x && b.x > c.x, "the arms fall back, left and right");
        assert_eq!(b.x - a.x, CHEV_SPAN);
    }

    /// BOTH AXES, AND THAT IS NOT BELT AND BRACES. The first cut of this test asserted only
    /// `apex.x > a.x` for a shut header, and a chevron drawn pointing DOWN passes that: its apex
    /// really is to the right of its left arm. Painting the open marker on a shut section went
    /// green. What separates the two is where the ARMS sit: right pointing hangs both arms on one
    /// vertical, down pointing lays both on one horizontal.
    /// AN UNSELECTED ROW PAINTS NOTHING LEFT OF THE TEXT COLUMN.
    ///
    /// The rail carried an invented 6px status square in a left gutter. Deleting the drawing is
    /// easy and deleting it PERMANENTLY is what this is for: the square is exactly the kind of
    /// mark that gets reintroduced by someone who reads `st: Option<State>` on the signature and
    /// assumes the row is meant to show it.
    ///
    /// Driven with every one of the five states, because a square would have been painted for all
    /// of them and a test that only tried `Idle` would pass while four states still drew one. The
    /// row is unselected so the selection bar, which legitimately starts at x=0, is not painted.
    #[test]
    fn an_unselected_row_paints_nothing_left_of_the_text_column() {
        for st in [
            State::Idle,
            State::Working,
            State::You,
            State::Wrong,
            State::Settled,
        ] {
            let sh = painted(move |ui| {
                nav_row(ui, "Inventory", false, Some(st), false);
            });
            let mut seen = 0usize;
            for (_, sh) in &sh {
                let left = match sh {
                    egui::Shape::Rect(r) => r.rect.left(),
                    egui::Shape::Text(t) => t.pos.x,
                    egui::Shape::Circle(c) => c.center.x - c.radius,
                    _ => continue,
                };
                seen += 1;
                assert!(
                    left >= TEXT_X - 0.5,
                    "something is painted at x={left}, left of the text column at {TEXT_X}, on a \
                     row in state {st:?}"
                );
            }
            assert!(
                seen > 0,
                "the row painted nothing at all in state {st:?}, so this proved nothing"
            );
        }
    }

    /// A SECTION HEADER AND ITS ROWS START THEIR WORDS AT THE SAME X.
    ///
    /// MEASURED OFF REAL INK, NOT COMPARED AGAINST `TEXT_X`. A test that reads the same constant
    /// the drawing reads agrees with itself and not with the screen, and would still pass if both
    /// call sites moved together to somewhere wrong.
    ///
    /// The header paints through `tracked`, one shape per character, so its leftmost text shape is
    /// where its word begins. The row paints its label in one shape and no leading mark at all, so
    /// the leftmost TEXT on each is the thing this compares.
    #[test]
    fn the_header_and_its_rows_share_one_text_column() {
        let leftmost = |v: &[(Rect, egui::Shape)]| {
            v.iter()
                .filter_map(|(_, s)| match s {
                    egui::Shape::Text(t) => Some(t.pos.x),
                    _ => None,
                })
                .fold(f32::INFINITY, f32::min)
        };

        let head = leftmost(&painted(|ui| {
            section(
                ui,
                "CHARACTER",
                SectionHead {
                    open: true,
                    foldable: true,
                    lit: false,
                },
            );
        }));
        let row = leftmost(&painted(|ui| {
            nav_row(ui, "Inventory", false, Some(State::Settled), false);
        }));

        assert!(head.is_finite(), "the header painted no text at all");
        assert!(row.is_finite(), "the row painted no text at all");
        assert!(
            head > 0.0 && row > 0.0,
            "a word starting at x=0 means the rail lost its gutter entirely"
        );
        assert!(
            (head - row).abs() < 0.5,
            "the header starts its word at {head} and the row starts its at {row}; they are one              column and must agree"
        );
    }

    /// A HEADING WITH NOTHING UNDER IT PROMISES NOTHING: NO CHEVRON, NO CLICK, NO HOVER LIGHT.
    ///
    /// PLAY is that heading today. It is where the launcher will go and it holds no rows, and each
    /// of the three absences below is a separate promise the app would otherwise be making:
    ///   the chevron    says something is folded away under here, and nothing is.
    ///   the click      says pressing this does something, and it does not.
    ///   the hover      is how this rail says a thing is pressable at all.
    ///
    /// THE FENCE IS THE OTHER HALF AND IT IS WHY THE FOLDABLE CASE IS DRIVEN TOO. A `section` that
    /// had stopped painting anything would satisfy every absence here, which is the failure this
    /// tree keeps finding, so the same call with `foldable: true` is required to paint the marker
    /// this one is required not to.
    #[test]
    fn a_heading_with_nothing_to_open_paints_no_chevron_and_takes_no_click() {
        for open in [false, true] {
            /* The fence: with rows, this very call paints a marker. */
            let folds = painted(|ui| {
                section(
                    ui,
                    "GENERAL",
                    SectionHead {
                        open,
                        foldable: true,
                        lit: false,
                    },
                );
            });
            let _ = marker(&folds);

            let flat = painted(|ui| {
                section(
                    ui,
                    "PLAY",
                    SectionHead {
                        open,
                        foldable: false,
                        lit: false,
                    },
                );
            });
            assert!(
                chevrons(&flat).is_empty(),
                "open={open}: a chevron points at a section with nothing in it"
            );
            /* AND IT STILL PAINTS ITS NAME. Losing the chevron must not lose the heading. */
            assert!(
                flat.iter()
                    .any(|(_, sh)| matches!(sh, egui::Shape::Text(_))),
                "open={open}: the heading painted no word at all"
            );
        }

        /* THE CLICK, WHICH IS THE PROMISE THE CHEVRON MAKES IN WORDS. The fence first: the same
         * heading WITH rows really does report one, so the refusal below is a difference and not
         * a harness that never clicks anything. */
        assert!(
            takes_a_click(|ui| section(
                ui,
                "GENERAL",
                SectionHead {
                    open: false,
                    foldable: true,
                    lit: false,
                },
            )),
            "a section with rows stopped reporting its click, so the refusal below proves nothing"
        );
        assert!(
            !takes_a_click(|ui| section(
                ui,
                "PLAY",
                SectionHead {
                    open: false,
                    foldable: false,
                    lit: false,
                },
            )),
            "a heading with nothing under it reported a click; pressing it would toggle a section \
             that has nothing to show"
        );

        /* AND IT IS THE SAME AT THE NARROW WIDTH, where the rail is markers only. A heading that
         * drew its chevron conditionally and nothing else would VANISH here, which reads as the
         * app losing a section rather than as a section having nothing to open. */
        let narrow = painted(|ui| {
            section_narrow(
                ui,
                "PLAY",
                SectionHead {
                    open: false,
                    foldable: false,
                    lit: false,
                },
            );
        });
        assert!(
            chevrons(&narrow).is_empty(),
            "the narrow rail drew a chevron for an empty section"
        );
        assert_eq!(
            action_marks(&narrow).len(),
            1,
            "the narrow heading vanished entirely instead of holding its place with a mark"
        );
    }

    #[test]
    fn the_header_paints_the_chevron_and_turns_it_with_the_section() {
        let shut = painted(|ui| {
            section(
                ui,
                "TRADESMAN",
                SectionHead {
                    open: false,
                    foldable: true,
                    lit: false,
                },
            );
        });
        let ([a, apex, b], col) = marker(&shut);
        assert_eq!(a.x, b.x, "shut: the arms hang on one vertical");
        assert!(apex.x > a.x, "shut: and the apex leads right off it");
        assert_eq!(col, GOLD_DEEP, "a shut header, unhovered, rests in shadow");

        let open = painted(|ui| {
            section(
                ui,
                "TRADESMAN",
                SectionHead {
                    open: true,
                    foldable: true,
                    lit: false,
                },
            );
        });
        let ([a, apex, b], col) = marker(&open);
        assert_eq!(a.y, b.y, "open: the arms lie on one horizontal");
        assert!(apex.y > a.y, "open: and the apex leads down off it");
        assert_eq!(col, GOLD, "and the marker takes the header text colour");
    }

    /// THE HEADER PAINTS NO ATTENTION DOT, AT EITHER WIDTH, AND THIS READS THE PAINT TO SAY SO.
    ///
    /// Three tests stood here and drove the dot: that it drew only when the caller asked, that it
    /// sat clear of the chevron, and that it was four pixels of gold. `nav::section_wants_you` was
    /// the only thing that ever asked it, it could only ever answer off the Sources row, and that
    /// row is a section of the Settings screen now. The dot went with it, and the field with the
    /// dot; the argument, and what still reports the fact it carried, is written out in `nav.rs`
    /// where the function stood.
    ///
    /// AN ABSENCE PROVES NOTHING UNLESS THE FRAME REALLY PAINTED, so the chevron is asserted
    /// PRESENT first at both widths and both states. Without that fence a header that drew nothing
    /// at all would pass this, which is the failure mode every absence test in this tree is fenced
    /// against.
    #[test]
    fn a_section_header_paints_no_attention_dot_at_either_width() {
        for open in [false, true] {
            for (width, sh) in [
                (
                    "wide",
                    painted(move |ui| {
                        section(
                            ui,
                            "TRADESMAN",
                            SectionHead {
                                open,
                                foldable: true,
                                lit: false,
                            },
                        );
                    }),
                ),
                (
                    "narrow",
                    painted(move |ui| {
                        section_narrow(
                            ui,
                            "TRADESMAN",
                            SectionHead {
                                open,
                                foldable: true,
                                lit: false,
                            },
                        );
                    }),
                ),
            ] {
                /* the fence: the header really drew its marker */
                let ([a, apex, b], _) = marker(&sh);
                assert!(
                    a != apex && b != apex,
                    "{width} open={open}: the chevron collapsed, so this frame proves nothing"
                );
                let circles = sh
                    .iter()
                    .filter(|(_, s)| matches!(s, egui::Shape::Circle(_)))
                    .count();
                assert_eq!(
                    circles, 0,
                    "{width} open={open}: a section header painted a circle; the attention dot is \
                     gone and nothing has replaced it"
                );
            }
        }
    }

    /// THE LABEL OWNS THE ROW RIGHT UP TO THE ATTENTION BAR'S LANE, AND NOTHING TAKES A BITE OUT
    /// OF IT.
    ///
    /// This is what is LEFT of `the_label_gets_back_every_pixel_an_absent_badge_would_have_taken`
    /// after the badge went, and it is deliberately the half that asserts a presence. That test
    /// drew the same row twice, with a count and without, and compared: with no count left to
    /// draw, the comparison has nothing to compare and the "no pill was painted" half of it would
    /// have been a test asserting the absence of something now absent by construction, which is
    /// green forever and proves nothing.
    ///
    /// What it proves instead is a live number. `nav_row` clips the label to
    /// `rect.right() - BAR_LANE`, and the frame is read for the RECT THE ROW ACTUALLY TOOK rather
    /// than for a constant, so a clip that subtracts anything more (the badge span coming back
    /// uninvited, a margin someone adds) moves the right edge and fails here.
    #[test]
    fn the_label_owns_the_row_up_to_the_attention_bars_lane() {
        let row_right = Cell::new(0.0f32);
        let sh = painted(|ui| {
            row_right.set(nav_row(ui, "In flight", false, None, false).rect.right());
        });
        let clip = sh
            .iter()
            .find_map(|(clip, s)| match s {
                egui::Shape::Text(t) if t.galley.text() == "In flight" => Some(clip.right()),
                _ => None,
            })
            .expect("the label is painted");
        assert!(
            row_right.get() > 0.0,
            "the row took no width, so this proved nothing"
        );
        assert_eq!(
            clip,
            row_right.get() - BAR_LANE,
            "the label is clipped to the bar's lane and to nothing else"
        );
    }

    /// A COLLAPSED ROW DROPS THE WORDS AND KEEPS THE SIGNAL, which is the one sentence the narrow
    /// rail is for.
    ///
    /// It replaces `the_badge_survives_the_collapsed_rail_and_the_label_does_not`, which asserted
    /// the same rule with the badge standing in for "the signals". The badge is gone and the
    /// trailing attention bar is the only signal left, so the bar is what the rule is now about,
    /// and this reads it as INK: a 3px rect in YOU at the row's right edge. Both halves matter and
    /// each catches its own mutation: paint the label when narrow and the text assert fails, drop
    /// the bar from the narrow path and the rect assert does.
    #[test]
    fn a_collapsed_row_paints_its_attention_bar_and_not_its_word() {
        let sh = painted(|ui| {
            nav_row(ui, "In flight", false, Some(State::You), true);
        });
        assert_eq!(
            texts(&sh),
            Vec::<String>::new(),
            "narrow drops the words: {:?}",
            texts(&sh)
        );
        let bar = sh
            .iter()
            .find_map(|(_, s)| match s {
                egui::Shape::Rect(r) if r.fill == YOU && r.rect.width() < 4.0 => Some(r.rect),
                _ => None,
            })
            .expect("a collapsed You row still paints its trailing bar");
        assert!(
            bar.width() > 0.0 && bar.height() > 0.0,
            "the bar has no ink"
        );
    }

    /// ONE MARKER AT BOTH RAIL WIDTHS. The collapsed header drew a rotating bar while the wide one
    /// turned a chevron, which is two shapes for one act, chosen by a resize. This reads the arms,
    /// not just the apex: a down chevron satisfies `apex.x > a.x` as happily as a right one does,
    /// and that is the exact hole that let a mutated marker pass in the wide header's first test.
    #[test]
    fn the_collapsed_header_turns_the_same_chevron() {
        let shut = painted(|ui| {
            section_narrow(
                ui,
                "TRADESMAN",
                SectionHead {
                    open: false,
                    foldable: true,
                    lit: false,
                },
            );
        });
        let ([a, apex, b], _) = marker(&shut);
        assert!(apex.x > a.x && apex.x > b.x, "shut: the apex leads RIGHT");
        assert_eq!(a.x, b.x, "shut: the arms hang on one vertical");
        assert_eq!(
            b.y - a.y,
            CHEV_SPAN,
            "and they sweep the same 7px cell the wide header does"
        );

        let open = painted(|ui| {
            section_narrow(
                ui,
                "TRADESMAN",
                SectionHead {
                    open: true,
                    foldable: true,
                    lit: false,
                },
            );
        });
        let ([a, apex, b], _) = marker(&open);
        assert!(apex.y > a.y && apex.y > b.y, "open: the apex leads DOWN");
        assert_eq!(a.y, b.y, "open: the arms lie on one horizontal");
        assert_eq!(b.x - a.x, CHEV_SPAN);
    }

    /// THE MARKER AND THE WORD ARE ONE INK, and that is asserted as an EQUALITY against the label
    /// rather than against a named constant, because the rule is that the two brighten together.
    /// The defect it catches is a marker pinned to one fixed value while the label alone answers
    /// the hover, which looks correct in whichever single state the author happened to open.
    ///
    /// BOTH STATES ARE READ IN ONE TEST ON PURPOSE. That is what makes the test discriminating
    /// without a mutation run: pin the marker to GOLD and the shut case fails, pin it to GOLD_DEEP
    /// and the open case does, and the closing `assert_ne` refuses the escape of collapsing the two
    /// states onto one colour that satisfies both equalities at once.
    #[test]
    fn the_marker_is_painted_in_the_same_ink_as_the_word_it_marks() {
        let mut inks = Vec::new();
        for open in [false, true] {
            let sh = painted(move |ui| {
                section(
                    ui,
                    "TRADESMAN",
                    SectionHead {
                        open,
                        foldable: true,
                        lit: false,
                    },
                );
            });
            let (_, marker_col) = marker(&sh);
            assert_eq!(
                marker_col,
                label_ink(&sh),
                "open={open}: the marker takes the header's own colour, not a fixed one"
            );
            inks.push(marker_col);
        }
        assert_eq!(
            inks[0], GOLD_DEEP,
            "shut and unhovered, a header rests in shadow"
        );
        assert_eq!(inks[1], GOLD, "open, it takes the body of the metal");
        assert_ne!(
            inks[0], inks[1],
            "and the two states are not one colour wearing two names"
        );
    }
    /// EVERY WORD THE RAIL PAINTS FITS INSIDE THE RAIL.
    ///
    /// READ OFF THE PAINTED GALLEY, never off a character count or a guessed advance width. Cinzel
    /// is a display face and the section headings are letter-spaced on top of it, so an estimate
    /// here would be wrong in the direction that matters: a heading that overhangs its chevron, or
    /// a wordmark clipped to "EQL GRIMOIR", looks like a bug in the font rather than in a number.
    ///
    /// THIS IS THE TEST THE WIDTH CHANGE NEEDED. `RAIL_WIDE` went 252 to 176 and nothing else in
    /// the file records what that width has to hold. Every column it checks is the one the painter
    /// actually uses: the wordmark from its own left inset, a heading from `TEXT_X` to the chevron,
    /// a row from `TEXT_X` to `BAR_LANE`.
    #[test]
    fn every_word_the_rail_paints_fits_inside_it() {
        /* The right edge of the ink of every string painted, by string. */
        let ink = |sh: &[(Rect, egui::Shape)]| -> Vec<(String, f32)> {
            sh.iter()
                .filter_map(|(_, s)| match s {
                    egui::Shape::Text(t) => {
                        Some((t.galley.text().to_owned(), t.pos.x + t.galley.rect.width()))
                    }
                    _ => None,
                })
                .collect()
        };

        /* THE WORDMARK. `brand` paints it band by band through `ramp_text`, so the string arrives
         * as several shapes at the same left inset; the widest right edge is the word's. */
        let mark = ink(&painted(|ui| {
            brand(ui, false);
        }));
        assert!(
            !mark.is_empty(),
            "the brand painted no text, so this proved nothing"
        );
        for (word, right) in &mark {
            assert!(
                *right <= RAIL_WIDE - 6.0,
                "the wordmark {word:?} reaches x={right} in a {RAIL_WIDE}pt rail"
            );
        }

        /* THE HEADINGS, each from the text column to the near side of its chevron. */
        for (head, rows) in crate::nav::NAV {
            let sh = painted(|ui| {
                section(
                    ui,
                    head,
                    SectionHead {
                        open: true,
                        foldable: !rows.is_empty(),
                        lit: rows.is_empty(),
                    },
                );
            });
            let got = ink(&sh);
            assert!(!got.is_empty(), "{head} painted no text");
            for (word, right) in got {
                assert!(
                    right <= RAIL_WIDE - 20.0,
                    "{head}: the glyph {word:?} reaches x={right}, into the chevron's lane, in a \
                     {RAIL_WIDE}pt rail. `tracked` lays a heading out glyph by glyph, so what is \
                     named here is the letter that overhangs and not the whole word"
                );
            }

            /* AND EVERY ROW UNDER IT, AND EVERY SECTION UNDER THOSE. The clip is what would hide
             * an overhang on screen, so the galley is measured and the clip is not consulted.
             *
             * A NESTED SECTION HAS 15 FEWER POINTS THAN A ROW and is the likeliest thing here to
             * overrun, which is exactly why it is measured rather than assumed to fit because its
             * words are shorter. */
            for (label, row_id) in *rows {
                let sh = painted(|ui| {
                    nav_row(ui, label, false, Some(State::Idle), false);
                });
                let got = ink(&sh);
                assert!(!got.is_empty(), "the row {label:?} painted no text");
                for (word, right) in got {
                    assert!(
                        right <= RAIL_WIDE - BAR_LANE,
                        "the row {word:?} reaches x={right}, into the attention bar's lane, in a \
                         {RAIL_WIDE}pt rail"
                    );
                }

                for (name, _, _) in crate::nav::sections_of(*row_id) {
                    let sh = painted(|ui| {
                        sub_row(ui, name, false);
                    });
                    let got = ink(&sh);
                    assert!(!got.is_empty(), "the section {name:?} painted no text");
                    for (word, right) in got {
                        assert!(
                            right <= RAIL_WIDE - BAR_LANE,
                            "the section {word:?} under {label} reaches x={right} in a \
                             {RAIL_WIDE}pt rail"
                        );
                    }
                }
            }
        }
    }
}
