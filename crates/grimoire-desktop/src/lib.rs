//! EQL Grimoire desktop: the modules behind the binary in `main.rs`.
//!
//! WHY A LIBRARY TARGET AND NOT ONE BINARY.
//! Nearly everything here is a rule with a test: the kill grammar, the
//! inventory columns, the gear score, the valet walk, the poSky ledger, the quest step machine.
//! The binary draws one screen at a time, so at any moment most of that API has exactly one
//! caller. In a library a `pub` item is the crate's API by definition, and `-D warnings` cannot
//! see whether the binary reaches it. THAT IS THE TRAP THIS FILE NAMES: a green clippy on the lib
//! target is not evidence of reach. Two checks stand in for it, and each has a blind spot the
//! other covers:
//!
//!   1. rustc's dead-code lint on a bin-only copy of the crate (no lib.rs, the modules declared
//!      in main.rs, every `allow(dead_code)` stripped): every function, method, struct, constant
//!      and field the binary never reaches. Its blind spot is a field on a struct that derives
//!      `PartialEq`, `Eq`, `Hash`, `PartialOrd` or `Ord`: the derive reads every field, so the
//!      lint says nothing. Only `Clone` and `Debug` are ignored by that lint.
//!   2. `reach.rs`, a test in this crate: for every struct behind one of those derives, every
//!      field must have a `.field` read or a destructuring read in production text. Its blind
//!      spot is a field that shares its name with a read field on another struct, and it is a
//!      text floor, not a type check.
//!
//! The README records what each check found and what was done about it (wired into a production
//! path, or cut). Round one certified "zero unused" on check 1 alone and an adversarial pass
//! found fourteen fields behind the derive blind spot; check 2 exists so that cannot repeat
//! silently. The next such pass starts from these two floors, not from zero.
//!
//! The module set is the contract, fixed by decision D9; the binary re-exports nothing and
//! reaches in by path.
pub mod castmsg;
pub mod channel_art;
pub mod chat;
pub mod chrome;
pub mod class;
pub mod data;
pub mod fights;
pub mod fonts;
pub mod hotkeys;
pub mod hp;
pub mod ingest;
pub mod irc;
pub mod nav;
pub mod overlay;
pub mod persona;
pub mod pin;
pub mod player;
/// The reachability floor rustc cannot run: test only, see the module note.
#[cfg(test)]
mod reach;
pub mod screens;
pub mod secret;
pub mod settings;
pub mod shell;
pub mod store;
pub mod theme;
pub mod titlebar;
pub mod twitch_auth;
/// A CHANGE TO THE MODULE SET, STATED AS ONE. The note above calls this list the contract, fixed
/// by decision D9, so a new entry is a decision rather than a file that appeared. `updater` is the
/// security-critical half of the self-update feature: manifest parsing, signature verification,
/// the version rules, and the file dance. It has no UI, no threads and no network, which is what
/// makes every rule in it testable; the poll thread and the Settings section that drive it land
/// with the other half.
pub mod updater;
pub mod watcher;
pub mod windows;
pub mod ytchat;
