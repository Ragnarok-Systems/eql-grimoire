// The whole client-side binding to the engine. No build step, no npm, no bundler.
//
// The wasm module is the *only* place the Grimoire's maths lives. There is deliberately no
// JavaScript fallback: a fallback is a second implementation, and two implementations of a
// price is how a broker loses an argument it cannot win. If the engine is missing, callers
// get an exception and the UI says so.
//
//   const eng = await Grimoire.load('grimoire_wasm.wasm');
//   eng.chance(146, 146);                       // { chance, con, trivial_to_him }
//   eng.quote(recipe, 10, hand);                // full quote
//   eng.harvest(await file.text());             // crafting out of a log
//
// Build the module with:
//   rustup target add wasm32-unknown-unknown
//   cargo build --release -p grimoire-wasm --target wasm32-unknown-unknown

const Grimoire = (() => {
  const ABI = 1;

  class Engine {
    constructor(instance) {
      this.x = instance.exports;
      const got = this.x.grimoire_abi_version();
      if (got !== ABI) {
        throw new Error(`grimoire.js speaks ABI ${ABI}, the module speaks ${got} — rebuild one of them`);
      }
      this.enc = new TextEncoder();
      this.dec = new TextDecoder();
    }

    // The one door. Synchronous, which is what lets the UI call it inline while rendering.
    call(request) {
      const bytes = this.enc.encode(JSON.stringify(request));
      const ptr = this.x.grimoire_alloc(bytes.length);
      new Uint8Array(this.x.memory.buffer, ptr, bytes.length).set(bytes);

      // grimoire_call consumes the input buffer; do not free it here.
      const len = this.x.grimoire_call(ptr, bytes.length);
      const out = this.x.grimoire_result_ptr();
      // Copy before freeing — and before any later call can grow the heap and detach this view.
      const text = this.dec.decode(new Uint8Array(this.x.memory.buffer, out, len).slice());
      this.x.grimoire_free(out, len);

      const value = JSON.parse(text);
      if (value && value.error) throw new Error(`grimoire: ${value.error}`);
      return value;
    }

    chance(skill, trivial, mastery = 0) {
      return this.call({ op: 'chance', skill, trivial, mastery });
    }
    quote(recipe, qty, hand, buyerSupplies = false) {
      return this.call({ op: 'quote', recipe, qty, hand, buyer_supplies: buyerSupplies });
    }
    hands(recipe, qty, hands, buyerSupplies = false) {
      return this.call({ op: 'hands', recipe, qty, hands, buyer_supplies: buyerSupplies });
    }
    regard(score, ratings) {
      return this.call({ op: 'regard', score, ratings });
    }
    order(phase, actor, event) {
      return this.call({ op: 'order', phase, actor, event });
    }
    mayCommission(terms, sameServer, sameGuild, buyer) {
      return this.call({
        op: 'may_commission', terms,
        same_server: sameServer, same_guild: sameGuild, buyer,
      });
    }
    harvest(log) {
      return this.call({ op: 'harvest', log });
    }
    inventory(dump) {
      return this.call({ op: 'inventory', dump });
    }

    // A corpus is bytes. Hand it in as a plain array; the engine reads it through the same
    // range-reader the CDN path uses, so behaviour is identical whether it came from a file
    // input or a fetch.
    // `corpusBytes` may be null when the engine already holds the corpus (the dev server
    // does). Sending 139 KB of JSON array per lookup when the far side already has the file
    // is how a page that works becomes a page nobody waits for.
    catalogue(corpusBytes) {
      return this.call(corpusBytes ? { op: 'catalogue', corpus: Array.from(corpusBytes) }
                                   : { op: 'catalogue' });
    }
    recipe(corpusBytes, key) {
      return this.call(corpusBytes ? { op: 'recipe', corpus: Array.from(corpusBytes), key }
                                   : { op: 'recipe', key });
    }
  }

  // Money, rendered the way the game does: drop the empty denominations.
  function coin(copper) {
    if (!copper) return '0c';
    const sign = copper < 0 ? '−' : '';
    let n = Math.abs(copper);
    const parts = [];
    for (const [div, suffix] of [[1000, 'p'], [100, 'g'], [10, 's'], [1, 'c']]) {
      const q = Math.floor(n / div);
      n -= q * div;
      if (q) parts.push(`${q}${suffix}`);
    }
    return sign + parts.join(' ');
  }

  // A second *transport*, not a second engine.
  //
  // `grimoire serve` answers on POST /engine with the same `grimoire_wasm::dispatch` the
  // wasm module wraps, so a page driven this way computes exactly what the shipped page
  // computes. It exists because `wasm32-unknown-unknown` cannot be installed everywhere, and
  // a UI nobody can click is a UI nobody has tested.
  //
  // The request is synchronous because the app renders synchronously. `XMLHttpRequest` with
  // async=false is the only way to do that in a page, it is deprecated, and it is fine here:
  // localhost, development, one round trip per call.
  class Remote {
    constructor(endpoint) { this.endpoint = endpoint; }
    call(request) {
      const x = new XMLHttpRequest();
      x.open('POST', this.endpoint, false);
      x.setRequestHeader('content-type', 'application/json');
      x.send(JSON.stringify(request));
      if (x.status !== 200) throw new Error(`engine ${x.status}: ${x.responseText.slice(0, 120)}`);
      const value = JSON.parse(x.responseText);
      if (value && value.error) throw new Error(`grimoire: ${value.error}`);
      return value;
    }
  }
  // Same surface as Engine, so nothing downstream can tell the difference.
  for (const m of ['chance', 'quote', 'hands', 'regard', 'order', 'mayCommission',
                   'harvest', 'inventory', 'catalogue', 'recipe']) {
    Remote.prototype[m] = Engine.prototype[m];
  }

  async function connect(endpoint) {
    const r = new Remote(endpoint);
    let probe;
    try {
      probe = r.chance(100, 100);          // fail loudly here rather than mid-render
    } catch (e) {
      throw new Error(`${endpoint}: ${e.message}`);
    }
    // Absence of `.error` is not proof this is the engine — a static file server answering
    // 200 with `{}` would pass that bar. Check the shape a real `chance` reply has.
    if (!probe || typeof probe.chance !== 'number' || typeof probe.con !== 'string'
        || typeof probe.trivial_to_him !== 'boolean') {
      throw new Error(`${endpoint} answered, but not with a well-formed engine response`);
    }
    return r;
  }

  const LOOPBACK_HOSTS = new Set(['127.0.0.1', 'localhost', '::1', '[::1]']);

  /* Grimoire.pick(env) -> { ladder, rung, why }
   *
   * Order, fixed: an explicit `?engine=` parameter in `env.search` wins outright and produces
   * a ladder of exactly one door, `remote` — an operator who named a door does not get a
   * different one, even if it fails. Otherwise, a loopback hostname (127.0.0.1, localhost, or
   * the IPv6 loopback literal) offers the `agent` door first, because that is this page being
   * served by the local agent. `wasm` is offered next whenever WebAssembly is available — after
   * `agent` on a loopback hostname, alone everywhere else. A frame never offers `agent`, even on
   * a loopback hostname: the local agent is structurally unreachable from a sandboxed iframe.
   * When nothing qualifies, the ladder is empty and `why` names what was missing.
   *
   * Pure: reads only `env`'s own fields (no `location`, `window`, `navigator` or `fetch`),
   * every field may be absent, and `env` itself is never assigned to.
   */
  function pick(rawEnv) {
    const env = rawEnv || {};
    const search = env.search || '';
    const hostname = env.hostname || '';
    const inFrame = !!env.inFrame;
    const hasWasm = !!env.hasWasm;
    const agentProbe = env.agentProbe || null;

    const explicit = new URLSearchParams(search).get('engine');
    const loopback = LOOPBACK_HOSTS.has(hostname);
    const rung = inFrame ? 1 : loopback ? 3 : hasWasm ? 2 : 1;

    if (explicit) {
      const reason = `an explicit ?engine=${explicit} parameter`;
      return { ladder: [{ door: 'remote', endpoint: explicit, reason }], rung, why: reason };
    }

    const ladder = [];
    const excluded = [];

    if (loopback && inFrame) {
      excluded.push('a frame blocks the local agent even on a loopback hostname');
    } else if (loopback) {
      if (agentProbe && agentProbe.ok === false) {
        excluded.push(`the local agent at ${hostname} failed its probe` +
          (agentProbe.reason ? ` (${agentProbe.reason})` : ''));
      } else {
        const reason = agentProbe && agentProbe.ok
          ? 'the loopback agent answered a probe'
          : 'a loopback hostname, not yet probed';
        ladder.push({ door: 'agent', endpoint: '/engine', reason });
      }
    }

    if (hasWasm) {
      ladder.push({ door: 'wasm', endpoint: 'grimoire_wasm.wasm', reason: 'WebAssembly is available' });
    } else {
      excluded.push('this browser has no WebAssembly');
    }

    const why = !ladder.length ? `no door is available: ${excluded.join('; ')}`
              : excluded.length ? `${ladder[0].reason}, after ${excluded.join('; ')}`
              : ladder[0].reason;

    return { ladder, rung, why };
  }

  // Grimoire.open(env, io) walks pick(env)'s ladder and adopts the first door that answers.
  // `io` supplies the one impure act each door needs: `io.wasm(endpoint)` fetches and
  // instantiates the module; `io.connect(endpoint)` opens an HTTP transport, used for both
  // the `remote` and `agent` doors alike (the difference between them is only which endpoint,
  // and whether the caller named it explicitly). A door whose act throws is recorded in
  // `skipped` with the thrown message and the walk moves to the next rung. A second call
  // returns the engine the first call already opened rather than opening a second door.
  let liveEngine = null;
  async function open(env, io) {
    if (liveEngine) return liveEngine;
    const { ladder, rung, why } = pick(env);
    const skipped = [];
    for (const step of ladder) {
      try {
        const engine = step.door === 'wasm' ? await io.wasm(step.endpoint) : await io.connect(step.endpoint);
        Object.assign(engine, { door: step.door, endpoint: step.endpoint, rung, why: step.reason });
        if (io.publish) io.publish({ door: step.door, endpoint: step.endpoint, rung, why: step.reason });
        liveEngine = engine;
        return engine;
      } catch (e) {
        skipped.push({ door: step.door, endpoint: step.endpoint, reason: e.message });
      }
    }
    const err = new Error(skipped.length ? skipped[skipped.length - 1].reason : why);
    err.skipped = skipped;
    err.why = why;
    throw err;
  }

  async function load(url = 'grimoire_wasm.wasm') {
    let instance;
    try {
      // No imports at all — the module is freestanding, which is why there is no glue file
      // to keep in sync with it.
      const res = await fetch(url);
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
      ({ instance } = await WebAssembly.instantiate(await res.arrayBuffer(), {}));
    } catch (e) {
      throw new Error(
        `could not load the engine from ${url} (${e.message}). Build it with: ` +
        `cargo build --release -p grimoire-wasm --target wasm32-unknown-unknown`
      );
    }
    return new Engine(instance);
  }

  return { load, connect, coin, Engine, Remote, ABI, pick, open };
})();

if (typeof module !== 'undefined') module.exports = Grimoire;
