# RELEASING

How a Grimoire release is cut, what has to exist before the first one, and what the pipeline will
refuse to do.

Everything here is done by `.github/workflows/release.yml`, which runs on `windows-latest` and is
triggered by pushing a version tag. It is the only thing in this repository allowed to write to
the update bucket. `ci.yml` is a different job on a different runner and builds no Windows binary
at all.

---

## 0. The one-way door: the key comes before the first release

**A released binary trusts the public key it was compiled with, and nothing else, forever.** There
is no channel by which a shipped binary can be taught a new key, because teaching it one would be
an update, and an update is the thing the key authorises.

So, in this order, before one binary reaches one stranger:

1. Generate the keypair, on a machine that is not CI:

   ```
   minisign -G -W -p grimoire.pub -s grimoire.key
   ```

   `-W` makes a key with no password on purpose. The workflow has no terminal to type a password
   into, and putting the key and its password in two GitHub secrets is not two factors, it is one
   factor with an extra place to leak from. The GitHub secret is the protection.

2. Paste line 2 of `grimoire.pub`, the base64 key that starts `RW`, into `KEYS` in
   `crates/grimoire-desktop/src/updater/verify.rs`, and commit it. It is a public key. It belongs
   in the source in the clear.

3. Put the entire contents of `grimoire.key` into the `MINISIGN_SECRET_KEY` repository secret, and
   a second copy somewhere offline that survives losing the GitHub account. A password manager
   entry and a printed copy. Then delete `grimoire.key` from the machine that made it.

4. Only now tag a release.

A binary shipped before step 2 can never be auto-updated: its owners have to be told to download a
new installer by hand, and nothing in the app can tell them so. If the private key is lost after
step 4, every binary already in the wild is permanently unupdatable and the only remedy is the
same manual reinstall.

The workflow enforces this. It refuses to run if `updater/verify.rs` is missing, if `KEYS` holds
no real key, or if the private key in the secret does not match one of the keys in `KEYS`. That
last check is the one that matters most: signing with a key the field does not trust produces a
pipeline that is green at every step and a fleet that refuses every update.

`KEYS` is a list with one entry, not a single string, and it has to stay a list. Rotation only
works forwards: release N+1 must carry the new key while still being signed by the old one, so
every key the field might hold has to be acceptable to the signer.

### Step 2 is no longer enforced by this document alone

Until recently the rule above was a comment and a paragraph, in a tree whose doctrine is that a
comment saying so is a comment. The key-agreement guard did not catch the case that actually
happens, which is not an attack but a click: whoever cuts the first release generates a real
keypair, puts the secret in `MINISIGN_SECRET_KEY`, forgets step 2, and the workflow says nothing,
because the development anchor generated on 2026-09-11 is a real minisign key that the secret
still has to match. Three mechanical guards now stand where that paragraph was.

* **The build.** A release-profile build that still carries the development anchor in `KEYS` does
  not compile. `cargo test` and `cargo clippy` are debug builds, so ordinary work is unaffected;
  `cargo run --release` on this machine needs `GRIMOIRE_DEV_RELEASE=1`, which CI never sets.
* **The pipeline.** The release workflow reads the `KEYS` declaration out of `verify.rs` and
  refuses by name if the development anchor is still in it, whatever any environment variable
  says. Its error message points back at this section.
* **The screen.** While the development anchor is the trust root, the UPDATES section of the
  Settings screen says so in words, so the fact is visible to whoever is running the build rather
  than buried in a doc comment.

### The manifest now expires, and the publisher sets the window

`MANIFEST_LIFETIME_DAYS` in the workflow's `env:` block (45 today) is added to `published` and
written into the signed manifest as `expires`. Clients refuse a manifest whose expiry has passed,
which is the half of the replay defence that works on a machine with nothing stored: a fresh
install, a reinstall, a lost `update\state.json` and the first check after a channel switch all
have no `published` floor at all, and those are exactly the machines a replayed envelope reaches.
Adding the field is why the manifest format went from 1 to 2. A channel that publishes nothing for
longer than the window starts telling its clients the manifest has expired, which is the intended
behaviour and is a different sentence from "you are up to date".

---

## 1. Secrets that must exist

All four are **repository** secrets (Settings, Secrets and variables, Actions, New repository
secret). The workflow checks all four before it does anything else and names the ones that are
missing.

| Secret | What it holds | Where it comes from |
|---|---|---|
| `MINISIGN_SECRET_KEY` | the entire contents of `grimoire.key`, both lines, generated with `minisign -G -W` so it carries no password | section 0 above |
| `R2_ACCOUNT_ID` | the Cloudflare account id, which is the first label of the S3 endpoint `https://<account id>.r2.cloudflarestorage.com` | Cloudflare dashboard, R2 overview |
| `R2_ACCESS_KEY_ID` | the Access Key ID of an R2 API token | Cloudflare dashboard, R2, Manage API tokens |
| `R2_SECRET_ACCESS_KEY` | the Secret Access Key shown once when that token is created | same place, and it is shown exactly once |

The R2 token should be scoped to **Object Read and Write on the `grimoire-updates` bucket only**.
Nothing in this pipeline needs bucket creation, bucket listing, or access to any other bucket, and
a token that can do more is a token that can do more when it leaks.

The bucket name (`grimoire-updates`) and the public host (`https://updates.ragnarok.systems`) are
not secrets. They are printed in the manifest that every client downloads. They live as plain
`env:` values at the top of the workflow.

`GITHUB_TOKEN` is provided by Actions and is used only to create the GitHub Release. The job
declares `permissions: contents: write` for that and nothing else.

---

## 2. Cutting a release

1. Decide the version and put it in `Cargo.toml`, under `[workspace.package]`. That one value
   reaches the binary through `env!("CARGO_PKG_VERSION")`, so the app and the manifest cannot
   disagree about it. Commit.

2. Tag that commit and push the tag:

   ```
   git tag v0.2.0
   git push origin v0.2.0
   ```

   A plain version goes to the **stable** channel. A semver pre-release, `v0.2.0-beta.1`, goes to
   the **beta** channel and is marked as a prerelease on GitHub. Nothing else selects the channel,
   and the channel is written inside the signed manifest so a client can catch a beta manifest
   served at the stable path.

3. Watch the run. It builds, runs the gate on Windows, scans the binary, signs, self-checks, then
   publishes to R2 and to a GitHub Release.

4. When it finishes, `https://updates.ragnarok.systems/grimoire/stable/latest.json` is the new
   manifest and the release page carries the exe, its `.minisig`, the manifest and its `.minisig`.

To rehearse without publishing, use **Run workflow** from the Actions tab. A manual run defaults
to a dry run, and a manual run from anything that is not a version tag is forced to a dry run no
matter what the input says.

### What is published, and where

```
grimoire-updates bucket                                     public URL prefix
  grimoire/<channel>/<version>/grimoire-desktop-<version>-windows-x86_64.exe
  grimoire/<channel>/<version>/grimoire-desktop-<version>-windows-x86_64.exe.minisig
  grimoire/<channel>/<version>/manifest.json
  grimoire/<channel>/<version>/manifest.json.minisig
  grimoire/<channel>/latest.json                            the pointer, written last
```

Everything under a version prefix is immutable and cached forever. `latest.json` is the only
object that ever changes, and it is uploaded with `no-cache`.

---

## 3. What the pipeline refuses to do

Each of these stops the run. None of them can be argued with by re-running.

- **Publish before the key exists**, or with a private key that does not match the public key
  compiled into the app.
- **Publish a version whose tag and `Cargo.toml` disagree.**
- **Publish a version that has already been published.** Versioned objects are immutable so that a
  published sha256 stays true. Cut a new version instead.
- **Publish a binary carrying a build machine path.** The release build remaps `CARGO_HOME`,
  `RUSTUP_HOME` and the workspace out of the binary, and the job then scans the bytes for what is
  left. Measured on 2026-09-11: without the remap the binary carries 620 copies of the build
  account's user name, and with it, none. The scan also requires the remapped prefix to be
  present, so it cannot pass by finding nothing at all.
- **Publish a manifest whose artifacts are missing, the wrong size, the wrong hash, or unsigned.**
  Before anything is uploaded the job re-reads the envelope it just assembled, exactly as a client
  would, and checks every artifact it names against the file on disk.
- **Point `latest.json` at bytes that are not there.** The artifact is uploaded first and read
  back from the public host, and only then does the pointer move.

If the scan stops a release because of a path you believe is legitimate, the fix is either a remap
that covers it or an allowance in the scan step that names the source file the string comes from.
There is exactly one allowance today: the default EverQuest log folder,
`C:\Users\Public\Daybreak Game Company\Installed Games\EverQuest Legends\Logs`, which is a literal
in `crates/grimoire-desktop/src/ingest.rs`.

---

## 4. Withdrawing or rolling back a release

No rebuild is needed for either, because every version keeps its own immutable copy of its
manifest.

- **Roll back to an older version:** take `grimoire/<channel>/<older version>/manifest.json`, put
  it in a new envelope with a `rollback` object naming the version being moved away from, sign it
  and upload it as `latest.json`. A client only ever installs a version lower than the one it is
  running when the manifest says so explicitly.
- **Withdraw a bad release:** publish a manifest listing that version in `yanked`. A client that
  has already taken it falls back at the next launch.

Both of those are operator actions against the bucket with the private key, not workflow runs.

---

## 5. Verifying a download by hand

```
minisign -Vm grimoire-desktop-0.2.0-windows-x86_64.exe -P <the key from KEYS>
```

To check the manifest itself, save the `manifest` string from `latest.json` to `manifest.json` and
the `signature` string to `manifest.json.minisig`, then run the same command against it. The
signed bytes are exactly the bytes of that string, which is why the manifest travels as a string
inside the envelope rather than as a re-serialisable object.

---

## 6. Known limits, stated rather than discovered later

- **The release job is the first time this repository has ever compiled its Windows-only code in
  CI.** `ci.yml` runs on `ubuntu-latest`, which is why `wry`, `webview2-com` and `windows-core`
  sit behind `[target.'cfg(windows)'.dependencies]`. The gate inside the release job runs `cargo
  fmt`, `cargo clippy` and `cargo test` on Windows. Expect the first run to find things.
- **The gate runs with `GRIMOIRE_NO_DATA=1`.** The wiki corpus under `crates/grimoire-desktop/data`
  is deliberately not committed, and the eleven tests that read it fail rather than skip when it is
  absent, so the flag is how the runner says out loud that it has no corpus.
- **CI cannot build a data bundle**, for the same reason: the corpus is not in the repository, so
  the runner has no copy to package. The manifest this workflow publishes names one artifact, the
  app. A data bundle needs a publisher that has the corpus.
- **The one object in the bucket today is a placeholder.** `grimoire/stable/latest.json` currently
  holds a smoke-test document with `"version": "0.0.0-smoketest"` and no artifacts. It has no
  envelope and no signature, so a client built to the spec refuses it at the first gate, which is
  the correct behaviour. The first real release overwrites it.
