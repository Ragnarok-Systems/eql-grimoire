#!/usr/bin/env node
/* Checks eql-grimoire/docs/ACTIVITY-SPIKE.md against the shape REQ-011 through REQ-017 fix.
 *
 * Zero dependencies, plain Node. Reads the document as text and validates structure only — it
 * cannot and does not check that a verdict is true, only that the document is shaped so a
 * verdict from someone who was there can be told apart from a rumour: eight questions, four
 * platform rows each, a verdict from the closed set, a reason on anything that isn't YES, a
 * decision line, an observed block, and a header that isn't hiding a secret.
 *
 *   node web/spike/check-result.mjs [path-to-doc]
 *
 * Exit 0 = the document is well-formed. Exit 1 = it is not, and the last line named why.
 */
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DEFAULT_DOC = resolve(HERE, "..", "..", "docs", "ACTIVITY-SPIKE.md");
const docPath = process.argv[2] ? resolve(process.argv[2]) : DEFAULT_DOC;

const QUESTIONS = ["A1 LAUNCH", "A2 FILE-INPUT", "A3 DROP-PASTE", "A4 SANDBOX", "A5 WASM", "A6 RANGE", "A7 FSA", "A8 LOOPBACK"];
const PLATFORMS = ["desktop-app", "web", "ios", "android"];
const VERDICTS = new Set(["YES", "NO", "PARTIAL", "ERROR", "NOT-TESTED"]);
const HEADER_FIELDS = ["date", "desktop-app-version", "web-version", "ios-version", "android-version", "application-id", "deploy-url"];

const errors = [];
function fail(msg) { errors.push(msg); }

/* REQ-016: refuse anything shaped like a bot token, a client secret or an API key, without
 * echoing what was found back into the failure message. */
const SECRET_PATTERNS = [
  { name: "bot-token-shaped string", re: /[A-Za-z0-9_-]{20,30}\.[A-Za-z0-9_-]{6,7}\.[A-Za-z0-9_-]{27,40}/ },
  { name: "OpenAI-style secret key", re: /\bsk-[A-Za-z0-9]{20,}\b/ },
  { name: "assignment shaped like a secret", re: /\b(?:client[_-]?secret|api[_-]?key|access[_-]?token|bot[_-]?token)\b\s*[:=]\s*['"]?[A-Za-z0-9_\-.]{16,}/i },
];
/* A Discord application/snowflake id is public and is 15-21 ASCII digits. Anything else in that
 * header field is treated as a value shaped like a secret placed where a public id belongs. */
const SNOWFLAKE = /^\d{15,21}$/;

let text;
try {
  text = readFileSync(docPath, "utf8");
} catch (e) {
  console.error("check-result: cannot read " + docPath + ": " + e.message);
  process.exit(1);
}

for (const p of SECRET_PATTERNS) {
  if (p.re.test(text)) fail("document contains a " + p.name + " (redacted; not echoed)");
}

const lines = text.split(/\r\n|\n/);

/* ── Header block: every line before the first "## " heading. ── */
const firstHeadingIdx = lines.findIndex((l) => /^##\s+/.test(l));
const headerLines = firstHeadingIdx === -1 ? lines : lines.slice(0, firstHeadingIdx);
const header = {};
for (const l of headerLines) {
  const m = /^([a-z0-9-]+):\s*(.*)$/i.exec(l.trim());
  if (m) header[m[1].toLowerCase()] = m[2].trim();
}
for (const f of HEADER_FIELDS) {
  if (!header[f]) fail("header field is missing or empty: " + f);
  else if (f === "application-id" && !SNOWFLAKE.test(header[f]) && header[f] !== "NOT-TESTED" && !/^NOT-TESTED\b/.test(header[f])) {
    for (const p of SECRET_PATTERNS) {
      if (p.re.test(header[f])) fail("header field application-id holds a value shaped like a secret, not a public application id (redacted; not echoed)");
    }
  }
}

/* ── Per-question sections. ── */
const sectionStarts = [];
lines.forEach((l, i) => {
  const m = /^##\s+(.+?)\s*$/.exec(l);
  if (m) sectionStarts.push({ idx: i, title: m[1].trim() });
});

const seenTitles = new Set();
for (const s of sectionStarts) {
  if (QUESTIONS.includes(s.title)) {
    if (seenTitles.has(s.title)) fail("question appears more than once: " + s.title);
    seenTitles.add(s.title);
  }
}
for (const q of QUESTIONS) {
  if (!seenTitles.has(q)) fail("question missing entirely: " + q);
}

let verdictCellCount = 0;
let decisionCount = 0;
let observedCount = 0;

for (let si = 0; si < sectionStarts.length; si++) {
  const { idx, title } = sectionStarts[si];
  if (!QUESTIONS.includes(title)) continue; // a non-question heading (e.g. a title) is not this loop's business
  const end = si + 1 < sectionStarts.length ? sectionStarts[si + 1].idx : lines.length;
  const body = lines.slice(idx + 1, end);

  const seenPlatforms = new Set();
  for (const l of body) {
    const rowM = /^-\s*platform:\s*([a-z-]+)\s*\|\s*verdict:\s*([A-Z-]+)\s*\|\s*reason:\s*(.*)$/.exec(l.trim());
    if (!rowM) continue;
    const [, platform, verdict, reason] = rowM;
    verdictCellCount++;
    if (!PLATFORMS.includes(platform)) {
      fail(title + ": row names an unknown platform: " + platform);
      continue;
    }
    if (seenPlatforms.has(platform)) fail(title + ": platform " + platform + " has more than one row");
    seenPlatforms.add(platform);
    if (!VERDICTS.has(verdict)) {
      fail(title + " / " + platform + ": verdict is not in the closed set: " + verdict);
    }
    if (verdict !== "YES" && VERDICTS.has(verdict) && !reason.trim()) {
      fail(title + " / " + platform + ": verdict " + verdict + " carries no reason");
    }
  }
  for (const p of PLATFORMS) {
    if (!seenPlatforms.has(p)) fail(title + ": missing platform row: " + p);
  }

  const decisionLines = body.filter((l) => /^decision:\s*\S/.test(l.trim()));
  decisionCount += decisionLines.length;
  if (decisionLines.length === 0) fail(title + ": missing a decision: line");

  const observedLines = body.filter((l) => /^observed:\s*\S/.test(l.trim()));
  observedCount += observedLines.length;
  if (observedLines.length === 0) fail(title + ": missing at least one observed: block");
}

const questionsInspected = sectionStarts.filter((s) => QUESTIONS.includes(s.title)).length;

if (errors.length) {
  console.error("check-result: FAILED (" + docPath + ")");
  for (const e of errors) console.error("  - " + e);
  process.exit(1);
}

console.log(
  "check-result: OK — questions inspected: " + questionsInspected +
  ", verdict cells inspected: " + verdictCellCount +
  ", decision: lines found: " + decisionCount +
  ", observed: blocks found: " + observedCount
);
