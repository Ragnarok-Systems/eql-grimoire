//! WHETHER THIS IS A GOOD MOMENT TO DOWNLOAD OR RESTART.
//!
//! The updater only fetches, installs and restarts in a quiet moment. Under EverQuest Legends the
//! quiet moment came from the combat log (no fight in progress). Monsters & Memories writes no
//! combat log, so the signal now comes from what the reader is doing in Grimoire itself: a stream
//! playing in Watch or picture in picture is something a restart would cut off.
//!
//! The three states keep the updater's rule exactly: only [`Activity::Idle`] lets it act.
//! [`Activity::Settling`] is the short grace after the busy thing ends (a stream paused for a
//! moment is not over), so a restart never lands the instant a video stops.

/// What the reader is doing, as far as the updater is concerned.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Activity {
    /// Something the reader would hate to have cut off is happening (a stream is playing).
    Busy,
    /// The busy thing just ended; held for [`SETTLE_SECONDS`] in case it resumes.
    Settling,
    /// Nothing in progress. The updater may download, install and restart.
    #[default]
    Idle,
}

/// How long after a stream stops before the moment counts as quiet.
pub const SETTLE_SECONDS: u64 = 30;

impl Activity {
    /// Fold "is something busy now" and "seconds since it last was" into one state.
    pub fn from_busy(busy_now: bool, seconds_since_busy: Option<u64>) -> Self {
        if busy_now {
            Activity::Busy
        } else if seconds_since_busy.is_some_and(|s| s < SETTLE_SECONDS) {
            Activity::Settling
        } else {
            Activity::Idle
        }
    }

    /// The narrow question: is the busy thing happening right now?
    pub fn busy(self) -> bool {
        self == Activity::Busy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_playing_is_busy_then_settles_then_goes_idle() {
        assert_eq!(Activity::from_busy(true, Some(0)), Activity::Busy);
        assert_eq!(Activity::from_busy(false, Some(1)), Activity::Settling);
        assert_eq!(
            Activity::from_busy(false, Some(SETTLE_SECONDS - 1)),
            Activity::Settling
        );
        assert_eq!(
            Activity::from_busy(false, Some(SETTLE_SECONDS)),
            Activity::Idle
        );
        assert_eq!(Activity::from_busy(false, None), Activity::Idle);
        assert_eq!(Activity::default(), Activity::Idle);
        assert!(Activity::Busy.busy() && !Activity::Settling.busy());
    }
}
