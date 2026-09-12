// Shared harness plumbing for app.test.js and bench.test.js: an ephemeral port, and an output
// directory unique to this run.
//
// WHY THIS EXISTS. Both suites used to hardcode a port (:8137 and :8134) and write screenshots
// into /tmp. The Gnomish harness runs board items in PARALLEL git worktrees, so five copies of
// these suites start at once. Exactly one won the bind and the other four died on
// `EADDRINUSE :::8137` before a single check ran, and all five raced for the same /tmp/bench.png.
// A guard that fails when it runs concurrently is worse than no guard: it fails CORRECT work and
// burns every retry attempt the harness has.
//
// Nothing here is test-specific — it is the contention fix, in one place, so the two suites
// cannot drift apart on it.

const fs = require('fs');
const path = require('path');

/* Bind port 0 and read the port the OS assigned back off the listening server.
 *
 * The OS picks a free port and hands it over IN THE BIND — there is no window between choosing
 * and owning it. Do NOT replace this with "open a socket, note the port, close it, bind that
 * port": that is the same race with extra steps, because another worktree can take the port in
 * the gap. Callers must await this before navigating; `server.address()` is null until the
 * 'listening' event.
 *
 * Bound to 127.0.0.1 rather than the wildcard the old code used. These suites only ever fetch
 * over loopback, and the old `listen(PORT)` published a directory server on every interface. */
function listen(server) {
  return new Promise((resolve, reject) => {
    const onError = (e) => reject(e);
    server.once('error', onError);
    server.listen(0, '127.0.0.1', () => {
      server.removeListener('error', onError);
      // A bind failure is now impossible, but a later socket error must not be an unhandled
      // 'error' event — that is what turned the old collision into a bare stack trace.
      server.on('error', (e) => console.error('  server error after listen:', e.message));
      resolve(server.address().port);
    });
  });
}

/* Where this run's artefacts go.
 *
 * Under the WORKTREE's own target/, which .gitignore already excludes — the harness refuses to
 * start on a dirty tree and gates on the diff, so a screenshot dropped anywhere tracked would
 * fail the very work it is meant to verify. The leaf is pid + start time, so two runs in the same
 * worktree do not overwrite each other either. GRIMOIRE_TEST_OUT lets web/gate.mjs put both
 * suites' shots in one directory per gate run; it is created lazily, on the first shot. */
const OUT = process.env.GRIMOIRE_TEST_OUT
  || path.join(__dirname, '..', 'target', 'web-tests', `${process.pid}-${Date.now().toString(36)}`);

async function shot(page, name, opts = {}) {
  fs.mkdirSync(OUT, { recursive: true });
  const file = path.join(OUT, name);
  await page.screenshot({ path: file, ...opts });
  return file;
}

module.exports = { listen, shot, OUT };
