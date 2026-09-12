// Plain-Node test for Grimoire.pick and Grimoire.open. No browser, no Playwright, no
// network, no built binary — `pick` is a pure function of a plain object and `open` is
// driven here through a mock `io`, exactly as G9-02/G9-05 will drive it through a real one.
//
//   node web/engine.test.js

const assert = require('assert');
const path = require('path');

const GRIMOIRE_PATH = path.join(__dirname, 'grimoire.js');
const Grimoire = require(GRIMOIRE_PATH);

// `open` caches the engine it opens for the lifetime of the module (EDGE-008), so any case
// that calls it needs a module instance nobody else has touched yet.
function freshGrimoire() {
  delete require.cache[require.resolve(GRIMOIRE_PATH)];
  return require(GRIMOIRE_PATH);
}

// The case table AC-001 counts separately from the printed output.
const cases = [];
function kase(name, fn) { cases.push({ name, fn }); }

kase('explicit endpoint', () => {
  const { ladder } = Grimoire.pick({ search: '?engine=http://example.com:9999/engine', hostname: 'x', hasWasm: true });
  assert.strictEqual(ladder.length, 1);
  assert.strictEqual(ladder[0].door, 'remote');
  assert.strictEqual(ladder[0].endpoint, 'http://example.com:9999/engine');
});

kase('loopback hostname', () => {
  const { ladder, rung } = Grimoire.pick({ hostname: '127.0.0.1', hasWasm: true });
  assert.strictEqual(ladder[0].door, 'agent');
  assert.strictEqual(ladder[0].endpoint, '/engine');
  assert.strictEqual(rung, 3);
});

kase('loopback IPv6 literal', () => {
  for (const hostname of ['::1', '[::1]']) {
    const { ladder, rung } = Grimoire.pick({ hostname, hasWasm: true });
    assert.strictEqual(ladder[0].door, 'agent', `hostname ${hostname}`);
    assert.strictEqual(rung, 3, `hostname ${hostname}`);
  }
});

kase('public host in a frame', () => {
  const { ladder, rung } = Grimoire.pick({ hostname: 'example.com', inFrame: true, hasWasm: true });
  assert.strictEqual(ladder.length, 1);
  assert.strictEqual(ladder[0].door, 'wasm');
  assert.strictEqual(rung, 1);
  assert.ok(!ladder.some(d => d.door === 'agent'));
});

kase('public host top level', () => {
  const { ladder, rung } = Grimoire.pick({ hostname: 'example.com', inFrame: false, hasWasm: true });
  assert.strictEqual(ladder.length, 1);
  assert.strictEqual(ladder[0].door, 'wasm');
  assert.strictEqual(rung, 2);
});

kase('no wasm', () => {
  const { ladder, why } = Grimoire.pick({ hostname: 'example.com', hasWasm: false });
  assert.strictEqual(ladder.length, 0);
  assert.ok(why && why.length > 0 && /WebAssembly/i.test(why), why);
});

kase('no wasm and in a frame', () => {
  const { ladder, rung, why } = Grimoire.pick({ hostname: 'example.com', inFrame: true, hasWasm: false });
  assert.strictEqual(ladder.length, 0);
  assert.strictEqual(rung, 1);
  assert.ok(/WebAssembly/i.test(why), why);
});

kase('unknown fields absent', () => {
  const { ladder, rung, why } = Grimoire.pick({});
  assert.strictEqual(ladder.length, 0);
  assert.strictEqual(rung, 1);
  assert.ok(why.length > 0);
});

kase('agent probe failed', () => {
  const { ladder, rung, why } = Grimoire.pick({
    hostname: '127.0.0.1', hasWasm: true, agentProbe: { ok: false, reason: 'refused' },
  });
  assert.strictEqual(ladder.length, 1);
  assert.strictEqual(ladder[0].door, 'wasm');
  assert.strictEqual(rung, 3);
  assert.ok(/agent/i.test(why), why);
});

kase('agent probe succeeded', () => {
  const { ladder, rung } = Grimoire.pick({
    hostname: '127.0.0.1', hasWasm: true, agentProbe: { ok: true },
  });
  assert.strictEqual(ladder[0].door, 'agent');
  assert.strictEqual(ladder[0].endpoint, '/engine');
  assert.strictEqual(rung, 3);
});

kase('both doors available', () => {
  const env = Object.freeze({ hostname: '127.0.0.1', hasWasm: true, agentProbe: null });
  const first = Grimoire.pick(env);
  assert.strictEqual(first.ladder.length, 2);
  assert.strictEqual(first.ladder[0].door, 'agent');
  assert.strictEqual(first.ladder[1].door, 'wasm');
  // EDGE-005: the order is deterministic, asserted over a hundred calls, not one.
  for (let i = 0; i < 100; i++) {
    assert.deepStrictEqual(Grimoire.pick(env), first);
  }
});

kase('nothing available', () => {
  const { ladder, rung, why } = Grimoire.pick({
    hostname: '127.0.0.1', hasWasm: false, agentProbe: { ok: false, reason: 'refused' },
  });
  assert.strictEqual(ladder.length, 0);
  assert.strictEqual(rung, 3);
  assert.ok(/WebAssembly/i.test(why) && /agent/i.test(why), why);
});

// AC-006: pick is pure over a frozen input, and mutates nothing it was handed.
kase('purity', () => {
  const env = Object.freeze({ search: '', hostname: '127.0.0.1', inFrame: false, hasWasm: true, agentProbe: null });
  const clone = JSON.parse(JSON.stringify(env));
  const first = Grimoire.pick(env);
  for (let i = 0; i < 100; i++) {
    assert.deepStrictEqual(Grimoire.pick(env), first);
  }
  assert.deepStrictEqual(env, clone);
});

// REQ-007/REQ-009: open() walks the ladder, adopts the first door, and publishes the facts.
kase('open: adopts an explicit remote door and publishes it', async () => {
  const G = freshGrimoire();
  let published = null;
  const io = {
    connect: async (endpoint) => ({ tag: 'remote-engine', endpoint }),
    wasm: async () => { throw new Error('should not be called'); },
    publish: (meta) => { published = meta; },
  };
  const eng = await G.open({ search: '?engine=http://x/engine', hasWasm: true }, io);
  assert.strictEqual(eng.door, 'remote');
  assert.strictEqual(eng.endpoint, 'http://x/engine');
  assert.strictEqual(published.door, 'remote');
});

// REQ-008/EDGE-004: a probe that answers but not as the engine is skipped, not adopted.
kase('open: skips a malformed agent probe and falls back to wasm', async () => {
  const G = freshGrimoire();
  const io = {
    connect: async () => { throw new Error('/engine answered, but not with a well-formed engine response'); },
    wasm: async () => ({ tag: 'wasm-engine' }),
    publish: () => {},
  };
  const eng = await G.open({ hostname: '127.0.0.1', hasWasm: true }, io);
  assert.strictEqual(eng.door, 'wasm');
});

// EDGE-001/REQ-005: an explicit door that refuses does not fall back to wasm.
kase('open: an explicit door that refuses does not fall back', async () => {
  const G = freshGrimoire();
  const io = {
    connect: async () => { throw new Error('http://nowhere/engine: engine 404: no'); },
    wasm: async () => ({ tag: 'wasm-engine' }),
    publish: () => {},
  };
  await assert.rejects(
    () => G.open({ search: '?engine=http://nowhere/engine', hasWasm: true }, io),
    /http:\/\/nowhere\/engine/,
  );
});

// EDGE-008: a second call returns the same engine rather than opening a second door.
kase('open: called twice returns the same engine', async () => {
  const G = freshGrimoire();
  let wasmCalls = 0;
  const io = {
    connect: async () => { throw new Error('no agent here'); },
    wasm: async () => { wasmCalls++; return { tag: 'wasm-engine' }; },
    publish: () => {},
  };
  const env = { hostname: '127.0.0.1', hasWasm: true };
  const first = await G.open(env, io);
  const second = await G.open(env, io);
  assert.strictEqual(first, second);
  assert.strictEqual(wasmCalls, 1);
});

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
  if (failed) {
    console.error(`${failed} of ${cases.length} case(s) failed`);
    process.exit(1);
  }
})();
