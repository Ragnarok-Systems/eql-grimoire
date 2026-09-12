# DEPLOY — what ships out of `web/`, and what still blocks a deploy

**Read this first: no Discord Activity can be configured from this document today.** The
values a portal configuration needs are the ones `docs/ACTIVITY-SPIKE.md` exists to carry, and
every row in that document currently reads `NOT-TESTED` because no human has run its
End-to-End Verification against a real client. The blocking list is at the bottom of this file,
named question by question. The rest of this document is the part of the deploy that is settled
today and enforced by a command: what ships, what does not, and what may never enter the tree.

Owner note: this file owns the deploy set, the exclusion rule and the checker's contract. Every
other fact here is cited to the document that owns it and is not restated.

---

## 1. What is deployed

One directory, `web/`, copied as-is. There is no build step, no bundler and no package
manifest, and that is a property to defend rather than a gap to fill. The same bytes serve
every door — a variant built for one door is the defect, not the feature.

The deploy set is `web/` minus the exclusions in §2. `node web/deploy.check.mjs` prints its file
count, its byte total and a `sha256` over the set; that hash is what "same bytes, all doors" is
compared with, rather than a visual inspection.

Two files in the deploy set are build outputs and are absent from a fresh checkout:

| File | Produced by |
|---|---|
| `web/grimoire_wasm.wasm` | `cargo build --release -p grimoire-wasm --target wasm32-unknown-unknown`, then copied into `web/` (see `README.md`) |
| `web/corpus.grim` | `grimoire corpus web/corpus.grim --from data --trivials data/trivials-measured.csv` (see `README.md`) |

Both must be present before the directory is copied to a host, because the wasm door fetches
`corpus.grim` itself and has nothing to price with otherwise.

## 2. What is excluded, and why the rule is a rule

The exclusion rule lives in one place — `EXCLUDE_DIR` / `EXCLUDE_FILE` / `isExcluded` in
`web/deploy.check.mjs` — so the runbook and the checker cannot drift apart. Copying this list
into a deploy script would create a second copy that drifts; drive the deploy from the checker's
own listing instead.

| Excluded | Why |
|---|---|
| `*.test.js` | Playwright and plain-Node test drivers |
| `testenv.js` | the browser, binary and corpus scaffolding those drivers need |
| `*.mjs` | node-only tooling, including this checker and `gate.mjs` |
| `url-mappings.json` | repository configuration the checker reads; see §3 |
| `spike/` | the probe page and its checker; the Activity story forbids deploying it |
| `fixtures/` | log fixtures the tests read |

`url-mappings.json` is deliberately outside the scanned bundle as well as outside the deploy.
If the declaration were inside the set the checker scans, an entry could satisfy itself by
appearing in its own file, and the unreferenced-mapping direction of the check would be dead.

## 3. The declaration and the checker

`web/url-mappings.json` is the declared list of every external host the deployed bundle may
reach. It is the source of truth in the repository: the Discord developer portal is configured
**from** it, never the other way round, and a host that reaches production without an entry here
is a hole in the policy that nothing is watching.

```
node web/deploy.check.mjs              # the gate: exit 0 or 1
node web/deploy.check.mjs --selftest   # every rule driven red against a synthetic bundle
```

The checker is plain Node with zero dependencies. It refuses, each with the file and the line:

- a host the bundle reaches that the declaration does not name;
- an entry in the declaration that no deployed file reaches;
- an entry naming no spike question id, naming one `docs/ACTIVITY-SPIKE.md` does not carry, or
  naming one whose rows in that document still read `NOT-TESTED` — a citation pointing at an
  unanswered row is a guess wearing a citation;
- a declaration target shaped like a per-deployment URL rather than a stable hostname (§5);
- a string shaped like an API key, a bot token or a client secret, reported without echoing the
  value;
- the third-party API host, the request-header name that carried the key, or the key field's
  own identifier, anywhere in the deploy set;
- a `window.open(` call, a `target="_blank"` attribute, or an anchor whose `href` begins with a
  scheme;
- a test file or a spike artifact inside the deploy set.

The declaration currently carries **zero entries**, and that is the intended end state for
external hosts rather than an omission: with the in-page API resolver deleted and the fonts
self-hosted, the bundle reaches nothing outside its own origin. The one entry that would still
be needed — the root mapping to the deploy origin — is not written, because no deploy host
exists yet and `docs/ACTIVITY-SPIKE.md` records `deploy-url` as `NOT-TESTED`.

**Current state of the real tree: the checker exits non-zero.** `web/app.html` still carries the
in-page API resolver and the two external font requests. Those live in a file this work item was
not given, and §7 records that.

## 4. What may never enter the repository

No bot token, no client secret, no signing key, no API key. The application id is public and
belongs in this runbook once one exists. The checker enforces the difference, so a future paste
of the wrong portal line fails a command rather than a review.

## 5. Stable host, never a preview URL

The mapping must target a stable hostname. A rollback that changes the origin silently
invalidates every mapping and produces an Activity that loads a white page — the failure mode
with the fewest available diagnostics in the entire product, because there is no console inside
the iframe. The checker refuses a declaration target that carries a hex-shaped leading label, a
`preview` or `staging` marker, or more labels than a stable apex plus one subdomain, so the
refusal fires at check time rather than at load time.

That refusal is a shape rule over the string in the declaration. It is not sourced from any
vendor's documented URL scheme, and it is deliberately conservative rather than exact; §7 names
what would make it exact.

`docs/SPEC.md` §9.1 names the hosting shape this bundle is packaged for. The concrete deploy
host, the rollback command and the roll-forward command are not recorded here because no deploy
exists to record them from, and a rollback step nobody has run is not a rollback step.

## 6. Debt recorded rather than left silent

**The corpus is sent whole.** `docs/ACTIVITY-SPIKE.md` A6 asks whether a `Range` request
survives a declared URL mapping and its rows read `NOT-TESTED`, so its own decision says to plan
the corpus fetch path as sent whole. If the answer comes back no, the fallback is sharding the
corpus by domain, which the format already allows. Cutting those shards is corpus work and is not this deploy's; the debt is
written down here so it does not come due silently. The artifact is not present in this
checkout, so no current size is recorded rather than a stale one.

**The engine may have no home inside the Activity.** `docs/ACTIVITY-SPIKE.md` A5 asks whether
WebAssembly compiles under the Activity's content-security-policy and its rows read
`NOT-TESTED`. If that comes back no, the Activity has no engine at all — which changes the shape
of the product rather than the shape of the deploy. It is unanswered, so it is neither planned
around nor assumed away.

## 7. Blocking list — why no Activity can be configured from this file

Each line names the exact claim and what would settle it. None of these can be closed by
reasoning about them.

| Blocked | The claim that needs settling | What settles it |
|---|---|---|
| The portal configuration | Whether a `LAUNCH_ACTIVITY` response from a text-channel command opens the Activity and the loading overlay dismisses | `docs/ACTIVITY-SPIKE.md` A1, run against a real scratch application on each client |
| The content-security-policy this bundle must satisfy, and the `allow` / `sandbox` values | The literal header and attribute values Discord serves | `docs/ACTIVITY-SPIKE.md` A4 — its own decision calls this blocking and says to run it first |
| Whether the wasm door survives | Whether `WebAssembly.instantiate` succeeds under that policy | `docs/ACTIVITY-SPIKE.md` A5 |
| Whether the corpus can be ranged | Whether `Range: bytes=0-63` returns `206` through a declared mapping | `docs/ACTIVITY-SPIKE.md` A6 |
| The root mapping entry, and the stable-host rule's exact form | The deploy host's real hostname, and the form of the per-deployment URLs it mints | A real deploy, plus the host's own documentation |
| The SDK handshake and the external-link flow | The Embedded App SDK's handshake and its external-link command, which `docs/SPEC.md` §9.4 records as TypeScript-only | The SDK's own published package and documentation, plus A1 and A4 |
| The font byte budget | The measured size of the shipped subsets | Subsetting and weighing the families `web/app.html` requests today |
| Which of the two positions in `docs/SPEC.md` on the launch question survives | `docs/SPEC.md` §9.4 warns that a text-channel launch is unconfirmed, §11 Q1 of the same document records it as resolved yes, and `docs/ACTIVITY-SPIKE.md` A1 records it unresolved. Nothing in the three says which of them owns the answer | A1, plus a decision on which document owns the launch answer |
