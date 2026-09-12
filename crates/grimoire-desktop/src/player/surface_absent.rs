//! The player on a platform that has no WebView2. Everything here is `cfg(not(windows))`.
//!
//! THIS IS A PLATFORM GAP STATED OUT LOUD, NOT A STUB THAT PRETENDS. Embedded playback is a
//! WebView2 feature and WebView2 is Windows only. `wry` could in principle host a webkit2gtk
//! surface on Linux, but nothing in this app has ever been measured there, and the tree's CI is a
//! bare ubuntu image with no webkit2gtk development packages for `wry` to link against, so the
//! dependency is declared under `[target.'cfg(windows)'.dependencies]` and this file is what the
//! other platforms compile.
//!
//! The API is the same as [`super::surface::Player`]'s, so the App and the Watch screen have one
//! code path. Every answer it gives is the truthful one for a machine with no surface: there is no
//! webview, sound is off, and [`Player::problem`] names the reason. The Watch screen prints that
//! sentence, which is why this is not a silent no-op: a person on such a machine is told why the
//! video band is words instead of a picture.

use super::{Problem, Sound, Stage};

const WHY: &str =
    "embedded playback needs Microsoft WebView2, which exists on Windows only; use the buttons \
     above to watch in your browser";

pub struct Player {
    problem: Option<Problem>,
}

impl Player {
    pub fn new() -> Player {
        Player {
            /* A REFUSAL, NOT A FAILURE, and the distinction is what keeps the Watch here button
             * greyed out here instead of offering a retry that could never do anything. No click
             * puts WebView2 on a machine that is not Windows. */
            problem: Some(Problem::Refused(WHY.to_owned())),
        }
    }

    /// The same snapshot the Windows surface hands out, filled in with the truth for a machine
    /// that has no WebView2: a reason, nothing playing, sound off, no profile folder because none
    /// was created, and no tracking prevention level because there is no Edge profile to set one
    /// on.
    pub fn view(&self) -> super::PlayerView {
        super::PlayerView {
            problem: self.problem.clone(),
            playing: false,
            sound: Sound::Off,
            profile: None,
            tracking_prevention: None,
            hosted_elsewhere: false,
        }
    }

    pub fn stop(&mut self) {}

    /// The reader asked again. Nothing here can change, and the rule that says so is the shared
    /// one: [`super::after_retry`] keeps a refusal and this platform has nothing else. Written as
    /// the call rather than as an empty body so the two implementations answer the same question
    /// with the same function, and so this platform's build reaches it too.
    pub fn retry(&mut self) {
        self.problem = super::after_retry(self.problem.take());
    }

    /// Takes the same request the Windows surface takes and hosts nothing. The arguments are named
    /// with a leading underscore rather than dropped from the signature so the two implementations
    /// cannot drift apart without the compiler saying so.
    ///
    /// THE SCALE ARGUMENT WENT WITH THE SEAT. It used to be a third parameter beside the stage,
    /// which meant a root rect could be paired with another window`s scale; it now lives inside
    /// `Seat::Body` where the rect it describes is.
    pub fn sync(&mut self, _host: &eframe::Frame, _stage: Option<&Stage>) {}

    /// The main window`s handle. There is no surface here, so there is nothing to report.
    pub fn root_hwnd(&self) -> Option<isize> {
        None
    }

    /// Whether the pop-out may host the video. Never, here: there is no surface to move.
    ///
    /// FALSE AND NOT TRUE, so the pop-out on this platform draws its picture and its chrome rather
    /// than clearing a rectangle for a video that is never coming.
    pub fn can_host_elsewhere(&self) -> bool {
        false
    }
}

impl Default for Player {
    fn default() -> Self {
        Player::new()
    }
}
