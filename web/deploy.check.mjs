#!/usr/bin/env node
// deploy.check.mjs — the gate on what `web/` is allowed to ship.
//
//   node web/deploy.check.mjs              check the real tree, exit 0 or 1
//   node web/deploy.check.mjs --selftest   drive every rule red on a synthetic bundle
//
// Plain Node, zero dependencies, no build step, no npm install (G9-04 REQ-019). It reads
// files and prints findings; it never writes and never opens a socket.
//
// The rules below are facts about THIS REPOSITORY — which strings are in which file — and
// nothing here rests on a claim about what Discord, Cloudflare or a browser does at runtime.
// The one place a platform verdict is required is the justification carried by each entry in
// web/url-mappings.json, and rule `spike-cites` refuses an entry whose cited row in
// docs/ACTIVITY-SPIKE.md has not been answered rather than guessing on its behalf.

import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { dirname, join, relative } from 'node:path';

const WEB = dirname(fileURLToPath(import.meta.url));
const ROOT = dirname(WEB);
const SPIKE_DOC = join(ROOT, 'docs', 'ACTIVITY-SPIKE.md');
const DECL = join(WEB, 'url-mappings.json');

/* ══════════ WHAT IS DEPLOYED ══════════
   REQ-018: the deployable bundle is one directory and the same bytes serve every door.
   The deploy set is web/ minus the exclusions below. Every exclusion is a rule, not a
   judgement call, so that the runbook and this file cannot drift apart:

     *.test.js   Playwright and plain-Node test drivers
     testenv.js  the browser/binary/corpus scaffolding those drivers need
     gate.mjs    the post-merge guard runner
     spike/      the probe page and its checker (the story forbids deploying it)
     fixtures/   log fixtures the tests read
     *.mjs       node-only tooling, which this file is one of
     url-mappings.json  the declaration this file reads, not a page asset
*/
const EXCLUDE_DIR = new Set(['spike', 'fixtures', 'node_modules', '.git']);
// url-mappings.json is repository configuration read BY the checker, not a page asset. It
// must stay outside the scanned bundle: an entry that could satisfy itself by appearing in
// the declaration would defeat the unreferenced-mapping direction of REQ-004 entirely.
const EXCLUDE_FILE = new Set(['testenv.js', 'url-mappings.json']);
const isExcluded = (rel) => {
  const parts = rel.split('/');
  if (parts.slice(0, -1).some((d) => EXCLUDE_DIR.has(d))) return true;
  const base = parts[parts.length - 1];
  return base.endsWith('.test.js') || base.endsWith('.mjs') || EXCLUDE_FILE.has(base);
};

// Line-based rules read text. A font file or a wasm module is bytes; it counts toward the
// hash and the byte total but is not scanned for URLs or key shapes, and the run says so.
const TEXT_EXT = /\.(html|js|css|json|svg|txt|md)$|^_headers$/;

function walk(dir, base = dir, out = []) {
  for (const name of readdirSync(dir).sort()) {
    const abs = join(dir, name);
    const rel = relative(base, abs).split('\\').join('/');
    const st = statSync(abs);
    if (st.isDirectory()) {
      if (!EXCLUDE_DIR.has(name)) walk(abs, base, out);
    } else {
      out.push({ rel, abs, size: st.size });
    }
  }
  return out;
}

function loadBundle(webDir) {
  const all = walk(webDir);
  return all.map((f) => {
    const excluded = isExcluded(f.rel);
    const text = !excluded && TEXT_EXT.test(f.rel.split('/').pop()) ? readFileSync(f.abs, 'utf8') : null;
    return { ...f, excluded, text };
  });
}

/* ══════════ THE RULES ══════════
   Each rule takes the in-memory bundle and returns findings. Pure functions of their
   arguments so --selftest can drive every one of them red without touching the disk.
   A finding is { rule, file, line, msg }; line 0 means "the file as a whole".            */

const deployed = (b) => b.filter((f) => !f.excluded);
const scannable = (b) => deployed(b).filter((f) => f.text !== null);
const lines = (f) => f.text.split(/\r?\n/);

// XML namespace URIs are identifiers, not fetches: `xmlns="http://www.w3.org/2000/svg"`
// names a grammar and no request is ever made for it. Exact matches only.
const NON_FETCHED = new Set([
  'http://www.w3.org/2000/svg',
  'http://www.w3.org/1999/xlink',
  'http://www.w3.org/1999/xhtml',
]);

const URL_RE = /https?:\/\/[^\s"'`)<>\\]+/g;

// EDGE-004 / REQ-004, first direction: a host in the bundle that the declaration does not name.
function referencedHosts(bundle) {
  const hits = [];
  for (const f of scannable(bundle)) {
    lines(f).forEach((text, i) => {
      for (const m of text.matchAll(URL_RE)) {
        const url = m[0].replace(/[.,;]+$/, '');
        let host;
        try { host = new URL(url).host; } catch { continue; }
        if ([...NON_FETCHED].some((n) => url.startsWith(n))) continue;
        hits.push({ file: f.rel, line: i + 1, host, url });
      }
    });
  }
  return hits;
}

function ruleDeclaredBothWays(bundle, decl) {
  const found = [];
  const hits = referencedHosts(bundle);
  const declared = new Set(decl.mappings.map((m) => m.target));
  for (const h of hits) {
    if (!declared.has(h.host)) {
      found.push({
        rule: 'undeclared-host', file: h.file, line: h.line,
        msg: `reaches ${h.host}, which web/url-mappings.json does not declare`,
      });
    }
  }
  // EDGE-005 / REQ-004, second direction: a declared host nothing reaches is a permitted
  // origin nobody is watching, and it fails exactly as loudly.
  const seen = new Set(hits.map((h) => h.host));
  for (const m of decl.mappings) {
    if (!seen.has(m.target)) {
      found.push({
        rule: 'unused-mapping', file: 'web/url-mappings.json', line: 0,
        msg: `declares "${m.prefix}" -> ${m.target}, which no deployed file references`,
      });
    }
  }
  return found;
}

// REQ-005 / REQ-020. Reports file and line and never the value.
const SECRET_SHAPES = [
  [/\bsk-ant-[A-Za-z0-9_-]{8,}/, 'an Anthropic-shaped API key'],
  [/\bsk-[A-Za-z0-9]{20,}/, 'an sk- prefixed API key'],
  [/\bghp_[A-Za-z0-9]{20,}/, 'a GitHub personal access token'],
  [/\bxox[abprs]-[A-Za-z0-9-]{10,}/, 'a Slack token'],
  [/\b[A-Za-z0-9_-]{24,28}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{27,40}\b/, 'a Discord bot token'],
  [/(api[_-]?key|client[_-]?secret|bot[_-]?token|private[_-]?key|access[_-]?token)\s*[:=]\s*["'`][^"'`\s]{16,}["'`]/i,
    'a secret assigned a long literal'],
];

function ruleNoSecrets(bundle) {
  const found = [];
  for (const f of scannable(bundle)) {
    lines(f).forEach((text, i) => {
      for (const [re, what] of SECRET_SHAPES) {
        if (re.test(text)) {
          found.push({ rule: 'secret-shaped', file: f.rel, line: i + 1, msg: `${what} (value withheld)` });
          break;
        }
      }
    });
  }
  return found;
}

/* REQ-007: the observable form of REQ-006. The in-page resolver is gone when a search over
   the deployable bundle for the API host, for the request-header name that carried the key
   and for the key field's own identifier returns zero hits. Kept here rather than in a
   comment so it stays true after the deletion lands.                                       */
const DELETED_RESOLVER = [
  ['api.anthropic.com', 'the third-party API host'],
  ['x-api-key', 'the request-header name that carried the key'],
  ['BRAIN.key', "the key field's own identifier"],
];

function ruleResolverDeleted(bundle) {
  const found = [];
  for (const f of scannable(bundle)) {
    lines(f).forEach((text, i) => {
      for (const [needle, what] of DELETED_RESOLVER) {
        if (text.includes(needle)) {
          found.push({
            rule: 'in-page-key-path', file: f.rel, line: i + 1,
            msg: `carries ${what}; REQ-006 deletes the in-page API resolver, it is not gated`,
          });
        }
      }
    });
  }
  return found;
}

/* REQ-011 / EDGE-008. A forward rule: the bundle has zero of all three today and the rule
   exists so it still has zero when the first quest browser lands. Inside the Activity a
   `target="_blank"` silently does nothing useful, and every item and zone name is a link. */
const LINK_RULES = [
  [/window\.open\s*\(/, 'raw-window-open', 'calls window.open(; route it through activity.js openExternal'],
  [/target\s*=\s*["']_blank["']/, 'raw-target-blank', 'sets target="_blank"; route it through activity.js openExternal'],
  // An anchor specifically. A <link rel=stylesheet> carries an href too and is governed by
  // the declared-host rule instead; REQ-011 is about where a click takes the player.
  [/<a\s[^>]*href\s*=\s*["'][a-zA-Z][a-zA-Z0-9+.-]*:/, 'raw-external-href', 'anchors an href beginning with a scheme; route it through activity.js openExternal'],
];

function ruleExternalLinks(bundle) {
  const found = [];
  for (const f of scannable(bundle)) {
    lines(f).forEach((text, i) => {
      for (const [re, rule, msg] of LINK_RULES) {
        if (re.test(text)) found.push({ rule, file: f.rel, line: i + 1, msg });
      }
    });
  }
  return found;
}

/* REQ-015 / EDGE-006. A mapping pointed at a per-deployment URL produces, after the next
   rollback, an Activity that loads a white page with no console — the failure mode with the
   fewest available diagnostics in the product. This is a shape refusal over the string in
   the declaration, not a claim about how any host mints its URLs: it refuses a target that
   carries a per-deployment marker, and the runbook records what a stable target looks like. */
function rulePreviewTarget(decl) {
  const found = [];
  for (const m of decl.mappings) {
    const labels = String(m.target).split('.');
    const why = /^[0-9a-f]{7,}$/i.test(labels[0]) ? 'a hex-shaped leading label'
      : /preview|staging|--/.test(m.target) ? 'a per-deployment marker'
      : labels.length > 3 ? 'more labels than a stable apex plus one subdomain'
      : null;
    if (why) {
      found.push({
        rule: 'preview-target', file: 'web/url-mappings.json', line: 0,
        msg: `target ${m.target} looks like a per-deployment URL (${why}); REQ-015 requires a stable hostname`,
      });
    }
  }
  return found;
}

// REQ-018 / AC-014: the deploy set carries no test file and no spike artifact.
function ruleDeploySetClean(bundle) {
  return deployed(bundle)
    .filter((f) => /\.test\.js$/.test(f.rel) || f.rel.startsWith('spike/') || f.rel.startsWith('fixtures/'))
    .map((f) => ({
      rule: 'test-in-deploy', file: f.rel, line: 0,
      msg: 'a test file or spike artifact is inside the deploy set',
    }));
}

/* REQ-003 / REQ-001 / AC-011. Every entry names the spike question that justifies it, that
   id exists in docs/ACTIVITY-SPIKE.md, and the row it names has been answered. A mapping
   justified by a row reading NOT-TESTED is a guess wearing a citation, which is the exact
   thing REQ-001 forbids, so it fails here rather than at load time in Discord.            */
function spikeRows(md) {
  const rows = new Map();
  const heads = [...md.matchAll(/^## (A\d+)\b(.*)$/gm)];
  heads.forEach((h, i) => {
    const start = h.index + h[0].length;
    const end = i + 1 < heads.length ? heads[i + 1].index : md.length;
    const body = md.slice(start, end);
    const verdicts = [...body.matchAll(/verdict:\s*([A-Z-]+)/g)].map((m) => m[1]);
    rows.set(h[1], { verdicts, answered: verdicts.length > 0 && verdicts.every((v) => v !== 'NOT-TESTED') });
  });
  return rows;
}

function ruleSpikeCites(decl, rows) {
  const found = [];
  for (const m of decl.mappings) {
    const id = m.question;
    if (!id) {
      found.push({ rule: 'spike-cites', file: 'web/url-mappings.json', line: 0,
        msg: `entry "${m.prefix}" names no question id from docs/ACTIVITY-SPIKE.md` });
      continue;
    }
    if (!rows.has(id)) {
      found.push({ rule: 'spike-cites', file: 'web/url-mappings.json', line: 0,
        msg: `entry "${m.prefix}" cites ${id}, which docs/ACTIVITY-SPIKE.md does not carry` });
      continue;
    }
    if (!rows.get(id).answered) {
      found.push({ rule: 'spike-cites', file: 'web/url-mappings.json', line: 0,
        msg: `entry "${m.prefix}" cites ${id}, whose rows in docs/ACTIVITY-SPIKE.md read NOT-TESTED; REQ-001 calls an unbacked value a guess` });
    }
  }
  return found;
}

function readDeclaration(path) {
  if (!existsSync(path)) {
    return { decl: null, error: { rule: 'no-declaration', file: 'web/url-mappings.json', line: 0,
      msg: 'the declared URL mapping does not exist; REQ-003 makes it the source of truth in the repository' } };
  }
  let raw;
  try { raw = JSON.parse(readFileSync(path, 'utf8')); }
  catch (e) {
    return { decl: null, error: { rule: 'no-declaration', file: 'web/url-mappings.json', line: 0,
      msg: `is not valid JSON: ${e.message}` } };
  }
  if (!Array.isArray(raw.mappings)) {
    return { decl: null, error: { rule: 'no-declaration', file: 'web/url-mappings.json', line: 0,
      msg: 'has no "mappings" array' } };
  }
  return { decl: raw, error: null };
}

export function check(bundle, decl, rows) {
  return [
    ...ruleDeclaredBothWays(bundle, decl),
    ...ruleNoSecrets(bundle),
    ...ruleResolverDeleted(bundle),
    ...ruleExternalLinks(bundle),
    ...rulePreviewTarget(decl),
    ...ruleDeploySetClean(bundle),
    ...ruleSpikeCites(decl, rows),
  ];
}

/* ══════════ THE RUN ══════════ */

function report(findings) {
  for (const f of findings) {
    const where = f.line ? `${f.file}:${f.line}` : f.file;
    console.error(`  ${where}: [${f.rule}] ${f.msg}`);
  }
}

function main() {
  const bundle = loadBundle(WEB);
  const ship = deployed(bundle);
  const { decl, error } = readDeclaration(DECL);

  if (error) {
    console.error('deploy.check: FAIL');
    report([error]);
    process.exit(1);
  }

  const rows = existsSync(SPIKE_DOC) ? spikeRows(readFileSync(SPIKE_DOC, 'utf8')) : new Map();
  const findings = check(bundle, decl, rows);

  // AC-001's three printed things, read back into the end-to-end note.
  const declaredList = decl.mappings.map((m) => m.target).sort();
  const referencedList = [...new Set(referencedHosts(bundle).map((h) => h.host))].sort();
  const deployBytes = ship.reduce((n, f) => n + f.size, 0);
  const wholeDirBytes = bundle.reduce((n, f) => n + f.size, 0);
  const hash = createHash('sha256');
  for (const f of ship.slice().sort((a, b) => (a.rel < b.rel ? -1 : 1))) {
    hash.update(f.rel); hash.update(readFileSync(f.abs));
  }

  console.log(`declared hosts   (${declaredList.length}): ${declaredList.join(', ') || '(none)'}`);
  console.log(`referenced hosts (${referencedList.length}): ${referencedList.join(', ') || '(none)'}`);
  console.log(`deploy set       : ${ship.length} files, ${deployBytes} bytes`);
  console.log(`web/ whole       : ${bundle.length} files, ${wholeDirBytes} bytes (difference is the excluded set)`);
  console.log(`deploy sha256    : ${hash.digest('hex')}`);

  if (findings.length) {
    console.error(`\ndeploy.check: FAIL — ${findings.length} finding(s)`);
    report(findings);
    process.exit(1);
  }
  console.log('\ndeploy.check: OK');
}

/* ══════════ THE RULES SEEN RED ══════════
   A check that has never been seen to fail proves nothing about the deploy it passed. Each
   case below is one of G9-04's acceptance criteria driven against a synthetic bundle: the
   clean bundle produces no finding, the broken one produces exactly the named rule.       */

function selftest() {
  const rows = new Map([['A4', { verdicts: ['YES'], answered: true }], ['A9', { verdicts: ['NOT-TESTED'], answered: false }]]);
  const file = (rel, text) => ({ rel, abs: rel, size: Buffer.byteLength(text), excluded: false, text });
  const clean = [file('app.html', '<a href="#cart">cart</a>\nfetch("corpus.grim")\n')];
  const emptyDecl = { mappings: [] };
  const cases = [];
  const kase = (name, rule, bundle, decl, r = rows) => cases.push({ name, rule, bundle, decl, rows: r });

  kase('AC-001 clean bundle, empty declaration', null, clean, emptyDecl);
  kase('AC-002 undeclared external host', 'undeclared-host',
    [file('app.html', 'x\nconst u = "https://evil.example.com/x";\n')], emptyDecl);
  kase('AC-003 declared entry nothing references', 'unused-mapping',
    clean, { mappings: [{ prefix: '/wiki', target: 'wiki.example.com', question: 'A4' }] });
  kase('AC-005 key-shaped string', 'secret-shaped',
    [file('app.html', 'const k = "sk-ant-api03-AAAAAAAABBBBBBBBCCCCCCCC";\n')], emptyDecl);
  kase('AC-004/007 the deleted API host', 'in-page-key-path',
    [file('app.html', 'url:"https://api.anthropic.com/v1/messages"\n')], emptyDecl);
  kase('AC-007 window.open', 'raw-window-open', [file('app.html', 'window.open(u)\n')], emptyDecl);
  kase('AC-007 target=_blank', 'raw-target-blank', [file('app.html', '<a target="_blank">w</a>\n')], emptyDecl);
  kase('AC-007 scheme-prefixed href', 'raw-external-href',
    [file('app.html', '<a href="https://wiki.example.com/Bar">Bar</a>\n')], emptyDecl);
  kase('AC-012 preview-shaped target', 'preview-target',
    [file('app.html', 'https://a1b2c3d4e5.grimoire.pages.dev/x\n')],
    { mappings: [{ prefix: '/', target: 'a1b2c3d4e5.grimoire.pages.dev', question: 'A4' }] });
  kase('AC-014 a test file inside the deploy set', 'test-in-deploy',
    [file('app.test.js', 'assert(1)\n')], emptyDecl);
  kase('AC-011 an entry citing an id the spike does not carry', 'spike-cites',
    [file('app.html', 'https://cdn.example.com/x\n')],
    { mappings: [{ prefix: '/cdn', target: 'cdn.example.com', question: 'A42' }] });
  kase('REQ-001 an entry citing a NOT-TESTED row', 'spike-cites',
    [file('app.html', 'https://cdn.example.com/x\n')],
    { mappings: [{ prefix: '/cdn', target: 'cdn.example.com', question: 'A9' }] });

  let failed = 0;
  for (const c of cases) {
    const got = check(c.bundle, c.decl, c.rows).map((f) => f.rule);
    const ok = c.rule === null ? got.length === 0 : got.includes(c.rule);
    console.log(`${ok ? 'ok  ' : 'FAIL'} ${c.name} -> ${got.length ? got.join(',') : '(no findings)'}`);
    if (!ok) failed++;
  }
  console.log(`\n${cases.length - failed} of ${cases.length} rule cases behaved`);
  process.exit(failed ? 1 : 0);
}

if (process.argv.includes('--selftest')) selftest();
else main();
