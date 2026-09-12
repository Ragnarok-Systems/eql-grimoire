# Activity Spike Result — G9-03

This document is the deliverable REQ-011 fixes: one section per question defined at
`web/spike/activity.html`, filled in from what was actually observed on real Discord clients,
per platform, with a verbatim value beside every non-obvious verdict. See `web/spike/activity.html`
for the probes and `web/spike/check-result.mjs` for the checker this file must satisfy.

**Status of this run: no human tester has executed the End-to-End Verification steps yet.** The
agent session that wrote `web/spike/activity.html` and `web/spike/check-result.mjs` has no Discord
account, no scratch Discord application, and no access to the desktop, web, iOS or Android Discord
clients — it cannot itself produce the primary-source observations this document exists to carry.
Every cell below is therefore `NOT-TESTED` with that reason, honestly, rather than inferred from
`docs/SPEC.md`.

date: 2026-08-30
desktop-app-version: NOT-TESTED — no scratch application or Discord desktop session was run this pass
web-version: NOT-TESTED — no scratch application or Discord web session was run this pass
ios-version: NOT-TESTED — no scratch application or Discord iOS session was run this pass
android-version: NOT-TESTED — no scratch application or Discord Android session was run this pass
application-id: NOT-TESTED — no scratch Discord application has been created yet
deploy-url: NOT-TESTED — web/spike/ has not been deployed anywhere yet

## A1 LAUNCH

Does a `LAUNCH_ACTIVITY` response, interaction callback type 12, from a **text-channel** command
actually open the Activity, and does the loading overlay dismiss?

- platform: desktop-app | verdict: NOT-TESTED | reason: no human tester ran the End-to-End Verification steps against a scratch Discord application on this client
- platform: web | verdict: NOT-TESTED | reason: no human tester ran the End-to-End Verification steps against a scratch Discord application on this client
- platform: ios | verdict: NOT-TESTED | reason: no human tester ran the End-to-End Verification steps against a scratch Discord application on this client
- platform: android | verdict: NOT-TESTED | reason: no human tester ran the End-to-End Verification steps against a scratch Discord application on this client

observed: desktop-app
```
none — session had no Discord access; per AC-001 this row must come from the client itself, not from anything web/spike/activity.html renders
```

decision: A1 is unresolved. `docs/SPEC.md` §9.4's warning and §11 Q1's "resolved, yes" remain two
positions on the same question until a human runs step 2 of the End-to-End Verification section
against a real scratch application from a guild text channel; no later story should treat Q1 as
settled by this document as it stands.

## A2 FILE-INPUT

Does `<input type="file">` open a picker inside the iframe and deliver a `File` whose bytes can
be read?

- platform: desktop-app | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run
- platform: web | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run
- platform: ios | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run
- platform: android | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run

observed: desktop-app
```
none — session had no Discord access
```

decision: A2 is unresolved. Rung 1's inventory-upload path has no
primary-source confirmation yet; do not build the upload UI against an assumed `YES` until a
human runs this probe with a real `-Inventory.txt` on all four platforms.

## A3 DROP-PASTE

Do a drop handler and a `paste` handler receive file or text data in the iframe?

- platform: desktop-app | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run
- platform: web | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run
- platform: ios | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run
- platform: android | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run

observed: desktop-app
```
none — session had no Discord access
```

decision: A3 is unresolved. Treat drag-drop and paste as unavailable fallbacks until observed;
the file-input path in A2 remains the only ingestion route the board can plan against.

## A4 SANDBOX

The literal `allow` and `sandbox` attribute values on the iframe and the literal
`content-security-policy` header served with the Activity document.

- platform: desktop-app | verdict: NOT-TESTED | reason: no running Activity session to read window.frameElement or client devtools from
- platform: web | verdict: NOT-TESTED | reason: no running Activity session to read window.frameElement or client devtools from
- platform: ios | verdict: NOT-TESTED | reason: no running Activity session to read window.frameElement or client devtools from
- platform: android | verdict: NOT-TESTED | reason: no running Activity session to read window.frameElement or client devtools from

observed: desktop-app
```
none — session had no Discord access; the probe's own attempt to read window.frameElement and a meta CSP tag is in web/spike/activity.html, unexercised here
```

decision: A4 is unresolved and blocking. G9-04's URL-mapping list needs the literal CSP text this
row would carry; do not write that mapping list from inference. Run this probe first among the
eight, since A6 and A8 both depend on knowing what the CSP actually allows.

## A5 WASM

Does `WebAssembly.instantiate` succeed under that CSP, and does `instantiateStreaming` over a
fetched module succeed?

- platform: desktop-app | verdict: NOT-TESTED | reason: no running Activity session; the eight-byte-module probe in web/spike/activity.html was not exercised inside Discord
- platform: web | verdict: NOT-TESTED | reason: no running Activity session; the eight-byte-module probe in web/spike/activity.html was not exercised inside Discord
- platform: ios | verdict: NOT-TESTED | reason: no running Activity session; the eight-byte-module probe in web/spike/activity.html was not exercised inside Discord
- platform: android | verdict: NOT-TESTED | reason: no running Activity session; the eight-byte-module probe in web/spike/activity.html was not exercised inside Discord

observed: desktop-app
```
none — session had no Discord access
```

decision: A5 is unresolved. If it comes back `NO` on any platform, rung 1 has no engine on that
platform at all and every later door in the four-rung ladder is void there; treat wasm support as
unconfirmed, not assumed, until this row is filled.

## A6 RANGE

Does a request carrying `Range: bytes=0-63` through a declared URL mapping return `206` with
exactly 64 bytes equal to the first 64 bytes of the artifact fetched whole?

- platform: desktop-app | verdict: NOT-TESTED | reason: no declared URL mapping exists yet and no running Activity session to fetch through it
- platform: web | verdict: NOT-TESTED | reason: no declared URL mapping exists yet and no running Activity session to fetch through it
- platform: ios | verdict: NOT-TESTED | reason: no declared URL mapping exists yet and no running Activity session to fetch through it
- platform: android | verdict: NOT-TESTED | reason: no declared URL mapping exists yet and no running Activity session to fetch through it

observed: desktop-app
```
none — session had no Discord access
```

decision: A6 is unresolved. `PLAN-APP.md`'s ten-minute sizing assumed this would be quick to
confirm; until it is, plan the corpus fetch path as "sent whole" (the current default) rather
than relying on partial-range delivery.

## A7 FSA

Is `showDirectoryPicker` present on `window` inside the iframe, and what happens when it is
called?

- platform: desktop-app | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run
- platform: web | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run
- platform: ios | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run
- platform: android | verdict: NOT-TESTED | reason: probe requires a running Activity and a user gesture inside it; no session was run

observed: desktop-app
```
none — session had no Discord access
```

decision: A7 is unresolved. The File System Access API is expected to be blocked in a
cross-origin iframe (delegating it takes `allow="file-system"` on the iframe, and nothing suggests
Discord sets that), so rung 2 (the website door, not the Activity) is where `showDirectoryPicker`
is expected to work. Do not offer a directory-picker
control inside the Activity UI until this row confirms the absent/throws/works distinction
EDGE-008 calls for; a control that is present and throws is worse than one that is not offered.

## A8 LOOPBACK

What exactly happens when the iframe requests `http://127.0.0.1:8787`?

- platform: desktop-app | verdict: NOT-TESTED | reason: no running Activity session to observe the fetch rejection or any permission prompt from
- platform: web | verdict: NOT-TESTED | reason: no running Activity session to observe the fetch rejection or any permission prompt from
- platform: ios | verdict: NOT-TESTED | reason: no running Activity session to observe the fetch rejection or any permission prompt from
- platform: android | verdict: NOT-TESTED | reason: no running Activity session to observe the fetch rejection or any permission prompt from

observed: desktop-app
```
none — session had no Discord access
```

decision: A8 is unresolved as an *observed* fact, though three independent structural reasons
make the refusal expected: Discord's CSP limits requests to the app's own proxy; a URL mapping is
fetched by Discord's edge, not by the player's machine, so no mapping can reach the player's
loopback; and Chrome's Local Network Access gates requests from a public origin to `127.0.0.0/8`. Rung 3 (`grimoire-agent` on
loopback) stays the only way to reach the player's local process; this row exists to put the
refusal's exact wording on file rather than leave it as an inference, per REQ-004's stated intent.
