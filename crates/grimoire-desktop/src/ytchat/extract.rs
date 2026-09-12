//! The script that is injected into the YouTube live chat popout, and the address it is injected
//! at. Text only. Nothing in this file runs a browser, parses a message or touches the screen.
//!
//! WHAT THIS FILE IS. Two constants and one function. `EXTRACT_JS` is the whole page side bridge,
//! handed to `wry::WebViewBuilder::with_initialization_script` by `ytchat::surface`, and
//! `live_chat_url` builds the address that webview is pointed at. The Rust side of the bridge, the
//! parse of what the script posts, lives in `ytchat::model`; this file holds no knowledge of the
//! `YtMessage` shape beyond the JSON key names the script writes.
//!
//! WHY AN INITIALIZATION SCRIPT AND NOT `evaluate_script`. This surface reloads on purpose (a
//! stalled continuation is recovered by reloading the page) and re-navigates whenever the video id
//! changes, and both wipe anything `evaluate_script` installed. `with_initialization_script`
//! becomes WebView2's `AddScriptToExecuteOnDocumentCreated`, which runs before any page script on
//! every document created, so the bridge survives both. Two consequences are written into the
//! script itself. On Windows wry adds the script to SUBFRAMES as well, regardless of the
//! `for_main_frame_only` flag it takes (wry 0.56.1 says so twice in its own doc), so the first
//! statement refuses to run anywhere but the top frame: a YouTube ad iframe must not install a
//! second observer, a second `fetch` wrapper and a second `setInterval` in this process. And wry
//! has no API to REMOVE an initialization script, so the second statement is a sentinel guard on
//! `window.__ytChat`: the script must be safe to run twice against the same live document.
//!
//! WHY A MutationObserver AND NOT POLLING, AND WHY THIS IS THE LOAD BEARING CHOICE. The bridge
//! webview is placed off the parent's client rectangle, and Microsoft documents that a WebView2
//! whose controller is not visible has "code that throttles activities on the page like animations
//! and some tasks are run less frequently". That is Chromium's hidden page timer throttling, and it
//! was measured on the live page: a nominal `setTimeout(1000)` returned in 1101, 2002 and 1991 ms,
//! and a nominal one second `setInterval` delivered six ticks in 38.8 seconds. A bridge whose
//! delivery path is a timer therefore delivers chat in clumps of up to a minute and calls that
//! normal. A `MutationObserver` callback is a microtask queued by the DOM mutation itself, not by
//! the timer queue, so it is not subject to that throttle at all. Everything that MUST be prompt
//! (reading a new row and posting it) hangs off the observer. Everything on the one second
//! `setInterval` is housekeeping that loses nothing by arriving late: re-selecting the Live chat
//! mode, re-resolving `#items`, un-pausing the scroller and the liveness ping.
//!
//! WHY EVERY NODE IS READ INSIDE ITS OWN try/catch, AND WHY THIS IS NOT DEFENSIVE PADDING. An
//! exception thrown out of a `MutationObserver` callback does not surface anywhere: it does not
//! reject a promise, it does not reach `window.onerror` in a way this process can see, and it does
//! not stop the page. It kills the callback for that batch, and a throw on a shape the script did
//! not expect will keep killing it on every batch that carries the same shape. The observable
//! result is a chat that goes quiet and stays quiet with no error on any side of the bridge, which
//! is exactly the failure this app refuses everywhere else: "nobody is talking" is a fact about the
//! room and "the reader is broken" is a fact about this machine, and drawing the second as the
//! first shows an empty, calm, wrong chat. So `sweep` wraps each node separately, one bad row costs
//! one row, and the four tick jobs are wrapped separately so a YouTube markup change that breaks
//! the mode switch cannot also stop the un-pauser.
//!
//! WHY THE BATCH IS THROTTLED, AND WHY 250 MS. Every `postMessage` crosses the WebView2 IPC
//! boundary, lands on the UI thread and wakes a repaint. `src/chat.rs` already records that one
//! repaint per message is a measured defect and answers it with a coalescer, and this is the same
//! answer moved one side earlier, into the page, where the burst never crosses the boundary at all.
//! 250 ms matches that coalescer's window deliberately, so the merged feed has one cadence and not
//! two. Most of the time it costs nothing, because YouTube drip feeds one node at a time
//! (`isSmoothed_` was true and `chatRateMs_` sampled 7706, 18380 and 23254 ms on the live page). It
//! earns its keep on the flush after a scroll pause: a burst of six rows was observed going out as
//! one message. The throttle is a `setTimeout`, so on a throttled renderer the flush is LATE, never
//! lost, which is the right way round.
//!
//! WHY `timestampUsec` IS OPTIONAL AND MUST NEVER BE LOAD BEARING. `#timestamp` in the DOM reads
//! "7:44 PM", minute granularity, useless for ordering a merged two platform feed. The microsecond
//! stamp exists, but only inside Polymer's private state, and the path is not the one the briefing
//! recorded: on YouTube desktop build f82dea74 `'__data' in el` is `false` and the stamp is at
//! `el.inst.data.timestampUsec`, with `el.controllerProxy.data` aliasing the same object. Reading
//! only the documented `__data` path yields `null` for 100% of rows today. So the script tries all
//! three inside one catch and returns `null` when none of them answers. This is YouTube's framework
//! internals: it is not an attribute, it is not in `dataset`, it is not a contract, and it can
//! vanish on any deploy without a deprecation. `model::YtMessage::ts_usec` is therefore
//! `Option<i64>` and the merged feed must degrade to arrival order when it is `None`, not refuse
//! the row and not crash. The reason to try at all is that arrival order is measurably wrong: DOM
//! insert lag was 2.1 to 5.8 seconds normally and 44 to 66 seconds after a pause flush, and two
//! rows were appended in the same millisecond with stamps 20 seconds apart.
//!
//! THE THREE THINGS THE TICK FIXES, EACH OF WHICH SILENTLY EATS MESSAGES. All three were produced
//! on the live page, none of them raises an error, and each one is re-checked every second rather
//! than once at startup because a reload undoes all three.
//!   1. Every load and every reload comes up in "Top chat", which YouTube's own dropdown describes
//!      as "Some messages, such as potential spam, may not be visible", against "All messages are
//!      visible" for "Live chat". A feed left on the default is lossy by YouTube's own admission.
//!   2. Switching to Live chat REPLACES the `#items` element. An identity check across the switch
//!      returned false, and an observer bound to the old node then reported nothing forever while
//!      76 new children existed in the new one. So `#items` is re-resolved every tick and the
//!      observer re-attached when the node identity changes. Re-sweeping the new node's children is
//!      free because `read` dedups on `.id`.
//!   3. Scrolled off the bottom the list pauses. Polls keep succeeding, rows go into the
//!      renderer's `activeItems_` buffer and NEVER enter the DOM, so a DOM observer loses them with
//!      no symptom at all: children stayed at 86 and `addedNodes` stayed at 10 across a 35 second
//!      pause that swallowed two rows. Restoring `scrollTop` and dispatching `scroll` flushed the
//!      buffer intact within 8 seconds. A synthetic click on `#show-more` did NOT resume it, which
//!      is why the button is not used.
//!
//! THE `ping` ROW EXISTS BECAUSE MESSAGE SILENCE IS NOT A STALL, AND IT IS THE ONLY HONEST
//! LIVENESS SIGNAL. This room measured 3.1 messages a minute on YouTube and 3.68 on Twitch, and a
//! 93 second gap between appends was observed on a feed that was polling perfectly. So a watchdog
//! that reloads on quiet chat reloads a healthy room, drops its backlog and learns nothing. What
//! separates the two cases is whether `get_live_chat` is still succeeding, which is why the script
//! wraps `window.fetch` and counts successful responses rather than timing the DOM. The count rides
//! out on a row with `kind: "ping"` every 20 seconds, carrying `polls`. `ytchat::model::parse_batch`
//! must SKIP that row without counting it as a refusal (a refusal counter that ticks every 20
//! seconds by design is a tripwire that cannot fire), and `ytchat::surface` watches for the PING
//! ROW itself: it reloads when no payload of any kind has arrived for 90 seconds, and never on
//! message silence alone, because the ping keeps arriving through silence. One poll gap of 34.7
//! seconds was observed
//! against a nominal 10 second cadence, so a 15 second rule would reload a healthy feed.
//! THAT CONDITION WAS CHECKED AND HALF MET. `polls` and the `window.fetch` wrapper were read by
//! nothing and are DELETED. The `ping` ROW stays, because `surface.rs` does consume it: it is the
//! heartbeat the watchdog measures, and without it a quiet room would be indistinguishable from a
//! dead one. This crate's
//! reachability floor is about Rust items and cannot see into a string, so this paragraph is the
//! only thing standing where that test would.
//!
//! WHAT THE SCRIPT DELIBERATELY DOES NOT ADMIT. `yt-live-chat-viewer-engagement-message-renderer`
//! is YouTube's own "Welcome to live chat" guidelines card. It is not a viewer, it is re-emitted on
//! every reload, and admitting it would put YouTube's boilerplate into a merged feed of what people
//! said. It is absent from the kind table, which is the whole mechanism: an unrecognised tag name
//! yields `null` from `read` and is dropped before it is ever queued.

/// The address of the signed out live chat popout, without the video id.
///
/// TOP LEVEL, NOT FRAMED, AND MEASURED. This page answers 200 to a signed out client with 249 KB
/// carrying `ytInitialData`, a `liveChatRenderer` and a continuation, with no consent wall and no
/// iframe of its own. It is loaded the way `player::Feed::YouTubeChannel` is loaded, as the
/// webview's own top level document, so the `X-Frame-Options` question that forces the player's
/// host page never arises here.
pub const LIVE_CHAT_URL_BASE: &str = "https://www.youtube.com/live_chat?is_popout=1&v=";

/// Uppercase hex, for the one escape `live_chat_url` can need.
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// The live chat popout address for one video id.
///
/// WHY THIS PERCENT ENCODES AT ALL, GIVEN THAT A YOUTUBE VIDEO ID CANNOT NEED IT. An id is eleven
/// characters of `A-Za-z0-9_-` and every one of those is unreserved, so on every real input this
/// function is a concatenation and the encoder never fires. It is here because the id does not come
/// from a constant: it comes from `watcher::Channel::video_id`, which comes from a scrape of a page
/// this app does not control, and the value it produces is being pasted into a query string. An `&`
/// or a `#` arriving from a changed page shape would silently append a parameter or a fragment to
/// the address instead of failing, and the webview would load a DIFFERENT room, or no room, and the
/// bridge would report a quiet chat. Encoding turns that into a URL that plainly does not resolve.
///
/// The unreserved set is RFC 3986's: `A-Za-z0-9` plus `-`, `.`, `_` and `~`. Those four are passed
/// through, which is why a normal id survives byte for byte; everything else, including every
/// non ASCII byte of a UTF-8 sequence, becomes `%XX` with uppercase hex.
pub fn live_chat_url(video_id: &str) -> String {
    let mut out = String::with_capacity(LIVE_CHAT_URL_BASE.len() + video_id.len());
    out.push_str(LIVE_CHAT_URL_BASE);
    for b in video_id.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0x0f) as usize] as char);
            }
        }
    }
    out
}

/// The page side bridge, verbatim, as it was run against the live page.
///
/// PROVENANCE. This is not a sketch. It was injected into
/// `https://www.youtube.com/live_chat?is_popout=1&v=UdIx8u6qmKo` with a stub `window.ipc` and run
/// to completion: 14 batches posted, batch sizes `[77, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 6, ...]`, 91
/// messages with 0 duplicate ids and 0 null stamps, 90 text and 1 paid carrying `amount: "¥2,000"`,
/// one `viewer-engagement` card correctly refused (92 DOM children, 91 emitted), the mode switched
/// itself to Live chat after a reload, the coalescer folded a burst of 6 into one message, and the
/// `polls` counter was watched rising 0, 4, 12. Every number in its comments is from that session.
///
/// THE LITERAL IS `r##"..."##`, NOT `r#"..."#`, AND THAT IS FORCED. The script contains
/// `querySelector("#items...")` and `querySelector("#label-text")`, so the byte pair `"#` appears
/// inside it and a single hash raw string would end at the first selector. Anyone widening this
/// script with a new id selector must not narrow the delimiter back.
///
/// WHAT IT POSTS. One `window.ipc.postMessage(JSON.stringify(array))` per flush. Every element of
/// the array has the same key set, so `model::parse_batch` never has to ask whether a key is
/// present: `kind` (one of `"text"`, `"paid"`, `"member"`, `"ping"`), `id` (the dedup key, an
/// opaque stable string such as `"ChwKGkNOcXoyZnVhMXBZREZZRVNkZ1lkVWdVTjF3"`), `author`,
/// `authorType` (observed `""`, `"member"`, `"owner"`; `"moderator"` did not appear in this window
/// and must still be handled), `body`, `ts` (the optional microsecond number, or `null`), `pieces`
/// (an array of `{t:"text",v}` and `{t:"emote",name,url}`), and `amount` (a raw localized string on
/// a paid row, `null` otherwise). A `ping` row carries no extra fields: it exists so the Rust
/// side can tell a quiet room from a dead one, and its arrival IS the whole signal.
///
/// WHY `body` IS REBUILT FROM THE PIECES AND NOT TAKEN FROM `textContent`. `#message.textContent`
/// drops an `<img class="emoji">` entirely and leaves the hole behind as a double space, measured
/// as `"put everything in Bee  tier"`. The emote's name is in its `alt`, which for a unicode emote
/// is the literal glyph and for a channel emote is the `:name:` form, so `body` is accumulated
/// piece by piece as the child nodes are walked and the `alt` is spliced in where the image sat.
pub const EXTRACT_JS: &str = r##"
(function () {
  "use strict";
  // On Windows wry adds an initialization script to every subframe regardless of the
  // for_main_frame_only flag it accepts, so this runs in YouTube's ad and player iframes too. A
  // second observer, a second fetch wrapper and a second interval in this process would double
  // every row and double the IPC traffic, so anything but the top frame leaves immediately.
  if (window !== window.top) { return; }
  // wry has no API to remove an initialization script, so this one lives for the life of the
  // webview and runs again on every reload and every navigation. It must be safe to run twice.
  if (window.__ytChat) { return; }
  var SEND_MS = 250, PING_MS = 20000, TICK_MS = 1000;
  // yt-live-chat-viewer-engagement-message-renderer is deliberately NOT here: it is
  // YouTube's own "Welcome to live chat" guidelines card, it is re-emitted on every
  // reload, and admitting it would spam the merged feed with non-viewer text.
  var KIND = {
    "yt-live-chat-text-message-renderer": "text",
    "yt-live-chat-paid-message-renderer": "paid",
    "yt-live-chat-membership-item-renderer": "member"
  };
  var seen = new Set(), queue = [], timer = null, el = null, obs = null, lastPing = 0;

  // THE `window.fetch` WRAPPER IS GONE, AND ITS OWN AUTHOR SAID TO DELETE IT.
  //
  // It counted successful get_live_chat responses into a `polls` number that rode out on the
  // ping row, and NOTHING ON THE RUST SIDE EVER READ IT: `surface.rs` reloads on 90 seconds with
  // no payload at all, which the ping row already provides on its own. The module note above this
  // script wrote the condition itself, "IF NEITHER LANE CONSUMES `polls`, DELETE THE `fetch`
  // WRAPPER", and the condition is met.
  //
  // IT IS NOT MERELY DEAD, IT IS INTRUSIVE. Monkeypatching `window.fetch` replaces a function on
  // a page this app does not own, for the whole life of the document, for every request the page
  // makes and not only chat ones. That is a real risk to somebody else's code (a thrown wrapper
  // breaks THEIR request, not ours) taken in exchange for a number nobody reads.
  //
  // LIVENESS IS STILL NOT "A NODE APPEARED": this room measured 3.1 msg/min and went 93 seconds
  // silent while perfectly healthy. The heartbeat is the `ping` row below, which arrives on a
  // timer whether or not anybody spoke, and that is what `surface.rs` actually watches.

  // 250ms mirrors the Coalescer in src/chat.rs. YouTube drip feeds one node at a time most
  // of the time (isSmoothed_ true, chatRateMs_ 7.7s to 23.3s), but a scroll-pause flush
  // released 6 at once here, and that burst must cross the IPC boundary once, not six times.
  function flush() {
    timer = null;
    if (!queue.length) { return; }
    var batch = queue;
    queue = [];
    try { window.ipc.postMessage(JSON.stringify(batch)); } catch (e) {}
  }
  function push(item) {
    queue.push(item);
    if (timer === null) { timer = setTimeout(flush, SEND_MS); }
  }

  // MEASURED, and it contradicts the obvious reading: el.__data does NOT exist on build
  // f82dea74 ('__data' in el === false). The microsecond stamp lives at
  // el.inst.data.timestampUsec, with el.controllerProxy.data aliasing the same object. All
  // three are tried inside one catch because #timestamp is minute granularity ("8:38 PM")
  // and DOM arrival order is NOT send order: insert lag measured 2.1s to 5.8s normally and
  // 44s to 66s after a scroll-pause flush. The merged feed must sort on this, not arrival.
  // It is Polymer private state, so it is allowed to be null and nothing may depend on it.
  function usec(n) {
    try {
      var d = (n.__data && n.__data.data) || (n.inst && n.inst.data) || (n.controllerProxy && n.controllerProxy.data);
      return d && d.timestampUsec ? Number(d.timestampUsec) : null;
    } catch (e) { return null; }
  }

  function read(n) {
    var kind = KIND[n.tagName.toLowerCase()];
    if (!kind) { return null; }
    var id = n.id;
    if (!id || seen.has(id)) { return null; }
    seen.add(id);
    // The page's own cap is 250 rows (maxItemsToDisplay in ytInitialData), but this set also has
    // to absorb the 75 to 77 row backlog that every reload replays, so it is bounded here rather
    // than left to grow for the life of a stream that can run for hours.
    if (seen.size > 4000) { seen = new Set(Array.from(seen).slice(-2000)); }
    var msg = n.querySelector("#message"), pieces = [], body = "", i, c, name;
    for (i = 0; msg && i < msg.childNodes.length; i++) {
      c = msg.childNodes[i];
      if (c.nodeType === 3) {
        if (c.nodeValue) { pieces.push({ t: "text", v: c.nodeValue }); body += c.nodeValue; }
      } else if (c.nodeType === 1 && c.tagName === "IMG") {
        // #message.textContent silently drops the img and leaves a double space
        // ("put everything in Bee  tier"), so body is rebuilt from the alt text.
        name = c.alt || c.getAttribute("shared-tooltip-text") || "";
        pieces.push({ t: "emote", name: name, url: c.src || "" });
        body += name;
      } else if (c.nodeType === 1 && c.textContent) {
        pieces.push({ t: "text", v: c.textContent });
        body += c.textContent;
      }
    }
    var an = n.querySelector("#author-name"), amt = n.querySelector("#purchase-amount");
    // Superchat renderers carry NO author-type attribute at all (measured null, not ""),
    // so author-is-owner is the only owner signal left on that renderer.
    return {
      kind: kind, id: id,
      author: an ? an.textContent.trim() : "",
      authorType: n.getAttribute("author-type") || (n.hasAttribute("author-is-owner") ? "owner" : ""),
      body: body, ts: usec(n), pieces: pieces,
      amount: amt ? amt.textContent.trim() : null
    };
  }

  // A throw inside a MutationObserver callback kills the observer with no error anywhere
  // and the feed dies silently, so every node is read inside its own try.
  function sweep(nodes) {
    for (var i = 0; i < nodes.length; i++) {
      try { if (nodes[i].nodeType === 1) { var it = read(nodes[i]); if (it) { push(it); } } } catch (e) {}
    }
  }

  // MEASURED: every load and every reload comes up in "Top chat", which YouTube itself
  // labels "Some messages, such as potential spam, may not be visible". Live chat says
  // "All messages are visible". The choice does not survive a reload, so it is re-checked
  // every tick rather than once at startup.
  function liveMode() {
    var lbl = document.querySelector("#label-text");
    if (!lbl || lbl.textContent.indexOf("Live chat") !== -1) { return; }
    var as = document.querySelectorAll("yt-dropdown-menu a.yt-dropdown-menu");
    for (var i = 0; i < as.length; i++) {
      if (as[i].textContent.trim().indexOf("Live chat") === 0) { as[i].click(); return; }
    }
  }

  // MEASURED: off the bottom the list scroll-pauses. Polls keep succeeding but the items go
  // into activeItems_ and NEVER enter the DOM, so a DOM observer loses them with no symptom
  // at all. Restoring scrollTop and firing 'scroll' flushed the 2 buffered items in one
  // tick. A synthetic click on #show-more did NOT resume it, so the button is not used.
  function unpause() {
    var sc = document.querySelector("#item-scroller");
    if (!sc) { return; }
    if (sc.scrollHeight - sc.scrollTop - sc.clientHeight > 2) {
      sc.scrollTop = sc.scrollHeight;
      sc.dispatchEvent(new Event("scroll", { bubbles: true }));
    }
  }

  // MEASURED: the Top chat to Live chat switch REPLACES the #items element (identity check
  // returned false), which leaves an observer bound to a detached node reporting nothing
  // forever. Re-resolve #items every tick and re-sweep its children instead of trusting the
  // first lookup. Re-sweeping is free because read() dedups on id.
  function attach() {
    var n = document.querySelector("#items.yt-live-chat-item-list-renderer");
    if (!n || n === el) { return; }
    if (obs) { obs.disconnect(); }
    el = n;
    obs = new MutationObserver(function (ms) {
      try { for (var i = 0; i < ms.length; i++) { sweep(ms[i].addedNodes); } } catch (e) {}
    });
    obs.observe(el, { childList: true });
    sweep(el.children);
  }

  // The watchdog's only input. It rides the ordinary row array so an existing kind match arm
  // skips it, and it carries polls rather than a boolean because the Rust side needs to know
  // that the number CHANGED, not that a tick happened: on a throttled renderer the ticks
  // themselves stretch, and a boolean would call that a stall.
  function ping() {
    var now = Date.now();
    if (now - lastPing < PING_MS) { return; }
    lastPing = now;
    push({ kind: "ping", id: "ping:" + now, author: "", authorType: "", body: "",
           ts: now * 1000, pieces: [], amount: null });
  }

  // Each job is wrapped separately: a YouTube markup change that breaks the mode switch must
  // not also stop the un-pauser or the heartbeat.
  var iv = setInterval(function () {
    try { liveMode(); } catch (e) {}
    try { attach(); } catch (e) {}
    try { unpause(); } catch (e) {}
    try { ping(); } catch (e) {}
  }, TICK_MS);
  window.__ytChat = { stop: function () { clearInterval(iv); if (obs) { obs.disconnect(); } window.__ytChat = null; } };
})();
"##;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every DOM name the script reaches for, paired with what breaks when it is gone. This list
    /// IS the test: a careless edit that renames or drops one of these has removed a measured
    /// dependency, and the failure it would produce on the live page is silence, not an error.
    const SELECTORS: &[(&str, &str)] = &[
        (
            "#items.yt-live-chat-item-list-renderer",
            "the container the observer attaches to",
        ),
        ("yt-live-chat-text-message-renderer", "an ordinary message"),
        ("yt-live-chat-paid-message-renderer", "a superchat"),
        (
            "yt-live-chat-membership-item-renderer",
            "a membership event",
        ),
        ("#message", "the body and the emote pieces"),
        ("#author-name", "who said it"),
        ("author-type", "member and owner, absent on a superchat"),
        (
            "author-is-owner",
            "the only owner signal a superchat carries",
        ),
        ("#purchase-amount", "the superchat amount"),
        (
            "#item-scroller",
            "the scroll pause that swallows rows off DOM",
        ),
        ("#label-text", "which of Top chat and Live chat is selected"),
        (
            "yt-dropdown-menu a.yt-dropdown-menu",
            "the anchors that switch to Live chat",
        ),
        ("get_live_chat", "the poll the liveness counter counts"),
        ("timestampUsec", "the optional microsecond ordering key"),
        (
            "window.ipc.postMessage",
            "the only way anything leaves the page",
        ),
    ];

    /// Every selector the script depends on is still spelled in it.
    ///
    /// MUTATION THAT MAKES THIS RED: delete or misspell any one of the strings in the script, for
    /// example changing `querySelector("#item-scroller")` to `"#items-scroller"`. That single
    /// character is a real and measured failure (the scroll pause buffers rows into `activeItems_`
    /// where a DOM observer cannot see them, with no error on either side), and rustc cannot see
    /// inside a string literal, so this is the only place it can be caught without a browser.
    #[test]
    fn the_script_still_names_every_selector_it_depends_on() {
        for (sel, why) in SELECTORS {
            assert!(
                EXTRACT_JS.contains(sel),
                "EXTRACT_JS no longer mentions {sel:?}, which is {why}"
            );
        }
    }

    /// The guidelines card is named only in the comment that refuses it, never as a kind.
    ///
    /// MUTATION THAT MAKES THIS RED: add
    /// `"yt-live-chat-viewer-engagement-message-renderer": "text"` to the `KIND` table. That is the
    /// exact edit a reader makes when they see the card on screen, do not recognise it and assume
    /// it was dropped by mistake, and it puts YouTube's own boilerplate into a feed of what people
    /// said, re-posted on every reload. The quoted form is what a table entry looks like; the
    /// comment above the table names the tag bare, so the comment is free to keep explaining why.
    #[test]
    fn the_engagement_card_is_refused_not_admitted() {
        assert!(
            !EXTRACT_JS.contains("\"yt-live-chat-viewer-engagement-message-renderer\""),
            "the guidelines card has been quoted into the script, which is how it gets admitted"
        );
        assert!(
            EXTRACT_JS.contains("yt-live-chat-viewer-engagement-message-renderer"),
            "the comment explaining why the card is refused has been deleted with it"
        );
    }

    /// The two guards that make an initialization script safe to install once and run forever.
    ///
    /// MUTATION THAT MAKES THIS RED: delete either `if (window !== window.top) { return; }` or
    /// `if (window.__ytChat) { return; }`. wry adds this script to every subframe on Windows and
    /// re-runs it on every reload and every navigation, and this surface reloads on purpose, so
    /// without the pair the process accumulates one observer, one `fetch` wrapper and one interval
    /// per frame per load, and every row is delivered as many times over.
    #[test]
    fn the_script_refuses_subframes_and_a_second_run() {
        assert!(
            EXTRACT_JS.contains("window !== window.top"),
            "the subframe guard is gone: an ad iframe will install a second bridge"
        );
        assert!(
            EXTRACT_JS.contains("if (window.__ytChat) { return; }"),
            "the re-entry guard is gone: a reload will install a second bridge"
        );
    }

    /// The microsecond stamp is read behind a catch and is allowed to be null.
    ///
    /// MUTATION THAT MAKES THIS RED: rewrite `usec` as a bare
    /// `return Number(n.inst.data.timestampUsec)` with no `try` and no `null`. It reads correctly
    /// today and throws the moment YouTube renames one link in a chain of its own private state,
    /// and a throw inside the observer callback kills the observer for good with no error
    /// anywhere. This is the difference between one field going missing and the whole feed dying.
    #[test]
    fn the_polymer_stamp_is_optional_and_caught() {
        let usec = EXTRACT_JS
            .split_once("function usec(n) {")
            .expect("the usec reader has been renamed or removed")
            .1
            .split_once("\n  }")
            .expect("the usec reader is no longer a function body this test can bound")
            .0;
        assert!(
            usec.contains("try {"),
            "usec no longer reads the private state inside a try"
        );
        assert!(
            usec.contains("catch (e) { return null; }"),
            "usec no longer degrades to null when YouTube's private state moves"
        );
        assert!(
            usec.contains("n.inst && n.inst.data"),
            "the measured path (el.inst.data, not el.__data) has been dropped from usec"
        );
    }

    /// Every one of the four housekeeping jobs is wrapped in its own try.
    ///
    /// MUTATION THAT MAKES THIS RED: collapse the tick body to a single
    /// `try { liveMode(); attach(); unpause(); ping(); } catch (e) {}`. That reads like the same
    /// thing and is not: a YouTube markup change that makes `liveMode` throw would then also stop
    /// the un-pauser, so the feed would silently sit in Top chat AND lose every row that arrives
    /// while the list is scrolled up.
    #[test]
    fn each_tick_job_is_isolated() {
        for job in ["liveMode()", "attach()", "unpause()", "ping()"] {
            let wrapped = format!("try {{ {job}; }} catch (e) {{}}");
            assert!(
                EXTRACT_JS.contains(&wrapped),
                "the tick no longer isolates {job}, so one broken job stops the others"
            );
        }
    }

    /// A real video id survives byte for byte, and the address is the measured one.
    ///
    /// MUTATION THAT MAKES THIS RED: drop `-` or `_` from the unreserved set in `live_chat_url`.
    /// Both appear in YouTube's id alphabet, so encoding them produces an address that answers
    /// with a chat for no video, and the bridge would then report a permanently empty room rather
    /// than an error. Also red if `is_popout=1` is dropped from the base, which is the difference
    /// between the standalone chat document and a page this webview cannot read.
    #[test]
    fn a_real_video_id_is_not_encoded_at_all() {
        assert_eq!(
            live_chat_url("UdIx8u6qmKo"),
            "https://www.youtube.com/live_chat?is_popout=1&v=UdIx8u6qmKo"
        );
        assert_eq!(
            live_chat_url("a-B_9.z~0"),
            "https://www.youtube.com/live_chat?is_popout=1&v=a-B_9.z~0"
        );
        assert!(!live_chat_url("UdIx8u6qmKo").contains('%'));
    }

    /// Anything that could change what the address MEANS is escaped.
    ///
    /// MUTATION THAT MAKES THIS RED: replace the body of `live_chat_url` with
    /// `format!("{LIVE_CHAT_URL_BASE}{video_id}")`. On every real id that is the same string, which
    /// is exactly why the shortcut is tempting. The id comes from a scrape of a page this app does
    /// not control, and an `&` or a `#` arriving from a changed page shape would append a parameter
    /// or a fragment instead of failing, pointing the bridge at a different room while every log
    /// line still looks healthy.
    #[test]
    fn a_separator_in_the_id_cannot_change_the_query() {
        assert_eq!(
            live_chat_url("abc&foo=bar"),
            "https://www.youtube.com/live_chat?is_popout=1&v=abc%26foo%3Dbar"
        );
        assert_eq!(
            live_chat_url("abc#frag"),
            "https://www.youtube.com/live_chat?is_popout=1&v=abc%23frag"
        );
        assert_eq!(
            live_chat_url("a b"),
            "https://www.youtube.com/live_chat?is_popout=1&v=a%20b"
        );
        /* Non ASCII is escaped byte by byte, uppercase, so a UTF-8 sequence cannot arrive raw. */
        assert_eq!(
            live_chat_url("é"),
            "https://www.youtube.com/live_chat?is_popout=1&v=%C3%A9"
        );
        /* An empty id yields the bare base rather than a panic. It is the caller's job not to ask,
         * and this records that asking costs an address that plainly does not resolve. */
        assert_eq!(live_chat_url(""), LIVE_CHAT_URL_BASE);
    }

    /// No em-dash and no en-dash anywhere in the text this file ships.
    ///
    /// MUTATION THAT MAKES THIS RED: type an em-dash into any comment in `EXTRACT_JS`. The house
    /// rule forbids both characters, and this string is the one place in the crate where a
    /// reviewer's eye slides past prose because it looks like code. `\u{2014}` is the em-dash and
    /// `\u{2013}` the en-dash.
    #[test]
    fn the_script_carries_no_dashes_the_house_forbids() {
        for (ch, name) in [('\u{2014}', "em-dash"), ('\u{2013}', "en-dash")] {
            assert!(!EXTRACT_JS.contains(ch), "EXTRACT_JS contains a {name}");
            assert!(
                !LIVE_CHAT_URL_BASE.contains(ch),
                "the base url contains a {name}"
            );
        }
    }
}
