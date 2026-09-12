//! The Windows half of the player: the WebView2 child window itself. Everything decided here is
//! decided by measurement; the parent module's header records what was measured and what it
//! corrected.
//!
//! WHY THE WHOLE FILE IS `cfg(windows)`. WebView2 is Windows only, and `wry` on Linux links
//! webkit2gtk through pkg-config at BUILD time, which this repository's CI (ubuntu-latest, bare)
//! does not have. A dependency that cannot build on the machine that gates the tree is a red CI on
//! every push, so `wry` is declared under `[target.'cfg(windows)'.dependencies]` and this module
//! is compiled to match. `super::surface_absent` is what the other platforms get, and it says so
//! out loud rather than pretending to host anything.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Profile3, ICoreWebView2_13, ICoreWebView2_8,
    COREWEBVIEW2_TRACKING_PREVENTION_LEVEL, COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE,
};
use windows_core::Interface;
use wry::http::{Request, Response};
use wry::{WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};

use super::hwnd;
use super::{Feed, Problem, Seat, SeatId, Sound, Stage};

/// How often the browser is asked whether the document is emitting audio. The answer only changes
/// when a stream starts, stops or is unmuted, and the call crosses a COM boundary, so it is not
/// worth doing on every frame of a UI that repaints on a one second tick anyway.
const AUDIO_POLL_EVERY: Duration = Duration::from_millis(500);

/// The player surface, and the one [`wry::WebContext`] this process ever has.
///
/// THE CONTEXT IS CREATED AT APP START AND THE WEBVIEW IS NOT. The profile folder is read inside
/// `build_as_child` and cannot be changed afterwards, so the context has to exist before any
/// webview does; but most launches never open the Watch screen, so the webview itself is built on
/// first need and dropped when the surface is stopped.
pub struct Player {
    /// Held for the process lifetime. See the type note.
    web_context: wry::WebContext,
    /// The profile folder, kept so a failure can name it.
    profile: Option<PathBuf>,
    webview: Option<wry::WebView>,
    /// What the live webview was built for. A change here is a teardown and a rebuild, which is
    /// also how a platform switch is served.
    built_for: Option<(Feed, bool)>,
    /// The last bounds pushed, in physical pixels. `set_bounds` is skipped when the rect has not
    /// moved, which is every frame the window is still.
    last_bounds: Option<(i32, i32, u32, u32)>,
    last_visible: Option<bool>,
    /// Why there is no surface, in words that name the folder or the call that failed, and whether
    /// asking again could change it. See [`Problem`]: the two lifetimes used to share one
    /// `Option<String>`, and a single failed `set_bounds` killed the player until the app was
    /// restarted.
    problem: Option<Problem>,
    sound: Sound,
    /// The tracking prevention level read back AFTER setting it, never the value asked for.
    /// `None` until a webview has been built.
    tracking_prevention: Option<i32>,
    last_audio_poll: Option<Instant>,

    /// WHICH WINDOW THE SURFACE IS PARENTED INTO RIGHT NOW. `Body` until a move says otherwise.
    ///
    /// IT IS NOT PART OF `built_for`, AND THAT ABSENCE IS THE WHOLE VOLUME GUARANTEE. A change to
    /// `built_for` tears the webview down and builds a new one, which navigates a fresh page and
    /// re-applies `muted=` from the URL. If the seat lived there, moving the video to the pop-out
    /// would restart the stream at the default volume, which is the exact thing the owner asked
    /// not to happen. A move is a `SetParent` and never a rebuild.
    seated: SeatId,
    /// The main window's handle, read back off the webview after a successful build.
    ///
    /// READ BACK RATHER THAN PASSED IN, because `GetParent` of the container is the truth about
    /// where the surface actually came from, and the alternative is threading a handle through
    /// every call site and trusting it. This is what home is when the pop-out closes.
    root_hwnd: Option<isize>,
    /// The last window region pushed: the client size it was computed for, and the shapes cut out
    /// of it. Skipped when neither changed, which is most frames.
    last_region: Option<((u32, u32), hwnd::Holes)>,
    /// Whether a carved hole actually passes the POINTER through, probed once, the first time a
    /// region with holes in it is pushed. See [`Self::probe_notch`].
    notch_works: Option<bool>,
}

impl Player {
    /// Creates the one [`wry::WebContext`], against the per-user profile folder.
    ///
    /// The folder is created here rather than at build time so that a machine where it cannot be
    /// created says so once, at start, with the path in the message, instead of failing inside
    /// WebView2 with an error that names neither.
    pub fn new() -> Player {
        /* BOTH OF THESE ARE REFUSALS AND NEITHER IS RETRYABLE, which is not a judgement call. The
         * `WebContext` below is built from `profile` and its folder cannot be changed afterwards,
         * so a `None` here means this process's context already points at wry's default, beside
         * the binary. Offering a retry would offer a surface on the wrong profile. */
        let (profile, problem) = match super::profile_dir() {
            None => (
                None,
                Some(Problem::Refused("this system does not name a per-user local data folder, so there is nowhere to put the WebView2 profile".to_owned())),
            ),
            Some(dir) => match std::fs::create_dir_all(&dir) {
                Ok(()) => (Some(dir), None),
                Err(e) => (
                    None,
                    Some(Problem::Refused(format!(
                        "could not create {}: {e}",
                        dir.display()
                    ))),
                ),
            },
        };
        Player {
            web_context: wry::WebContext::new(profile.clone()),
            profile,
            webview: None,
            built_for: None,
            last_bounds: None,
            last_visible: None,
            problem,
            sound: Sound::Off,
            tracking_prevention: None,
            last_audio_poll: None,
            seated: SeatId::Body,
            root_hwnd: None,
            last_region: None,
            notch_works: None,
        }
    }

    /// Everything a screen may know, in primitives. THE ONLY WAY OUT of this type: the surface is
    /// driven through [`Self::sync`] and read through here, so a screen can neither hold the
    /// webview nor invent a word about it.
    pub fn view(&self) -> super::PlayerView {
        super::PlayerView {
            problem: self.problem.clone(),
            /* THE ONE FACT THE STOP BUTTON IS DECIDED FROM. Not "should there be a surface" but
             * "is there one": the two part company the moment the channel goes offline. */
            playing: self.webview.is_some(),
            sound: self.sound.clone(),
            profile: self.profile.clone(),
            tracking_prevention: self.tracking_prevention,
            hosted_elsewhere: self.seated != SeatId::Body,
        }
    }

    /// Tear the surface down. The child window goes with it, and so does playback: there is no
    /// hidden webview left streaming somebody's bandwidth after they asked it to stop.
    ///
    /// A FAILURE GOES WITH IT TOO. Whatever the last attempt could not do, it could not do it to a
    /// surface that no longer exists, so the sentence would outlive the thing it was about. A
    /// refusal stays, because that one is about the machine and not about any surface.
    pub fn stop(&mut self) {
        self.webview = None;
        self.built_for = None;
        self.last_bounds = None;
        self.last_visible = None;
        self.sound = Sound::Off;
        self.last_audio_poll = None;
        self.problem = super::after_retry(self.problem.take());
    }

    /// The reader has asked for the player again, so the last failure gets another go.
    ///
    /// AUTOMATIC OR USER DRIVEN, AND IT IS BOTH, SPLIT BY WHICH CALL FAILED. A placement retries
    /// itself: the surface is still alive, `super::keeps_syncing` lets the next frame through and
    /// `super::placement_problem` clears the sentence as soon as a push works, so a hiccup while
    /// the window is being dragged heals before anybody reads it. A BUILD does not retry itself,
    /// because the ordinary reason `build_as_child` fails is a machine with no WebView2 runtime,
    /// and a frame-rate loop against that spends the reader's CPU to learn nothing. That one waits
    /// for this, which `App::answer` calls on `Ask::WatchHere`: the Watch here button and every
    /// pill in the app raise it, so the door back in is the same control that opened it.
    pub fn retry(&mut self) {
        self.problem = super::after_retry(self.problem.take());
    }

    /// One frame of the surface.
    ///
    /// `stage` is `Some` only on a frame the ROOT viewport drew the Watch screen with a rectangle
    /// reserved for the video. `None` HIDES the surface rather than dropping it, because leaving
    /// the Watch screen for a moment should not restart the stream; [`Self::stop`] is the door that
    /// drops it, and the Watch screen offers it.
    ///
    /// `host` must be the ROOT window. eframe's `Frame` carries the root handle even when it is
    /// handed to a deferred viewport's callback, which is why only the root pass ever calls this:
    /// a surface built from a pop-out window's pass would appear over the main window's body.
    pub fn sync(&mut self, host: &eframe::Frame, stage: Option<&Stage>) {
        /* A SURFACE MAY NEVER BE LEFT PARENTED INTO A WINDOW THAT IS GOING AWAY, and the
         * commonest way that happens has no error in it at all: pop out a live channel, walk
         * the main window to the Parser so nothing stages, then close the pop-out. Every branch
         * below would be skipped, so this runs before all of them. */
        self.check_seat_alive();
        let Some(stage) = stage else {
            /* HOME FIRST, THEN HIDE. Hiding a webview parented into a destroyed window is a
             * call into a handle that no longer exists; and the moment anything stages again
             * the surface has to be somewhere the root can place it. */
            let done = self.reseat(SeatId::Body).and_then(|()| self.show(false));
            self.note(done);
            return;
        };
        if !super::keeps_syncing(self.problem.as_ref(), self.webview.is_some()) {
            /* A refusal, or a build that failed and left nothing to place. Neither is retried on a
             * timer; see `super::keeps_syncing` and `Self::retry`. */
            return;
        }

        let want = (stage.feed.clone(), stage.sound);
        if self.built_for.as_ref() != Some(&want) {
            /* A different feed or a different sound flag is a different page. Rebuilding rather
             * than navigating is what makes the platform switch and the sound switch one mechanism
             * instead of two, and it is the teardown path exercised on every use. */
            self.webview = None;
            self.last_bounds = None;
            self.last_visible = None;
            self.last_audio_poll = None;
            self.sound = if stage.sound {
                Sound::AskedButSilent
            } else {
                Sound::Off
            };
            /* WHERE THE FIRST BUILD GOES. `build_as_child` takes the ROOT handle and there is no
             * other kind, so a surface asked for while the pop-out holds the seat is born in the
             * main window and moved a few lines below, in this same frame. It is sized to its
             * destination rather than to the folio so the move is not also a resize. */
            self.seated = SeatId::Body;
            let (born_rect, born_ppp) = match &stage.seat {
                Seat::Body {
                    rect,
                    pixels_per_point,
                    ..
                } => (*rect, *pixels_per_point),
                Seat::PopOut { hwnd, .. } => {
                    let (w, h) = hwnd::client_size(*hwnd).unwrap_or((640, 360));
                    (
                        egui::Rect::from_min_size(
                            egui::pos2(0.0, 0.0),
                            egui::vec2(w as f32, h as f32),
                        ),
                        1.0,
                    )
                }
            };
            match self.build(host, &want.0, want.1, born_rect, born_ppp) {
                Ok(wv) => {
                    /* The one line a support question needs, and every value in it is a readback:
                     * the bounds actually pushed, the profile folder actually used, and the
                     * tracking prevention level read back out of the profile after being set. */
                    log::info!(
                        "player: surface up for {:?}, bounds {:?} physical px, profile {}, tracking prevention {:?}",
                        want.0,
                        self.last_bounds,
                        self.profile.as_ref().map_or_else(
                            || "(none)".to_owned(),
                            |p| p.display().to_string()
                        ),
                        self.tracking_prevention
                    );
                    /* Home, read off the surface itself rather than trusted from a parameter. */
                    self.root_hwnd = hwnd::parent_of(wv.hwnd().0 as isize);
                    self.webview = Some(wv);
                    self.built_for = Some(want);
                    self.problem = None;
                }
                Err(e) => {
                    log::warn!("player: no surface: {e}");
                    self.built_for = None;
                    self.problem = Some(Problem::Failed(e));
                    return;
                }
            }
        }

        /* THE MOVE, BEFORE THE PLACEMENT, because placing against the old parent would push a
         * rectangle in the wrong window's coordinate space and then memo it as done. */
        if let Err(e) = self.reseat(stage.seat_id()) {
            self.note(Err(e));
            return;
        }

        /* ONE OUTCOME FOR THE WHOLE FRAME'S PLACEMENT, ASSIGNED ONCE. Two independent assignments
         * would let a `set_visible` that worked erase a `set_bounds` that did not, in the same
         * frame, and the reader would be told nothing about a player sitting in the wrong place. */
        let placed = match &stage.seat {
            Seat::Body {
                rect,
                pixels_per_point,
                occluded,
            } => self
                /* The body never carves: nothing of the app's is meant to be inside the video
                 * there, and a region left over from the pop-out would punch two holes in the
                 * folio. */
                .carve(&[])
                .and_then(|()| self.place(*rect, *pixels_per_point))
                .and_then(|()| self.show(!*occluded)),
            Seat::PopOut { hwnd, carve_px } => self.place_pip(*hwnd, carve_px),
        };
        self.note(placed);
        self.poll_audio(stage.sound);
    }

    /// Records this frame's placement outcome. See [`super::placement_problem`] for why a call
    /// that worked clears the last one that did not, and why a no-op clears nothing.
    fn note(&mut self, outcome: Result<(), String>) {
        self.problem =
            super::placement_problem(outcome, self.webview.is_some(), self.problem.take());
    }

    /// Move the surface into `want`, or do nothing if it is already there.
    ///
    /// WHAT `reparent` ACTUALLY DOES HERE, because the answer decides what this function has to do
    /// afterwards. This surface was built with `build_as_child`, so `is_child` is true, so
    /// wry's `reparent` is exactly `SetParent(container, target)` and nothing else: the bounds
    /// reset, the parent subclass and the recorded parent all sit behind `if !self.is_child`
    /// (wry 0.56.1 webview2/mod.rs:1780-1799). Bounds are therefore ours, below.
    ///
    /// AND WHAT IT DOES NOT DO: it never touches an `ICoreWebView2` method, so the document is
    /// never navigated, reloaded or suspended. That is the whole reason the volume survives, and
    /// it was measured rather than assumed: same document nonce, volume held, audio graph still
    /// running, playback position advancing straight through the move, measured on this machine.
    fn reseat(&mut self, want: SeatId) -> Result<(), String> {
        if self.seated == want {
            return Ok(());
        }
        let target = match want {
            SeatId::PopOut(h) if !hwnd::alive(h) => {
                return Err(
                    "the pop-out window went away before the player could move into it".to_owned(),
                );
            }
            SeatId::PopOut(h) => h,
            /* Home. The readback is preferred and the handle eframe carries is the fallback, so
             * a build that somehow never recorded one still has a way back. */
            SeatId::Body => match self.root_hwnd {
                Some(h) if hwnd::alive(h) => h,
                _ => return Err("the main window's handle is not known".to_owned()),
            },
        };
        let Some(wv) = &self.webview else {
            /* Nothing to move. Recording the seat anyway is what makes the next build land in
             * the right place instead of moving on its second frame. */
            self.seated = want;
            return Ok(());
        };
        wv.reparent(target)
            .map_err(|e| format!("could not move the player: {e}"))?;
        /* Microsoft's own notification that an ancestor moved. wry never calls it on the child
         * path, so without this WebView2 computes popup, IME and drag coordinates against an
         * origin it was never told about. Side-effect free, so it is called rather than
         * argued about. */
        unsafe {
            let _ = wv.controller().NotifyParentWindowPositionChanged();
        }
        self.seated = want;
        /* THE LOAD-BEARING THREE LINES. `place`, `show` and `carve` all short-circuit when the
         * value has not changed. A pop-out whose client rect happens to compute the same physical
         * tuple as the last folio rect would skip the ONE push that repositions the child against
         * its new parent, and nothing anywhere would report a thing. */
        self.last_bounds = None;
        self.last_visible = None;
        self.last_region = None;
        Ok(())
    }

    /// The pop-out has gone: bring the surface home before anything else touches it.
    ///
    /// A closed window's handle is not merely stale, it is a handle into freed memory as far as
    /// every later call is concerned. This is checked every frame rather than hooked to the close,
    /// because the close can come from the OS, from Alt+F4, from the taskbar or from the chip, and
    /// only one of those runs any of this app's code.
    fn check_seat_alive(&mut self) {
        if let SeatId::PopOut(h) = self.seated {
            if !hwnd::alive(h) {
                /* The seat is already invalid, so the move must not be asked to leave it: say we
                 * are nowhere, then go home. */
                self.seated = SeatId::Body;
                self.last_bounds = None;
                self.last_visible = None;
                self.last_region = None;
                if let (Some(wv), Some(root)) = (&self.webview, self.root_hwnd) {
                    let done = wv
                        .reparent(root)
                        .map_err(|e| format!("could not bring the player home: {e}"));
                    self.note(done);
                }
            }
        }
    }

    /// Fill the pop-out, edge to edge, and withdraw from the chrome's own shapes.
    ///
    /// THE RECTANGLE IS READ FROM THE OS AND NOT CARRIED IN A `Stage`. The pop-out's client area is
    /// what the video fills, so asking `GetClientRect` at placement time is both simpler and more
    /// correct than computing a rect one pass earlier in another window's points at another
    /// window's scale. It also means a resize needs no plumbing at all.
    fn place_pip(&mut self, hwnd: isize, carve_px: &[(i32, i32, i32, i32)]) -> Result<(), String> {
        let Some((w, h)) = hwnd::client_size(hwnd) else {
            return Err("the pop-out window has no room for the player".to_owned());
        };
        let bounds = (0, 0, w, h);
        if self.last_bounds != Some(bounds) {
            let Some(wv) = &self.webview else {
                return Ok(());
            };
            match wv.set_bounds(wry::Rect {
                position: wry::dpi::PhysicalPosition::new(0, 0).into(),
                size: wry::dpi::PhysicalSize::new(w, h).into(),
            }) {
                Ok(()) => self.last_bounds = Some(bounds),
                Err(e) => {
                    self.last_bounds = None;
                    return Err(format!("could not place the player: {e}"));
                }
            }
        }
        /* The notch is refused wholesale once it is known not to work, rather than pushed and
         * hoped for; see `probe_notch`. */
        let want: &[(i32, i32, i32, i32)] = if self.notch_works == Some(false) {
            &[]
        } else {
            carve_px
        };
        self.carve(want)?;
        self.probe_notch(hwnd, want);
        self.show(true)
    }

    /// Push a window region, or take one away. Skipped when nothing changed.
    ///
    /// A FAILURE FORGETS THE REGION, for the reason spelled out on [`Self::place`].
    fn carve(&mut self, holes: &[(i32, i32, i32, i32)]) -> Result<(), String> {
        let client = match self.seated {
            SeatId::PopOut(h) => hwnd::client_size(h),
            SeatId::Body => None,
        };
        /* Off the pop-out there is nothing to carve and nothing to undo unless one was left. */
        let Some(client) = client else {
            if self.last_region.is_none() {
                return Ok(());
            }
            self.last_region = None;
            return Ok(());
        };
        let want = (client, holes.to_vec());
        if self.last_region.as_ref() == Some(&want) {
            return Ok(());
        }
        let Some(wv) = &self.webview else {
            return Ok(());
        };
        match hwnd::carve(wv.hwnd().0 as isize, client, holes) {
            Ok(()) => {
                self.last_region = Some(want);
                Ok(())
            }
            Err(e) => {
                self.last_region = None;
                Err(e)
            }
        }
    }

    /// Does a carved hole actually pass the POINTER through? Asked once, the first time holes are
    /// pushed, and never again.
    ///
    /// WHY THIS EXISTS. Both halves of the notch were measured working on the machine this was
    /// built on, but WebView2 composites through DirectComposition and a runtime or a driver that
    /// stopped honouring a USER32 window region would take the chrome with it. Invisible chrome is
    /// survivable. UNCLICKABLE chrome is not: a floating window with no way to close it is the one
    /// thing this window may not become.
    ///
    /// `WindowFromPoint` IS A QUESTION ABOUT A POINT AND NOT ABOUT THE CURSOR, so this runs wherever
    /// the mouse happens to be. If the close chip's own centre does not resolve to the pop-out, the
    /// notch is abandoned for the rest of the run and the video simply stays whole; the pop-out then
    /// reports that it cannot host and the stream stays in the main window, where every control
    /// still works. Failing to a working app beats failing to a pretty one.
    fn probe_notch(&mut self, hwnd: isize, holes: &[(i32, i32, i32, i32)]) {
        if self.notch_works.is_some() || holes.is_empty() {
            return;
        }
        let Some((ox, oy)) = hwnd::client_origin(hwnd) else {
            return;
        };
        let (l, t, r, b) = holes[0];
        let at = (ox + (l + r) / 2, oy + (t + b) / 2);
        let works = hwnd::root_at(at.0, at.1) == Some(hwnd);
        self.notch_works = Some(works);
        if !works {
            log::warn!(
                "player: this machine does not pass the pointer through a carved hole, so the \
                 pop-out cannot carry its own controls; the video will stay in the main window"
            );
        }
    }

    /// The main window`s handle, as the surface itself reports it.
    ///
    /// The registry needs this for ONE thing: so its handle lookup can refuse to hand back the
    /// window the surface is already parented into. `None` until a surface has been built, which
    /// is also exactly when there is nothing that could be moved anywhere.
    pub fn root_hwnd(&self) -> Option<isize> {
        self.root_hwnd
    }

    /// Whether the pop-out may host the video at all. False once the notch is known not to work.
    pub fn can_host_elsewhere(&self) -> bool {
        self.notch_works != Some(false)
    }

    /// `set_bounds` only when the rectangle moved.
    ///
    /// A FAILURE FORGETS THE BOUNDS, and that line is the difference between a retry and a dead
    /// player. `last_bounds` is the "already there" memo; leaving the old value in it after a push
    /// that did not happen means the very next frame compares the new rect against a value it
    /// never reached and, on a window that is standing still, skips the call forever.
    fn place(&mut self, rect: egui::Rect, pixels_per_point: f32) -> Result<(), String> {
        let bounds = super::physical_bounds(rect, pixels_per_point);
        if self.last_bounds == Some(bounds) {
            return Ok(());
        }
        let (x, y, w, h) = bounds;
        let Some(wv) = &self.webview else {
            return Ok(());
        };
        match wv.set_bounds(wry::Rect {
            position: wry::dpi::PhysicalPosition::new(x, y).into(),
            size: wry::dpi::PhysicalSize::new(w, h).into(),
        }) {
            Ok(()) => {
                self.last_bounds = Some(bounds);
                Ok(())
            }
            Err(e) => {
                self.last_bounds = None;
                Err(format!("could not place the player: {e}"))
            }
        }
    }

    /// `set_visible` only when the answer changed. The surface yields its region whenever something
    /// egui draws would cross it; see [`Stage::occluded`].
    ///
    /// A FAILURE FORGETS THE ANSWER, for the reason spelled out on [`Self::place`].
    fn show(&mut self, visible: bool) -> Result<(), String> {
        if self.last_visible == Some(visible) {
            return Ok(());
        }
        let Some(wv) = &self.webview else {
            return Ok(());
        };
        match wv.set_visible(visible) {
            Ok(()) => {
                self.last_visible = Some(visible);
                Ok(())
            }
            Err(e) => {
                self.last_visible = None;
                Err(format!("could not show or hide the player: {e}"))
            }
        }
    }

    /// Asks the BROWSER whether the document is emitting audio, and records the answer.
    ///
    /// THIS IS THE READBACK THE BRIEF ASKED FOR AND A SPIKE SKIPPED. A player's own `isMuted()`
    /// returns the flag we set, so reading it back proves only that the message arrived.
    /// `ICoreWebView2_8::IsDocumentPlayingAudio` is the browser's own account of whether sound is
    /// coming out, and it was measured answering `false` for a page asked to start muted and `true`
    /// for the same video on the same profile asked to start unmuted.
    fn poll_audio(&mut self, asked_for_sound: bool) {
        if !asked_for_sound {
            self.sound = Sound::Off;
            return;
        }
        let now = Instant::now();
        if self
            .last_audio_poll
            .is_some_and(|t| now.duration_since(t) < AUDIO_POLL_EVERY)
        {
            return;
        }
        self.last_audio_poll = Some(now);
        let Some(wv) = &self.webview else { return };
        let Ok(w8) = wv.webview().cast::<ICoreWebView2_8>() else {
            return;
        };
        let mut playing = windows_core::BOOL(0);
        /* SAFETY: `w8` is a live WebView2 interface owned by the webview this Player holds, and
         * `playing` is a stack local of exactly the out-parameter's type. */
        let ok = unsafe { w8.IsDocumentPlayingAudio(&mut playing) }.is_ok();
        self.sound = if ok && playing.as_bool() {
            Sound::On
        } else {
            Sound::AskedButSilent
        };
    }

    /// Builds the child surface. Every choice here is load bearing; see the parent module's header.
    fn build(
        &mut self,
        host: &eframe::Frame,
        feed: &Feed,
        sound: bool,
        rect: egui::Rect,
        pixels_per_point: f32,
    ) -> Result<wry::WebView, String> {
        /* THE ONE LINE THAT DIFFERS BETWEEN A PLAYER AND A PAGE, and everything below it is shared.
         * See `super::load`: `served` is `Some` for the two framed players and `None` for the
         * channel page, which is loaded top level because it carries `X-Frame-Options: SAMEORIGIN`
         * and could not be framed by us even if we wanted it to be. */
        let (url, served) = super::load(feed, sound);
        let (x, y, w, h) = super::physical_bounds(rect, pixels_per_point);

        let webview = {
            /* The context is borrowed for the length of the builder only. The built webview owns
             * nothing of it, which is what lets this Player hold both. */
            let ctx = &mut self.web_context;
            let mut builder = WebViewBuilder::new_with_web_context(ctx)
                /* Windows serves custom protocols from https://<scheme>.localhost with this on and
                 * http://<scheme>.localhost with it off. Twitch's frame-ancestors names the https
                 * form only, and the http form of the same host was measured BLOCKED. Set for both
                 * kinds of surface: it decides the shape of OUR origin, which a top level load
                 * never asks for, so leaving it on costs nothing and keeps one builder. */
                .with_https_scheme(true)
                .with_background_color((0, 0, 0, 255));
            if let Some(page) = served {
                builder = builder
                    /* wry turns this into --autoplay-policy=no-user-gesture-required, which is what
                     * lets a stream start without a click inside the page. It does NOT unmute
                     * anything: the mute flag is in the embed URL and starts set.
                     *
                     * IT IS ON THE FRAMED ARM ONLY, DELIBERATELY. A channel page has a featured
                     * video on it and this app is not asking for anything on that page to start by
                     * itself. Nothing this app draws asked for it, so nothing this app builds
                     * grants it. */
                    .with_autoplay(true)
                    /* The page is a value, not a file. Nothing is read from disk to serve it. */
                    .with_custom_protocol(
                        super::SCHEME.to_owned(),
                        move |_id, _req: Request<Vec<u8>>| {
                            Response::builder()
                                .header("Content-Type", "text/html")
                                .body(std::borrow::Cow::Owned(page.as_bytes().to_vec()))
                                .unwrap_or_else(|_| {
                                    Response::new(std::borrow::Cow::Owned(Vec::new()))
                                })
                        },
                    );
            }
            builder
                .with_url(url)
                .with_bounds(wry::Rect {
                    position: wry::dpi::PhysicalPosition::new(x, y).into(),
                    size: wry::dpi::PhysicalSize::new(w, h).into(),
                })
                /* eframe::Frame is the ROOT window's handle (eframe 0.36 epi.rs:701). */
                .build_as_child(host)
                .map_err(|e| format!("could not create the player surface: {e}"))?
        };
        self.last_bounds = Some((x, y, w, h));
        self.tracking_prevention = disable_tracking_prevention(&webview);
        Ok(webview)
    }
}

/* THERE IS NO SECOND WAY TO FIND HOME, AND THERE WAS ALMOST ONE. A fallback that dug the root
 * handle out of `eframe::Frame` was written and taken back out, because it would be a second
 * answer to a question that already has one. `root_hwnd` is `GetParent` of the container, read
 * back straight after the build, and the build ALWAYS goes into the root: `build_as_child` takes
 * a `HasWindowHandle` and `eframe::Frame` is the only thing in this process that has one. So the
 * readback is not merely the better answer, it is the answer, and it says where the surface
 * demonstrably is rather than where a handle claims it should be. Two sources here would mean a
 * frame where they disagree and a video parented into whichever one was consulted. */

impl Default for Player {
    fn default() -> Self {
        Player::new()
    }
}

/// Turns Edge Tracking Prevention OFF on THIS APP'S OWN PROFILE and returns the level read back
/// afterwards, or `None` when the runtime is too old to carry the interface.
///
/// WHY, MEASURED. WebView2 defaults to "Balanced" (level 2), and on a fresh profile that, not
/// cookie partitioning, is what withholds Twitch's cookies from the player iframe. Twitch's
/// cookies are unpartitioned and reach a third party iframe fine once it is off. The scope is our
/// own profile folder and nothing else on the machine.
///
/// THE RETURN VALUE IS A READBACK. `SetPreferredTrackingPreventionLevel` returning `Ok` says the
/// call was accepted, not that the level changed; the level is read again afterwards and that is
/// what is reported. Measured on this machine: before 2, after 0.
fn disable_tracking_prevention(webview: &wry::WebView) -> Option<i32> {
    /* SAFETY: every call below is on a live WebView2 interface obtained by QueryInterface from the
     * webview this process owns, with out-parameters that are stack locals of the declared type. */
    unsafe {
        let profile = webview
            .webview()
            .cast::<ICoreWebView2_13>()
            .ok()?
            .Profile()
            .ok()?;
        let p3 = profile.cast::<ICoreWebView2Profile3>().ok()?;
        p3.SetPreferredTrackingPreventionLevel(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE)
            .ok()?;
        let mut level = COREWEBVIEW2_TRACKING_PREVENTION_LEVEL(-1);
        p3.PreferredTrackingPreventionLevel(&mut level).ok()?;
        Some(level.0)
    }
}
