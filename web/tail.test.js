// Plain-Node test for web/tail.js. No browser, no Playwright, no network, no built binary and
// no harness of its own — the same bar `web/engine.test.js` already meets next door.
//
//   node web/tail.test.js
//
// It drives the pure half over hand-built file objects and over two byte fixtures, and it
// drives the shim over a hand-built `win` whose handles are plain objects. Which inputs are
// real bytes and which are the test's own invention is printed beside every case that has
// both, because a fixture the test authored itself proves only that the test agrees with
// itself.

const assert = require('assert');
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const Tail = require(path.join(__dirname, 'tail.js'));

const FIXTURES = path.join(__dirname, 'fixtures');
const REAL = path.join(FIXTURES, 'eqlog-tail-200k.txt');
const PLANTED = path.join(FIXTURES, 'eqlog-invalid-byte.txt');

// The case table. AC-001 compares the number of `case:` lines the run prints against the
// number of entries declared here, counted separately.
const cases = [];
function kase(name, fn) { cases.push({ name, fn }); }

const B = (s) => Buffer.from(s, 'utf8');
const U = (b) => new Uint8Array(b);

/** A line counter that shares no code with `advance`: it walks bytes and counts 0x0A. */
function countNewlines(buf) {
  let n = 0;
  for (let i = 0; i < buf.length; i++) if (buf[i] === 0x0a) n++;
  return n;
}

/** A second, external line counter, when the machine has one. Printed as evidence; the byte
 *  scan above is the assertion, so a box without `wc` still runs the suite. */
function wcLines(file) {
  try {
    return parseInt(String(execFileSync('wc', ['-l', file], { encoding: 'utf8' })).trim().split(/\s+/)[0], 10);
  } catch (_) { return null; }
}

/** Feed `bytes` through `advance` in fixed-size chunks. Each chunk is what a read RETURNED. */
function feed(bytes, chunkSize) {
  let cursor = Tail.newCursor(0);
  const lines = [];
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const r = Tail.advance(cursor, bytes.subarray(i, Math.min(i + chunkSize, bytes.length)));
    lines.push(...r.lines);
    cursor = r.cursor;
  }
  return { lines, cursor };
}

/* ── the fixtures, read once ───────────────────────────────────────────────── */

const realBuf = fs.readFileSync(REAL);
const plantedBuf = fs.readFileSync(PLANTED);

/* ── AC-004 ────────────────────────────────────────────────────────────────── */

kase('AC-004 bootstrapOffset is 0 below the cap, size-cap above it, checked at the boundary', () => {
  const cap = 8 * 1024 * 1024;
  assert.strictEqual(Tail.BOOTSTRAP_CAP, cap);
  assert.strictEqual(Tail.bootstrapOffset(0), 0);
  assert.strictEqual(Tail.bootstrapOffset(cap - 1), 0);
  assert.strictEqual(Tail.bootstrapOffset(cap), 0);            // exactly at the cap: read it whole
  assert.strictEqual(Tail.bootstrapOffset(cap + 1), 1);        // one byte over: skip exactly one
  assert.strictEqual(Tail.bootstrapOffset(160 * 1024 * 1024), 160 * 1024 * 1024 - cap);
  assert.strictEqual(Tail.bootstrapOffset(100, 10), 90);       // an explicit cap overrides
  assert.strictEqual(Tail.bootstrapOffset(5, 10), 0);
});

/* ── AC-005 ────────────────────────────────────────────────────────────────── */

kase('AC-005 chooseDisplayFile is stable over identical modification times, 100 calls', () => {
  const entries = Object.freeze([
    { name: 'eqlog_Reviir_neriak.txt', lastModified: 1700000000000 },
    { name: 'eqlog_Reviir_freeport.txt', lastModified: 1700000000000 },
    { name: 'eqlog_Reviir_qeynos.txt', lastModified: 1700000000000 },
  ]);
  const first = Tail.chooseDisplayFile(entries);
  assert.ok(first);
  for (let i = 0; i < 100; i++) {
    assert.strictEqual(Tail.chooseDisplayFile(entries).name, first.name, `call ${i} flipped`);
  }
  // 100 calls over one array only catches a rule that is random. The rule that actually
  // flickers is one that falls back on enumeration order, and a directory's enumeration order
  // is not ours to depend on — so rotate the input and demand the same answer.
  for (let i = 0; i < entries.length; i++) {
    const rotated = entries.slice(i).concat(entries.slice(0, i));
    assert.strictEqual(Tail.chooseDisplayFile(rotated).name, first.name,
      `rotating the entry list by ${i} changed the display file`);
  }
  assert.deepStrictEqual(entries.map((e) => e.name),
    ['eqlog_Reviir_neriak.txt', 'eqlog_Reviir_freeport.txt', 'eqlog_Reviir_qeynos.txt'],
    'chooseDisplayFile mutated what it was handed');
  // And it is the newest when they differ, not merely the stable one.
  const newer = Tail.chooseDisplayFile([
    { name: 'a.txt', lastModified: 1 }, { name: 'b.txt', lastModified: 9 }, { name: 'c.txt', lastModified: 5 },
  ]);
  assert.strictEqual(newer.name, 'b.txt');
  assert.strictEqual(Tail.chooseDisplayFile([]), null);
});

/* ── AC-002 ────────────────────────────────────────────────────────────────── */

kase('AC-002 the real-log fixture and its provenance', () => {
  const head = realBuf.subarray(0, 200).toString('utf8').split('\r\n')[0];
  assert.ok(/PROVENANCE/.test(head), head);
  const text = realBuf.toString('latin1');
  assert.ok(/bytes a real EverQuest Legends client wrote/.test(text.slice(0, 2000)),
    'the fixture must record that it is a real client log');
  assert.ok(/names were replaced before check-in/.test(text.slice(0, 2000)),
    'the fixture must record that other players names were scrubbed');
  const scanned = countNewlines(realBuf);
  const wc = wcLines(REAL);
  console.log(`      provenance: ${head}`);
  console.log(`      real bytes: ${realBuf.length}; lines by byte scan: ${scanned}; lines by wc -l: ${wc === null ? 'wc unavailable' : wc}`);
  assert.ok(scanned >= 1200, `fixture holds ${scanned} lines, under the 1,200 floor`);
  if (wc !== null) assert.strictEqual(wc, scanned, 'wc -l and the byte scan disagree');
});

kase('AC-002 chunk invariance over real client bytes at 1, 7, 4096 and whole', () => {
  const bytes = U(realBuf);
  const sizes = [1, 7, 4096, bytes.length];
  const runs = sizes.map((n) => feed(bytes, n));
  const base = runs[0].lines;
  console.log(`      chunk sizes 1/7/4096/whole are IMPOSED BY THIS TEST (no reader asks for one byte); the bytes are the game's`);
  console.log(`      lines per run: ${runs.map((r) => r.lines.length).join(', ')}`);
  assert.ok(base.length >= 1200, `only ${base.length} lines`);
  for (let i = 1; i < runs.length; i++) {
    assert.strictEqual(runs[i].lines.length, base.length, `chunk size ${sizes[i]} yielded a different line count`);
    for (let j = 0; j < base.length; j++) {
      assert.strictEqual(runs[i].lines[j], base[j], `chunk size ${sizes[i]}, line ${j}`);
    }
  }
  // The cursor lands on the byte count actually fed, in every run.
  for (let i = 0; i < runs.length; i++) assert.strictEqual(runs[i].cursor.offset, bytes.length);
});

/* ── AC-003 ────────────────────────────────────────────────────────────────── */

kase('AC-003 REAL input: a boundary inside a real multi-byte character, counter stays 0', () => {
  const bytes = U(realBuf);
  let at = -1;
  for (let i = 0; i < bytes.length; i++) if (bytes[i] > 0x7f) { at = i; break; }
  assert.ok(at >= 0, 'the real fixture carries no non-ASCII character; re-cut it (AC-003)');
  const lineStart = realBuf.lastIndexOf(0x0a, at) + 1;
  const lineEnd = realBuf.indexOf(0x0a, at);
  const whole = realBuf.subarray(lineStart, lineEnd);
  console.log(`      REAL: first non-ASCII byte at fixture offset ${at} (0x${bytes[at].toString(16)}), in the line at offset ${lineStart}, ${lineEnd - lineStart} bytes`);

  // Split inside that character's byte sequence: one byte of it in the first chunk.
  let cursor = Tail.newCursor(0);
  const lines = [];
  for (const part of [bytes.subarray(0, at + 1), bytes.subarray(at + 1)]) {
    const r = Tail.advance(cursor, part);
    lines.push(...r.lines);
    cursor = r.cursor;
  }
  const emitted = lines.find((l) => /[^\x00-\x7f]/.test(l));
  assert.ok(emitted, 'the non-ASCII line was not emitted');
  assert.ok(Buffer.from(emitted, 'utf8').equals(whole),
    'the emitted line is not byte-equal to the same line read whole off the fixture');
  assert.strictEqual(cursor.replaced, 0,
    `the real fixture decodes cleanly, so the replacement counter must be 0, not ${cursor.replaced}`);
  console.log(`      REAL: character survived the split, line byte-equal to disk, replacement counter ${cursor.replaced}`);
});

kase('AC-003 SYNTHETIC input: a planted 0xC3 yields exactly one replacement and a counter of 1', () => {
  const text = plantedBuf.toString('latin1');
  assert.ok(/PARTLY SYNTHETIC/.test(text), 'the planted fixture must say it is synthetic');
  const m = /# PLANTED: one 0xC3 at byte offset (\d+) of this file\./.exec(text);
  assert.ok(m, 'the planted fixture must record where the byte was planted');
  const recorded = parseInt(m[1], 10);
  const high = [];
  for (let i = 0; i < plantedBuf.length; i++) if (plantedBuf[i] > 0x7f) high.push(i);
  console.log(`      SYNTHETIC: header records offset ${recorded}; bytes above 0x7F found at [${high.join(', ')}]`);
  assert.deepStrictEqual(high, [recorded],
    'the planted byte is not where the header says it is (a line-ending rewrite would do this)');
  assert.strictEqual(plantedBuf[recorded], 0xc3);
  assert.ok(plantedBuf[recorded + 1] < 0x80 || plantedBuf[recorded + 1] > 0xbf,
    'the byte after 0xC3 must be one that cannot continue it');

  const r = feed(U(plantedBuf), 4096);
  const bad = r.lines.filter((l) => l.indexOf('�') >= 0);
  assert.strictEqual(bad.length, 1, `expected one mangled line, got ${bad.length}`);
  assert.strictEqual((bad[0].match(/�/g) || []).length, 1, 'expected exactly one replacement character');
  assert.strictEqual(r.cursor.replaced, 1, `expected a counter of exactly 1, got ${r.cursor.replaced}`);
  console.log(`      SYNTHETIC: one replacement character, counter ${r.cursor.replaced}`);
});

kase('AC-003 a validly encoded U+FFFD the game itself wrote is not counted as a replacement', () => {
  // The client writes EF BF BD when a player pastes a character it cannot render. Those bytes
  // decode correctly. Counting them would report a mangled name where nothing was mangled.
  const bytes = U(Buffer.concat([B('[Wed Jul 15 23:19:31 2026] Levels 1'), Buffer.from([0xef, 0xbf, 0xbd]), B('29 here.\r\n')]));
  const r = feed(bytes, 3);
  assert.strictEqual(r.lines.length, 1);
  assert.strictEqual((r.lines[0].match(/�/g) || []).length, 1, 'the character itself must survive');
  assert.strictEqual(r.cursor.replaced, 0, 'a valid U+FFFD is not a replacement');
});

/* ── the cursor contract ───────────────────────────────────────────────────── */

kase('REQ-003 the remainder is bytes and never a decoded string', () => {
  const r = Tail.advance(Tail.newCursor(0), U(Buffer.from([0xe2, 0x82])));   // two thirds of a euro sign
  assert.strictEqual(r.lines.length, 0);
  assert.ok(r.cursor.remainder instanceof Uint8Array, 'remainder is not a byte array');
  assert.strictEqual(typeof r.cursor.remainder, 'object');
  assert.strictEqual(r.cursor.remainder.length, 2);
  const done = Tail.advance(r.cursor, U(Buffer.from([0xac, 0x0a])));
  assert.strictEqual(done.lines.length, 1);
  assert.strictEqual(done.lines[0], '€');
  assert.strictEqual(done.cursor.replaced, 0);
});

kase('REQ-008 a short read is never padded and the cursor never runs past bytes that came back', () => {
  // The caller asked for 4,096 bytes at offset 100 and got 12. `advance` is handed the 12.
  const cursor = Tail.newCursor(100);
  const got = U(B('abc\r\ndef\r\nxy'));
  assert.strictEqual(got.length, 12);
  const r = Tail.advance(cursor, got);
  assert.strictEqual(r.cursor.offset, 112, 'the offset advanced by the count requested, not the count returned');
  assert.deepStrictEqual(r.lines, ['abc\r', 'def\r']);
  assert.deepStrictEqual(Array.from(r.cursor.remainder), Array.from(B('xy')));
  // And no NUL ever enters the stream from a buffer nobody filled.
  for (const l of r.lines) assert.ok(l.indexOf(' ') < 0);
});

kase('REQ-008 replacement is detected by size regression and by a name change under the handle', () => {
  const cur = { offset: 5000, remainder: new Uint8Array(0), replaced: 0, name: 'eqlog_Reviir_neriak.txt' };
  assert.deepStrictEqual(Tail.detectReplacement(cur, { name: 'eqlog_Reviir_neriak.txt', size: 6000 }).replaced, false);
  const shrank = Tail.detectReplacement(cur, { name: 'eqlog_Reviir_neriak.txt', size: 12 });
  assert.strictEqual(shrank.replaced, true);
  assert.ok(/shrank/.test(shrank.why), shrank.why);
  const renamed = Tail.detectReplacement(cur, { name: 'eqlog_Reviir_freeport.txt', size: 9000 });
  assert.strictEqual(renamed.replaced, true);
  assert.ok(/name/.test(renamed.why), renamed.why);
});

/* ── the degradations ──────────────────────────────────────────────────────── */

kase('REQ-009 the tail is offered only on all three, and the sentence names what is missing', () => {
  const ok = Tail.tailAvailability({ hasPicker: true, topLevel: true, rung: 2 });
  assert.strictEqual(ok.ok, true);
  assert.strictEqual(ok.missing.length, 0);

  const firefox = Tail.tailAvailability({ hasPicker: false, topLevel: true, rung: 2 });
  assert.strictEqual(firefox.ok, false);
  assert.ok(/directory picker/.test(firefox.sentence), firefox.sentence);

  const framed = Tail.tailAvailability({ hasPicker: true, topLevel: false, rung: 1 });
  assert.strictEqual(framed.ok, false);
  assert.strictEqual(framed.missing.length, 2, 'a cross-origin frame is missing both the top level and the rung');
  assert.ok(/top-level/.test(framed.sentence) && /rung/.test(framed.sentence), framed.sentence);

  const nothing = Tail.tailAvailability({});
  assert.strictEqual(nothing.ok, false);
  assert.strictEqual(nothing.missing.length, 3);
  // A control that is merely disabled, with no sentence, is forbidden.
  for (const v of [firefox, framed, nothing]) assert.ok(v.sentence.length > 40, v.sentence);
});

kase('REQ-011 three folder states, three sentences, never collapsed', () => {
  const none = Tail.folderState({ picked: false });
  const unreadable = Tail.folderState({ picked: true, readable: false });
  const empty = Tail.folderState({ picked: true, readable: true, logCount: 0 });
  const watching = Tail.folderState({ picked: true, readable: true, logCount: 2 });
  assert.deepStrictEqual([none.state, unreadable.state, empty.state, watching.state],
    ['none', 'unreadable', 'empty', 'watching']);
  const sentences = new Set([none.sentence, unreadable.sentence, empty.sentence, watching.sentence]);
  assert.strictEqual(sentences.size, 4, 'two folder states share a sentence');
  assert.ok(/not the same as it being empty/.test(unreadable.sentence), unreadable.sentence);
  assert.ok(/log on/.test(empty.sentence), empty.sentence);
});

kase('EDGE-009 the handle count per poll is capped, rotates so nothing starves, and says so', () => {
  const names = [];
  for (let i = 0; i < 30; i++) names.push(`eqlog_R_zone${String(i).padStart(2, '0')}.txt`);
  const seen = new Set();
  let sentence = '';
  for (let cycle = 0; cycle < 30; cycle++) {
    const p = Tail.planPoll(names, cycle, 8);
    assert.strictEqual(p.read.length, 8);
    p.read.forEach((n) => seen.add(n));
    sentence = p.sentence;
  }
  assert.strictEqual(seen.size, 30, 'rotation starved a file');
  assert.ok(/8 read per poll/.test(sentence), sentence);
  const small = Tail.planPoll(['a', 'b'], 0, 8);
  assert.deepStrictEqual(small.read, ['a', 'b']);
  assert.deepStrictEqual(small.deferred, []);
});

kase('EDGE-004/EDGE-006 staleness is the age of the last successful read, not a liveness claim', () => {
  const fresh = Tail.staleness(10_000, 9_500);
  assert.strictEqual(fresh.stale, false);
  const hidden = Tail.staleness(70_000, 10_000);      // a throttled hidden tab, one poll a minute
  assert.strictEqual(hidden.stale, true);
  assert.ok(/60s ago/.test(hidden.sentence), hidden.sentence);
  assert.ok(/not live/.test(hidden.sentence), hidden.sentence);
  const never = Tail.staleness(1, null);
  assert.strictEqual(never.seconds, null);
  assert.strictEqual(never.stale, true);
});

kase('REQ-002 only eqlog text files are opened', () => {
  for (const n of ['eqlog_Reviir_neriak.txt', 'eqlog_A_b.TXT', 'eqlog_x.txt']) assert.ok(Tail.isLogName(n), n);
  for (const n of ['dbg.txt', 'Sky.txt', 'Reviir-Inventory.txt', 'eqlog_.txt.bak', '', null]) {
    assert.ok(!Tail.isLogName(n), String(n));
  }
});

/* ── the shim, over hand-built handles ─────────────────────────────────────── */

/** A File the way the browser hands one over: a snapshot. `slice(n).arrayBuffer()` returns
 *  what is there NOW, and a reference held across a write does not grow — which is why the
 *  shim asks the handle again every poll rather than keeping one of these. */
function fakeFile(name, buf, lastModified) {
  return {
    name, size: buf.length, lastModified,
    slice(from) {
      const part = buf.subarray(Math.min(from, buf.length));
      return { arrayBuffer: async () => part.buffer.slice(part.byteOffset, part.byteOffset + part.byteLength) };
    },
  };
}

/** A directory handle over a mutable table of files, plus the `win` the shim needs. */
function fakeWorld(files) {
  const table = new Map(Object.entries(files));
  let timerFn = null;
  const dir = {
    name: 'Logs',
    async *entries() {
      for (const [name] of table) yield [name, { kind: 'file', name, getFile: async () => table.get(name) }];
    },
  };
  const win = {
    document: { body: { dataset: { rung: '2', door: 'wasm' } } },
    showDirectoryPicker: async () => dir,
    setInterval: (fn) => { timerFn = fn; return 1; },
    clearInterval: () => { timerFn = null; },
    top: null, self: null,
  };
  win.top = win; win.self = win;
  return { win, dir, table, tick: () => (timerFn ? timerFn() : Promise.resolve()) };
}

kase('AC-012 two logs appended alternately: both files lines arrive, each on its own cursor', async () => {
  const a = Buffer.from('[Wed Jul 15 23:19:30 2026] A one\r\n');
  const b = Buffer.from('[Wed Jul 15 23:19:30 2026] B one\r\n');
  const w = fakeWorld({
    'eqlog_R_neriak.txt': fakeFile('eqlog_R_neriak.txt', a, 100),
    'eqlog_R_qeynos.txt': fakeFile('eqlog_R_qeynos.txt', b, 200),
  });
  const got = [];
  const t = Tail.create({ win: w.win, engine: { harvest: (s) => got.push(s) }, cap: 1 << 30 });
  await t.pick();
  got.length = 0;                                   // the bootstrap is the end of each file

  const a2 = Buffer.concat([a, Buffer.from('[Wed Jul 15 23:19:31 2026] A two\r\n')]);
  const b2 = Buffer.concat([b, Buffer.from('[Wed Jul 15 23:19:32 2026] B two\r\n')]);
  w.table.set('eqlog_R_neriak.txt', fakeFile('eqlog_R_neriak.txt', a2, 300));
  await t.poll();
  w.table.set('eqlog_R_qeynos.txt', fakeFile('eqlog_R_qeynos.txt', b2, 400));
  await t.poll();

  const all = got.join('');
  assert.ok(/A two/.test(all), 'the first file\'s appended line never arrived');
  assert.ok(/B two/.test(all), 'the second file\'s appended line never arrived — the busiest-file heuristic is hazard H22');
  assert.strictEqual(t.view().displayFile, 'eqlog_R_qeynos.txt', 'the display hint is the newest, and only a hint');
});

kase('AC-006 SYNTHETIC size regression: the cursor rewinds and the counter goes 0 -> 1', async () => {
  const big = Buffer.from('[Wed Jul 15 23:19:30 2026] one\r\n[Wed Jul 15 23:19:31 2026] two\r\n');
  const w = fakeWorld({ 'eqlog_R_neriak.txt': fakeFile('eqlog_R_neriak.txt', big, 100) });
  const t = Tail.create({ win: w.win, engine: null, cap: 1 << 30 });
  await t.pick();
  assert.strictEqual(t.view().replacedFiles, 0, 'the counter must start at 0');
  const before = big.length;

  const small = Buffer.from('[Wed Jul 15 23:20:00 2026] fresh\r\n');
  w.table.set('eqlog_R_neriak.txt', fakeFile('eqlog_R_neriak.txt', small, 500));
  await t.poll();
  console.log(`      SYNTHETIC (hand-built file object, not a real rotation): size ${before} -> ${small.length}`);
  const v = t.view();
  assert.strictEqual(v.replacedFiles, 1, `the replacement counter must read exactly 1, not ${v.replacedFiles}`);
  // The notice has to survive the poll that produced it, or the panel says nothing happened.
  assert.ok(/was replaced/.test(v.notice || ''), `notice=${v.notice}`);
  assert.ok(/eqlog_R_neriak\.txt/.test(v.notice || ''), `notice=${v.notice}`);
  // And the cursor rewound to a fresh bootstrap rather than sitting past the new end of file.
  const again = await t.poll();
  assert.strictEqual(t.view().replacedFiles, 1, 'a settled file must not keep counting itself');
});

kase('REQ-006 the poll is single flight: a slow read makes one late poll, never a queue', async () => {
  let inFlight = 0, peak = 0, opens = 0;
  const buf = Buffer.from('[Wed Jul 15 23:19:30 2026] one\r\n');
  const w = fakeWorld({ 'eqlog_R_neriak.txt': fakeFile('eqlog_R_neriak.txt', buf, 100) });
  const slow = { name: 'eqlog_R_neriak.txt', kind: 'file', getFile: async () => {
    inFlight++; peak = Math.max(peak, inFlight); opens++;
    await new Promise((r) => setTimeout(r, 20));
    inFlight--;
    return fakeFile('eqlog_R_neriak.txt', buf, 100);
  } };
  w.dir.entries = async function* () { yield ['eqlog_R_neriak.txt', slow]; };
  const t = Tail.create({ win: w.win, engine: null, cap: 1 << 30 });
  await t.pick();                                          // the folder is adopted and read once
  assert.strictEqual(opens, 1, 'the adopting poll did not run');
  // Now three timer ticks land while a read is still awaiting. `poll` must not begin.
  const overlapping = [t.poll(), t.poll(), t.poll()];
  await Promise.all(overlapping);
  assert.strictEqual(peak, 1, `${peak} reads were in flight at once`);
  assert.strictEqual(opens, 2, `the overlapping polls were queued, not skipped (${opens} reads in total)`);
});

kase('EDGE-002 a file the game holds open is reported unreadable BY NAME, never as empty', async () => {
  const w = fakeWorld({ 'eqlog_R_neriak.txt': null });
  w.dir.entries = async function* () {
    yield ['eqlog_R_neriak.txt', { kind: 'file', name: 'eqlog_R_neriak.txt',
      getFile: async () => { throw new Error('NotReadableError'); } }];
  };
  const t = Tail.create({ win: w.win, engine: null });
  await t.pick();
  const v = t.view();
  assert.ok(/eqlog_R_neriak\.txt/.test(v.error), v.error);
  assert.ok(/not empty/.test(v.error), v.error);
  assert.strictEqual(v.folder.state, 'watching', 'a file that will not open is not an empty folder');
});

kase('EDGE-001 a folder the browser refuses is a named sentence, not a rejected promise', async () => {
  const w = fakeWorld({});
  w.win.showDirectoryPicker = async () => { throw new Error('SecurityError: the folder is blocked'); };
  const t = Tail.create({ win: w.win, engine: null });
  const v = await t.pick();
  assert.ok(/did not hand over a folder/.test(v.error), v.error);
  assert.ok(/blocked/.test(v.error), v.error);
  assert.ok(/upload control/.test(v.error), v.error);
  assert.strictEqual(v.watching, false);
});

kase('EDGE-007/EDGE-010 one watcher per pick, and no tail at all below rung 2', async () => {
  const buf = Buffer.from('[Wed Jul 15 23:19:30 2026] one\r\n');
  const w = fakeWorld({ 'eqlog_R_neriak.txt': fakeFile('eqlog_R_neriak.txt', buf, 100) });
  let live = 0;
  w.win.setInterval = () => { live++; return live; };
  w.win.clearInterval = () => { live--; };
  const t = Tail.create({ win: w.win, engine: null });
  await t.pick();
  await t.pick();                                   // the same folder picked twice
  assert.strictEqual(live, 1, `${live} watchers on one folder; two would double every count`);

  const framed = fakeWorld({});
  framed.win.document.body.dataset.rung = '1';
  framed.win.top = { other: true };
  const t2 = Tail.create({ win: framed.win, engine: null });
  assert.strictEqual(t2.availability.ok, false);
  const v = await t2.pick();
  assert.strictEqual(v.watching, false, 'the tail started at rung 1');
  assert.ok(/rung 1/.test(v.error), v.error);
});

/* ── AC-007 ────────────────────────────────────────────────────────────────── */

kase('AC-007 web/tail.js cannot transmit: a source search finds none of the four', () => {
  const src = fs.readFileSync(path.join(__dirname, 'tail.js'), 'utf8');
  const hits = [];
  for (const name of ['fetch', 'XMLHttpRequest', 'WebSocket', 'sendBeacon']) {
    const n = (src.match(new RegExp(name, 'g')) || []).length;
    if (n) hits.push(`${name} x${n}`);
  }
  console.log(`      SOURCE PROPERTY of web/tail.js only — it says nothing about the rest of the page`);
  assert.deepStrictEqual(hits, [], `web/tail.js can transmit: ${hits.join(', ')}`);
  for (const name of ['navigator.userAgent', 'navigator.vendor', 'navigator.platform']) {
    assert.ok(src.indexOf(name) < 0, `${name} is a user-agent string, and this board detects capability`);
  }
});

/* ── run ───────────────────────────────────────────────────────────────────── */

(async () => {
  let failed = 0;
  for (const { name, fn } of cases) {
    try {
      await fn();
      console.log(`case: ${name}`);
    } catch (e) {
      console.log(`case: ${name} FAILED: ${e.message}`);
      failed++;
    }
  }
  console.log(`${cases.length} case(s) declared`);
  if (failed) {
    console.error(`${failed} of ${cases.length} case(s) failed`);
    process.exit(1);
  }
})();
