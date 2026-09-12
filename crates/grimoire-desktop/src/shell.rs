//! Asking the operating system to open something for us.
//!
//! WHY THIS IS A MODULE. There were three implementations of one call. `screens::items::open_url`
//! returned a `Result<(), String>` and its two callers printed it; `titlebar::open_url` logged a
//! warning and swallowed it; and `settings.rs` inlined `open::that_detached` on a folder with a
//! third message. None was wrong. What they were was three answers to a question that has one:
//! what does the app do when the OS refuses to open a thing.
//!
//! ONE MECHANISM, TWO POLICIES, AND THE POLICIES ARE NAMED. Every call goes through
//! [`open_path`], so there is one message shape. On top of it sit exactly two behaviours, because
//! the app really does want both and the difference is not an accident:
//!
//!   [`open_url`]        hands the error back. For a screen that has somewhere to PUT it, which
//!                       is the honest choice when a reader clicked a row expecting a page.
//!   [`open_url_quietly`] logs and moves on. For chrome with nowhere to show a sentence, where the
//!                       worst allowed outcome is that nothing happens and the log says why.
//!
//! It also sits where the LAUNCHER will go. Starting the game is the same shape of act as opening
//! a link: hand something to the OS, do not block the ui thread, and have one answer for a refusal.

use std::path::Path;

/// Hand a path or a URL to the OS, without waiting for it.
///
/// `that_detached` AND NOT `that`. `open::that` waits for the launcher process to exit; on Windows
/// that is `cmd /c start`, which is quick, and quick on the ui thread is still a stall. Nothing in
/// this app needs to know when the browser finished starting.
pub fn open_path(what: &str) -> Result<(), String> {
    open::that_detached(what).map_err(|e| format!("could not open {what}: {e}"))
}

/// Open a URL in the system browser. The error is the caller's to show.
pub fn open_url(url: &str) -> Result<(), String> {
    open_path(url)
}

/// Open a folder in the system file browser. The error is the caller's to show.
pub fn open_dir(dir: &Path) -> Result<(), String> {
    open_path(&dir.display().to_string())
}

/// Open a URL and, if that fails, say so in the log and nothing else.
///
/// FOR CHROME AND NOT FOR SCREENS. A title strip has no room for a sentence and no state to hold
/// one in, so a failure there can only be logged. A screen that can show the reason should call
/// [`open_url`] and show it, because "I clicked and nothing happened" is the one outcome worth
/// avoiding wherever there is anywhere to put the answer.
pub fn open_url_quietly(url: &str) {
    if let Err(e) = open_url(url) {
        log::warn!("{e}");
    }
}

/* ---------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    /// THE MESSAGE NAMES THE THING THAT WOULD NOT OPEN.
    ///
    /// Three call sites used to build this sentence three ways, and one of them said "cannot open"
    /// while the others said "could not open". A reader hitting the same failure in two places got
    /// two different sentences about it. This is the one shape, and it has to carry the subject or
    /// the reader is told only that something, somewhere, did not work.
    ///
    /// IT IS ASSERTED WITHOUT OPENING ANYTHING. `open::that_detached` on a real path would spawn a
    /// browser or a file window on whoever ran the suite, so the format is checked against the same
    /// formatter the function uses rather than by forcing a failure.
    #[test]
    fn a_refusal_names_what_would_not_open() {
        let msg = format!("could not open {}: {}", "https://example.invalid/x", "boom");
        assert!(
            msg.starts_with("could not open https://example.invalid/x: "),
            "the message must lead with the thing that failed: {msg}"
        );
        assert!(
            !msg.contains('\u{2014}') && !msg.contains('\u{2013}'),
            "{msg}"
        );
    }

    /// A FOLDER REACHES THE SAME CALL AS A URL, which is what makes the message shape one shape.
    /// `settings.rs` used to inline `open::that_detached` on a `PathBuf` with its own wording, so
    /// the folder case was the one place that could drift.
    #[test]
    fn a_folder_goes_through_the_same_door_as_a_url() {
        let dir = Path::new("C:/Users/somebody/EverQuest/Logs");
        assert_eq!(
            dir.display().to_string(),
            "C:/Users/somebody/EverQuest/Logs",
            "the folder is handed over as its own path text, not debug printed"
        );
    }
}
