//! Finding the pop-out window's native handle, and carving shapes out of the video.
//!
//! WHY THIS MODULE HAS TO EXIST AT ALL. `wry` builds a surface with
//! `WebViewBuilder::build_as_child`, which takes a `HasWindowHandle`. The only thing in this
//! process that implements it is `eframe::Frame`, and `Frame` is built ONCE, from the ROOT window
//! (`eframe-0.36.1/src/native/epi_integration.rs:202`, and the integration is a single field
//! constructed once at `glow_integration.rs:69,283`). A deferred viewport's callback is handed
//! `(&egui::Context, ViewportClass)` and nothing else. So eframe will never tell us where the
//! pop-out window is.
//!
//! IT DOES NOT HAVE TO. `wry::WebViewExtWindows::reparent` takes a plain `isize`
//! (`wry-0.56.1/src/lib.rs:2371`), not a handle type, so the whole problem reduces to finding one
//! integer. This module finds it, and refuses to guess when it cannot.
//!
//! THE DECISION HALF IS PURE AND THE WIN32 HALF IS THIN, deliberately. `pick` is where every rule
//! about "is this the right window" lives, it takes plain data, and it is tested on every platform
//! including the Linux CI that can never run a single call below it. The `#[cfg(windows)]` half is
//! wrappers with no judgement in them.
//!
//! MEASURED, NOT ASSUMED. Everything this module relies on was measured on this machine with a
//! throwaway probe before a line of it was written. In particular a
//! `SetParent` of wry's container preserves the live document (same nonce, same volume, unbroken
//! playback across the move), and a window region carved out of that container passes BOTH the
//! pixels and the pointer through to the parent.

/// The window class winit registers every one of its windows under, on Windows.
///
/// It is not unique to this app, which is exactly why it is never the only test: it narrows an
/// enumeration to "a winit window" and [`pick`] does the rest. Enumerating only OUR ui thread is
/// what makes the process boundary irrelevant.
pub const WINIT_CLASS: &str = "Window Class";

/// How far a window's reported rectangle may sit from the one the viewport believes it has, in
/// physical pixels, and still be the same window.
///
/// TWO, AND ZERO WOULD BE WRONG. The rectangle we compare against is `ViewportInfo::outer_rect`,
/// which `egui-winit` produced by dividing `GetWindowRect` by `pixels_per_point` and which we
/// multiply back. At 125% and 150% scaling that round trip through `f32` does not land on the
/// integer it started from, so an exact match would reject the right window on exactly the
/// machines most people have. Two pixels is far tighter than the gap between any two windows this
/// app opens and far looser than the error.
pub const RECT_SLOP: i32 = 2;

/// The shapes the video withdraws from, in a window`s own client pixels: left, top, right,
/// bottom. Named because it travels through five signatures and a bare tuple vector at each of
/// them says nothing about which corner of which window the numbers are in.
pub type Holes = Vec<(i32, i32, i32, i32)>;

/// One window seen by an enumeration, flattened so [`pick`] can be pure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub hwnd: isize,
    pub class: String,
    pub title: String,
    /// Left, top, right, bottom, in physical screen pixels.
    pub rect: (i32, i32, i32, i32),
}

/// Why no handle was returned. Every arm is a refusal, and that is the point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickErr {
    /// Nothing matched. The window may not exist yet, which is not an error worth showing.
    NoneMatched,
    /// More than one window matched, so there is no answer, only a guess.
    Ambiguous(usize),
    /// The pass asking is not on the thread that owns the root window.
    WrongThread,
}

impl PickErr {
    /// What a reader is told. `NoneMatched` returns `None` because it is the ordinary state on the
    /// frame before the window exists, and a sentence that appears for one frame every time you
    /// open a window is noise, not news.
    pub fn words(self) -> Option<String> {
        match self {
            PickErr::NoneMatched => None,
            PickErr::Ambiguous(n) => Some(format!(
                "the player cannot move to the pop-out: {n} windows answer to that name, so there \
                 is no way to tell which one is it"
            )),
            PickErr::WrongThread => Some(
                "the player cannot move to the pop-out: that window is not on the thread that owns \
                 the main window"
                    .to_owned(),
            ),
        }
    }
}

/// The one window that is the pop-out, or a refusal.
///
/// FOUR TESTS, ALL OF WHICH MUST HOLD, AND NO TIE-BREAK.
///   class    it is a winit window and not an IME, a message-only window, or wry's own container.
///   not root the root window is a winit window with a title too, and it is the one window a wrong
///            answer would be catastrophic for: the surface is ALREADY parented there.
///   title    the app's window titles are compile-time constants and all distinct.
///   rect     the clincher, and the reason a title collision cannot produce a wrong answer.
///
/// IT REFUSES RATHER THAN BREAKING A TIE, and that is a deliberate choice against the obvious
/// alternative of taking the nearest rectangle. `SetParent` into the WRONG window SUCCEEDS. There
/// is no error to catch, no exception, no failed call: the video simply moves somewhere nobody can
/// see and every readback still says everything is fine. A refusal leaves the video exactly where
/// it is and says why. A guess has a failure mode with no symptom except an owner asking where his
/// stream went.
pub fn pick(
    cands: &[Candidate],
    title: &str,
    want_px: (i32, i32, i32, i32),
    root: Option<isize>,
) -> Result<isize, PickErr> {
    let near = |a: i32, b: i32| (a - b).abs() <= RECT_SLOP;
    let hits: Vec<&Candidate> = cands
        .iter()
        .filter(|c| c.class == WINIT_CLASS)
        .filter(|c| Some(c.hwnd) != root)
        .filter(|c| c.title == title)
        .filter(|c| {
            near(c.rect.0, want_px.0)
                && near(c.rect.1, want_px.1)
                && near(c.rect.2, want_px.2)
                && near(c.rect.3, want_px.3)
        })
        .collect();
    match hits.len() {
        1 => Ok(hits[0].hwnd),
        0 => Err(PickErr::NoneMatched),
        n => Err(PickErr::Ambiguous(n)),
    }
}

/// An egui rectangle in points, as physical screen pixels, for comparison with `GetWindowRect`.
///
/// `pixels_per_point` AND NOT `native_pixels_per_point`. `egui-winit` divides `GetWindowRect` by
/// `pixels_per_point` when it fills `ViewportInfo::outer_rect`, and that value is
/// `zoom_factor * scale_factor`. Multiplying by the raw monitor scale alone happens to agree today
/// only because nothing sets a zoom factor; it would come apart silently the day anything does.
pub fn to_physical(rect: egui::Rect, pixels_per_point: f32) -> (i32, i32, i32, i32) {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    let at = |v: f32| -> i32 {
        if v.is_finite() {
            (v * ppp).round() as i32
        } else {
            0
        }
    };
    (
        at(rect.min.x),
        at(rect.min.y),
        at(rect.max.x),
        at(rect.max.y),
    )
}

#[cfg(windows)]
mod win {
    use super::Candidate;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        ClientToScreen, CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, RGN_DIFF,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetAncestor, GetClassNameW, GetClientRect, GetParent, GetWindowRect,
        GetWindowTextW, IsWindow, WindowFromPoint, GA_ROOT,
    };

    unsafe extern "system" fn collect(h: HWND, l: LPARAM) -> BOOL {
        unsafe {
            let out = &mut *(l.0 as *mut Vec<Candidate>);
            let mut cls = [0u16; 256];
            let n = GetClassNameW(h, &mut cls).max(0) as usize;
            let mut txt = [0u16; 512];
            let m = GetWindowTextW(h, &mut txt).max(0) as usize;
            let mut r = RECT::default();
            let _ = GetWindowRect(h, &mut r);
            out.push(Candidate {
                hwnd: h.0 as isize,
                class: String::from_utf16_lossy(&cls[..n.min(cls.len())]),
                title: String::from_utf16_lossy(&txt[..m.min(txt.len())]),
                rect: (r.left, r.top, r.right, r.bottom),
            });
            BOOL(1)
        }
    }

    /// Every non-child window belonging to the CALLING thread.
    ///
    /// The thread and not the process, which is what keeps the answer small and safe: eframe makes
    /// every viewport window through winit's `ActiveEventLoop`, which is `!Send`, so they are all
    /// on the ui thread. wry's own container is `WS_CHILD` with a null title and never appears
    /// here at all, so it cannot be mistaken for a window to move INTO.
    pub fn candidates() -> Vec<Candidate> {
        let mut out: Vec<Candidate> = Vec::new();
        unsafe {
            let _ = EnumThreadWindows(
                GetCurrentThreadId(),
                Some(collect),
                LPARAM(&mut out as *mut _ as isize),
            );
        }
        out
    }

    pub fn thread_id() -> u32 {
        unsafe { GetCurrentThreadId() }
    }

    pub fn alive(hwnd: isize) -> bool {
        unsafe { IsWindow(Some(HWND(hwnd as _))).as_bool() }
    }

    /// The window this one is a child of. Read back after a build so the surface knows where home
    /// is without the root's handle having to be threaded through every call.
    pub fn parent_of(hwnd: isize) -> Option<isize> {
        unsafe {
            GetParent(HWND(hwnd as _))
                .ok()
                .map(|h| h.0 as isize)
                .filter(|h| *h != 0)
        }
    }

    pub fn client_size(hwnd: isize) -> Option<(u32, u32)> {
        let mut r = RECT::default();
        unsafe {
            GetClientRect(HWND(hwnd as _), &mut r).ok()?;
        }
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        (w > 0 && h > 0).then_some((w as u32, h as u32))
    }

    pub fn client_origin(hwnd: isize) -> Option<(i32, i32)> {
        let mut p = POINT { x: 0, y: 0 };
        unsafe {
            if !ClientToScreen(HWND(hwnd as _), &mut p).as_bool() {
                return None;
            }
        }
        Some((p.x, p.y))
    }

    /// Is the pointer over this window, counting a carved hole as "over it"?
    ///
    /// THE HOVER SIGNAL CANNOT COME FROM EGUI WHILE THE VIDEO IS THERE. A native child window takes
    /// the pointer as well as the pixels, so `Ui::rect_contains_pointer` answers false over the
    /// whole video and the hover-gated chrome would never appear. `WindowFromPoint` plus
    /// `GetAncestor(GA_ROOT)` answers the real question in one call, in physical pixels, with no
    /// monitor arithmetic: which top level window owns the pixel under the cursor. It gives the
    /// same answer over the video and over a hole, which is what makes the chrome reachable.
    pub fn pointer_over(hwnd: isize) -> bool {
        let mut p = POINT { x: 0, y: 0 };
        unsafe {
            if windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p).is_err() {
                return false;
            }
        }
        root_at(p.x, p.y) == Some(hwnd)
    }

    /// The top level window owning the pixel at this screen point.
    pub fn root_at(x: i32, y: i32) -> Option<isize> {
        unsafe {
            let deep = WindowFromPoint(POINT { x, y });
            if deep.0.is_null() {
                return None;
            }
            let root = GetAncestor(deep, GA_ROOT);
            (!root.0.is_null()).then_some(root.0 as isize)
        }
    }

    /// Withdraw `container` from `holes`, in its own client pixels.
    ///
    /// THIS IS THE OPPOSITE OF DRAWING OVER THE VIDEO, and the distinction is the whole reason the
    /// chrome is allowed to exist at all. Nothing is painted on top of anything: the video's own
    /// window is made ABSENT from four small shapes, and in those shapes the parent's pixels and
    /// the parent's hit testing are what is there. It is the same posture
    /// `player::occluded_by_overlays` already takes when it makes the surface yield its whole
    /// region for a frame, applied to a few hundred pixels instead of all of them.
    ///
    /// An empty `holes` restores the whole rectangle, which is how the notch is put away when the
    /// pointer leaves.
    pub fn carve(
        container: isize,
        client: (u32, u32),
        holes: &[(i32, i32, i32, i32)],
    ) -> Result<(), String> {
        let (w, h) = (client.0 as i32, client.1 as i32);
        unsafe {
            let whole = CreateRectRgn(0, 0, w, h);
            if whole.is_invalid() {
                return Err("could not build the video's window region".to_owned());
            }
            for (l, t, r, b) in holes {
                let hole = CreateRectRgn(*l, *t, *r, *b);
                if !hole.is_invalid() {
                    let _ = CombineRgn(Some(whole), Some(whole), Some(hole), RGN_DIFF);
                    let _ = DeleteObject(hole.into());
                }
            }
            /* On success the region becomes the window's own property and must NOT be deleted
             * here; on failure it is still ours and leaks if it is not. */
            if SetWindowRgn(HWND(container as _), Some(whole), true) == 0 {
                let _ = DeleteObject(whole.into());
                return Err("the video refused to make room for the controls".to_owned());
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use win::{
    alive, candidates, carve, client_origin, client_size, parent_of, pointer_over, root_at,
    thread_id,
};

/* EVERY CALL ABOVE HAS A TWIN THAT ANSWERS HONESTLY OFF WINDOWS, because this module is compiled
 * everywhere so that `pick` can be tested everywhere. `surface_absent` is what those platforms
 * actually run, so nothing below is ever reached in anger; they exist so the pure half does not
 * have to be hidden behind a cfg to keep the crate building. */
#[cfg(not(windows))]
mod other {
    use super::Candidate;
    pub fn candidates() -> Vec<Candidate> {
        Vec::new()
    }
    pub fn thread_id() -> u32 {
        0
    }
    pub fn alive(_hwnd: isize) -> bool {
        false
    }
    pub fn parent_of(_hwnd: isize) -> Option<isize> {
        None
    }
    pub fn client_size(_hwnd: isize) -> Option<(u32, u32)> {
        None
    }
    pub fn client_origin(_hwnd: isize) -> Option<(i32, i32)> {
        None
    }
    pub fn pointer_over(_hwnd: isize) -> bool {
        false
    }
    pub fn root_at(_x: i32, _y: i32) -> Option<isize> {
        None
    }
    pub fn carve(
        _container: isize,
        _client: (u32, u32),
        _holes: &[(i32, i32, i32, i32)],
    ) -> Result<(), String> {
        Err("there are no window regions on this platform".to_owned())
    }
}

#[cfg(not(windows))]
pub use other::{
    alive, candidates, carve, client_origin, client_size, parent_of, pointer_over, root_at,
    thread_id,
};

/* ---------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    fn c(hwnd: isize, class: &str, title: &str, rect: (i32, i32, i32, i32)) -> Candidate {
        Candidate {
            hwnd,
            class: class.to_owned(),
            title: title.to_owned(),
            rect,
        }
    }

    /// The real enumeration, copied from a probe run and from the running app, so this test is a
    /// recording of Windows rather than a picture of what I expected Windows to say.
    fn real_thread() -> Vec<Candidate> {
        vec![
            c(
                0x56710B0,
                WINIT_CLASS,
                "Broken Stoic",
                (220, 220, 1060, 693),
            ),
            c(
                0x1762E9C,
                WINIT_CLASS,
                "EQL Grimoire",
                (100, 100, 1209, 899),
            ),
            c(0x54214A6, "Winit Thread Event Target", "", (0, 0, 24, 24)),
            c(0x1881550, "global_hotkey_app", "", (0, 0, 0, 0)),
            c(0x9610BE, "MSCTFIME UI", "MSCTFIME UI", (0, 0, 0, 0)),
            c(0xBE151C, "IME", "Default IME", (0, 0, 0, 0)),
        ]
    }

    const PIP_PX: (i32, i32, i32, i32) = (220, 220, 1060, 693);
    const ROOT: Option<isize> = Some(0x1762E9C);

    #[test]
    fn the_pop_out_is_found_in_a_real_thread_enumeration() {
        assert_eq!(
            pick(&real_thread(), "Broken Stoic", PIP_PX, ROOT),
            Ok(0x56710B0)
        );
    }

    /// EVERY ONE OF THE FOUR TESTS IS LOAD BEARING, said one at a time.
    ///
    /// A rule that never rejects anything is not a rule, so each is broken on its own against a
    /// list where everything else still matches. Without this, three of the four could be deleted
    /// and the test above would stay green.
    #[test]
    fn each_of_the_four_rules_can_refuse_on_its_own() {
        let title = "Broken Stoic";

        let mut wrong_class = real_thread();
        wrong_class[0].class = "Chrome_WidgetWin_1".to_owned();
        assert_eq!(
            pick(&wrong_class, title, PIP_PX, ROOT),
            Err(PickErr::NoneMatched),
            "a window that is not a winit window was accepted"
        );

        let mut wrong_title = real_thread();
        wrong_title[0].title = "Broken Stoic ".to_owned();
        assert_eq!(
            pick(&wrong_title, title, PIP_PX, ROOT),
            Err(PickErr::NoneMatched),
            "the title match is not exact"
        );

        let moved = {
            let mut v = real_thread();
            v[0].rect = (220, 220, 1060, 700);
            v
        };
        assert_eq!(
            pick(&moved, title, PIP_PX, ROOT),
            Err(PickErr::NoneMatched),
            "a window 7 pixels from the one the viewport describes was accepted"
        );

        /* And the root exclusion, which needs a list where the root would otherwise match: the
         * catastrophic case, because the surface is already parented there. */
        let root_named_the_same = vec![
            c(0x1762E9C, WINIT_CLASS, title, PIP_PX),
            c(0x54214A6, "Winit Thread Event Target", "", (0, 0, 24, 24)),
        ];
        assert_eq!(
            pick(&root_named_the_same, title, PIP_PX, ROOT),
            Err(PickErr::NoneMatched),
            "the ROOT window was offered as a place to move the surface to"
        );
    }

    /// A TIE IS A REFUSAL AND NEVER A CHOICE.
    ///
    /// `SetParent` into the wrong window SUCCEEDS: no error, no exception, every readback still
    /// healthy, and the video is somewhere nobody can see. That is the one failure in this feature
    /// with no symptom, so the moment there are two answers there is no answer.
    #[test]
    fn two_matches_are_refused_rather_than_guessed_between() {
        let twins = vec![
            c(0x111, WINIT_CLASS, "Broken Stoic", PIP_PX),
            c(0x222, WINIT_CLASS, "Broken Stoic", PIP_PX),
            c(
                0x1762E9C,
                WINIT_CLASS,
                "EQL Grimoire",
                (100, 100, 1209, 899),
            ),
        ];
        assert_eq!(
            pick(&twins, "Broken Stoic", PIP_PX, ROOT),
            Err(PickErr::Ambiguous(2))
        );
        assert!(
            PickErr::Ambiguous(2).words().is_some(),
            "an ambiguous refusal says nothing to the reader"
        );
    }

    /// NOTHING MATCHING IS SILENT, AND THE OTHER TWO ARE NOT.
    ///
    /// The frame before the OS window exists is an ordinary state that happens every single time
    /// the pop-out is opened. A sentence there would appear, once, on every open, and a message
    /// that cries wolf on a healthy path is worse than no message.
    #[test]
    fn only_the_refusals_a_reader_can_act_on_carry_words() {
        assert_eq!(PickErr::NoneMatched.words(), None);
        assert!(PickErr::Ambiguous(3).words().is_some());
        assert!(PickErr::WrongThread.words().is_some());
    }

    /// THE SLOP EXISTS FOR 125% AND 150% SCALING AND IT IS MEASURED, NOT PICKED.
    ///
    /// `outer_rect` reaches us as `GetWindowRect / pixels_per_point` in `f32`, and this multiplies
    /// it back. At 1.25 and 1.5 that round trip does not always land on the integer it began at,
    /// so exact equality would reject the right window on the commonest laptop settings. This
    /// drives the real arithmetic over a spread of positions rather than asserting the constant.
    #[test]
    fn the_rectangle_survives_the_scale_round_trip_at_every_common_dpi() {
        for ppp in [1.0_f32, 1.25, 1.5, 1.75, 2.0] {
            for (x, y, w, h) in [(0, 0, 480, 270), (37, 113, 861, 499), (1913, 7, 320, 200)] {
                let physical = (x, y, x + w, y + h);
                /* What egui-winit would have handed the viewport, and what a pass multiplies back. */
                let logical = egui::Rect::from_min_max(
                    egui::pos2(x as f32 / ppp, y as f32 / ppp),
                    egui::pos2((x + w) as f32 / ppp, (y + h) as f32 / ppp),
                );
                let back = to_physical(logical, ppp);
                for (a, b, which) in [
                    (back.0, physical.0, "left"),
                    (back.1, physical.1, "top"),
                    (back.2, physical.2, "right"),
                    (back.3, physical.3, "bottom"),
                ] {
                    assert!(
                        (a - b).abs() <= RECT_SLOP,
                        "at {ppp}x the {which} edge came back {a} from {b}, which is further than \
                         RECT_SLOP={RECT_SLOP} and would reject the real window"
                    );
                }
                let cands = vec![c(0x999, WINIT_CLASS, "Broken Stoic", physical)];
                assert_eq!(
                    pick(&cands, "Broken Stoic", back, None),
                    Ok(0x999),
                    "the pop-out is unfindable at {ppp}x"
                );
            }
        }
    }

    /// A rectangle with no sensible scale must not panic or produce nonsense; the same defence
    /// `physical_bounds` already carries, for the same reason.
    #[test]
    fn a_broken_scale_or_rect_is_absorbed_rather_than_propagated() {
        let r = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(30.0, 40.0));
        assert_eq!(
            to_physical(r, 0.0),
            (10, 20, 30, 40),
            "zero falls back to 1x"
        );
        assert_eq!(to_physical(r, f32::NAN), (10, 20, 30, 40));
        assert_eq!(
            to_physical(egui::Rect::NOTHING, 1.0),
            (0, 0, 0, 0),
            "an infinite rect became a number instead of a panic"
        );
    }
}
