//! The YouTube half of the merged chat feed: a hidden WebView2 child running
//! `youtube.com/live_chat`, the page side script that reads it, and the rules that fold what it
//! says into the Twitch log the app already keeps.
//!
//! THREE FILES, ONE DIRECTION OF DEPENDENCY, AND THAT IS THE WHOLE LAYOUT.
//!
//!   * [`extract`] is TEXT ONLY. Two constants and one function: the live chat address and the
//!     JavaScript that is injected into the page. It runs no browser, parses no message and knows
//!     nothing about the Rust shapes beyond the JSON key names its script writes.
//!   * [`model`] is PURE. It turns one `window.ipc.postMessage` payload into rows, counting what it
//!     cannot read rather than dropping it, and it merges the two platforms into the one ordered
//!     column a screen draws. No I/O, no thread, no webview.
//!   * [`surface`] is the only part that touches the world. It owns the page behind a
//!     `Pane` trait seam, hands every payload to `model::parse_batch`, deduplicates by the
//!     YouTube row id, bounds the log, and watches the page for the kind of silence that is the
//!     bridge dying rather than the room being quiet.
//!
//! `surface` names both of the others; neither of them names `surface`, and `extract` names
//! nothing at all. So the two halves that hold every rule worth arguing about are testable with no
//! browser, no window and no network, which is the arrangement `chat::Wire` and
//! `twitch_auth::Wire` already have with a socket and a token endpoint.
//!
//! WHERE THE SEAMS BETWEEN THE THREE ARE, since a payload crosses all of them. The script posts a
//! JSON array whose every element carries `kind`, `id`, `author`, `authorType`, `body`, `ts`,
//! `pieces` and `amount`, with `polls` added on the `kind:"ping"` heartbeat row; `model` reads
//! exactly those names; `surface` never parses a payload itself. A key that one side renames and
//! the other does not is invisible to rustc, so the agreement lives in `extract`'s doc on
//! `EXTRACT_JS`, in `model::parse_batch`'s doc, and in the fixtures in `model`'s tests, which are
//! copies of what the script actually posted against the live page rather than shapes invented by
//! the person writing the test.

pub mod extract;
pub mod model;
pub mod surface;
