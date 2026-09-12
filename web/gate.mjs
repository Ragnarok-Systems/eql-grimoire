#!/usr/bin/env node
/* The web half of the per-item gate.
 *
 * WHY THIS EXISTS. `web/app.test.js` and `web/bench.test.js` need two things that are NOT in the
 * repository: the `grimoire` binary and `web/corpus.grim`. Both are gitignored (`/target`, `*.grim`),
 * so a fresh `git worktree` — which is exactly where the harness runs every item — has neither, and
 * both tests died spawning a binary that was not there. Wiring the tests directly as guards made all
 * 52 items fail by construction. This script supplies the prerequisites first, then runs them.
 *
 * It prefers a binary that already exists (the gate's own `cargo build --workspace` produces the
 * debug one) over building a second time, because every worktree carries its own `target/` and a
 * release build per item is minutes of wall clock for no extra signal. It does NOT share a target
 * directory between worktrees: doing that links one worker's rlibs into another's build.
 *
 *   node web/gate.mjs
 *
 * Exit 0 = both suites passed.
 */
import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const WEB = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(WEB, "..");
const EXE = process.platform === "win32" ? ".exe" : "";
const say = (m) => console.log("[web-gate] " + m);

/* 1. A binary. Release if a hand run left one, else the debug one the gate already built, else
 *    build debug — never release, for the wall-clock reason above. */
function binary() {
  for (const p of [join(ROOT, "target", "release", "grimoire" + EXE),
                   join(ROOT, "target", "debug", "grimoire" + EXE)]) {
    if (existsSync(p)) { say("binary " + p); return p; }
  }
  say("no binary found, building debug");
  const b = spawnSync("cargo", ["build", "--workspace"], { cwd: ROOT, stdio: "inherit", shell: true });
  if (b.status !== 0) { say("cargo build failed"); process.exit(1); }
  const p = join(ROOT, "target", "debug", "grimoire" + EXE);
  if (!existsSync(p)) { say("build reported success but " + p + " is absent"); process.exit(1); }
  say("binary " + p);
  return p;
}

/* 2. A corpus. Cut from `data/` with `grimoire corpus`, using the binary from step 1. */
function corpus(bin) {
  const out = join(WEB, "corpus.grim");
  if (existsSync(out)) { say("corpus present"); return out; }
  say("cutting corpus");
  mkdirSync(WEB, { recursive: true });
  try {
    execFileSync(bin, ["corpus", out, "--from", join(ROOT, "data"),
                       "--trivials", join(ROOT, "data", "trivials-measured.csv")],
                 { cwd: ROOT, stdio: "inherit" });
  } catch { say("corpus cut failed"); process.exit(1); }
  /* The cut is CONTENT ADDRESSED: it ignores the name it was handed and writes
     corpus-<hash>.grim. app.html:3504 fetches the bare "corpus.grim", so the artifact is copied
     to that name here. Doing it in the gate rather than renaming by hand is the whole point -
     a worktree has no corpus at all and nobody is standing there to rename one. */
  if (!existsSync(out)) {
    const cut = readdirSync(WEB).filter((n) => /^corpus-.*.grim$/.test(n)).sort();
    if (!cut.length) { say("corpus cut reported success but produced no corpus-*.grim"); process.exit(1); }
    copyFileSync(join(WEB, cut[cut.length - 1]), out);
    say("copied " + cut[cut.length - 1] + " -> corpus.grim");
  }
  if (!existsSync(out)) { say("corpus still absent after copy"); process.exit(1); }
  return out;
}

const bin = binary();
corpus(bin);

/* 3. Both suites, with the binary handed to them explicitly.
 *
 * They also get one output directory for this gate run. Both suites take a screenshot, and both
 * used to write it into /tmp - a path shared by every worktree on the machine. The harness runs
 * items in PARALLEL worktrees, so that was five runs writing one file. Under target/, which
 * .gitignore excludes, so the shots cannot dirty the tree the harness gates the diff on. */
const OUT = join(ROOT, "target", "web-tests", `${process.pid}-${Date.now().toString(36)}`);
mkdirSync(OUT, { recursive: true });
say("artefacts in " + OUT);

let failed = 0;
for (const t of ["app.test.js", "bench.test.js"]) {
  say("running " + t);
  const r = spawnSync(process.execPath, [join(WEB, t)],
    { cwd: ROOT, stdio: "inherit",
      env: { ...process.env, GRIMOIRE_BIN: bin, GRIMOIRE_TEST_OUT: OUT } });
  if (r.status !== 0) { say(t + " FAILED"); failed++; }
}
if (failed) { say(failed + " suite(s) failed"); process.exit(1); }
say("both suites passed");
