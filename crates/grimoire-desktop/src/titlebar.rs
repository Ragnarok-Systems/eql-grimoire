//! The custom title bar, and every piece of chrome that names Broken Stoic. Decision D8.
//!
//! WHAT LIVES HERE AND WHY IT IS ONE FILE.
//! Three things on screen say who this app is built for: the maker's line under the wordmark, the
//! live pill in every title strip, and the footer. They share one URL, one painted glyph and one
//! idea of what "live" means, so they share a file rather than three copies that drift. The title
//! strip is here because a frameless viewport (decision D3) has no OS chrome and has to paint its
//! own drag region, pin and window buttons, and the pill is the one thing every strip carries.
//!
//! EVERYTHING THAT DECIDES A STRING IS A PURE FUNCTION. `pill_label`, `age_label`, `dot_of` and
//! `footer_label` take primitives and return values; the painting functions only arrange what those
//! return. That is what makes the states testable without a window: a test proves "never checked"
//! does not draw as "offline" without instantiating egui, and that "2m" is two minutes and not
//! one hundred and twenty seconds.
//!
//! THE ONE ROUND SHAPE. The theme is square everywhere (see theme::install) except the pill, which
//! is fully rounded on purpose. It is the one soft shape on screen, and that is exactly what makes
//! it read as a status chip rather than one more panel. Nothing else here may borrow the radius.

use crate::settings::{Platform, DISPLAY_NAME};
use crate::theme::*;
use crate::watcher::{Channel, Status};
use chrono::{DateTime, Duration, Utc};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Pos2, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, Vec2, ViewportCommand,
};

/* ------------------------------------------------------------------ constants -- */

/// Height of the frameless title strip.
pub const STRIP_H: f32 = 32.0;

/// Twitch's own brand purple, `#9146FF`, and its lighter hover, `#A970FF`.
///
/// THESE LIVE HERE AND NOT IN `theme`, DELIBERATELY. `theme` is the Broken Stoic palette: a ground,
/// a five stop gold ramp, and the five state colours, and its rule is that nothing outside a state
/// may borrow a state colour. This purple is neither. It is a FOREIGN MARK, owned by Twitch, and
/// the only reason it is on screen is that a Twitch glyph drawn in gold reads as an ornament while
/// the same glyph in this purple is instantly recognised as the platform. Keeping it beside the one
/// function that paints that glyph is what stops it leaking into the palette and being reached for
/// as "the purple accent" by something that is not Twitch.
pub const TWITCH_PURPLE: Color32 = Color32::from_rgb(0x91, 0x46, 0xFF);
pub const TWITCH_PURPLE_HI: Color32 = Color32::from_rgb(0xA9, 0x70, 0xFF);

/// YouTube's own mark red, `#FF0000`, and a lightened form of it for hover.
///
/// THE SAME EXCEPTION THE PURPLE ARGUES, AND IT IS ARGUED AGAIN RATHER THAN INHERITED, because
/// "there is already a foreign colour in this file" is not a reason for a second one. The reason
/// is the one above: a platform's mark is recognised by its colour before it is read, a YouTube
/// badge drawn in gold is an ornament the designer chose, and the same badge in this red is the
/// platform. `theme` stays the Broken Stoic palette and neither of these belongs in it.
///
/// AND IT IS NOT `WRONG`. `WRONG` is `#C74A3C`, a desaturated brick, and everywhere in this app it
/// means a refusal. This is `#FF0000`: fully saturated, with no green and no blue in it at all,
/// which is the separation that keeps a YouTube mark from reading as an error beside a WRONG row.
/// `no_platform_colour_is_a_state_colour` holds both halves of that claim, the exact inequality
/// and a floor under the distance, because two reds differing in their last bits would satisfy the
/// first and fail a reader.
///
/// `_HI` IS NOT A SECOND BRAND COLOUR. YouTube publishes no hover red, so this is `#FF0000` opened
/// up the way `TWITCH_PURPLE_HI` opens up the purple: same hue, more light, nothing claimed.
pub const YOUTUBE_RED: Color32 = Color32::from_rgb(0xFF, 0x00, 0x00);
/// LIGHTER AND NOT PINKER, and the state palette is why.
///
/// This was `#FF5555`, which sat 26 away from `WRONG` on its furthest channel once `WRONG` took
/// the design's own red (`#EF476F`). `no_platform_colour_is_a_state_colour` caught it: a reader
/// has to be able to tell a platform's mark from a fault, and two reds that close on a small pill
/// are one red.
///
/// THE STATE COLOUR IS THE ONE THAT STAYS PUT, because it is a design token shared by every screen
/// and this is one hover on one pill. Lightening toward white keeps it plainly YouTube's red.
pub const YOUTUBE_RED_HI: Color32 = Color32::from_rgb(0xFF, 0x80, 0x80);

/// A platform's own mark colour, resting and hovered. The one place the two foreign colours above
/// are chosen between, so nothing else has to know which platform owns which.
pub fn mark_colours(on: Platform) -> (Color32, Color32) {
    match on {
        Platform::Twitch => (TWITCH_PURPLE, TWITCH_PURPLE_HI),
        Platform::YouTube => (YOUTUBE_RED, YOUTUBE_RED_HI),
    }
}

/// The pill's height, and therefore its corner radius times two.
const PILL_H: f32 = 20.0;

/// Width of one window button in the strip: pin, minimise, maximise, close.
const BTN_W: f32 = 30.0;

/// The repository the footer links to, as declared by `package.repository` in Cargo.toml.
///
/// EMPTY MEANS NO LINK AND NO WORDS, NOT A GUESS. There is no `repository` field in either
/// manifest, this checkout's origin is a local path, and on 2026-09-02 neither the owner's
/// account nor the Ragnarok-Systems org holds an `eql-grimoire` repository, so nothing in this
/// crate knows where its source lives and there is no GitHub to name. Decision D8 words the
/// footer as `v{version} · <licence> · Source on GitHub` with the last words a link; the house rule
/// says nothing on screen may be invented. Round one drew the words dimmed with a hover
/// explaining that no URL was compiled in, and that is still the words "Source on GitHub" on a
/// page with no GitHub behind them. So the resolution now is: the footer draws the two facts it
/// has (version, licence) and draws the source link ONLY when a repository is declared, worded
/// "Source on GitHub" when that URL is on github.com and "Source" otherwise (`source_label`).
/// Declare `repository` in Cargo.toml and the D8 footer appears with no code change. Cargo
/// defines this env var for every crate, as the empty string when the field is absent.
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/* ---------------------------------------------------------------- pure rules -- */

/// What the dot in the pill means. Three values, because two would force "never checked" to lie.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dot {
    /// The channel answered and it is streaming.
    Live,
    /// The channel answered and it is not.
    Offline,
    /// No source has answered yet, or the last poll failed with nothing known. Decision D2: this is
    /// never shown as offline, because offline is a fact about the channel and this is a fact
    /// about us.
    Unknown,
}

impl Dot {
    /// The dot colour, AND NOT ONE OF THE FIVE STATE COLOURS.
    ///
    /// THIS DOT USED TO BE `WRONG` RED, and that was the one place in the tree that spent a state
    /// colour on something that is not that state. `theme.rs` states the rule without an exception
    /// ("nothing else in the app may carry these colours, because the moment a decorative element
    /// borrows `WRONG` red, a red row stops meaning a refusal"), `persona.rs` cut its whole regard
    /// ladder out of the con colours rather than break it, and a channel that is broadcasting is
    /// not a refusal by any reading. The old comment answered that by asserting an exemption for
    /// itself, which is the one move a rule with no exceptions does not allow.
    ///
    /// SO LIVE IS THE PLATFORM'S OWN MARK COLOUR. The strip this pill sits in already carries that
    /// platform's mark in that exact colour two inches to its left, and a reader who knows what
    /// the purple means on this strip knows what a purple dot means without being taught. The
    /// STATE is carried by the word beside it in any case (`LIVE`, `offline`, `unknown`), so the
    /// colour is doing recognition and not semantics. `WRONG` bought nothing here except a
    /// collision with the one vocabulary the app cannot afford to blur.
    ///
    /// IT TAKES THE PLATFORM, AND THAT IS NOT A TIDY-UP. This used to return `TWITCH_PURPLE` flat,
    /// and its argument for doing so opened "the pill reports one Twitch channel". The pill
    /// reports whichever channel `settings::Settings::watch_on` names, so that premise is simply
    /// false, and a purple dot on a strip set to YouTube would be this file asserting a platform
    /// the pill is not reading. One argument, applied to whichever mark is being reported.
    ///
    /// AND IT IS THE ONLY THING ON THE PILL THAT ANSWERS "WHICH PLATFORM" NOW. The pill stopped
    /// printing the word (see `pill_label`), so this dot is the whole of that signal on the face of
    /// it, with the tooltip carrying it in words. That is more weight than it used to hold, and it
    /// is exactly the weight colour can take: recognition for a reader who already knows the marks,
    /// with the words a hover away for one who does not. It is not asked to carry the STATE too,
    /// which is the load it would break under: `offline` and `unknown` are the same grey on both
    /// platforms, and the state word says which.
    ///
    /// OFFLINE AND UNKNOWN ARE `TEXT_3`, THE DIMMEST TEXT, and they do not vary by platform,
    /// because a brand colour on a channel that is NOT broadcasting would be recognition spent on
    /// nothing. They were `IDLE` once and they are not the rail's idle state either; nothing on
    /// screen moved by a pixel value when that changed, since `IDLE` and `TEXT_3` are the same
    /// colour to the byte, but this file stopped claiming a state it is not in.
    pub fn color(self, on: Platform) -> Color32 {
        match self {
            Dot::Live => mark_colours(on).0,
            Dot::Offline | Dot::Unknown => TEXT_3,
        }
    }

    /// The word after the handle.
    pub fn word(self) -> &'static str {
        match self {
            Dot::Live => "LIVE",
            Dot::Offline => "offline",
            Dot::Unknown => "unknown",
        }
    }

    /// Unknown is drawn hollow: nothing is known, so nothing is filled in, the same rule that
    /// leaves the rail's idle square unfilled. The pill's mark is the mockup's dot (`●`), so the
    /// hollow form here is a ring and not the square the rail draws; one rule, filled against
    /// unfilled, on the shape each place already has. Offline is a filled grey dot because it
    /// IS known.
    pub fn filled(self) -> bool {
        !matches!(self, Dot::Unknown)
    }
}

/// Map the watcher's tri-state to a dot.
pub fn dot_of(live: Option<bool>) -> Dot {
    match live {
        Some(true) => Dot::Live,
        Some(false) => Dot::Offline,
        None => Dot::Unknown,
    }
}

/// The pill's text after the dot: `Broken Stoic · LIVE`, `Broken Stoic · offline`. The dot itself
/// is painted, not typed, because it carries a colour the text does not.
///
/// IT PRINTS THE NAME AND NOT THE WATCHER'S HANDLE, which is what it used to do. The pill read
/// `Broken_Stoic` while the title strip five pixels away read `BROKEN STOIC`, two spellings of one
/// channel on one row, and the pill's was the login: a string shaped by Twitch's rule that a login
/// may not contain a space. `settings::DISPLAY_NAME` is the channel's own spelling of itself and
/// the strip derives its caps from the same constant, so the row cannot disagree with itself
/// again. The login is not dropped, only moved: `pill_tooltip` prints it on the hover, which is
/// where the machine's spelling of a thing belongs.
///
/// AND IT NO LONGER NAMES THE PLATFORM, WHICH IS THE OWNER'S CALL AND HIS REASON IS THE RIGHT ONE.
/// The argument that put `Twitch` on the pill was that `LIVE` is ambiguous once two platforms are
/// polled. It is not, because the pill was never asking the reader's question. The pill's subject
/// is the CHANNEL, one person who streams, and its question is "is he on". Which of his two
/// addresses the app dialled to find out is an implementation fact, and it was being set in the
/// middle of the sentence, in the widest run on the strip, between the name and the answer. Both
/// platforms are still polled; the preference still decides which channel this reads and what the
/// click plays; and `pill_tooltip` still prints the platform, one line down, where a fact about
/// the machine belongs. Two facts, one separator, and the answer is no longer the third thing a
/// reader gets to.
///
/// THE PARAMETER WENT WITH THE WORD, deliberately. Leaving `on` in the signature and ignoring it
/// would leave a caller believing the platform still reaches the label; the compiler now says it
/// does not. `pill_tooltip` and the painter take `Platform` themselves, for the line and the dot's
/// colour, which are the two places it still decides something.
pub fn pill_label(live: Option<bool>) -> String {
    format!("{DISPLAY_NAME} · {}", dot_of(live).word())
}

/// How old a check is, in the smallest unit that keeps it to two or three characters: `59s`, `2m`,
/// `3h`, `4d`. Truncates rather than rounds, so `119s` is `1m` and not `2m`; an age that rounds up
/// claims the check is staler than it is. A negative age (clock skew between the poll thread's
/// stamp and now) is `0s`, never a minus sign.
pub fn age_label(age: Duration) -> String {
    let s = age.num_seconds().max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3_600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3_600)
    } else {
        format!("{}d", s / 86_400)
    }
}

/// The age of a check relative to `now`, or `None` when there has never been one. `now` is a
/// parameter rather than read inside so the function is a pure rule a test can pin.
pub fn age_of(checked_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Option<String> {
    checked_at.map(|t| age_label(now - t))
}

/// How old a check must be before the PILL shows its age.
///
/// Two and a half poll intervals. Under it the watcher is keeping up and the age is noise; over it
/// at least two polls have failed or the thread has stopped, and the state on screen is no longer
/// something the app is entitled to assert quietly.
pub const PILL_STALE_AFTER: Duration = Duration::seconds(225);

/// Whether the pill should print the age at all, and this is the whole rule.
///
/// AN AGE BESIDE A STATE WORD READS AS THE DURATION OF THAT STATE, NOT AS THE AGE OF THE CHECK.
/// `offline 1m` parses as "offline for a minute". `LIVE 30s` parses as "live for thirty seconds",
/// which for a stream is a plausible, useful sounding number and is flatly wrong: it is how long
/// ago we asked, not how long he has been on. A wrong number that looks right is worse than no
/// number, so in the ordinary case the pill shows none and the tooltip carries "checked 1m ago via
/// twitch-gql", where the words make it unambiguous.
///
/// It comes back for exactly one case: when the check is STALE. Then the state on the pill may be
/// hours old and still be drawn as fact, and the age stops being clutter and becomes the warning
/// that the thing you are reading is not current. `Unknown` never needs it, because "unknown"
/// already says the app does not know.
pub fn pill_shows_age(dot: Dot, checked_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    match (dot, checked_at) {
        (Dot::Unknown, _) | (_, None) => false,
        (_, Some(t)) => now - t >= PILL_STALE_AFTER,
    }
}

/// The crate version, from the manifest, so the footer cannot disagree with `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The licence, from the manifest, for the same reason. The workspace declares `AGPL-3.0-only`,
/// and `grimoire-forge`'s licence gate holds the manifest and the `LICENSE` file to each other.
pub fn licence() -> &'static str {
    env!("CARGO_PKG_LICENSE")
}

/// Where the source link goes, if anywhere. See `REPOSITORY`.
pub fn source_url() -> Option<&'static str> {
    if REPOSITORY.is_empty() {
        None
    } else {
        Some(REPOSITORY)
    }
}

/// The words for a source link: D8's "Source on GitHub" when the repository IS on GitHub, plain
/// "Source" for any other host. The host is a fact about the URL, not a fact the footer asserts.
pub fn source_label(url: &str) -> &'static str {
    let host = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    if host.starts_with("github.com/") || host.starts_with("www.github.com/") {
        "Source on GitHub"
    } else {
        "Source"
    }
}

/// The footer's source link, words and URL, or None when nothing is declared and therefore
/// nothing is drawn.
pub fn source_link() -> Option<(&'static str, &'static str)> {
    let url = source_url()?;
    Some((source_label(url), url))
}

/// The hover text for the pill: which source answered, how long ago, and what it said. Every line
/// is a fact the watcher recorded; a field it did not fill is a line that is not printed.
///
/// THE HANDLE IS HERE BECAUSE THE PILL STOPPED PRINTING IT. `pill_label` sets the channel's NAME
/// now, and the login the watcher actually polled is still a fact, so it moved one line down
/// rather than out of the window. It is `ch.handle`, echoed back by whichever rung answered, and
/// not the constant: if a source ever answers for a different login, the hover is where that shows.
///
/// AND THE PLATFORM IS HERE FOR EXACTLY THE SAME REASON, one revision later. The pill stopped
/// setting `Twitch` between the name and the state word; the preference is still what decides
/// which channel the pill reads and what its click plays, so it is still a fact the reader may
/// need, and this is where a fact about the machine goes. It sits beside the handle because the
/// two answer one question together: which address was dialled, and as whom.
pub fn pill_tooltip(ch: &Channel, on: Platform, now: DateTime<Utc>) -> String {
    /* The first line is the pill's own label, so the words under the pointer are the words on
     * the pill and the two cannot say different things about one channel. */
    let mut s = format!("{}\n", pill_label(ch.live));
    match dot_of(ch.live) {
        Dot::Live => s.push_str("live now\n"),
        Dot::Offline => s.push_str("not streaming\n"),
        Dot::Unknown => s.push_str("live status unknown\n"),
    }
    s.push_str(&format!("platform: {}\n", on.label()));
    s.push_str(&format!("handle: {}\n", ch.handle));
    if let Some(t) = ch.title.as_deref().filter(|t| !t.is_empty()) {
        s.push_str(&format!("title: {t}\n"));
    }
    if let Some(g) = ch.game.as_deref().filter(|g| !g.is_empty()) {
        s.push_str(&format!("playing: {g}\n"));
    }
    if let Some(v) = ch.viewers {
        s.push_str(&format!("viewers: {v}\n"));
    }
    match age_of(ch.checked_at, now) {
        Some(age) => s.push_str(&format!("checked {age} ago via {}\n", ch.source)),
        None => s.push_str("not checked yet\n"),
    }
    if let Some(e) = ch.error.as_deref().filter(|e| !e.is_empty()) {
        s.push_str(&format!("last poll: {e}\n"));
    }
    /* WHERE THE CLICK GOES, AND THIS LINE HAD TO CHANGE WITH IT. It read "click to open" followed
     * by `Platform::url`, while the click handed that URL to the system browser. The click plays
     * the stream in this app's own body now (`Ask::WatchHere`), so the old line would be a hover
     * promising a destination the click no longer visits, which is the exact shape of defect this
     * file's tests exist to catch. (The URL itself is not spelled out even in this comment: the
     * test below forbids a literal one anywhere in this file's production text, and it caught this
     * comment on the first run.)
     *
     * TWO WORDINGS, BECAUSE THE CLICK REALLY DOES TWO THINGS. Live, it starts the player. Offline
     * or unknown, there is no stream to start and the app does not embed an offline channel
     * (`screens::watch::feed_for` refuses, and Twitch fills an offline channel with other people's
     * streams), so the click opens the Watch screen and that screen says what the poller knows. A
     * single wording would be wrong in one of the two states, and the state is right here. */
    s.push_str(match dot_of(ch.live) {
        Dot::Live => "click to watch here, in the main window",
        _ => "click to open Watch in the main window",
    });
    s
}

/* -------------------------------------------------------------------- glyphs -- */

/// The Twitch mark, reduced to what survives at nine pixels: a speech bubble with the cut corner at
/// top left, the diagonal at bottom right, the tail hanging off the bottom left, and the two bars
/// inside it. Painted, not a font glyph, because no face in the binary carries it and a colour
/// emoji fallback would be the one thing on the rail not cut from the palette.
pub fn twitch_glyph(p: &egui::Painter, rect: Rect, col: Color32) {
    let (x, y, w, h) = (rect.left(), rect.top(), rect.width(), rect.height());
    let stroke = Stroke::new(1.0, col);
    let body = y + h * 0.82;
    let pts = vec![
        Pos2::new(x + w * 0.22, y),
        Pos2::new(x + w, y),
        Pos2::new(x + w, y + h * 0.60),
        Pos2::new(x + w * 0.78, body),
        Pos2::new(x + w * 0.50, body),
        Pos2::new(x + w * 0.28, y + h),
        Pos2::new(x, y + h),
        Pos2::new(x, y + h * 0.22),
    ];
    p.add(egui::Shape::closed_line(pts, stroke));
    for fx in [0.45, 0.70] {
        let bx = x + w * fx;
        p.line_segment(
            [Pos2::new(bx, y + h * 0.25), Pos2::new(bx, y + h * 0.52)],
            stroke,
        );
    }
}

/// The YouTube mark, reduced to what survives at ten pixels: the badge with its corners taken off,
/// and the play head filled inside it.
///
/// BUILT THE WAY `twitch_glyph` IS BUILT, and that is a requirement rather than a preference. Both
/// marks sit on one row at one size, so a badge drawn from a font glyph, an emoji, or a two pixel
/// stroke would read as the heavier or the odder of a pair that is supposed to read as a set. It
/// is a `Shape::closed_line` at one pixel, sized to the rect it is handed, exactly as the Twitch
/// bubble is. `web/app.html`, the authority for anything visual, has no YouTube mark and no Twitch
/// one either, so there was nothing to port and the precedent in this file is the whole brief.
///
/// THE BADGE IS HOLLOW AND THE PLAY HEAD IS FILLED, which is the split the Twitch mark already
/// makes: a hollow body with solid ink inside it (there, two bars drawn as segments). At this size
/// a one pixel outlined triangle is four pixels across and closes up into a blob, and the play
/// head is the half of this mark that a reader recognises, so it is the half that gets the ink.
///
/// THE CORNERS ARE CUT, NOT ROUNDED. YouTube's badge has a generous radius; an arc across two
/// pixels is one anti-aliased smudge and costs more than the straight chamfer standing in for it,
/// which is the same reduction the Twitch bubble makes of its own cut corner.
///
/// AND IT IS WIDER THAN TALL, so it is inset from the top and bottom of the square rect it is
/// given rather than filling it. A square YouTube badge is recognisably not the mark it imitates.
pub fn youtube_glyph(p: &egui::Painter, rect: Rect, col: Color32) {
    let (x, y, w, h) = (rect.left(), rect.top(), rect.width(), rect.height());
    let stroke = Stroke::new(1.0, col);
    let (top, bot) = (y + h * 0.18, y + h * 0.82);
    let cut_x = w * 0.16;
    let cut_y = (bot - top) * 0.26;
    let pts = vec![
        Pos2::new(x + cut_x, top),
        Pos2::new(x + w - cut_x, top),
        Pos2::new(x + w, top + cut_y),
        Pos2::new(x + w, bot - cut_y),
        Pos2::new(x + w - cut_x, bot),
        Pos2::new(x + cut_x, bot),
        Pos2::new(x, bot - cut_y),
        Pos2::new(x, top + cut_y),
    ];
    p.add(egui::Shape::closed_line(pts, stroke));

    let mid = (top + bot) * 0.5;
    let half = (bot - top) * 0.26;
    p.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(x + w * 0.40, mid - half),
            Pos2::new(x + w * 0.40, mid + half),
            Pos2::new(x + w * 0.64, mid),
        ],
        col,
        Stroke::NONE,
    ));
}

/// One platform's mark, whichever it is. The maker's block walks `Platform::ALL` through this, so
/// a third platform is drawn by adding a variant and an arm and nothing else moves.
pub fn platform_glyph(p: &egui::Painter, on: Platform, rect: Rect, col: Color32) {
    match on {
        Platform::Twitch => twitch_glyph(p, rect, col),
        Platform::YouTube => youtube_glyph(p, rect, col),
    }
}

/// A push pin in a ten pixel box: head, cap, needle. Filled head when pinned, hollow when not,
/// because decision D3 wants a window that is on top to say so and a window that is not to say
/// that too.
///
/// PUBLIC BECAUSE THE PICTURE IN PICTURE WINDOW HAS NO TITLE STRIP AND STILL HAS TO DRAW THIS.
/// `windows::pip` is the Watch window's whole body and it paints its pin into a hover bar of its
/// own; D3's rule is that a window which is always on top says so with THIS glyph, filled against
/// hollow, so a second hand drawn pin in `windows.rs` would be the one signal the doctrine names
/// drifting from itself. One shape, two callers.
pub fn pin_glyph(p: &egui::Painter, c: Pos2, pinned: bool, col: Color32) {
    let top = c.y - 5.0;
    let head = Rect::from_min_size(Pos2::new(c.x - 3.0, top), Vec2::new(6.0, 4.5));
    if pinned {
        p.rect_filled(head, CornerRadius::ZERO, col);
    } else {
        p.rect_stroke(
            head,
            CornerRadius::ZERO,
            Stroke::new(1.0, col),
            StrokeKind::Inside,
        );
    }
    let cap = top + 5.5;
    p.line_segment(
        [Pos2::new(c.x - 5.0, cap), Pos2::new(c.x + 5.0, cap)],
        Stroke::new(1.0, col),
    );
    p.line_segment(
        [Pos2::new(c.x, cap), Pos2::new(c.x, top + 10.0)],
        Stroke::new(1.0, col),
    );
}

fn minimise_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    p.line_segment(
        [Pos2::new(c.x - 4.5, c.y), Pos2::new(c.x + 4.5, c.y)],
        Stroke::new(1.0, col),
    );
}

/// One square when the window can grow; two offset squares when it is maximised and the button
/// now means restore. Same convention as every OS, so nobody has to learn it.
fn maximise_glyph(p: &egui::Painter, c: Pos2, maximised: bool, col: Color32, bg: Color32) {
    let s = Stroke::new(1.0, col);
    if maximised {
        let back = Rect::from_min_size(Pos2::new(c.x - 2.5, c.y - 4.5), Vec2::splat(7.0));
        let front = Rect::from_min_size(Pos2::new(c.x - 4.5, c.y - 2.5), Vec2::splat(7.0));
        p.rect_stroke(back, CornerRadius::ZERO, s, StrokeKind::Inside);
        p.rect_filled(front, CornerRadius::ZERO, bg);
        p.rect_stroke(front, CornerRadius::ZERO, s, StrokeKind::Inside);
    } else {
        let r = Rect::from_center_size(c, Vec2::splat(9.0));
        p.rect_stroke(r, CornerRadius::ZERO, s, StrokeKind::Inside);
    }
}

/// The close cross. Public for the same reason as [`pin_glyph`]: an undecorated window with no
/// title strip still needs a way out, and the picture in picture window draws this one rather than
/// a second cross of its own.
pub fn close_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.2, col);
    p.line_segment(
        [
            Pos2::new(c.x - 4.0, c.y - 4.0),
            Pos2::new(c.x + 4.0, c.y + 4.0),
        ],
        s,
    );
    p.line_segment(
        [
            Pos2::new(c.x - 4.0, c.y + 4.0),
            Pos2::new(c.x + 4.0, c.y - 4.0),
        ],
        s,
    );
}

/* ------------------------------------------------------------------- helpers -- */

/// Width `chrome::tracked` will consume for `s`, computed the same way it draws: each glyph's own
/// galley width plus the track, INCLUDING the trailing track after the last glyph. Kept in step
/// with that function by construction rather than by a shared constant, because the width of a
/// Cinzel glyph is a fact about the font and not something to estimate from the size.
pub fn tracked_width(ui: &Ui, s: &str, size: f32, track: f32) -> f32 {
    let f = crate::fonts::display(size);
    s.chars()
        .map(|ch| {
            ui.painter()
                .layout_no_wrap(ch.to_string(), f.clone(), GOLD_DIM)
                .size()
                .x
                + track
        })
        .sum()
}

/* THE LOG-AND-MOVE-ON POLICY IS `crate::shell::open_url_quietly` NOW, and this strip is exactly
 * what that policy is for: a title bar has no room for a sentence and no state to hold one in, so
 * a failure here can only be logged. What went was the second COPY of the call, not the choice. */
use crate::shell::open_url_quietly as open_url;

fn text_w(ui: &Ui, s: &str, f: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(s.to_owned(), f.clone(), TEXT)
        .size()
        .x
}

/* ---------------------------------------------------------------- maker line -- */

/// `BROKEN STOIC` with the platform glyphs centred against it: the maker's mark in the title strip.
///
/// ```text
///   +-- outer -------------------------------+
///   | +-- name ------+  +- tw -+  +- yt -+   |
///   | | BROKEN STOIC |  | (tw) |  | (yt) |   |
///   | +--------------+  +------+  +------+   |
///   +----------------------------------------+
/// ```
///
/// THREE LINKS, NOT ONE, AND THAT IS THE CHANGE. This was a single hit target over the words and
/// the one glyph, and it opened Twitch. There are two marks now and each is a link to the platform
/// it names, because a mark that is not the link to its own platform is decoration wearing a
/// brand's colour. The WORDS keep a link of their own and it follows the PREFERENCE: the name is
/// the one part of the block that names no platform, so sending it anywhere fixed would be
/// arbitrary the moment there are two, and the preference is exactly the answer to "which one, if
/// you do not say".
///
/// THE HIT RECTS ARE EXPANDED BY TWO AND THE GAP BETWEEN GLYPHS IS WIDER THAN FOUR, deliberately.
/// The words were given three pixels of slack so a pointer need not find the ink; two neighbours
/// each given three would overlap by a pixel and a half, and egui breaks a tie by handing the band
/// to whichever was registered last, so a strip of the Twitch mark would silently open YouTube.
///
/// NEITHER GLYPH IS DIMMED TO SHOW WHICH IS PREFERRED. Both are at full mark colour, which is what
/// they are: two live links to two real channels. The preference is stated in WORDS on the pill a
/// few pixels away, and inventing a second, colour-only signal for it here would be a thing on
/// screen that no source carries and that a reader would have to be taught.
///
/// WHY `BUILT FOR` IS GONE.
/// It was set beside the name, then stacked above it, and both readings were worse than cutting it.
/// A title strip is single line by convention because it is dense: this one already carries the
/// window's name, this mark, the live pill and four window buttons inside 32 pixels, and a two row
/// stack in that space forces type small enough to squint at while reading as clutter. `BUILT FOR`
/// is nine characters of connective tissue that state a relationship the POSITION already states,
/// which is the same argument that removed "brought to you by" from the rail. What survives is the
/// name and the platform mark, which is what a dedication is when it is set well. The full phrase
/// is still there on hover, where a phrase costs nothing.
///
/// WHY BOXES AND NOT OFFSETS. Each part measures its OWN rect and the glyph is centred against
/// `name`, so changing either size leaves the rest correct. The version this replaced centred the
/// glyph by adding half of one measured height to a `y` computed elsewhere, which is the same class
/// of bug as the clip rect that once sliced the tail off Cinzel's Q: an assumed extent standing in
/// for a measured one.
///
/// `at` is the top left of the OUTER box. Nothing is returned: the strip does not read a response
/// from this, and each of the three parts now handles its own click, so a response covering all of
/// them would describe a hit target that no longer exists.
pub fn maker_block(ui: &Ui, at: Pos2, name_size: f32, rest: Color32, watch_on: Platform) {
    /* THE CAPS ARE DERIVED, NOT TYPED. A `const NAME: &str = "BROKEN STOIC"` sat here and was the
     * second spelling of one channel's name in this file, disagreeing with the pill five pixels
     * away. `settings::DISPLAY_NAME` is the one spelling and this is it, shouted. */
    let name_text = DISPLAY_NAME.to_uppercase();

    let name_track = name_size * 0.23;
    let name_f = crate::fonts::display(name_size);
    /* tracked() lays out one glyph at a time and returns the width INCLUDING a trailing track, so
     * the ink of a run stops one track short of what it reports. */
    let name_w = tracked_width(ui, &name_text, name_size, name_track) - name_track;
    /* every character is a cap, so one cap's galley IS the run's ink height */
    let name_h = ui
        .painter()
        .layout_no_wrap("B".to_owned(), name_f, rest)
        .size()
        .y;

    let name = Rect::from_min_size(at, Vec2::new(name_w, name_h));

    /* The WORDS, and their own link, which follows the preference. Registered AFTER whatever
     * allocated the space around it, so egui resolves an exact hit here and a click on the words
     * reaches the channel rather than the surface underneath, which in the title strip is the
     * window drag region. */
    let r = ui.interact(name.expand(3.0), ui.id().with("maker-name"), Sense::click());
    let hot = r.hovered();
    let col = if hot { GOLD_HI } else { rest };
    crate::chrome::tracked(ui, name.left_top(), &name_text, name_size, name_track, col);

    /* The underline belongs to the WORDS, so it spans the name and stops there. Run under the outer
     * box it would pass beneath the glyphs and read as a rule under the whole mark. */
    if hot {
        let uy = name.bottom() + 1.0;
        ui.painter().line_segment(
            [Pos2::new(name.left(), uy), Pos2::new(name.right(), uy)],
            Stroke::new(1.0, GOLD_HI),
        );
    }

    /* The dedication in full, where it costs nothing: the strip shows the name, the hover says what
     * the name is doing there, and the URL says where the click goes. */
    let r = r
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(format!("BUILT FOR {name_text}\n{}", watch_on.url()));
    if r.clicked() {
        open_url(&watch_on.url());
    }

    /* THE MARK, AND THERE IS ONE OF THEM NOW.
     *
     * It used to walk `Platform::ALL` and paint both, which said that the channel is on Twitch
     * AND on YouTube. That is true, and it is not what this strip is for: everything else in the
     * window is about the ONE platform the reader picked in settings. The live pill reads it, the
     * Watch screen plays it, the artwork is fetched from it. A pair of marks beside all of that
     * is the only place in the app still answering a question nobody asked, and the second one is
     * a link to a channel the reader has said they do not watch on.
     *
     * THE COLOUR IS THE PLATFORM'S OWN AND THE WORDS STAY GOLD, which is the older argument and
     * survives unchanged: the name belongs to this app, the mark belongs to the platform. A glyph
     * tinted to match the text reads as an ornament the designer chose; in the platform's colour
     * it is read as the platform without anyone having to look twice. */
    let side = name_size * MAKER_GLYPH;
    let g = Rect::from_min_size(
        Pos2::new(
            name.right() + name_size * MAKER_GAP,
            name.center().y - side * 0.5,
        ),
        Vec2::splat(side),
    );
    let gr = ui.interact(
        g.expand(MAKER_SLACK),
        ui.id().with(("maker-glyph", watch_on.key())),
        Sense::click(),
    );
    let (mark, mark_hi) = mark_colours(watch_on);
    platform_glyph(
        ui.painter(),
        watch_on,
        g,
        if gr.hovered() { mark_hi } else { mark },
    );
    let gr = gr
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(format!(
            "{DISPLAY_NAME} on {}\n{}",
            watch_on.label(),
            watch_on.url()
        ));
    if gr.clicked() {
        open_url(&watch_on.url());
    }
}

/// The gap between the words and the first mark, as a multiple of the name's type size.
const MAKER_GAP: f32 = 0.70;
/// One mark's side, same units.
const MAKER_GLYPH: f32 = 1.05;
/* THERE IS NO GAP BETWEEN MARKS ANY MORE BECAUSE THERE IS ONE MARK. `MAKER_GLYPH_GAP` stood here
 * at 0.55, and its whole reason was that two hit rects each expanded by `MAKER_SLACK` would
 * overlap at anything narrower and leave the second glyph unclickable behind the first. With one
 * mark there is no second hit rect, so the constant goes with the gap it measured rather than
 * being left behind as a number meaning nothing. */
/// How far past its ink a mark still accepts a click.
const MAKER_SLACK: f32 = 2.0;

/// The size `maker_block` will occupy, measured without drawing, so a caller can centre it in a
/// strip before it knows where to put it. Kept beside the drawing function because the two must
/// agree; a caller that guessed this would be back to the remembered numbers the box model exists
/// to remove.
pub fn maker_block_size(ui: &Ui, name_size: f32) -> Vec2 {
    let name_text = DISPLAY_NAME.to_uppercase();
    let name_track = name_size * 0.23;
    let name_w = tracked_width(ui, &name_text, name_size, name_track) - name_track;
    let name_h = ui
        .painter()
        .layout_no_wrap("B".to_owned(), crate::fonts::display(name_size), GOLD_DIM)
        .size()
        .y;
    /* ONE MARK, SO THERE IS NO GAP BETWEEN MARKS TO ACCOUNT FOR. This used to multiply by
     * `Platform::ALL.len()` so that adding a platform widened the block without anyone
     * remembering to; `maker_block` paints the reader's chosen platform and only that, so the
     * width no longer depends on how many exist. It does not depend on WHICH one either: both
     * glyphs are drawn into the same square.
     *
     * THE SIZE TAKES NO `watch_on` FOR THAT REASON, and `the_maker_block_measures_its_own_run`
     * is what holds the two functions to the same arithmetic. */
    let side = name_size * MAKER_GLYPH;
    Vec2::new(name_w + name_size * MAKER_GAP + side, name_h.max(side))
}

/* THERE IS NO GLYPH ONLY FORM OF THE MAKER'S MARK, AND THIS NOTE IS HERE SO ONE IS NOT WRITTEN
 * AGAIN. `maker_glyph` used to sit at this point in the file: the Twitch mark alone, with the same
 * hover, underline and click, documented as "the maker's mark when the rail is collapsed". Nothing
 * ever called it. The identifier occurred exactly once in the whole repository, at its own
 * definition, so its doc comment described a behaviour the shipped window did not have, and
 * `chrome::brand`'s narrow branch says in so many words why: the maker's line moved into this
 * strip, which is on screen at EVERY rail width, so a glyph in the collapsed rail would be the
 * second link to one channel on one screen. The strip is where the mark lives; nothing else needs
 * a smaller copy of it. */

/* ---------------------------------------------------------------------- pill -- */

/// The pill's content, decided once and shared by measuring and painting so the two cannot drift.
struct PillParts {
    dot: Dot,
    on: Platform,
    age: Option<String>,
}

impl PillParts {
    fn of(ch: &Channel, on: Platform) -> Self {
        let now = Utc::now();
        let dot = dot_of(ch.live);
        /* The age is carried only when it is a warning. See `pill_shows_age`: beside a state word
         * it reads as the duration of that state, so in the ordinary case it is not printed and the
         * tooltip says "checked 1m ago" in words that cannot be misread. */
        /* "4m ago" and not "4m". The word is two characters and it is what turns the number from a
         * duration of the state into a time since the check, which is the whole misreading this
         * rule exists to stop. It only ever appears in the stale case, so the extra width is paid
         * exactly when something is wrong and never in the ordinary one. */
        let age = if pill_shows_age(dot, ch.checked_at, now) {
            age_of(ch.checked_at, now).map(|a| format!("{a} ago"))
        } else {
            None
        };
        Self { dot, on, age }
    }
    /// The word after the separator. `LIVE` is a caps run and goes in Cinzel; the others are
    /// lowercase words and go in the body face, where Cinzel has no business.
    fn word_font(&self) -> FontId {
        match self.dot {
            Dot::Live => crate::fonts::display(10.0),
            _ => FontId::proportional(11.0),
        }
    }
}

/// How much of the strip's left end is kept for the lead and the window drag, whatever the pill
/// would otherwise want. A window with no draggable strip at all is a window a person cannot move.
const PILL_MIN_LEAD: f32 = 26.0;
const PILL_PAD: f32 = 9.0;
const DOT_R: f32 = 3.0;
const BODY_FONT: f32 = 11.0;
/// The space either side of a `·`.
const PILL_SEP_GAP: f32 = 5.0;

/// Widths of each run, left to right, so the painter and the allocator agree to the pixel.
struct PillLayout {
    name_w: f32,
    sep_w: f32,
    word_w: f32,
    total: f32,
}

fn pill_layout(ui: &Ui, parts: &PillParts) -> PillLayout {
    let body = FontId::proportional(BODY_FONT);
    let name_w = text_w(ui, DISPLAY_NAME, &body);
    let sep_w = text_w(ui, "·", &body);
    let word_w = text_w(ui, parts.dot.word(), &parts.word_font());
    let g = PILL_SEP_GAP;
    /* ONE SEPARATOR NOW, NOT TWO. The platform run and the separator that carried it came out
     * together: a gap measured for a run that is no longer painted is a pill with dead air in the
     * middle of it, and `the_pill_measures_what_it_paints` is what holds these two walks together.
     * The pill went from about 207 pixels back to about 137, which is what took the strip back
     * under its own 320 point minimum with room to drag. */
    let mut total = PILL_PAD + DOT_R * 2.0 + 6.0 + name_w + g + sep_w + g + word_w;
    if let Some(age) = parts.age.as_deref() {
        total += 7.0 + text_w(ui, age, &FontId::monospace(9.5));
    }
    total += PILL_PAD;
    PillLayout {
        name_w,
        sep_w,
        word_w,
        total,
    }
}

/// The pill's size, for a caller that lays it out by hand. It still depends on the PREFERENCE,
/// though no longer on the platform's NAME: the preference picks which channel the pill reads, and
/// the two channels can be in different states, so the state word and the stale age it may carry
/// are not the same width on both.
pub fn pill_size(ui: &Ui, live: &Status, watch_on: Platform) -> Vec2 {
    Vec2::new(
        pill_layout(ui, &PillParts::of(live.on(watch_on), watch_on)).total,
        PILL_H,
    )
}

/// How far below a galley's TOP its first baseline sits.
///
/// `Glyph::pos` is documented by epaint as "Baseline position, relative to the row", so the first
/// glyph's y IS the drop from the row's top to the baseline, and the row's own y places that row
/// inside the galley. An empty string has no glyphs and no baseline, so it drops nothing.
fn baseline_drop(g: &egui::Galley) -> f32 {
    g.rows.first().map_or(0.0, |r| {
        r.pos.y + r.row.glyphs.first().map_or(0.0, |gl| gl.pos.y)
    })
}

/// Draw `s` with its BASELINE on `baseline_y`, rather than with its box centred on some y.
///
/// WHY THIS EXISTS, AND IT IS THE SAME BUG AS THE CLIPPED Q IN `chrome::ramp_text`.
///
/// The pill sets four runs in three faces at three sizes: the handle and the separator in Plex Sans
/// at 11, `LIVE` in CINZEL at 10, and the age in mono at 9.5. Every one of them used to be drawn at
/// the pill's centre y with `Align2::LEFT_CENTER`, which centres the galley's BOX. A box spans that
/// font's ascent to descent, and those metrics differ per face, so centring four boxes on one y put
/// four baselines on four different heights. `LIVE` sat visibly high against `Broken_Stoic` because
/// Cinzel is all caps with no descender, so proportionally more of its box is above the baseline.
///
/// Text sits on a baseline. That is what makes a line of mixed faces read as one line, and no
/// bounding box is a substitute for it. Never let the box stand in for where the ink is.
fn baseline_text(p: &egui::Painter, at_x: f32, baseline_y: f32, s: &str, f: FontId, col: Color32) {
    let galley = p.layout_no_wrap(s.to_owned(), f, col);
    let drop = baseline_drop(&galley);
    p.galley(Pos2::new(at_x, baseline_y - drop), galley, col);
}

/// THE LIVE PILL, AND THIS IS THE ONLY FUNCTION IN THE APP THAT DRAWS ONE.
///
/// `● Broken Stoic · LIVE` with the dot in `TWITCH_PURPLE` or `YOUTUBE_RED` depending which
/// platform is preferred, `● Broken Stoic · offline` with it in `TEXT_3`, and a hollow ring with
/// `unknown` when nothing has answered yet. See `Dot::color`, which argues the mark colours.
///
/// Paint it into `rect`, given the response that owns that rect. Handles the hover tooltip (which
/// source answered, how long ago) and hands the CLICK back to its caller.
///
/// IT NO LONGER ACTS ON THE CLICK ITSELF, and that was change B. It used to call
/// `open_url(&watch_on.url())`, which sent the reader out of the app and into a browser. The owner
/// asked for the opposite: a click on the pill should connect him to the stream he chose in
/// settings, in the main viewing area, in app. Where the video may go is a question only the App
/// can answer (the surface attaches to the ROOT window's handle, and only `App::ui` holds it), so
/// this widget reports the click, [`strip`] puts it in `Hits::watch`, and the three files that
/// draw a strip turn that into `screens::Ask::WatchHere`. The same reason `Cx::stage` is an out
/// parameter rather than a call. A caller that drops the response drops the feature, which is what
/// `no_pill_is_drawn_with_its_click_thrown_away` is for.
///
/// THERE USED TO BE A `pub fn pill` OVER THIS, WHICH ALLOCATED A RECT AND CALLED IT, AND THE APP
/// USED IT TWICE. The Watch screen opened its body with one and the main window's footer put
/// another at the bottom right, both handed `ui`, `cx.live` and `cx.settings.watch_on`: the same
/// function with the same three arguments as the strip's, in the same frame. (Written out rather
/// than as a call, because `no_pill_is_drawn_with_its_click_thrown_away` scans this file's
/// production text and cannot tell a quotation from the thing quoted.) Inputs that identical cannot
/// produce a disagreement, so what they produced was one sentence said three times, and on an
/// offline channel the Watch screen announced it four times before offering a control. Both went,
/// the wrapper went with them because a `pub fn` in a lib crate with no callers is dead code rustc
/// will never mention, and the pill now lives in exactly one row: the title strip, which D8 names
/// as its home, which sits beside the channel's own mark, and which every window has.
///
/// THIS DOC USED TO NAME TWO OF THE FIVE STATE COLOURS, and it was describing a drawing this file
/// stopped producing when they were taken out of the dot (`Dot::color` names them and argues the
/// removal). Prose naming a state colour for a thing that is not that state makes the same claim
/// the code was changed to stop making, so it cannot stand in a comment either.
///
/// THE AGE IS NOT ALWAYS AFTER IT, WHICH THIS ALSO USED TO SAY. `pill_shows_age` is the rule and it
/// is false in the ordinary case: never for `unknown`, never without a timestamp, and otherwise
/// only once the check is `PILL_STALE_AFTER` old. An age beside a state word reads as the duration
/// of that state rather than the age of the check, so it appears only when the state on the pill
/// may be hours stale and the reader needs telling.
///
/// ONE PILL, AND IT READS THE PREFERRED PLATFORM WITHOUT NAMING IT. This used to say YouTube was
/// polled and undrawn and left the call to the owner; the owner called it, and the answer was a
/// preference rather than a second pill. The strip is the app's most crowded row, `web/app.html`
/// (the authority for anything visual) carries no pill at all and therefore no second one to copy,
/// and two pills reporting two platforms would put four states on a 32 pixel row for a person who
/// watches on one of them. So `settings::Settings::watch_on` picks which channel this reads and
/// what its click plays; the WORD for that preference is on the hover, not in the middle of the
/// sentence (see `pill_label`). The other platform is not dropped: its mark is a link in the
/// maker's block a few pixels left, and the Watch screen still prints the YouTube state in words
/// beside its own link.
fn paint_pill(ui: &Ui, rect: Rect, resp: Response, live: &Status, watch_on: Platform) -> Response {
    let ch = live.on(watch_on);
    let parts = PillParts::of(ch, watch_on);
    let lay = pill_layout(ui, &parts);
    /* CLIPPED TO ITS OWN RECT, so a squeezed pill truncates instead of spilling.
     *
     * `pill` allocates exactly what `pill_layout` measured and this clip never bites there. The
     * STRIP is the caller that can hand over less: at a tool window's minimum width there is not
     * room for the controls, a draggable lead and a full pill at once, so `strip` clamps the rect
     * and the runs that do not fit end here rather than running out of the stadium and across the
     * window's title. The hover carries every word either way. */
    let p = &ui.painter().with_clip_rect(rect);
    let hot = resp.hovered();

    /* The one rounded shape. Radius is half the height, so it is a stadium and not a rounded box:
     * a 6px radius on a 20px pill reads as a button, a full radius reads as a chip. */
    let radius = CornerRadius::same((PILL_H / 2.0) as u8);
    p.rect_filled(rect, radius, if hot { PANEL_2 } else { PANEL });
    p.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, if hot { GOLD_DIM } else { GOLD_DEEP }),
        StrokeKind::Inside,
    );

    let cy = rect.center().y;

    /* THE ONE BASELINE EVERY RUN SITS ON, taken from the name.
     *
     * The name is the pill's subject and the longest run, so it keeps EXACTLY the position
     * `Align2::LEFT_CENTER` gave it: box centred on `cy`. Its baseline then becomes the line the
     * separators, the platform, the word and the age are set on, so the pill as a whole does not
     * move and only the runs that were riding high come down onto it. */
    let body = FontId::proportional(BODY_FONT);
    let anchor = p.layout_no_wrap(DISPLAY_NAME.to_owned(), body.clone(), TEXT);
    let baseline_y = cy - anchor.rect.height() * 0.5 + baseline_drop(&anchor);

    let mut x = rect.left() + PILL_PAD;

    let dot_col = parts.dot.color(parts.on);
    let dc = Pos2::new(x + DOT_R, cy);
    if parts.dot.filled() {
        p.circle_filled(dc, DOT_R, dot_col);
    } else {
        p.circle_stroke(dc, DOT_R, Stroke::new(1.0, dot_col));
    }
    x += DOT_R * 2.0 + 6.0;

    let sep = |x: &mut f32| {
        *x += PILL_SEP_GAP;
        baseline_text(p, *x, baseline_y, "·", body.clone(), TEXT_3);
        *x += lay.sep_w + PILL_SEP_GAP;
    };

    baseline_text(p, x, baseline_y, DISPLAY_NAME, body.clone(), TEXT);
    x += lay.name_w;
    /* THE PLATFORM RUN IS GONE FROM HERE, and its separator with it. See `pill_label` for the
     * argument. What the pill draws is the channel and the answer; which address was polled is on
     * the hover, and `parts.on` still decides the DOT's colour two runs back, which is the one
     * thing about the platform that this widget still paints. */
    sep(&mut x);
    let word_col = match parts.dot {
        Dot::Live => TEXT,
        _ => TEXT_2,
    };
    baseline_text(
        p,
        x,
        baseline_y,
        parts.dot.word(),
        parts.word_font(),
        word_col,
    );
    x += lay.word_w;

    if let Some(age) = &parts.age {
        x += 7.0;
        /* an age is a number someone compares against the next one: monospace */
        baseline_text(p, x, baseline_y, age, FontId::monospace(9.5), TEXT_3);
    }

    resp.on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(pill_tooltip(ch, watch_on, Utc::now()))
}

/* --------------------------------------------------------------------- strip -- */

/// A frameless title strip for a viewport: drag region, title in Cinzel, the live pill, a pin
/// glyph, and minimise / maximise / close.
///
/// WHAT THE FLAGS MEAN, AND THEY ARE ALL IN [`Hits`]. `pinned` is the current state, painted.
/// `hits.pin` is flipped when the pin is clicked: a caller that passes a fresh `false` each frame
/// reads it as "the pin was clicked", a caller that passes its persistent pinned flag gets it
/// toggled in place; both are correct because the strip never sets it to a fixed value.
/// `hits.close` is set true when the close glyph is clicked. The strip ALSO sends
/// `ViewportCommand::Close`, so a strip in the root viewport quits the app the way a real title bar
/// would; for a deferred viewport that command only raises `close_requested`, and the window
/// actually goes away when the caller honours it and stops showing the window. The strip does not
/// send `WindowLevel`: pinning belongs to whoever owns the window registry, so there is exactly one
/// place that decides what "on top" means.
///
/// Dragging anywhere on the strip that is not a control starts an OS window drag
/// (`ViewportCommand::StartDrag`), and a double click there toggles maximised, because those are
/// the two things a hand expects a title bar to do.
/// What the left end of a title strip says.
///
/// AN ENUM RATHER THAN A SENTINEL STRING, because the two cases are genuinely different things and
/// the difference is not a spelling. A tool window is named after its job, so its taskbar entry and
/// its strip agree. The main window carries the same name AND the dedication beside it: `strip`
/// draws `EQL Grimoire` at 13 in GOLD and then `BROKEN STOIC` at 9.5 in GOLD_DIM, and the drop
/// between them is what keeps two runs of caps from reading as two competing titles.
///
/// THE NAME IS NOT OPTIONAL AND THIS DOC USED TO SAY IT WAS DROPPED. The claim was that printing
/// `EQL Grimoire` here as well as in the rail's wordmark says one thing twice; the strip has said
/// it all along, and it has to, or the taskbar entry and the bar disagree and an alt-tab lands on
/// something unlabelled. The wordmark two rows below is on the RAIL, which the tool windows do not
/// have and which a collapsed rail cuts to `EQL`. See `strip`'s `Lead::Maker` arm.
#[derive(Clone, Copy)]
pub enum Lead<'a> {
    /// A tool window: Parser, Broken Stoic, Plane of Sky, Looking for group.
    Title(&'a str),
    /// The main window: the window's name, then `BROKEN STOIC` tracked and smaller, with the
    /// Twitch glyph, hover and click. `BUILT FOR` is not drawn; it survives on the hover, and
    /// `maker_block` records why it was cut.
    Maker,
}

impl Lead<'_> {
    /// A stable key for egui ids. `Maker` is one instance, so a constant is enough.
    fn key(self) -> &'static str {
        match self {
            Lead::Title(_) => "title",
            Lead::Maker => "maker",
        }
    }
}

/// What a title strip was clicked on, reported to whoever owns the window.
///
/// ONE OUT PARAMETER INSTEAD OF THREE BOOLEANS, and the third is what forced it: `strip` took
/// `on_pin` and `on_close` already, `watch` made eight arguments, and eight positional arguments
/// of which three are `&mut bool` is a call site where a transposition compiles and does the wrong
/// thing in silence. Named fields cannot be transposed.
///
/// NO `PartialEq` OR `Eq` HERE, DELIBERATELY. `reach.rs` explains it: those derives read every
/// field, which switches OFF rustc's own dead-field warning, and this struct exists precisely so
/// that a strip reporting a click nobody reads is a compile warning and not a quiet nothing.
#[derive(Default, Clone, Copy, Debug)]
pub struct Hits {
    /// The pin glyph was clicked.
    pub pin: bool,
    /// The close glyph was clicked. The strip has already sent `ViewportCommand::Close`.
    pub close: bool,
    /// The live PILL was clicked: show Broken Stoic in the main window's body and play it.
    ///
    /// WHY THE STRIP CANNOT ACT ON THIS ITSELF. The player surface attaches to the ROOT window's
    /// handle, which only `App::ui` holds; a strip drawn inside a tool window's deferred viewport
    /// could not place it at all, and one drawn in the root has no route to the Watch screen's
    /// state. So every strip reports the click and its caller raises `screens::Ask::WatchHere`,
    /// which the App answers by entering Watch and turning its playback on. See `paint_pill`.
    pub watch: bool,
}

pub fn strip(
    ui: &mut Ui,
    lead: Lead<'_>,
    live: &Status,
    watch_on: Platform,
    pinned: bool,
    hits: &mut Hits,
) {
    let base: Id = ui.id().with((
        "titlebar",
        lead.key(),
        match lead {
            Lead::Title(t) => t,
            Lead::Maker => "",
        },
    ));
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), STRIP_H), Sense::hover());
    let p = ui.painter().clone();

    p.rect_filled(rect, CornerRadius::ZERO, PANEL);
    p.line_segment(
        [
            Pos2::new(rect.left(), rect.bottom() - 0.5),
            Pos2::new(rect.right(), rect.bottom() - 0.5),
        ],
        Stroke::new(1.0, RULE),
    );

    /* Controls, laid right to left so close is always in the corner. */
    let mut right = rect.right();
    let mut next = || {
        right -= BTN_W;
        Rect::from_min_size(Pos2::new(right, rect.top()), Vec2::new(BTN_W, STRIP_H))
    };
    let close_r = next();
    let max_r = next();
    let min_r = next();
    let pin_r = next();
    let controls_left = pin_r.left();

    /* The pill sits just left of the controls, vertically centred, AND IT MAY NOT RUN OFF THE
     * LEFT EDGE OF THE STRIP.
     *
     * The line under this used to be `controls_left - 10 - pill_w` with nothing stopping it, on
     * the assumption that the pill always fits. That assumption was true and is not any more: a
     * tool window may be dragged to `min_inner_size` 320 wide (windows.rs), four window buttons
     * take 120 of that, and the pill grew from about 137 to about 207 when it started naming the
     * platform, so the arithmetic goes NEGATIVE. What that produced was not a cramped strip, it
     * was an inverted `drag_r` (min.x past max.x), which `Rect::contains` answers false for
     * everywhere: the whole window would have become undraggable, silently, at a width nobody
     * tests at. `the_strip_survives_the_narrowest_window_it_allows` is the assertion.
     *
     * WHAT GIVES, AND IN WHICH ORDER. The controls are fixed, because a close button that moves
     * is worse than anything else here. The lead was already the first thing to go: it is clipped
     * to the drag region, by design, so a long title stops short of the pill. When even that is
     * spent, the PILL is squeezed and `paint_pill` clips its own tail; the full text is on the
     * hover, which is where it always was. `PILL_MIN_LEAD` is what stays behind so the window
     * keeps a strip of itself to be dragged by. */
    let pill_w = pill_size(ui, live, watch_on).x;
    let pill_right = controls_left - 10.0;
    let pill_left = (pill_right - pill_w).max(rect.left() + PILL_MIN_LEAD);
    let pill_r = Rect::from_min_max(
        Pos2::new(pill_left, rect.center().y - PILL_H / 2.0),
        Pos2::new(pill_right.max(pill_left), rect.center().y + PILL_H / 2.0),
    );

    /* The drag region is everything left of the pill. It is registered FIRST so every control
     * registered after it wins an exact hit: egui breaks a tie toward the last widget. The `max`
     * is what keeps it a rectangle rather than an inside-out one; see above. */
    let drag_r = Rect::from_min_max(
        rect.left_top(),
        Pos2::new((pill_r.left() - 6.0).max(rect.left()), rect.bottom()),
    );
    let drag = ui.interact(drag_r, base.with("drag"), Sense::click_and_drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    let maximised = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    if drag.double_clicked() {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximised));
    }

    /* The lead, clipped to the drag region so a long one stops short of the pill instead of
     * running under it.
     *
     * `maker_block` registers its own hit rect and it is registered AFTER the drag region above, so
     * egui resolves an exact hit to it: the pointer over the words opens the channel, the pointer
     * anywhere else on the strip drags the window. That ordering is the whole reason a link can sit
     * inside a drag surface at all, and reversing the two lines would silently make the link dead. */
    match lead {
        Lead::Title(t) => {
            p.with_clip_rect(drag_r).text(
                Pos2::new(rect.left() + 12.0, rect.center().y),
                Align2::LEFT_CENTER,
                t,
                crate::fonts::display(13.0),
                GOLD,
            );
        }
        Lead::Maker => {
            /* THE NAME FIRST, THEN THE DEDICATION, SMALLER.
             *
             * Both belong here. The strip has to name the window, or the taskbar entry and the bar
             * disagree and an alt-tab lands on something unlabelled. And the dedication belongs
             * beside the live pill, because who the app is built for and whether he is streaming
             * are the same subject.
             *
             * What makes them one line rather than two competing titles is the drop: 13 for the
             * window's name against 9.5 for the maker's mark, GOLD against GOLD_DIM. The eye
             * reaches the pill having passed one subject, not two.
             *
             * The mark is a single run. `BUILT FOR` was set beside the name, then stacked above it,
             * and cutting it beat both: a title strip is single line by convention because it is
             * dense, this one already carries a window name, a mark, a live pill and four window
             * buttons inside 32 pixels, and nine characters of connective tissue were the cheapest
             * thing in it to lose. The full phrase survives on the mark's hover, where it costs
             * nothing. See `maker_block`.
             *
             * TWO SIZES, TWO NAMES, AND NEITHER CALLED `NAME_SIZE`. An earlier cut had the window
             * title's size and the maker's name size both under that one name in this scope and it
             * failed to compile, which was the lucky outcome: in different scopes it would have
             * built and one of them would have silently been the wrong type size. */
            const TITLE: &str = "EQL Grimoire";
            const TITLE_SIZE: f32 = 13.0;
            const MAKER_NAME_SIZE: f32 = 9.5;

            let old = ui.clip_rect();
            ui.set_clip_rect(drag_r);

            let title_f = crate::fonts::display(TITLE_SIZE);
            let name_w = ui
                .painter()
                .layout_no_wrap(TITLE.to_owned(), title_f.clone(), GOLD)
                .size()
                .x;
            p.with_clip_rect(drag_r).text(
                Pos2::new(rect.left() + 12.0, rect.center().y),
                Align2::LEFT_CENTER,
                TITLE,
                title_f,
                GOLD,
            );

            /* The lockup is two stacked rows plus a glyph, so its height is not any one font's
             * height and cannot be centred by halving a cap. `maker_block_size` measures the whole
             * outer box the way `maker_block` will draw it, and the strip centres THAT. The two
             * functions share their constants for exactly this reason: a caller that estimated
             * this would be back to the remembered numbers the box model exists to remove. */
            let block = maker_block_size(ui, MAKER_NAME_SIZE);
            let at = Pos2::new(
                rect.left() + 12.0 + name_w + 14.0,
                rect.center().y - block.y * 0.5,
            );
            maker_block(ui, at, MAKER_NAME_SIZE, GOLD_DIM, watch_on);

            ui.set_clip_rect(old);
        }
    }

    let pill_resp = ui.interact(pill_r, base.with("pill"), Sense::click());
    if paint_pill(ui, pill_r, pill_resp, live, watch_on).clicked() {
        hits.watch = true;
    }

    /* Buttons. Hover is a PANEL_2 fill and a brighter glyph, the same as every hovered row in the
     * rail. No red on close: WRONG is a state, and "your pointer is over the X" is not one. */
    let button = |ui: &Ui, r: Rect, name: &str, tip: &str| -> (Response, Color32) {
        let resp = ui.interact(r, base.with(name), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::ZERO, PANEL_2);
        }
        let col = if resp.hovered() { GOLD_HI } else { TEXT_2 };
        (resp.on_hover_text(tip), col)
    };

    let (pin, col) = button(
        ui,
        pin_r,
        "pin",
        if pinned {
            "unpin: let other windows cover this one"
        } else {
            "pin: keep this window on top"
        },
    );
    pin_glyph(
        &p,
        pin_r.center(),
        pinned,
        if pinned && col == TEXT_2 { GOLD } else { col },
    );
    if pin.clicked() {
        hits.pin = !hits.pin;
    }

    let (min, col) = button(ui, min_r, "min", "minimise");
    minimise_glyph(&p, min_r.center(), col);
    if min.clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
    }

    let (max, col) = button(
        ui,
        max_r,
        "max",
        if maximised { "restore" } else { "maximise" },
    );
    maximise_glyph(
        &p,
        max_r.center(),
        maximised,
        col,
        if max.hovered() { PANEL_2 } else { PANEL },
    );
    if max.clicked() {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximised));
    }

    let (close, col) = button(ui, close_r, "close", "close");
    close_glyph(&p, close_r.center(), col);
    if close.clicked() {
        hits.close = true;
        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
    }
}

/* -------------------------------------------------------------------- footer -- */

/// Footer, left side: `v0.1.0 · AGPL-3.0-only`, then ` · Source on GitHub` as a link when, and only when, a
/// repository is declared in the manifest (see `REPOSITORY`). The version and licence are read
/// from the manifest too, so nothing here is typed by hand.
pub fn footer_left(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let f_mono = FontId::monospace(10.5);
        let f_body = FontId::proportional(10.5);
        let sep = |ui: &mut Ui| {
            ui.label(
                egui::RichText::new(" · ")
                    .font(f_body.clone())
                    .color(TEXT_3),
            );
        };

        /* "v0.1.0": a version is compared digit by digit, so it is monospace. */
        ui.label(
            egui::RichText::new(format!("v{}", version()))
                .font(f_mono)
                .color(TEXT_3),
        );
        sep(ui);
        /* "AGPL-3.0-only": a caps run, so it is Cinzel, at the caption size. */
        ui.label(
            egui::RichText::new(licence())
                .font(crate::fonts::display(10.0))
                .color(TEXT_3),
        );

        if let Some((label, url)) = source_link() {
            sep(ui);
            let galley = ui
                .painter()
                .layout_no_wrap(label.to_owned(), f_body.clone(), GOLD_DIM);
            let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
            let hot = resp.hovered();
            let col = if hot { GOLD_HI } else { GOLD_DIM };
            ui.painter().galley(rect.left_top(), galley, col);
            if hot {
                let uy = rect.bottom() - 1.0;
                ui.painter().line_segment(
                    [Pos2::new(rect.left(), uy), Pos2::new(rect.right(), uy)],
                    Stroke::new(1.0, GOLD_HI),
                );
            }
            let resp = resp
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(url);
            if resp.clicked() {
                open_url(url);
            }
        }
    });
}

/* --------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    /// EVERY RUN IN THE PILL SITS ON ONE BASELINE.
    ///
    /// The pill sets four runs in three faces at three sizes, and they used to be drawn with
    /// `Align2::LEFT_CENTER`, which centres each galley's BOX on one y. A box spans its own font's
    /// ascent to descent, so four boxes centred on one y put four baselines on four DIFFERENT
    /// heights, and `LIVE` in Cinzel rode visibly high above `Broken_Stoic` in Plex Sans.
    ///
    /// This measures the painted shapes, not the arithmetic: for each text shape it takes the
    /// position it was actually painted at and adds that galley's own baseline drop. If any run
    /// ever goes back to being box-aligned, these numbers separate.
    ///
    /// The channel is deliberately stale (checked ten minutes ago, against `PILL_STALE_AFTER` of
    /// 225 seconds) so the AGE run is painted too. Without it the pill draws three runs and the
    /// mono face, which is the one most likely to sit differently, is never tested at all.
    ///
    /// IT DRAWS THE REAL STRIP NOW, AND THAT IS AN IMPROVEMENT THE PILL CUT FORCED. It used to
    /// call a `pub fn pill` wrapper that allocated a rect and painted into it. That wrapper had
    /// exactly two production callers, the Watch screen's body and the main window's footer, and
    /// both were removed as duplicates of the strip's own; a `pub fn` in a lib crate with no
    /// callers is dead code rustc never mentions, so it went too. Rebuilding it here as a test
    /// helper would have been a test-only copy of production layout, which measures itself. The
    /// strip is what actually draws a pill, so the strip is what this reads, and the runs are
    /// picked out of the frame by the pill's own stadium rather than by being the only thing on
    /// screen: the lead and the window glyphs are painted in this frame too and are not the
    /// pill's.
    #[test]
    fn every_run_in_the_pill_shares_one_baseline() {
        let live = Status {
            twitch: Channel {
                handle: "Broken_Stoic".to_owned(),
                live: Some(true),
                checked_at: Some(Utc::now() - chrono::Duration::seconds(600)),
                ..Channel::unchecked("Broken_Stoic")
            },
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };

        /* Wide enough that `strip` never clamps the pill, so every run is painted in full. */
        let shapes = strip_frame_at(900.0, Lead::Maker, Platform::Twitch, &live);
        let pill_r = stadium(&shapes);

        let runs: Vec<(String, f32)> = shapes
            .iter()
            .filter_map(|(_, s)| match s {
                /* The pill's own runs and no others: everything the strip paints to the LEFT of
                 * the stadium is the lead, and the window glyphs are to its right in another
                 * face entirely. */
                egui::Shape::Text(t) if t.pos.x >= pill_r.left() && t.pos.x < pill_r.right() => {
                    Some((
                        t.galley.text().to_owned(),
                        t.pos.y + baseline_drop(&t.galley),
                    ))
                }
                _ => None,
            })
            .collect();

        assert!(
            runs.len() >= 4,
            "the pill painted {} runs and this test needs the handle, the separator, the word and              the age: {runs:?}",
            runs.len()
        );
        assert!(
            runs.iter().any(|(t, _)| t.contains("LIVE")),
            "no LIVE run was painted, so the face most likely to sit wrong went untested: {runs:?}"
        );

        let first = runs[0].1;
        for (text, base) in &runs {
            assert!(
                (base - first).abs() < 0.5,
                "the run {text:?} sits on baseline {base} and {:?} sits on {first}; every run in                  the pill is one line of text and must share one baseline",
                runs[0].0
            );
        }
    }

    /// `theme.rs` states the state colour rule without an exception and `Dot::color` obeys it, but
    /// the doc on `pill` went on describing a `WRONG` red dot and an `IDLE` grey one for as long as
    /// it took an adversarial read to notice. Prose that names a state colour for something that is
    /// not that state makes the same claim the code was changed to stop making, so this file's
    /// account of the dot is held to the file's own rule.
    ///
    /// It reads the source rather than the drawing because there is nothing else to read: a doc
    /// comment leaves no trace in the binary. The window is the two items that describe the dot,
    /// which is where the drift was, and not the whole file: `Dot::color` has to be free to explain
    /// at length WHY those two colours were taken out, and does.
    #[test]
    fn the_pill_doc_does_not_claim_a_state_colour_the_dot_no_longer_draws() {
        let src = include_str!("titlebar.rs");
        /* The doc moved onto `paint_pill` when the `pub fn pill` wrapper it used to sit on was
         * deleted, so the window moved with it.
         *
         * THE PRODUCTION HALF ONLY, and the first cut of this did not do that: naming the marker
         * here put a second copy of it in the file, `matches().count()` read 2, and the test
         * failed on its own text. Cutting the tests off first is how every other source reading
         * test in this crate does it, and it also makes the uniqueness check mean something. */
        let body = &src[..src.find("\nmod tests {").unwrap_or(src.len())];
        const MARK: &str = "/// THE LIVE PILL, AND THIS IS THE ONLY FUNCTION";
        assert_eq!(
            body.matches(MARK).count(),
            1,
            "the pill doc's marker is not unique in the production text, so this test cannot say \
             which block it read"
        );
        let src = body;
        let from = src.find(MARK).expect("the pill's doc comment");
        let to = src[from..]
            .find("fn paint_pill(")
            .map(|i| from + i)
            .expect("the painter itself");
        let doc = &src[from..to];
        for claim in ["`WRONG`", "`IDLE`", "WRONG red", "IDLE grey"] {
            assert!(
                !doc.contains(claim),
                "the pill's doc still claims {claim}; Dot::color draws TWITCH_PURPLE and TEXT_3"
            );
        }
        assert!(
            doc.contains("TWITCH_PURPLE") && doc.contains("TEXT_3"),
            "the doc has to name the colours the dot actually draws"
        );
    }

    /// The strip draws the window's name AND the dedication. Two docs in this file and one in
    /// `chrome.rs` said the name had been dropped to avoid repeating the rail's wordmark, which
    /// would have made an alt tab land on an unlabelled window if anyone had acted on it.
    #[test]
    fn the_strip_names_the_window_and_the_maker_run_is_the_short_one() {
        let src = include_str!("titlebar.rs");
        assert!(
            src.contains("const TITLE: &str = \"EQL Grimoire\";"),
            "the taskbar entry and the strip have to agree"
        );
        /* THE MAKER RUN IS DERIVED, NOT DECLARED, AND THAT IS THE POINT OF THIS PAIR.
         *
         * It used to be `const NAME: &str = "BROKEN STOIC"` in two functions here, and this test
         * asserted that literal was present. Holding a literal in place is not the same as holding
         * it in AGREEMENT with the pill five pixels away, which is what it was not: the pill drew
         * the login. So the literal is forbidden now and the derivation is required, and
         * `the_pill_and_the_strip_agree_on_the_name` measures that the derived run is the one the
         * strip actually paints. */
        /* The PRODUCTION half only. This test names the literals it forbids, so a whole-file
         * search finds them in this very assertion and fails for the silliest possible reason. */
        let body = &src[..src.find("mod tests").unwrap_or(src.len())];
        /* A DECLARATION, not a mention. `maker_block` explains in a comment what used to sit there
         * and quotes it, which is worth keeping and is not a second name; a line that DECLARES the
         * constant starts with the keyword once the indent is off, and a comment line starts with
         * `*` or `/`. */
        assert!(
            !body
                .lines()
                .any(|l| l.trim_start().starts_with("const NAME")),
            "the maker run must come off settings::DISPLAY_NAME, not a literal in this file"
        );
        assert!(
            body.contains("DISPLAY_NAME.to_uppercase()"),
            "the maker run is the display name, shouted"
        );
        assert!(
            !body.contains("BUILT FOR BROKEN STOIC"),
            "BUILT FOR was cut from the run and survives only on the hover, built from the name"
        );
    }

    /// Decision D2: the pill's hover names WHICH SOURCE answered and how long ago. A tooltip that
    /// dropped the source would make a decapi fallback indistinguishable from a GQL answer.
    #[test]
    fn pill_tooltip_names_source_and_age() {
        let now = Utc::now();
        let mut ch = Channel::unchecked("Broken_Stoic");
        ch.live = Some(true);
        ch.title = Some("D4 motes with the guild".to_owned());
        ch.game = Some("EverQuest".to_owned());
        ch.viewers = Some(42);
        ch.checked_at = Some(now - Duration::seconds(125));
        ch.source = "gql";
        let tip = pill_tooltip(&ch, Platform::Twitch, now);
        assert!(
            tip.starts_with("Broken Stoic · LIVE\n"),
            "the first line is the pill's label: {tip}"
        );
        assert!(tip.contains("live now"), "{tip}");
        /* the platform the pill stopped printing did not leave the window; it moved here */
        assert!(tip.contains("platform: Twitch"), "{tip}");
        /* and neither did the login */
        assert!(tip.contains("handle: Broken_Stoic"), "{tip}");
        assert!(tip.contains("checked 2m ago via gql"), "{tip}");
        assert!(tip.contains("title: D4 motes with the guild"), "{tip}");
        assert!(tip.contains("playing: EverQuest"), "{tip}");
        assert!(tip.contains("viewers: 42"), "{tip}");
        /* THE DESTINATION IS THIS APP NOW, AND THE HOVER HAD BETTER SAY SO. It used to end
         * "click to open https://www.twitch.tv/Broken_Stoic" and hand that URL to the system
         * browser; the click plays the stream in the body instead, so a hover naming any URL at
         * all would be promising a journey the click does not make. */
        assert!(
            tip.ends_with("click to watch here, in the main window"),
            "the hover has to say where the click goes, and it goes here: {tip}"
        );
        assert!(
            !tip.contains("http"),
            "the pill's click opens no URL, so its hover may not print one: {tip}"
        );

        /* the same channel read as YouTube: only the platform line moves */
        let yt = pill_tooltip(&ch, Platform::YouTube, now);
        assert!(yt.starts_with("Broken Stoic · LIVE\n"), "{yt}");
        assert!(yt.contains("platform: YouTube"), "{yt}");
        assert!(
            yt.ends_with("click to watch here, in the main window"),
            "{yt}"
        );
        assert!(!yt.contains("http"), "{yt}");
    }

    /// AN OFFLINE PILL PROMISES THE SCREEN, NOT THE STREAM, and this is the half of the click that
    /// is easy to word dishonestly. `feed_for` refuses to embed a channel that is not live, so a
    /// hover reading "click to watch here" on an offline pill would be a claim the very next frame
    /// contradicts. The click still does something, and the words say what.
    #[test]
    fn the_hover_promises_a_stream_only_when_there_is_one() {
        let now = Utc::now();
        let mut ch = Channel::unchecked("Broken_Stoic");
        for (live, tail) in [
            (Some(true), "click to watch here, in the main window"),
            (Some(false), "click to open Watch in the main window"),
            (None, "click to open Watch in the main window"),
        ] {
            ch.live = live;
            let tip = pill_tooltip(&ch, Platform::Twitch, now);
            assert!(tip.ends_with(tail), "for {live:?} the hover reads: {tip}");
        }
    }

    /// A failed poll keeps the LAST KNOWN state and says what failed; it never turns into a fake
    /// offline, and a channel that was never checked says so instead of inventing an age.
    #[test]
    fn pill_tooltip_failed_poll_keeps_last_known_state() {
        let now = Utc::now();
        let mut ch = Channel::unchecked("Broken_Stoic");
        ch.live = Some(true);
        ch.checked_at = Some(now - Duration::seconds(3 * 3_600));
        ch.source = "gql";
        ch.error = Some("timed out".to_owned());
        let tip = pill_tooltip(&ch, Platform::Twitch, now);
        assert!(tip.contains("live now"), "{tip}");
        assert!(tip.contains("checked 3h ago via gql"), "{tip}");
        assert!(tip.contains("last poll: timed out"), "{tip}");
        assert!(!tip.contains("not streaming"), "{tip}");

        let never = Channel::unchecked("Broken_Stoic");
        let tip = pill_tooltip(&never, Platform::Twitch, now);
        assert!(tip.starts_with("Broken Stoic · unknown\n"), "{tip}");
        assert!(tip.contains("live status unknown"), "{tip}");
        assert!(tip.contains("not checked yet"), "{tip}");
        assert!(!tip.contains("offline"), "{tip}");
        assert!(!tip.contains(" ago"), "{tip}");
    }

    #[test]
    fn pill_label_live() {
        assert_eq!(pill_label(Some(true)), "Broken Stoic · LIVE");
    }

    #[test]
    fn pill_label_offline() {
        assert_eq!(pill_label(Some(false)), "Broken Stoic · offline");
    }

    #[test]
    fn pill_label_unknown_is_not_offline() {
        /* Decision D2: a failed or absent poll must never read as a fake offline. */
        let s = pill_label(None);
        assert_eq!(s, "Broken Stoic · unknown");
        assert!(!s.contains("offline"));
    }

    /// THE PILL DOES NOT NAME THE PLATFORM, IN ANY STATE, IN ANY WORDING.
    ///
    /// The owner's words: "That dosnt need to say twitch... polling there is fine. but dosnt need
    /// to say it." So this holds the claim itself and not one spelling of it: for every state, the
    /// label must contain NEITHER platform's label, and the whole label must be exactly the name,
    /// one separator and the state word, which is what leaves no room for a third run to creep
    /// back in beside them.
    ///
    /// IT READS THE PLATFORM NAMES FROM `Platform::label` rather than spelling them out, so a
    /// platform added or renamed in `settings.rs` is covered here the day it lands.
    #[test]
    fn the_pill_names_the_channel_and_the_state_and_nothing_else() {
        for live in [Some(true), Some(false), None] {
            let s = pill_label(live);
            for on in Platform::ALL {
                assert!(
                    !s.contains(on.label()),
                    "the pill still says {}: {s}",
                    on.label()
                );
            }
            assert_eq!(
                s,
                format!(
                    "{} · {}",
                    crate::settings::DISPLAY_NAME,
                    dot_of(live).word()
                ),
                "the pill is the channel and the answer, with one separator between them"
            );
            assert_eq!(
                s.matches('\u{00B7}').count(),
                1,
                "two separators means there is a third run on the pill: {s}"
            );
        }
    }

    /// THE PREFERENCE IS NOT DROPPED, IT MOVED TO THE HOVER. `pill_label` no longer takes a
    /// `Platform` at all, so the only way the reader can still find out which of the two channels
    /// the pill is reporting is the tooltip, and that makes this the test that keeps the fact in
    /// the window. Both platforms, so it cannot pass by hard-coding one.
    #[test]
    fn the_hover_carries_the_platform_the_pill_stopped_printing() {
        let now = Utc::now();
        let mut ch = Channel::unchecked("Broken_Stoic");
        ch.live = Some(true);
        for on in Platform::ALL {
            let tip = pill_tooltip(&ch, on, now);
            assert!(
                tip.contains(&format!("platform: {}", on.label())),
                "the hover has to name the platform the pill is reading: {tip}"
            );
            assert!(
                !pill_label(ch.live).contains(on.label()),
                "and the pill itself still must not"
            );
        }
    }

    /// THE PILL AND THE STRIP SPELL THE CHANNEL'S NAME THE SAME WAY, and this is the assertion the
    /// owner's complaint reduces to.
    ///
    /// The pill printed `Broken_Stoic`, the login, because it printed `Channel::handle`. Five
    /// pixels away the strip set a hand written `const NAME: &str = "BROKEN STOIC"`. Two spellings
    /// of one channel on one row, and neither could tell it was wrong, because each was internally
    /// consistent. Both come off `settings::DISPLAY_NAME` now, the strip by upper-casing it, and
    /// this holds the three claims that makes: the pill carries the name, the strip's caps ARE that
    /// name, and no underscore survives into either.
    ///
    /// IT READS THE PAINTED CAPS FROM `maker_block_size`, not from a literal in this test. That
    /// function measures the run the way `maker_block` draws it, so a strip that went back to a
    /// hand written literal of a DIFFERENT name would measure a different width and this would go
    /// red. A test that spelled `BROKEN STOIC` out itself would be the third copy of the problem.
    #[test]
    fn the_pill_and_the_strip_agree_on_the_name() {
        assert_eq!(crate::settings::DISPLAY_NAME, "Broken Stoic");
        /* THE PLATFORM LOOP WENT WITH THE PARAMETER. `pill_label` no longer takes a `Platform`,
         * so iterating one here would iterate a thing that cannot change the answer, which is a
         * loop that reads as coverage and is not any. The states are what vary. */
        for live in [Some(true), Some(false), None] {
            let s = pill_label(live);
            assert!(
                s.starts_with(crate::settings::DISPLAY_NAME),
                "the pill has to open with the name a reader sees: {s}"
            );
            assert!(!s.contains('_'), "the login is not the name: {s}");
        }
        /* the login is still the login, and still what the URLs are built from */
        assert_eq!(crate::settings::TWITCH_HANDLE, "Broken_Stoic");
        assert!(Platform::Twitch.url().ends_with("Broken_Stoic"));

        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 60.0))),
            ..Default::default()
        };
        let mut got = None;
        let mut out = ctx.run_ui(input, |ui| {
            got = Some((
                maker_block_size(ui, 9.5).x,
                tracked_width(ui, "BROKEN STOIC", 9.5, 9.5 * 0.23) - 9.5 * 0.23,
            ));
        });
        out.shapes.clear();
        out.drop_without_applying_deltas();
        let (block_w, caps_w) = got.expect("the closure ran");
        /* ONE MARK NOW, so one side and no gap between marks. This used to be two sides plus a
         * `MAKER_GLYPH_GAP`, which went with the second mark. */
        let side = 9.5 * MAKER_GLYPH;
        assert!(
            (block_w - (caps_w + 9.5 * MAKER_GAP + side)).abs() < 0.01,
            "the maker block measures {block_w} and BROKEN STOIC plus one mark is {}; the strip \
             is setting some other run",
            caps_w + 9.5 * MAKER_GAP + side
        );
    }

    #[test]
    fn dot_states_and_colours() {
        assert_eq!(dot_of(Some(true)), Dot::Live);
        assert_eq!(dot_of(Some(false)), Dot::Offline);
        assert_eq!(dot_of(None), Dot::Unknown);
        assert_eq!(Dot::Live.color(Platform::Twitch), TWITCH_PURPLE);
        assert_eq!(Dot::Live.color(Platform::YouTube), YOUTUBE_RED);
        for on in Platform::ALL {
            /* not streaming is not a brand: recognition spent on nothing */
            assert_eq!(Dot::Offline.color(on), TEXT_3);
            assert_eq!(Dot::Unknown.color(on), TEXT_3);
        }
        /* offline is a fact and is filled; unknown is the absence of one and is a hollow ring */
        assert!(Dot::Offline.filled());
        assert!(!Dot::Unknown.filled());
        assert!(Dot::Live.filled());
    }

    /// THE FIVE STATE COLOURS ARE FOR STATES, AND THE PILL IS NOT ONE.
    ///
    /// The live dot was `WRONG` red, which made this the only place in the tree spending a state
    /// colour on something that is not that state: a channel that is broadcasting is not a
    /// refusal, and `theme.rs` states the rule with no exception in it. This fails on that code.
    ///
    /// WORKING, WRONG and SETTLED are the three that can be checked by VALUE. `IDLE` and `YOU`
    /// cannot: `IDLE` is `TEXT_3` to the byte and `YOU` is `GOLD`, so no assertion can tell a dot
    /// drawn in the state from the same dot drawn in the palette entry it shares. Those two are
    /// held by naming the palette entry in `color` above, which is the same way `persona.rs` keeps
    /// its gold.
    #[test]
    fn no_dot_borrows_a_state_colour() {
        for d in [Dot::Live, Dot::Offline, Dot::Unknown] {
            for on in Platform::ALL {
                for (name, state) in [("WORKING", WORKING), ("WRONG", WRONG), ("SETTLED", SETTLED)]
                {
                    assert_ne!(
                        d.color(on),
                        state,
                        "{d:?} on {} is drawn in {name}, and a {name} row has to keep meaning \
                         {name}",
                        on.label()
                    );
                }
            }
        }
    }

    /// NEITHER FOREIGN MARK COLOUR MAY BE MISTAKEN FOR A STATE, AND THE RED IS THE ONE AT RISK.
    ///
    /// The purple was never in danger of being read as a refusal. `YOUTUBE_RED` is, because `WRONG`
    /// is also a red, and a six pixel dot is not much surface on which to tell two reds apart. An
    /// inequality alone would not settle that: `#C74A3D` is not `#C74A3C` and would satisfy it
    /// while failing every reader alive. So this asserts the inequality AND a floor under the
    /// distance, and the floor is met on the channel that actually separates them, which is
    /// saturation: `WRONG` is a brick with real green and blue in it (`0x4A`, `0x3C`) and the mark
    /// is `#FF0000` with none.
    ///
    /// It walks every state colour against every mark colour, hover forms included, because a
    /// `_HI` variant is the one a pointer produces and is exactly as capable of colliding.
    #[test]
    fn no_platform_colour_is_a_state_colour() {
        let states = [
            ("IDLE", IDLE),
            ("WORKING", WORKING),
            ("YOU", YOU),
            ("WRONG", WRONG),
            ("SETTLED", SETTLED),
        ];
        for on in Platform::ALL {
            let (mark, hi) = mark_colours(on);
            for (which, c) in [("mark", mark), ("hover", hi)] {
                for (name, s) in states {
                    assert_ne!(c, s, "the {} {which} is exactly {name}", on.label());
                    let far = [
                        (c.r() as i32 - s.r() as i32).abs(),
                        (c.g() as i32 - s.g() as i32).abs(),
                        (c.b() as i32 - s.b() as i32).abs(),
                    ]
                    .into_iter()
                    .max()
                    .unwrap_or(0);
                    assert!(
                        far >= 48,
                        "the {} {which} is within {far} of {name} on its furthest channel; a \
                         reader has to be able to tell a platform's mark from a state",
                        on.label()
                    );
                }
            }
        }
        /* and the mark colours are not each other, which is the whole reason there are two */
        assert_ne!(
            mark_colours(Platform::Twitch).0,
            mark_colours(Platform::YouTube).0
        );
    }

    /// The pill prints an age ONLY when the check has gone stale, because beside a state word a
    /// bare number reads as the duration of that state. `offline 1m` is not "offline for a minute"
    /// and `LIVE 30s` is not "live for thirty seconds", and the second one is the dangerous shape:
    /// a wrong number that looks like stream uptime.
    #[test]
    fn a_fresh_check_prints_no_age_and_a_stale_one_does() {
        let now = Utc::now();
        let fresh = Some(now - Duration::seconds(60));
        let stale = Some(now - Duration::seconds(400));

        // the ordinary case, both states: nothing after the word
        assert!(!pill_shows_age(Dot::Offline, fresh, now));
        assert!(!pill_shows_age(Dot::Live, fresh, now));

        // stale: the age comes back, because the state on screen may no longer be true
        assert!(pill_shows_age(Dot::Offline, stale, now));
        assert!(pill_shows_age(Dot::Live, stale, now));

        // unknown already says the app does not know, so an age adds nothing
        assert!(!pill_shows_age(Dot::Unknown, stale, now));
        // and never checked has no age to print
        assert!(!pill_shows_age(Dot::Offline, None, now));

        // the boundary is the threshold itself, not one second past it
        assert!(pill_shows_age(
            Dot::Offline,
            Some(now - PILL_STALE_AFTER),
            now
        ));
        assert!(!pill_shows_age(
            Dot::Offline,
            Some(now - PILL_STALE_AFTER + Duration::seconds(1)),
            now
        ));
    }

    /// The threshold is derived from the poll interval rather than picked: under it the watcher is
    /// keeping up, over it at least two polls have gone missing. A change to one must move the
    /// other, so this pins the relationship and not the number.
    #[test]
    fn stale_threshold_is_more_than_two_poll_intervals() {
        let poll = Duration::from_std(crate::watcher::POLL_EVERY).unwrap();
        assert!(
            PILL_STALE_AFTER > poll * 2,
            "a single missed poll must not read as stale"
        );
        assert!(
            PILL_STALE_AFTER < poll * 4,
            "three missed polls must not still look fine"
        );
    }

    #[test]
    fn age_label_units() {
        assert_eq!(age_label(Duration::seconds(0)), "0s");
        assert_eq!(age_label(Duration::seconds(59)), "59s");
        assert_eq!(age_label(Duration::seconds(60)), "1m");
        assert_eq!(age_label(Duration::seconds(119)), "1m");
        assert_eq!(age_label(Duration::seconds(120)), "2m");
        assert_eq!(age_label(Duration::seconds(3 * 3_600)), "3h");
        assert_eq!(age_label(Duration::seconds(86_399)), "23h");
        assert_eq!(age_label(Duration::seconds(86_400)), "1d");
        assert_eq!(age_label(Duration::seconds(4 * 86_400 + 5)), "4d");
    }

    #[test]
    fn age_label_never_negative() {
        assert_eq!(age_label(Duration::seconds(-5)), "0s");
    }

    #[test]
    fn age_of_none_when_never_checked() {
        assert_eq!(age_of(None, Utc::now()), None);
    }

    #[test]
    fn age_of_two_minutes() {
        let now = Utc::now();
        assert_eq!(
            age_of(Some(now - Duration::seconds(125)), now).as_deref(),
            Some("2m")
        );
    }

    #[test]
    fn footer_reads_the_manifest() {
        /* The footer prints these two verbatim; they are facts about Cargo.toml, not strings. */
        assert!(
            version().chars().next().is_some_and(|c| c.is_ascii_digit()),
            "{}",
            version()
        );
        /* AGPL-3.0-only since 2026-09-11. `licence()` is `CARGO_PKG_LICENSE`, so this goes red the
         * day the manifest and the footer disagree; `grimoire-forge`'s licence gate is what holds
         * the manifest and the LICENSE file to each other. */
        assert_eq!(licence(), "AGPL-3.0-only");
    }

    #[test]
    fn source_link_only_when_a_repository_is_declared() {
        /* The link is a compile time fact about Cargo.toml, never a guess: with no repository
         * declared there is no link AND no words, because "Source on GitHub" over nothing would
         * be the one invented fact on the page. */
        assert_eq!(
            source_url().is_some(),
            !env!("CARGO_PKG_REPOSITORY").is_empty()
        );
        assert_eq!(source_link().is_some(), source_url().is_some());
        if let Some((label, u)) = source_link() {
            assert!(u.starts_with("https://"), "{u}");
            assert_eq!(label, source_label(u));
        }
    }

    #[test]
    fn source_label_names_github_only_for_github() {
        assert_eq!(
            source_label("https://github.com/James-McMenamin/eql-grimoire"),
            "Source on GitHub"
        );
        assert_eq!(
            source_label("https://www.github.com/x/y"),
            "Source on GitHub"
        );
        assert_eq!(source_label("https://gitlab.com/x/y"), "Source");
        assert_eq!(source_label("https://notgithub.com/x/y"), "Source");
        assert_eq!(
            source_label("https://github.com.evil.example/x"),
            "Source",
            "a host that merely starts with the name"
        );
    }

    /// THE THIRD PLACE THE LOGIN WAS WRITTEN DOWN IS GONE, AND THIS IS WHAT IS LEFT OF THAT TEST.
    ///
    /// `TWITCH_URL` was a `const` in this file spelling `Broken_Stoic` out inside a literal URL,
    /// and it predated `settings::TWITCH_HANDLE` being the one constant the whole app reads. A
    /// `const` cannot call a function, so the old version of this test held the two literals equal
    /// and hoped. `Platform::url` is a function and BUILDS the URL from the handle, so there is no
    /// second literal left to drift: the relationship is derived and this pins the shape.
    #[test]
    fn a_platform_url_is_built_from_the_handle_and_never_spelled_out() {
        assert_eq!(
            Platform::Twitch.url(),
            "https://www.twitch.tv/Broken_Stoic",
            "the maker's mark opens the channel this app polls"
        );
        assert_eq!(
            Platform::Twitch.url(),
            format!("https://www.twitch.tv/{}", crate::settings::TWITCH_HANDLE)
        );
        assert_eq!(
            Platform::YouTube.url(),
            format!(
                "https://www.youtube.com/channel/{}",
                crate::settings::YOUTUBE_CHANNEL_ID
            )
        );
        /* no literal URL for either platform survives in this file */
        let src = include_str!("titlebar.rs");
        let body = &src[..src.find("mod tests").unwrap_or(src.len())];
        for host in ["twitch.tv/", "youtube.com/"] {
            assert!(
                !body.contains(host),
                "a {host} URL is written out in this file again; build it from Platform::url"
            );
        }
    }

    /* --------------------------------------------- the second mark, and the pill -- */

    /// One headless frame of the REAL title strip, flattened. `Shape::Vec` is nested by egui and a
    /// test that read only the top level would find nothing and pass, which is the shape of every
    /// reachability failure this tree has had.
    fn strip_frame(watch_on: Platform, live: &Status) -> Vec<egui::Shape> {
        strip_frame_at(900.0, Lead::Maker, watch_on, live)
            .into_iter()
            .map(|(_, s)| s)
            .collect()
    }

    /// The same at a chosen width and lead, each shape paired with THE CLIP IT WAS PAINTED UNDER.
    /// The clip is half of what this file's layout promises (`paint_pill` clips the pill's runs to
    /// the pill), and a test that flattened it away could not see that promise kept or broken.
    fn strip_frame_at(
        w: f32,
        lead: Lead<'static>,
        watch_on: Platform,
        live: &Status,
    ) -> Vec<(Rect, egui::Shape)> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        crate::theme::install(&ctx);
        strip_pass(
            &ctx,
            w,
            lead,
            watch_on,
            live,
            Vec::new(),
            &mut Hits::default(),
        )
    }

    /// One pass of the real strip on a context the CALLER owns, with input events, reporting what
    /// it was clicked on.
    ///
    /// THE CONTEXT IS A PARAMETER BECAUSE A CLICK NEEDS TWO PASSES. egui resolves interaction
    /// against the widget rects registered on the previous pass, so a press and release in the
    /// first frame a widget exists reaches nothing. `strip_frame_at` above is the one-pass case,
    /// and it goes through here so there is exactly one copy of the setup and the flattening.
    fn strip_pass(
        ctx: &egui::Context,
        w: f32,
        lead: Lead<'static>,
        watch_on: Platform,
        live: &Status,
        events: Vec<egui::Event>,
        hits: &mut Hits,
    ) -> Vec<(Rect, egui::Shape)> {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(w, STRIP_H))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            strip(ui, lead, live, watch_on, false, hits);
        });
        let shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();

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
        let mut flat = Vec::new();
        for cs in shapes {
            flatten(cs.clip_rect, cs.shape, &mut flat);
        }
        flat
    }

    /// The pill's stadium: the one `Shape::Rect` with the full corner radius. `theme` makes the
    /// app square everywhere except this, which is what makes it findable.
    fn stadium(shapes: &[(Rect, egui::Shape)]) -> Rect {
        let want = (PILL_H / 2.0) as u8;
        let found: Vec<Rect> = shapes
            .iter()
            .filter_map(|(_, s)| match s {
                egui::Shape::Rect(r) if r.corner_radius.nw == want => Some(r.rect),
                _ => None,
            })
            .collect();
        assert!(
            !found.is_empty(),
            "the strip painted no pill; the one round shape in this app is its stadium"
        );
        found[0]
    }

    /// THE STRIP SURVIVES THE NARROWEST WINDOW IT ALLOWS, AND THIS IS A REGRESSION TEST FOR A BUG
    /// THIS UNIT INTRODUCED AND THEN FOUND.
    ///
    /// Naming the platform took the pill from about 137 pixels to about 207. `windows.rs` sets
    /// tool windows a `min_inner_size` of 320 wide and the four window buttons take 120 of it, so
    /// `controls_left - 10 - pill_w` went NEGATIVE, which is not a cramped strip: `drag_r` was
    /// built from it as `Rect::from_min_max(left, pill.left - 6)`, an inside-out rectangle that
    /// `contains` answers false for at every point. The window would have become undraggable, in
    /// silence, at a width nothing tests at, and the pill would have been painted across the
    /// window's own title on its way off the left edge.
    ///
    /// SO THIS ASSERTS THE LAYOUT AND NOT THE ARITHMETIC. It reads the painted stadium back out of
    /// a real frame at exactly that minimum width, in both leads and on both platforms, and holds
    /// three things: the pill starts at or after `PILL_MIN_LEAD`, so there is a strip left to drag
    /// the window by and `drag_r` is a rectangle; it ends before the window buttons; and NOTHING
    /// the pill contains is painted outside it, which is the clip in `paint_pill` doing its work.
    #[test]
    fn the_strip_survives_the_narrowest_window_it_allows() {
        /* windows.rs: `.with_min_inner_size([320.0, 200.0])` for every tool window. */
        const MIN_W: f32 = 320.0;
        let mut live = Status {
            twitch: Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        /* the widest the pill ever gets: a stale check adds the age run beside the state word */
        live.twitch.checked_at = Some(Utc::now() - chrono::Duration::seconds(600));
        live.youtube.checked_at = live.twitch.checked_at;

        for lead in [Lead::Maker, Lead::Title("Looking for group")] {
            for on in Platform::ALL {
                let shapes = strip_frame_at(MIN_W, lead, on, &live);
                let pill = stadium(&shapes);
                assert!(
                    pill.left() >= PILL_MIN_LEAD - 0.01,
                    "the pill starts at {} on a {MIN_W} wide strip, so the drag region is {} wide \
                     or inside out and the window cannot be moved",
                    pill.left(),
                    pill.left() - 6.0
                );
                assert!(
                    pill.right() <= MIN_W - BTN_W * 4.0,
                    "the pill at {} runs under the window buttons",
                    pill.right()
                );
                /* NOTHING THE PILL CONTAINS ESCAPES IT. Every run drawn at or after the pill's
                 * left edge is one of the pill's own, and each has to be clipped to the pill: at
                 * this width the runs measure wider than the rect they were handed, so without
                 * the clip in `paint_pill` the tail would be drawn over the window buttons. */
                for (clip, s) in &shapes {
                    let egui::Shape::Text(t) = s else { continue };
                    assert!(
                        t.pos.x >= 0.0,
                        "a run was painted off the left edge at {:?}: {:?}",
                        t.pos,
                        t.galley.text()
                    );
                    if t.pos.x >= pill.left() {
                        assert!(
                            clip.right() <= pill.right() + 0.01,
                            "the run {:?} inside the pill is clipped to {clip:?}, which is wider \
                             than the pill {pill:?}; its tail runs out of the stadium",
                            t.galley.text()
                        );
                    }
                }
            }
        }
    }

    /// Every path painted in `want`, as (its points, whether it was filled in that colour).
    fn paths_in(shapes: &[egui::Shape], want: Color32) -> Vec<(Vec<Pos2>, bool)> {
        use egui::epaint::ColorMode;
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Path(p) => {
                    let stroked = matches!(p.stroke.color, ColorMode::Solid(c) if c == want);
                    let filled = p.fill == want;
                    (stroked || filled).then(|| (p.points.clone(), filled))
                }
                _ => None,
            })
            .collect()
    }

    /// THE YOUTUBE MARK REACHES A PIXEL, AND THIS IS THE ONLY THING THAT PROVES IT.
    ///
    /// The brief this was written against is blunt about the defect it keeps meeting: wiring that
    /// compiles, passes its tests, and paints nothing, most recently a nav row wired to a state
    /// `chrome::nav_row` does not draw a mark for. A function called `youtube_glyph` and a call to
    /// it are not evidence of anything. So this runs the REAL `strip` with the REAL `Lead::Maker`
    /// arm the main window uses, flattens what came out, and looks for shapes in `YOUTUBE_RED`.
    ///
    /// THE STRIP PAINTS THE CHOSEN PLATFORM'S MARK AND THE OTHER ONE'S NOT AT ALL.
    ///
    /// It used to paint both, and the test that stood here asserted exactly that: the YouTube badge
    /// NEXT TO the Twitch bubble, in that order, neither drawn over the other. That was right while
    /// the strip was saying "this channel is on both of these". It says something else now, which
    /// is what everything else in the window already said: the reader picked a platform in settings
    /// and this is it. A second mark is a link to a channel they have said they do not watch on.
    ///
    /// IT IS DRIVEN OVER BOTH SETTINGS, and that is the whole test rather than half of it. Checking
    /// only Twitch would pass on a strip that had simply stopped drawing the YouTube mark, which is
    /// a different change with the same symptom on one machine. Each platform is required to paint
    /// its own mark AND required not to paint the other's.
    ///
    /// IT READS THE PAINT AND BOTH HALVES OF EACH MARK, because the badge alone is a rounded box
    /// and the play head alone is a triangle, and either on its own is not the mark: an eight point
    /// hollow outline and a three point FILLED triangle. A function called `youtube_glyph` and a
    /// call to it are not evidence of anything; this runs the REAL `strip` with the REAL
    /// `Lead::Maker` arm the main window uses and looks at what came out.
    #[test]
    fn the_strip_paints_only_the_platform_the_reader_chose() {
        let live = Status {
            twitch: Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };

        for on in Platform::ALL {
            let shapes = strip_frame(on, &live);
            let (mine, theirs) = match on {
                Platform::Twitch => (TWITCH_PURPLE, YOUTUBE_RED),
                Platform::YouTube => (YOUTUBE_RED, TWITCH_PURPLE),
            };

            /* THE MARK IS THERE, in both its halves. Twitch's bubble is one eight point outline;
             * YouTube's is that plus a filled play head inside it. */
            let ours = paths_in(&shapes, mine);
            let badge = ours
                .iter()
                .find(|(p, _)| p.len() == 8)
                .unwrap_or_else(|| panic!("{on:?}: its own mark was never painted"));
            if on == Platform::YouTube {
                let head = ours
                    .iter()
                    .find(|(p, filled)| *filled && p.len() == 3)
                    .expect("the YouTube play head, a filled triangle, was never painted");
                let xs = |pts: &[Pos2]| {
                    (
                        pts.iter().map(|p| p.x).fold(f32::MAX, f32::min),
                        pts.iter().map(|p| p.x).fold(f32::MIN, f32::max),
                    )
                };
                let (bl, br) = xs(&badge.0);
                for p in &head.0 {
                    assert!(
                        p.x >= bl && p.x <= br,
                        "the play head at {p:?} is outside its badge ({bl} to {br})"
                    );
                }
            }

            /* AND THE OTHER PLATFORM'S IS NOT. This is the assertion the change is about. */
            assert!(
                paths_in(&shapes, theirs).is_empty(),
                "{on:?} is the chosen platform and the strip still painted the other one's mark"
            );

            /* And it is inside the strip rather than clipped off the end of it. */
            for p in &badge.0 {
                assert!(
                    p.y >= 0.0 && p.y <= STRIP_H && p.x >= 0.0 && p.x <= 900.0,
                    "{on:?}: a mark was painted at {p:?}, outside the strip"
                );
            }
        }
    }

    /// THE PREFERENCE CHANGES WHAT THE PILL SAYS, AND NOT ONLY WHAT A STRUCT HOLDS.
    ///
    /// This is the whole point of the setting, so it is proven the same way: two real strip frames,
    /// one preferring each platform, reading the words back out of the painted shapes. The two
    /// channels are given OPPOSITE states, so a pill that ignored the preference and kept reading
    /// `Status::twitch` would print `LIVE` under both and fail.
    ///
    /// AND IT REPORTS THE PREFERENCE WITHOUT PRINTING IT, WHICH IS CHANGE A. The platform's own
    /// name must not appear anywhere in the painted strip's PILL. This is checked against the real
    /// frame rather than against `pill_label`, because the label was only one of the two places
    /// the word came from: `paint_pill` drew `parts.on.label()` as a run of its own, so a label
    /// fixed on its own would still have left `Twitch` on screen. The maker's block at the far
    /// left of the same strip paints no platform WORD (its marks are painted glyphs, see
    /// `maker_block`), so any run equal to a platform label in this frame is the pill's.
    ///
    /// AND THE DOT FOLLOWS THE MARK. A purple dot on a strip that prefers YouTube would be this
    /// file asserting a platform the pill is not reading, which is exactly the claim `Dot::color`
    /// was changed to stop making. It is also now the ONLY thing the pill paints that answers
    /// "which platform", so it matters more than it did.
    #[test]
    fn the_pill_reports_the_preferred_platform_without_naming_it() {
        let mut live = Status {
            twitch: Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        live.twitch.live = Some(true);
        live.youtube.live = Some(false);

        for (on, word, other) in [
            (Platform::Twitch, "LIVE", "offline"),
            (Platform::YouTube, "offline", "LIVE"),
        ] {
            let shapes = strip_frame(on, &live);
            let runs: Vec<String> = shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
            assert!(
                runs.iter().any(|r| r == crate::settings::DISPLAY_NAME),
                "the pill has to print the name: {runs:?}"
            );
            for p in Platform::ALL {
                assert!(
                    !runs.iter().any(|r| r == p.label()),
                    "the strip still paints the word {} on its pill: {runs:?}",
                    p.label()
                );
            }
            assert!(
                runs.iter().any(|r| r == word),
                "the pill reports {} and that channel is {word}: {runs:?}",
                on.label()
            );
            assert!(
                !runs.iter().any(|r| r == other),
                "the pill printed the OTHER platform's state, so the preference reached nothing: \
                 {runs:?}"
            );
            assert!(
                !runs.iter().any(|r| r.contains('_')),
                "the login leaked back onto the pill: {runs:?}"
            );

            /* the dot: filled, in the mark colour of the platform being reported, only when live */
            let dots: Vec<Color32> = shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Circle(c) if c.radius > 0.0 => Some(c.fill),
                    _ => None,
                })
                .collect();
            let want = if word == "LIVE" {
                mark_colours(on).0
            } else {
                TEXT_3
            };
            assert!(
                dots.contains(&want),
                "the {} pill's dot is not {want:?}; it painted {dots:?}",
                on.label()
            );
            if word == "LIVE" {
                assert!(
                    !dots.contains(
                        &mark_colours(match on {
                            Platform::Twitch => Platform::YouTube,
                            Platform::YouTube => Platform::Twitch,
                        })
                        .0
                    ),
                    "the dot is drawn in the colour of a platform the pill is not reading"
                );
            }
        }
    }

    /// CHANGE B AT THE STRIP: CLICKING THE PILL REPORTS A WATCH, AND ONLY CLICKING THE PILL DOES.
    ///
    /// TWO PASSES, because egui resolves interaction against the previous pass's widget rects (see
    /// `strip_pass`). Pass one finds the painted stadium; pass two clicks its centre.
    ///
    /// THE MISS IS HALF THE TEST. A strip that raised `watch` from anywhere, or every frame, would
    /// satisfy the hit and fail this: the click lands in the DRAG lead at the far left, which is
    /// the part of the strip that moves the window, and nothing may be asked for there.
    ///
    /// AND THE OTHER TWO FLAGS STAY DOWN. `Hits` is one struct with three fields now, and the
    /// failure it exists to prevent is a click reported as the wrong one of them.
    #[test]
    fn a_click_on_the_pill_reports_a_watch_and_a_click_beside_it_does_not() {
        let mut live = Status {
            twitch: Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        live.twitch.live = Some(true);

        let click = |at: Pos2| {
            vec![
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
            ]
        };

        for lead in [Lead::Maker, Lead::Title("Parser")] {
            let ctx = egui::Context::default();
            crate::fonts::install(&ctx);
            crate::theme::install(&ctx);
            let mut hits = Hits::default();
            let first = strip_pass(
                &ctx,
                900.0,
                lead,
                Platform::Twitch,
                &live,
                Vec::new(),
                &mut hits,
            );
            assert!(!hits.watch, "a pass with no input reported a click");
            let pill = stadium(&first);

            /* the drag lead: left of the pill, on the strip's own row */
            let mut miss = Hits::default();
            strip_pass(
                &ctx,
                900.0,
                lead,
                Platform::Twitch,
                &live,
                click(Pos2::new(PILL_MIN_LEAD / 2.0, STRIP_H / 2.0)),
                &mut miss,
            );
            assert!(
                !miss.watch,
                "a click on the drag region of a {:?} strip reported a watch",
                lead.key()
            );

            let mut hit = Hits::default();
            strip_pass(
                &ctx,
                900.0,
                lead,
                Platform::Twitch,
                &live,
                click(pill.center()),
                &mut hit,
            );
            assert!(
                hit.watch,
                "clicking the pill on a {:?} strip reported nothing, so the owner's click reaches \
                 no stream",
                lead.key()
            );
            assert!(!hit.pin, "the pill's click was reported as the pin");
            assert!(!hit.close, "the pill's click was reported as close");
        }
    }

    /// THE STRIP'S PILL IS DRAWN WHOLE AT EVERY WIDTH THE MAIN WINDOW CAN REACH, AND THAT IS WHY
    /// THE FOOTER DOES NOT NEED ONE.
    ///
    /// THE ARGUMENT THIS RETIRES. The main window drew a second pill in its footer, bottom right,
    /// on every screen. One defence of it was available and had to be checked rather than waved
    /// off: `strip` CLAMPS its pill when a window is too narrow for the lead, the pill and the
    /// four controls at once, and `paint_pill` then clips the runs that do not fit, so on a narrow
    /// enough strip the pill reads `Broken Stoic ·` with the answer cut off it. A footer pill
    /// would be the reader's way to the word in that state.
    ///
    /// IT CANNOT HAPPEN IN THIS WINDOW, AND THESE ARE MEASURED NUMBERS RATHER THAN AN ESTIMATE.
    /// The widest the pill ever gets is 151.4 points, on both platforms: name, separator, state
    /// word and, once the check is `PILL_STALE_AFTER` old, the age run. The four controls take
    /// `BTN_W * 4` = 120 and the gap takes 10, so the clamp first bites at 151.4 + 10 + 120 +
    /// `PILL_MIN_LEAD` = 307.4 points of window. `main.rs` will not let the main window go under
    /// 880. That is 572 points of slack, and the first cut of this test asserted it at a width
    /// where nothing could ever have clamped, which is why the mutation below exists.
    ///
    /// (The same arithmetic says a TOOL window at its 320 minimum is not clamped either, by 12.6
    /// points. `the_strip_survives_the_narrowest_window_it_allows` is still the test that matters
    /// there, because it holds the layout when the clamp DOES bite; this one holds that in the one
    /// window that had a footer, it does not.)
    ///
    /// HOW IT MEASURES RATHER THAN ASSERTS. The stadium painted at the main window's own minimum
    /// has to be the same width as the one painted on a strip wide enough that no clamp is
    /// arithmetically possible. Equal widths mean nothing was taken off. The second claim is the
    /// margin itself, in points, so a change that leaves the pill whole but with almost nothing
    /// to spare is reported before the next one clips it.
    ///
    /// THE NUMBER IS READ OUT OF `main.rs`, NOT COPIED HERE. A minimum this test remembered would
    /// go on passing after somebody let the window get narrower, which is the one change that
    /// makes the retired argument true again. Between the two claims this demands a floor above
    /// 407.4: under 307.4 the pill is clipped and the width claim goes, and between the two the
    /// pill is whole on a margin too thin to have removed a fallback on the strength of.
    ///
    /// AND IT IS NOT "ANY EDIT TO THAT LITERAL FAILS", WHICH IS WHAT IT WOULD BE WORTH IF IT WERE.
    /// Dropping the floor to 500 leaves 192.6 points of margin and this passes; the mutation run
    /// keeps that case as its control precisely so the three that do fail mean something.
    ///
    /// BOTH PLATFORMS, because the preference still decides the dot's colour, and a run of it
    /// coming back to the label (it was there once) would change the width.
    #[test]
    fn the_strip_pill_is_never_squeezed_in_the_main_window() {
        /* THE FLOOR IS A CONSTANT NOW AND THIS USED TO READ IT OUT OF THE SOURCE.
         *
         * It found `.with_min_inner_size([` in `main.rs` and parsed the number after it. That
         * worked while the call carried a literal and broke the moment the owner set a design
         * size: the call now reads `.with_min_inner_size([FLOOR[0], FLOOR[1]])` and `FLOOR[0]`
         * does not parse as an `f32`.
         *
         * READING THE CONSTANT IS BETTER THAN READING THE SOURCE, and it is the fix rather than
         * a patch: a string search over another module's text is a test that fails on
         * FORMATTING. The value is `pub` and this crate can simply ask for it, so a change to
         * the floor reaches this assertion by the type system instead of by a substring.
         *
         * THE PATH IS THE BINARY'S. `main.rs` is the bin target and this is the lib, so the
         * constant is reached through the module this file already includes as source. */
        let min_w = crate::chrome::HARD_FLOOR[0];
        assert!(min_w > 0.0, "the minimum window width read as {min_w}");

        let mut live = Status {
            twitch: Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        /* the widest the pill ever gets: a stale check adds the age run beside the state word */
        live.twitch.checked_at = Some(Utc::now() - chrono::Duration::seconds(600));
        live.youtube.checked_at = live.twitch.checked_at;

        for on in Platform::ALL {
            /* Wide enough that `pill_right - pill_w` cannot fall under `PILL_MIN_LEAD`, so this
             * arm is the pill at its full measured width by construction. */
            let roomy = stadium(&strip_frame_at(2000.0, Lead::Maker, on, &live)).width();
            let at_min = stadium(&strip_frame_at(min_w, Lead::Maker, on, &live));
            assert!(
                (at_min.width() - roomy).abs() < 0.01,
                "on {on:?} the main window's narrowest strip ({min_w} wide) paints a pill {} wide \
                 where a roomy one paints {roomy}: it is being clamped, so the state word can be \
                 clipped off it and the footer pill that was cut had a job after all",
                at_min.width()
            );
            /* THE MARGIN, NAMED. `pill_left` is clamped up to `PILL_MIN_LEAD`, so how far above
             * that it landed is exactly how much narrower this window could get before the pill
             * starts losing characters. */
            let slack = at_min.left() - PILL_MIN_LEAD;
            assert!(
                slack > 100.0,
                "on {on:?} the pill at the main window's minimum has only {slack} points before \
                 the clamp bites; the footer pill was removed on the strength of that margin, so \
                 a margin this thin is a decision to revisit and not a test to relax"
            );
        }
    }

    /// NO PILL IS DRAWN WITH ITS CLICK THROWN AWAY, AND NO STRIP EITHER.
    ///
    /// THIS IS THE REACHABILITY FLOOR FOR CHANGE B. `pill` returns a `Response` and `strip` fills
    /// a `Hits`; both are easy to call and ignore, and rustc says nothing about either, because a
    /// dropped `Response` is a legal expression statement and a `Hits` written and not read is a
    /// struct that was written to. Ignoring one is not a cosmetic slip: it is the whole feature
    /// gone from that surface, in silence, exactly the way this tree has shipped uncalled code
    /// three times.
    ///
    /// WHAT IT DEMANDS. Every call that paints a pill in this crate's production text must be
    /// followed immediately by `.clicked()`, matched by counting parentheses from the call rather
    /// than by hoping the call fits on one line. And every file that draws a `titlebar::strip(..)`
    /// must name `Ask::WatchHere` somewhere in its production text, which is the only thing a
    /// caller can usefully do with `hits.watch`.
    ///
    /// AND IT NOW ALSO DEMANDS THAT THERE IS ONLY ONE. The count used to be four: `pub fn pill`,
    /// the strip's, the main window's footer, and the Watch screen's body. The last two were the
    /// same function with the same three arguments as the strip's, in the same frame, so they
    /// could not report anything different and did not: they were one sentence said three times
    /// on every screen, and four times on the Watch screen while the channel was offline. Cutting
    /// them left `pub fn pill` with no production caller at all, which in a lib crate is dead
    /// public API rustc never mentions, so that went too. What remains is `strip`'s, and `strip`
    /// is what every window in the app draws. A count of one is not a smaller version of the old
    /// assertion, it is the new rule: a second pill anywhere in these four files fails here.
    ///
    /// TEST TEXT IS CUT OUT FIRST, the same way `reach.rs` does it, so a call inside a test module
    /// cannot stand in for a caller and a `mod tests` cannot satisfy the strip rule.
    #[test]
    fn no_pill_is_drawn_with_its_click_thrown_away() {
        let files: [(&str, &str); 4] = [
            ("main.rs", include_str!("main.rs")),
            ("windows.rs", include_str!("windows.rs")),
            ("screens/watch.rs", include_str!("screens/watch.rs")),
            ("titlebar.rs", include_str!("titlebar.rs")),
        ];
        let mut pills = 0usize;
        let mut strips = 0usize;
        for (name, src) in files {
            let body = &src[..src.find("\nmod tests {").unwrap_or(src.len())];
            /* `paint_pill` is how this file's own `strip` draws it; the others go through `pill`. */
            for call in ["titlebar::pill(", "paint_pill(ui,"] {
                let mut from = 0usize;
                while let Some(at) = body[from..].find(call) {
                    let at = from + at;
                    let open = at + body[at..].find('(').expect("the call opens");
                    let mut depth = 0usize;
                    let mut close = open;
                    for (i, c) in body[open..].char_indices() {
                        match c {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    close = open + i;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    let tail: String = body[close + 1..]
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .take(10)
                        .collect();
                    /* EITHER READ IT OR HAND IT ON, and `}` is the second of those. A call whose
                     * closing paren is followed by the end of its block is that block's value, so
                     * the `Response` leaves through the function's return type. It cannot be a
                     * silent drop: a block tailing in a `Response` where `()` is expected does not
                     * compile. That is `pub fn pill`'s own body, which exists to pass the widget's
                     * response out to the three places that read it. */
                    assert!(
                        tail.starts_with(".clicked()") || tail.starts_with('}'),
                        "{name} draws a pill and drops its click: the call is followed by {tail:?}"
                    );
                    pills += 1;
                    from = close + 1;
                }
            }
            if body.contains("titlebar::strip(") {
                strips += 1;
                assert!(
                    body.contains("Ask::WatchHere"),
                    "{name} draws a title strip and never turns its pill's click into an ask, so \
                     the pill in that window is painted and dead"
                );
            }
        }
        /* A rule that matched nothing would pass in silence, which is the failure this whole test
         * is about. ONE call site: `strip`'s, which reads the click into `Hits::watch` and is the
         * one the main window's title bar and every tool window's share. Two files draw strips:
         * main.rs, windows.rs. */
        assert_eq!(
            pills, 1,
            "the pill call sites moved; count them and say so. There is meant to be exactly one, \
             in `strip`: the footer's and the Watch screen's were the same call with the same \
             arguments and were cut as duplicates"
        );
        assert_eq!(
            strips, 2,
            "the strip call sites moved; count them and say so"
        );
    }

    /// `pill_size` MEASURES EXACTLY WHAT `paint_pill` DRAWS, AND THIS IS THE STRONGER FORM OF THE
    /// TEST THAT USED TO SAY SO.
    ///
    /// It used to prove the agreement by ONE observable consequence of it: `YouTube` is wider than
    /// `Twitch` in Plex Sans, so the pill had to be wider when it preferred YouTube. Change A took
    /// the platform run off the pill, which takes that consequence with it. Rewriting it as "the
    /// two widths are now equal" would be the vacuous version: equal widths are also what a
    /// `pill_size` that measured NOTHING would return.
    ///
    /// SO IT READS THE PAINTED FRAME INSTEAD, which is the thing the old test was reaching for
    /// through a proxy. It takes the real stadium, and every run painted inside it, and holds two
    /// claims that between them pin the arithmetic to the ink:
    ///
    ///   TIGHT: the runs must reach within `PILL_PAD` (plus a hair for the last glyph's bearing)
    ///     of the stadium's right edge. A measurement that kept a width for a run the painter no
    ///     longer draws leaves a hole, and this is what finds it. With the platform run and its
    ///     separator still measured and not painted, the slack was over 40 points and this fails.
    ///   INSIDE: no run may cross the right edge. This is the other direction, the one the old
    ///     test named: a pill measured too narrow for what it paints.
    ///
    /// AND IT RUNS AT A GENEROUS WIDTH so the strip's own clamp (`the_strip_survives_the_narrowest
    /// _window_it_allows` covers the squeezed case) never truncates the runs and stands in for a
    /// measurement that fits.
    #[test]
    fn the_pill_measures_what_it_paints() {
        let mut live = Status {
            twitch: Channel::unchecked(crate::settings::TWITCH_HANDLE),
            youtube: Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        };
        for stale in [false, true] {
            /* both shapes of pill: the ordinary one, and the widest it ever gets (a stale check
             * adds the age run after the state word) */
            live.twitch.checked_at = stale.then(|| {
                Utc::now() - chrono::Duration::seconds(PILL_STALE_AFTER.num_seconds() * 2)
            });
            live.twitch.live = Some(true);
            live.youtube.checked_at = live.twitch.checked_at;
            live.youtube.live = Some(false);
            for on in Platform::ALL {
                let shapes = strip_frame_at(900.0, Lead::Maker, on, &live);
                let pill = stadium(&shapes);
                let runs: Vec<&egui::epaint::TextShape> = shapes
                    .iter()
                    .filter_map(|(_, s)| match s {
                        egui::Shape::Text(t) if t.pos.x >= pill.left() => Some(t),
                        _ => None,
                    })
                    .collect();
                assert!(
                    runs.len() >= 3,
                    "the pill paints the name, a separator and a state word at least: {}",
                    runs.len()
                );
                let ink_right = runs
                    .iter()
                    .map(|t| t.pos.x + t.galley.size().x)
                    .fold(f32::MIN, f32::max);
                let slack = pill.right() - ink_right;
                assert!(
                    slack >= -0.01,
                    "the pill was measured {slack} too narrow for what it paints, on {} \
                     (stale {stale}), so a run hangs out of the stadium",
                    on.label()
                );
                assert!(
                    slack <= PILL_PAD + 1.5,
                    "the pill reserves {slack} points past its last run on {} (stale {stale}), \
                     and its padding is {PILL_PAD}; pill_layout is measuring a run paint_pill does \
                     not draw",
                    on.label()
                );
            }
        }
    }
}
