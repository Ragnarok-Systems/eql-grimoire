//! Keeping a window above the others, and doing it exactly once.
//!
//! WHY THIS IS A MODULE AND NOT FOUR COPIES OF THREE LINES, which is what it was. Always on top was
//! never built as a thing; it was an IDIOM, written out wherever it was needed:
//!
//! ```text
//! if applied != Some(want) {
//!     ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level(want)));
//!     applied = Some(want);
//! }
//! ```
//!
//! That triple stood in `windows.rs` four times: once reconciling a tool window on its first draw,
//! twice more, byte identical, in the two places a pin glyph is clicked, and a fourth time for the
//! main window with different field names and a different send. Nothing was WRONG with any of them.
//! The cost is what a fifth window would have to do, which is find one of the four and copy it, and
//! what already happened while there were four: the main window's pin persists to settings and a
//! tool window's does not, which is not a decision anybody took, it is what falls out of two
//! implementations of one idea.
//!
//! WHAT IS SEPARATED FROM WHAT. This module owns the MECHANISM: what the OS has been told, and not
//! telling it twice. It owns no policy at all. Whether a window starts pinned, whether the answer
//! is written to disk, whether a click toggles it, and which window is even pinnable are all the
//! caller's, because they differ per window and always will. That split is the whole point: a new
//! always on top window brings its own policy and reuses every line of this.
//!
//! IT SENDS ON CHANGE AND NOT EVERY PASS, which is the one behaviour worth having a type for. These
//! windows repaint on a timer; a `WindowLevel` command every frame would be a syscall per frame per
//! window forever, for an answer that changes when a person clicks something.

use egui::{ViewportCommand, ViewportId, WindowLevel};

/// The window level for an answer. The whole of the mapping, in one place, so "pinned" cannot mean
/// `AlwaysOnTop` in one file and something else in another.
pub fn level(on_top: bool) -> WindowLevel {
    if on_top {
        WindowLevel::AlwaysOnTop
    } else {
        WindowLevel::Normal
    }
}

/// One window's "keep me on top" state: what is wanted, and what the OS was last told.
///
/// THE TWO FIELDS ARE NOT THE SAME QUESTION and keeping them apart is what this type is for.
/// `want` is the app's answer and changes the instant somebody clicks. `applied` is what the OS
/// has actually been told, which is `None` until the window has drawn once and can lag `want` by a
/// pass. Code that conflated them either re-sent the command forever or sent it never.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pin {
    want: bool,
    applied: Option<bool>,
}

impl Pin {
    /// A pin that wants `on_top` and has told the OS nothing yet.
    ///
    /// `applied` STARTS AT `None` EVEN WHEN `want` IS FALSE, and that is not the same as
    /// `Some(false)`. A viewport builder may or may not have carried the level already, so the
    /// first `reconcile` has to send whatever the answer is rather than assume the default took.
    pub const fn new(on_top: bool) -> Pin {
        Pin {
            want: on_top,
            applied: None,
        }
    }

    /// What the app wants. This is what a glyph is drawn from: it changes on the click rather than
    /// a pass later, so the mark and the press agree.
    pub fn wants(self) -> bool {
        self.want
    }

    /// What the OS was last told, or `None` if it has never been told anything.
    ///
    /// Callers that want "is it REALLY on top" read this. It is a different and weaker claim than
    /// [`wants`](Self::wants), which is why they are two methods and not one.
    pub fn applied(self) -> Option<bool> {
        self.applied
    }

    pub fn set(&mut self, on_top: bool) {
        self.want = on_top;
    }

    pub fn toggle(&mut self) {
        self.want = !self.want;
    }

    /// The window is gone, so nothing the OS was told about it holds any more.
    ///
    /// WITHOUT THIS A REOPENED WINDOW COMES BACK UNPINNED AND CLAIMS IT IS PINNED. A viewport that
    /// closes and is shown again is a NEW OS window at the default level, while `applied` still
    /// says the old one was told; the reconcile then sees no change and sends nothing.
    pub fn forget(&mut self) {
        self.applied = None;
    }

    /// Tell the OS, if it does not already have this answer. Returns whether a command was sent.
    ///
    /// `to` NAMES THE VIEWPORT OR IS `None` FOR THE ONE THIS PASS BELONGS TO. Both forms exist in
    /// this app: a tool window reconciles itself from inside its own deferred callback, where the
    /// context IS that window's; the main window is reconciled from the root pass, which has to
    /// name it. Taking an `Option` rather than making the caller pick between two methods is what
    /// keeps the "have I already sent this" bookkeeping in one place for both.
    pub fn reconcile(&mut self, ctx: &egui::Context, to: Option<ViewportId>) -> bool {
        if self.applied == Some(self.want) {
            return false;
        }
        let cmd = ViewportCommand::WindowLevel(level(self.want));
        match to {
            Some(id) => ctx.send_viewport_cmd_to(id, cmd),
            None => ctx.send_viewport_cmd(cmd),
        }
        self.applied = Some(self.want);
        true
    }
}

/* ---------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> egui::Context {
        egui::Context::default()
    }

    /// What a level MEANS, in the two words the OS understands. One line, and it is the line that
    /// would silently invert the whole feature.
    #[test]
    fn on_top_is_always_on_top_and_off_is_normal() {
        assert_eq!(level(true), WindowLevel::AlwaysOnTop);
        assert_eq!(level(false), WindowLevel::Normal);
    }

    /// THE COMMAND GOES ONCE PER CHANGE AND NOT ONCE PER PASS.
    ///
    /// This is the entire reason the type exists rather than a bare bool. These windows repaint on
    /// a timer, so a version that sent every pass would be a syscall per frame per window forever,
    /// for an answer that changes when somebody clicks something.
    ///
    /// IT IS DRIVEN THROUGH A WHOLE LIFE rather than asserted once: first send, repeats, a change,
    /// repeats again, and a change back. A test that only checked "the second call is quiet" would
    /// pass on a pin that had stopped sending anything at all after the first time.
    #[test]
    fn the_level_is_sent_on_change_and_never_twice_for_one_answer() {
        let ctx = ctx();
        let mut p = Pin::new(false);
        assert_eq!(p.applied(), None, "nothing has been told to the OS yet");

        assert!(p.reconcile(&ctx, None), "the first answer is always sent");
        assert_eq!(p.applied(), Some(false));
        assert!(!p.reconcile(&ctx, None), "the same answer was sent twice");
        assert!(!p.reconcile(&ctx, None));

        p.toggle();
        assert!(
            p.wants(),
            "the toggle is visible immediately, before any send"
        );
        assert_eq!(
            p.applied(),
            Some(false),
            "the toggle told the OS something on its own"
        );
        assert!(p.reconcile(&ctx, None), "a changed answer is sent");
        assert_eq!(p.applied(), Some(true));
        assert!(!p.reconcile(&ctx, None), "and then it is quiet again");

        p.set(false);
        assert!(p.reconcile(&ctx, None), "and it sends the way back too");
        assert!(!p.reconcile(&ctx, None));
    }

    /// `set` TO THE VALUE IT ALREADY HAS IS NOT A CHANGE. A caller that writes a preference every
    /// pass, which is what reading one out of settings looks like, must not make this chatty.
    #[test]
    fn setting_the_answer_it_already_has_sends_nothing() {
        let ctx = ctx();
        let mut p = Pin::new(true);
        assert!(p.reconcile(&ctx, None));
        for _ in 0..5 {
            p.set(true);
            assert!(!p.reconcile(&ctx, None), "a repeated set became a command");
        }
    }

    /// A CLOSED WINDOW HAS TO BE FORGOTTEN, and this is the bug that would otherwise be waiting.
    ///
    /// A viewport that closes and is shown again is a NEW OS window at the default level. The pin
    /// still remembers telling the old one, so without `forget` the reconcile sees no change, sends
    /// nothing, and the reopened window sits at Normal while its glyph says it is pinned. The
    /// caller cannot spot that; only the thing tracking `applied` can.
    #[test]
    fn a_forgotten_pin_tells_the_os_again_even_though_nothing_changed() {
        let ctx = ctx();
        let mut p = Pin::new(true);
        assert!(p.reconcile(&ctx, None));
        assert!(!p.reconcile(&ctx, None));

        p.forget();
        assert_eq!(p.applied(), None);
        assert!(
            p.wants(),
            "forgetting what the OS was told must not change what the app wants"
        );
        assert!(
            p.reconcile(&ctx, None),
            "a reopened window was never told it is pinned"
        );
    }

    /// A NAMED VIEWPORT AND THE CURRENT ONE KEEP THE SAME BOOKKEEPING.
    ///
    /// The two send forms are the only difference between how the main window and a tool window are
    /// reconciled, and it would be easy to give one of them its own "have I sent this" path. Then
    /// one of the two would be chatty or silent and nothing would say which.
    #[test]
    fn both_send_forms_are_sent_once_and_remembered_the_same_way() {
        let ctx = ctx();
        for to in [None, Some(ViewportId::ROOT)] {
            let mut p = Pin::new(true);
            assert!(p.reconcile(&ctx, to), "{to:?}: the first answer is sent");
            assert!(!p.reconcile(&ctx, to), "{to:?}: and not sent twice");
            assert_eq!(p.applied(), Some(true));
        }
    }

    /// A DEFAULT PIN WANTS NOTHING AND HAS TOLD NOTHING, so a window that never mentions pinning
    /// gets the ordinary level and one command, rather than being born claiming it already did.
    #[test]
    fn the_default_is_unpinned_and_untold() {
        let p = Pin::default();
        assert!(!p.wants());
        assert_eq!(p.applied(), None);
        assert_eq!(p, Pin::new(false));
    }
}
