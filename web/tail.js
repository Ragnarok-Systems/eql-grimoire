// Rung 2: the browser live tail. A directory handle, a one-second single-flight poll, a byte
// cursor per log, an 8 MB bootstrap off the end of each one. Nothing installed, nothing
// uploaded, no agent. A directory handle exists only on a top-level page, never inside a
// sandboxed iframe, so this rung is the website door's; this file implements it.
//
// SPLIT, AND THE SPLIT IS THE POINT. Everything above the SHIM banner is pure: plain data in,
// plain data out, no `window`, no `document`, no handles, no clock. That half is what
// `web/tail.test.js` drives, in plain Node, with no browser. Everything below the banner
// performs browser calls and makes no decision of its own — every branch it takes it takes by
// asking a function from the pure half. A decision that lives below the banner is a decision
// nothing can test, and this rung has too many limits to leave any of them untested.
//
// THE CURSOR RULE IS NOT THIS FILE'S. G1-02 owns it for the whole board and states it once.
// This is a second implementation of that one rule, in a language the Rust one cannot reach,
// and that is the only reason it is allowed to exist. Where the two disagree, G1-02 is right.
// `COMBAT-PARSER.md` §7 hazard H22 is the register of what goes wrong when it is got wrong,
// and two of the hazards it records are the two this file is shaped to avoid: a padded short
// read, and a remainder held as a decoded string.
//
// NOTHING LEAVES THE PAGE. This file performs no network call of any kind. The bytes it reads
// go to the engine in the page and nowhere else. That is a property of this source and
// `web/tail.test.js` checks it by reading the source.
//
//   const t = Tail.create({ win: window, engine: ENG, render: fn });
//   await t.pick();          // from a click, never on load

const Tail = (() => {
  'use strict';

  /* ══════════ PURE ══════════════════════════════════════════════════════════ */

  /** The newest 8 MB. `crates/grimoire-parse/src/lib.rs:7` states why that number and not
   *  another; this is the same constant, cited rather than re-argued. */
  const BOOTSTRAP_CAP = 8 * 1024 * 1024;

  /** How many handles one poll may open. EDGE-009: a player's Logs directory can hold
   *  hundreds of files, and an uncapped poll is a per-second cost on a machine that is
   *  running the game. The cap is stated on screen; `planPoll` rotates so nothing starves. */
  const HANDLE_CAP = 8;

  /** What the game names its chat logs. Only these are opened. */
  const LOG_NAME = /^eqlog_.+\.txt$/i;
  const isLogName = (name) => LOG_NAME.test(String(name || ''));

  /** `max(0, size - cap)`. Bootstrap starts here, never at byte zero, and never at the whole
   *  file: a 160 MB log read whole is a page nobody waits for. */
  function bootstrapOffset(size, cap = BOOTSTRAP_CAP) {
    const n = Number(size) || 0;
    const c = Number.isFinite(Number(cap)) ? Number(cap) : BOOTSTRAP_CAP;
    return Math.max(0, n - c);
  }

  const EMPTY = new Uint8Array(0);

  /** A fresh cursor. `remainder` is BYTES. It is never a decoded string, because a chunk
   *  boundary that splits a multi-byte character cannot be expressed in one. */
  function newCursor(offset = 0, name = null) {
    return { offset: Math.max(0, Number(offset) || 0), remainder: EMPTY, replaced: 0, name };
  }

  function toBytes(v) {
    if (v instanceof Uint8Array) return v;
    if (v && typeof v.byteLength === 'number' && !Array.isArray(v)) return new Uint8Array(v);
    return Uint8Array.from(v || []);
  }

  function concat(a, b) {
    if (!a.length) return b;
    if (!b.length) return a;
    const out = new Uint8Array(a.length + b.length);
    out.set(a, 0); out.set(b, a.length);
    return out;
  }

  const STRICT = new TextDecoder('utf-8', { fatal: true });
  const LOSSY = new TextDecoder('utf-8');

  /** Occurrences of the three bytes that ARE a validly encoded U+FFFD. The game writes these:
   *  a player pastes an en dash into general chat and the client emits EF BF BD. Those bytes
   *  decode correctly and are not a replacement, and counting the U+FFFD in the output without
   *  subtracting them would report a mangled name where none happened. */
  function encodedFFFD(bytes) {
    let n = 0;
    for (let i = 0; i + 2 < bytes.length; i++) {
      if (bytes[i] === 0xef && bytes[i + 1] === 0xbf && bytes[i + 2] === 0xbd) { n++; i += 2; }
    }
    return n;
  }

  /** One completed line, decoded. Decoding is per line and never per chunk, so a chunk that
   *  ends mid-character is not the decoder's problem — those bytes are still in `remainder`.
   *  A sequence that will not decode is replaced and COUNTED, because a silently mangled name
   *  is a join key that no longer joins. */
  function decodeLine(bytes) {
    try {
      return { text: STRICT.decode(bytes), replaced: 0 };
    } catch (_) {
      const text = LOSSY.decode(bytes);
      let seen = 0;
      for (const ch of text) if (ch === '�') seen++;
      return { text, replaced: Math.max(0, seen - encodedFFFD(bytes)) };
    }
  }

  /** advance(cursor, bytes) -> { lines, cursor }
   *
   *  `bytes` is what a read ACTUALLY returned, never what it asked for. The offset advances by
   *  `bytes.length` and by nothing else: that is the whole of the short-read rule. A
   *  zero-filled buffer returned whole instead puts NULs in the stream and a cursor past bytes
   *  nobody read.
   *
   *  Lines are split on 0x0A and carry any 0x0D through untouched, which is the same line
   *  stream `str::split('\n')` gives the native door; the parser trims the CR itself at
   *  `crates/grimoire-parse/src/line.rs:65`. The trailing incomplete line becomes the new
   *  remainder, as bytes.
   */
  function advance(cursor, bytes) {
    const cur = cursor || newCursor(0);
    const chunk = toBytes(bytes);
    const buf = concat(toBytes(cur.remainder), chunk);
    const lines = [];
    let replaced = Number(cur.replaced) || 0;
    let start = 0;
    for (let i = 0; i < buf.length; i++) {
      if (buf[i] === 0x0a) {
        const d = decodeLine(buf.subarray(start, i));
        lines.push(d.text);
        replaced += d.replaced;
        start = i + 1;
      }
    }
    return {
      lines,
      cursor: {
        offset: (Number(cur.offset) || 0) + chunk.length,
        remainder: buf.slice(start),
        replaced,
        name: cur.name === undefined ? null : cur.name,
      },
    };
  }

  /** The most recently modified log, for the header line and for nothing else. It is a display
   *  hint: it never decides which bytes are read. H22 records why — the busiest-file heuristic
   *  flips every poll for a player running more than one client, and every file is tailed
   *  independently anyway. Ties break on the name, ascending, so the header does not flicker
   *  once a second. Does not mutate what it is handed. */
  function chooseDisplayFile(entries) {
    const list = entries || [];
    let best = null;
    for (const e of list) {
      if (!e) continue;
      if (best === null) { best = e; continue; }
      const a = Number(e.lastModified) || 0, b = Number(best.lastModified) || 0;
      if (a > b || (a === b && String(e.name) < String(best.name))) best = e;
    }
    return best;
  }

  /** Replacement, detected two ways, per REQ-008. Either resets the cursor to a fresh
   *  bootstrap and increments the counter the panel shows, because a total that jumps with no
   *  explanation is how a session silently vanishes. */
  function detectReplacement(cursor, file) {
    const cur = cursor || newCursor(0);
    const name = file && file.name !== undefined ? String(file.name) : null;
    const size = Number(file && file.size) || 0;
    if (cur.name && name && name !== cur.name) {
      return { replaced: true, why: `the name under this handle changed from ${cur.name} to ${name}` };
    }
    if (size < (Number(cur.offset) || 0)) {
      return { replaced: true, why: `the file shrank from ${cur.offset} bytes to ${size}` };
    }
    return { replaced: false, why: null };
  }

  /** REQ-009. Three conditions, and when any one fails the tail control does not render, the
   *  rung-1 upload control renders in its place, and the sentence names what was missing. A
   *  disabled control with no explanation is forbidden. Capability, never a user-agent string.
   *
   *  env: { hasPicker, topLevel, rung } */
  function tailAvailability(env) {
    const e = env || {};
    const rung = Number(e.rung);
    const missing = [];
    if (!e.hasPicker) missing.push('a directory picker (this browser has none; the live tail is Chromium only)');
    if (!e.topLevel) missing.push('a top-level document (the File System Access API is not available to a cross-origin frame)');
    if (!(rung >= 2)) missing.push(`rung 2 or better (this page opened at rung ${Number.isFinite(rung) ? rung : 'unknown'})`);
    return {
      ok: missing.length === 0,
      missing,
      sentence: missing.length === 0
        ? 'Live tailing is available on this page.'
        : `No live tail here. This page is missing ${missing.join('; and ')}.`,
    };
  }

  /** REQ-011. Three folder states, each with its own sentence, and they are never collapsed:
   *  collapsing unreadable into empty is what makes a misconfiguration look identical to a
   *  player who never typed the log command.
   *
   *  view: { picked, readable, logCount } */
  function folderState(view) {
    const v = view || {};
    if (!v.picked) {
      return { state: 'none', sentence: 'No folder chosen yet. Pick your EverQuest Legends Logs folder to start tailing.' };
    }
    if (!v.readable) {
      return { state: 'unreadable', sentence: 'That folder could not be read. This is not the same as it being empty — the folder is there and the browser would not open it.' };
    }
    if (!(Number(v.logCount) > 0)) {
      return { state: 'empty', sentence: 'That folder was read and holds no eqlog_*.txt. Logging is probably off in the game: type /log on.' };
    }
    return { state: 'watching', sentence: `Watching ${v.logCount} log file${Number(v.logCount) === 1 ? '' : 's'}.` };
  }

  /** EDGE-009. Which handles this poll opens, capped, rotating so no file starves, and the
   *  sentence the panel prints so the cap is on screen rather than merely true. */
  function planPoll(names, cycle = 0, cap = HANDLE_CAP) {
    const list = (names || []).slice().sort();
    const n = Math.max(1, Number(cap) || HANDLE_CAP);
    if (list.length <= n) {
      return { read: list, deferred: [], sentence: `Reading all ${list.length} log file${list.length === 1 ? '' : 's'} each poll.` };
    }
    const at = ((Number(cycle) || 0) % list.length + list.length) % list.length;
    const read = [];
    for (let i = 0; i < n; i++) read.push(list[(at + i) % list.length]);
    const deferred = list.filter((x) => !read.includes(x));
    return {
      read,
      deferred,
      sentence: `${list.length} log files here, ${n} read per poll in rotation — an uncapped poll is a per-second cost on a machine running the game.`,
    };
  }

  /** EDGE-004 and EDGE-006. A poll that overruns is skipped, not queued, and a hidden tab's
   *  timers are throttled; either way the age of the last successful read is what the panel
   *  shows. A stalled tail must look stalled rather than looking quiet. */
  function staleness(nowMs, lastOkMs, budgetMs = 2000) {
    if (!lastOkMs) return { seconds: null, stale: true, sentence: 'Nothing read yet.' };
    const ms = Math.max(0, (Number(nowMs) || 0) - Number(lastOkMs));
    const s = Math.round(ms / 100) / 10;
    return {
      seconds: s,
      stale: ms > (Number(budgetMs) || 2000),
      sentence: ms > (Number(budgetMs) || 2000)
        ? `Last read ${s}s ago — not live right now.`
        : `Last read ${s}s ago.`,
    };
  }

  /* ══════════ SHIM ══════════════════════════════════════════════════════════
   *
   * Browser calls only. Every branch below asks the pure half above. Nothing here decides
   * anything, and nothing here reaches a network.
   */

  const DB_NAME = 'grimoire-tail';
  const STORE = 'handles';
  const KEY = 'logdir';

  function idbOpen(win) {
    return new Promise((resolve, reject) => {
      const idb = win && win.indexedDB;
      if (!idb) { reject(new Error('no indexedDB')); return; }
      const req = idb.open(DB_NAME, 1);
      req.onupgradeneeded = () => { req.result.createObjectStore(STORE); };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error || new Error('indexedDB refused'));
    });
  }

  function idbPut(win, value) {
    return idbOpen(win).then((db) => new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, 'readwrite');
      tx.objectStore(STORE).put(value, KEY);
      tx.oncomplete = () => { db.close(); resolve(true); };
      tx.onerror = () => { db.close(); reject(tx.error || new Error('indexedDB write refused')); };
    }));
  }

  function idbGet(win) {
    return idbOpen(win).then((db) => new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, 'readonly');
      const r = tx.objectStore(STORE).get(KEY);
      r.onsuccess = () => { db.close(); resolve(r.result || null); };
      r.onerror = () => { db.close(); reject(r.error || new Error('indexedDB read refused')); };
    }));
  }

  /** The environment `tailAvailability` grades. Capability only: no user-agent string, no
   *  vendor string, no platform string. `rung` is G9-01's, published on the body at boot; this
   *  story reads it and never detects the engine again. */
  function readEnv(win) {
    const w = win || {};
    const doc = w.document || {};
    const body = doc.body || {};
    const ds = body.dataset || {};
    let topLevel = true;
    try { topLevel = w.top === w.self; } catch (_) { topLevel = false; }
    return {
      hasPicker: typeof w.showDirectoryPicker === 'function',
      topLevel,
      rung: Number(ds.rung),
      door: ds.door || null,
    };
  }

  function create(deps) {
    const win = deps.win;
    const engine = deps.engine || null;
    const render = deps.render || (() => {});
    const now = deps.now || (() => Date.now());
    const cap = deps.cap === undefined ? BOOTSTRAP_CAP : deps.cap;

    const state = {
      dir: null,             // the directory handle
      dirName: null,
      cursors: new Map(),    // file name -> cursor
      files: [],             // last poll's entry list, for the display hint
      cycle: 0,
      replacedFiles: 0,      // REQ-008's named counter: files that were rotated under us
      lastOk: null,
      readable: true,
      polling: false,        // single flight
      timer: null,
      error: null,           // this poll could not do its job
      notice: null,          // this poll did its job and something happened worth saying
      handleCapNote: '',
    };

    const env = readEnv(win);
    const avail = tailAvailability(env);

    function view() {
      const folder = folderState({
        picked: !!state.dir,
        readable: state.readable,
        logCount: state.files.length,
      });
      const display = chooseDisplayFile(state.files);
      let replaced = 0;
      for (const c of state.cursors.values()) replaced += c.replaced;
      return {
        available: avail.ok,
        availability: avail,
        env,
        folder,
        dirName: state.dirName,
        displayFile: display ? display.name : null,
        staleness: staleness(now(), state.lastOk),
        replacedFiles: state.replacedFiles,
        decodeReplacements: replaced,
        handleCapNote: state.handleCapNote,
        watching: !!state.timer,
        error: state.error,
        notice: state.notice,
      };
    }

    const paint = () => render(view());

    /** One file, one poll. The handle is re-read from scratch every time: the object a handle
     *  hands back describes the file at the moment it was asked, so a held reference is a
     *  photograph and never grows. Re-reading is also correct if it did grow, which is why it
     *  is done this way rather than assumed either way. */
    async function pollOne(entry) {
      const handle = entry.handle;
      let file;
      try {
        file = await handle.getFile();
      } catch (e) {
        // EDGE-002. Unreadable is reported by name and is never reported as empty: the
        // difference is a misconfiguration versus a player who has not typed /log on.
        state.error = `${entry.name} could not be read (${e && e.message ? e.message : e}). It is not empty — it would not open.`;
        return { name: entry.name, lastModified: 0, unreadable: true };
      }

      let cursor = state.cursors.get(entry.name);
      if (!cursor) {
        cursor = newCursor(bootstrapOffset(file.size, cap), file.name);
        state.cursors.set(entry.name, cursor);
      } else {
        const rot = detectReplacement(cursor, file);
        if (rot.replaced) {
          state.replacedFiles += 1;
          state.notice = `${entry.name} was replaced: ${rot.why}. Re-bootstrapped from the end of the new file.`;
          cursor = newCursor(bootstrapOffset(file.size, cap), file.name);
          cursor.replaced = 0;
          state.cursors.set(entry.name, cursor);
        }
      }

      if (file.size <= cursor.offset) return { name: entry.name, lastModified: file.lastModified };

      const buf = await file.slice(cursor.offset).arrayBuffer();
      // What came back, never what was asked for.
      const got = new Uint8Array(buf);
      const { lines, cursor: next } = advance(cursor, got);
      next.name = file.name;
      state.cursors.set(entry.name, next);

      if (lines.length && engine && typeof engine.harvest === 'function') {
        // Text to the engine, which drops anything before the first properly stamped line
        // itself. The page establishes no line boundaries of its own beyond 0x0A.
        engine.harvest(lines.join('\n') + '\n');
      }
      return { name: entry.name, lastModified: file.lastModified };
    }

    async function poll() {
      if (state.polling || !state.dir) return;          // single flight: skipped, never queued
      state.polling = true;
      try {
        const names = [];
        const handles = new Map();
        for await (const [name, handle] of state.dir.entries()) {
          if (handle.kind === 'file' && isLogName(name)) { names.push(name); handles.set(name, handle); }
        }
        state.readable = true;
        const plan = planPoll(names, state.cycle, deps.handleCap);
        state.cycle += 1;
        state.handleCapNote = plan.sentence;

        const seen = [];
        for (const name of plan.read) {
          seen.push(await pollOne({ name, handle: handles.get(name) }));
        }
        // EDGE-008: a file that appeared since the last poll is in `names` and gets its own
        // cursor here, bootstrapped from its end like any other, never from byte zero.
        const carried = state.files.filter((f) => !plan.read.includes(f.name) && names.includes(f.name));
        state.files = seen.concat(carried);
        state.lastOk = now();
        // Only a FAILURE clears here. A replacement notice is not a failure and survives the
        // poll that produced it, because REQ-008 says the panel has to say it happened.
        if (!seen.some((s) => s.unreadable)) state.error = null;
      } catch (e) {
        state.readable = false;
        state.error = e && e.message ? e.message : String(e);
      } finally {
        state.polling = false;
        paint();
      }
    }

    async function adopt(handle) {
      stop();                                  // EDGE-007: one watcher, never two
      state.dir = handle;
      state.dirName = handle && handle.name ? handle.name : null;
      state.cursors = new Map();
      state.files = [];
      state.replacedFiles = 0;
      state.error = null;
      state.notice = null;
      try { await idbPut(win, handle); } catch (_) { /* a refresh will cost a re-pick, nothing else */ }
      state.timer = win.setInterval(poll, deps.intervalMs || 1000);
      await poll();
      return view();
    }

    /** From a click. EDGE-001/EDGE-010: a refusal is shown with the folder that was asked for,
     *  and the page falls to rung 1 rather than leaving a rejected promise in the console. */
    async function pick() {
      if (!avail.ok) { state.error = avail.sentence; paint(); return view(); }
      let handle;
      try {
        handle = await win.showDirectoryPicker({ id: 'eql-logs', mode: 'read' });
      } catch (e) {
        state.error = `The browser did not hand over a folder (${e && e.message ? e.message : e}). ` +
          'Some folders are refused outright. Use the upload control instead.';
        paint();
        return view();
      }
      return adopt(handle);
    }

    /** REQ-010. On load this only QUERIES; it never requests. A permission request with no
     *  gesture is refused and the refusal is charged to the handle, so the page renders a
     *  resume control and waits for a click instead. */
    async function restorable() {
      if (!avail.ok) return { has: false, granted: false };
      let handle = null;
      try { handle = await idbGet(win); } catch (_) { return { has: false, granted: false }; }
      if (!handle) return { has: false, granted: false };
      state.dirName = handle.name || null;
      let granted = false;
      try {
        granted = (await handle.queryPermission({ mode: 'read' })) === 'granted';
      } catch (_) { granted = false; }
      return { has: true, granted, name: handle.name || null, handle };
    }

    /** From a click, and only from a click. */
    async function resume() {
      const r = await restorable();
      if (!r.has) { state.error = 'No stored folder to resume.'; paint(); return view(); }
      let ok = r.granted;
      if (!ok) {
        try { ok = (await r.handle.requestPermission({ mode: 'read' })) === 'granted'; } catch (_) { ok = false; }
      }
      if (!ok) {
        state.error = 'Permission for the stored folder was not granted. Pick it again, or use the upload control.';
        paint();
        return view();
      }
      return adopt(r.handle);
    }

    function stop() {
      if (state.timer) { win.clearInterval(state.timer); state.timer = null; }
      paint();
      return view();
    }

    return { pick, resume, restorable, stop, poll, view, availability: avail, env };
  }

  return {
    // pure
    BOOTSTRAP_CAP, HANDLE_CAP, isLogName, bootstrapOffset, newCursor, advance,
    chooseDisplayFile, detectReplacement, tailAvailability, folderState, planPoll, staleness,
    // shim
    create, readEnv,
  };
})();

if (typeof module !== 'undefined') module.exports = Tail;
