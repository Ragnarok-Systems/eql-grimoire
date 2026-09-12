//! The IRC line, in three pieces that know nothing about each other.
//!
//! WHY THREE FILES AND NOT ONE. Each was written against the same 2.5 MB capture of real Twitch
//! traffic and each is defeated by a different thing, so keeping them apart keeps the failure
//! modes apart too. [`line`] knows the GRAMMAR of a line and none of its meaning; [`tags`] takes
//! one already cut tag blob and unescapes it; [`emotes`] turns a body and an emotes tag into the
//! runs a renderer draws. None of them opens a socket and none of them knows what Twitch is.
//!
//! THE READER IS `crate::chat` AND IT IS THE ONLY CALLER. These three are pure: no I/O, no clock,
//! no allocation beyond what an escape forces. That is what let them be tested against the capture
//! rather than against a mock, which is the whole reason the split earns its keep.

pub mod emotes;
pub mod line;
pub mod tags;
