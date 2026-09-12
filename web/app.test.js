// Drives app.html — the designed UI — against the REAL engine.
//
// The wasm target will not install everywhere, so the harness swaps `Grimoire.load` for a
// shim backed by `grimoire dispatch`: the same `grimoire_wasm::dispatch` the wasm module
// wraps. The page is synchronous by design, so requests are resolved in batches between
// reloads — the page records what it asked for, the harness answers it, and it reloads until
// nothing is missing. Usually two rounds.
//
//   node web/app.test.js [--shots]

/* Playwright is RESOLVED, not hard-pathed. This repo deliberately carries no package.json
   (no npm, per README), so playwright lives wherever npm put it globally - a different path on
   every machine and every OS. The previous literal was one container's Linux path and made this
   file unrunnable anywhere else, which is why 22 of the 52 stories touching web/ had no gate.
   PW overrides; otherwise ask npm where its global root is. */
function pwPath() {
  if (process.env.PW) return process.env.PW;
  try {
    const root = require("child_process")
      .execFileSync("npm", ["root", "-g"], { encoding: "utf8", shell: true }).trim();
    return require("path").join(root, "playwright");
  } catch { return "playwright"; }
}
const { chromium } = require(pwPath());
const { execFileSync } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
/* Ephemeral port + per-run output directory. The harness runs items in PARALLEL worktrees; a
   hardcoded port and a /tmp screenshot made five concurrent copies of this suite fight over two
   global resources. See web/testenv.js. */
const { listen, shot } = require('./testenv.js');

const ROOT = __dirname;
/* GRIMOIRE_BIN lets the gate point at whichever binary it already built. A git worktree has no
   target/ at all (it is gitignored), so hard-coding the release path made this file unrunnable in
   exactly the place the harness runs it. Default is unchanged for a hand run. */
const BIN = process.env.GRIMOIRE_BIN || path.join(ROOT, '..', 'target', 'release', 'grimoire');
const SHOTS = process.argv.includes('--shots');

// One process, many requests — spawning per call would take minutes.
function engineBatch(requests) {
  if (!requests.length) return [];
  const out = execFileSync(BIN, ['dispatch', '--corpus', path.join(ROOT, 'corpus.grim')], {
    input: requests.join('\n') + '\n',
    maxBuffer: 1 << 30,
  }).toString().trim().split('\n');
  if (out.length !== requests.length) {
    throw new Error(`asked ${requests.length}, got ${out.length} back`);
  }
  return out.map(JSON.parse);
}

const server = http.createServer((req, res) => {
  const f = path.join(ROOT, decodeURIComponent(req.url.split('?')[0]).replace(/^\/+/, ''));
  fs.readFile(f, (err, body) => {
    if (err) { res.writeHead(404); return res.end('no'); }
    res.writeHead(200, {
      'content-type': f.endsWith('.js') ? 'text/javascript'
        : f.endsWith('.html') ? 'text/html' : 'application/octet-stream',
    });
    res.end(body);
  });
});

let failures = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? '  ok  ' : ' FAIL '} ${name}${ok || !detail ? '' : '\n         ' + detail}`);
  if (!ok) failures++;
};

(async () => {
  const port = await listen(server);
  console.log(`serving web/ on http://127.0.0.1:${port}`);
  const browser = await chromium.launch();
  let page = null;
  const errors = [];

  // Seed with what the page certainly needs, so round one is not 294 misses.
  const catReq = JSON.stringify({ op: 'catalogue' });
  const cache = { [catReq]: engineBatch([catReq])[0] };
  const pending = cache[catReq].map(c => JSON.stringify({ op: 'recipe', key: c.key }));
  engineBatch(pending).forEach((v,i)=>{cache[pending[i]]=v;});
  console.log(`seeded ${Object.keys(cache).length + pending.length} engine answers`);

  // A fresh context each round: init scripts accumulate, and a reused page carries the
  // previous round's failed render with it.
  const freshPage = async (c) => {
    const ctx = await browser.newContext({ viewport: { width: 1440, height: 1100 } });
    const p = await ctx.newPage();
    p.on('pageerror', e => errors.push(String(e)));
    await p.addInitScript(c2 => {
      window.__cache = c2;
      window.__miss = [];
      window.__shim = () => {
        Grimoire.load = async () => ({
          call(r) {
            const k = JSON.stringify(r);
            if (k in window.__cache) return window.__cache[k];
            window.__miss.push(k);
            // Throwing here would stop the render at the first miss, so each round would
            // discover exactly one. Returning a shaped stub lets the page finish and
            // surface every miss at once. The round that asserts has a full cache.
            window.__stubbed = true;
            switch (r.op) {
              case 'chance': return { chance: 0.5, con: 'white', trivial_to_him: false };
              case 'quote': return { chance: 0.5, runs: 1, attempts: 1, materials: [],
                to_post: [], material_cost: 0, labour: 0, risk: 0, subtotal: 0,
                courtesy: 0, total: 0 };
              case 'catalogue': return [];
              default: return {};
            }
          },
          chance(s, t, m = 0) { return this.call({ op: 'chance', skill: s, trivial: t, mastery: m }); },
          quote(rec, q, h, b = false) { return this.call({ op: 'quote', recipe: rec, qty: q, hand: h, buyer_supplies: b }); },
          // The corpus is dropped from the key: the harness answers from a file, and a
          // 139 KB byte array in every cache key is what killed the first attempt.
          recipe(_c, k) { return this.call({ op: 'recipe', key: k }); },
          catalogue() { return this.call({ op: 'catalogue' }); },
        });
      };
    }, c);

    // The shim has to be in place before the app's inline script runs, and a readystate
    // hook is far too late. Appending it to grimoire.js is the only ordering that holds.
    const loader = fs.readFileSync(path.join(ROOT, 'grimoire.js'), 'utf8');
    await p.route('**/grimoire.js', route => route.fulfill({
      contentType: 'text/javascript',
      body: loader + '\n;window.__shim();Grimoire.__patched=true;\n',
    }));
    return p;
  };

  // Load, then act, then answer whatever the page asked for that we did not have, and
  // repeat until it stops asking. Two or three rounds in practice.
  const settle = async (act) => {
    for (let round = 0; round < 8; round++) {
      if (page) await page.context().close();
      errors.length = 0;
      page = await freshPage(cache);
      await page.goto(`http://127.0.0.1:${port}/app.html`, { waitUntil: 'networkidle' });
      await page.waitForTimeout(200);
      if (act) { await act(page); await page.waitForTimeout(250); }
      const miss = [...new Set(await page.evaluate(() => window.__miss || []))]
        .filter(k => !(k in cache));
      if (!miss.length) {
        const stubbed = await page.evaluate(() => !!window.__stubbed);
        if (stubbed) throw new Error('a stub was used on the asserting round');
        return round;
      }
      engineBatch(miss).forEach((v, i) => { cache[miss[i]] = v; });
      console.log(`  round ${round + 1}: answered ${miss.length} more`);
    }
    throw new Error('the page never stopped asking for new engine answers');
  };

  await settle(null);
  check('the engine banner is absent — engine and corpus both loaded',
    (await page.$('#engine')) === null);

  // The catalogue only draws once a hall is chosen — that is the design, not a bug.
  await settle(p => p.click('.hall[data-craft="The Jewelbox"]'));

  const crafts = await page.$$eval('[data-add]', els => els.length);
  check('the catalogue is drawn from the real corpus', crafts > 100, `${crafts} rows`);

  const names = await page.$$eval('#cat tr,[data-add]', els => els.slice(0,400).map(e => e.textContent.trim()));
  check('it holds recipes the wiki gave us, not the old invented ones',
    names.some(n => /Malachite|Potion of Accuracy|Silver /.test(n)) &&
    !names.some(n => /Philter of the Wolf|Batwing Crunchies/.test(n)),
    names.slice(0, 4).join(' · '));

  // Themes
  const swatches = await page.$$eval('#themes b', b => b.map(x => x.dataset.t));
  check('four themes are offered', swatches.length === 4, swatches.join(', '));

  const themed = [];
  for (const t of swatches) {
    await page.click(`#themes b[data-t="${t}"]`);
    const applied = await page.evaluate(() => document.body.dataset.theme);
    const bg = await page.evaluate(() =>
      getComputedStyle(document.querySelector('.book') || document.body).backgroundColor);
    themed.push({ t, applied, bg });
    if (SHOTS) await shot(page, `app-${t}.png`);
  }
  check('each swatch applies its own theme', themed.every(x => x.t === x.applied),
    JSON.stringify(themed.map(x => `${x.t}->${x.applied}`)));
  check('the themes are visually distinct',
    new Set(themed.map(x => x.bg)).size >= 3, themed.map(x => `${x.t} ${x.bg}`).join(' | '));

  await page.click('#themes b[data-t="grimoire"]');

  // The engine is the only source of prices.
  const src = fs.readFileSync(path.join(ROOT, 'app.html'), 'utf8');
  check('no JavaScript copy of the combine formula survives',
    !/S-\.75\*T\+51\.5|S-T\+66/.test(src));
  check('no invented recipe or price table survives',
    /let R = \[\];/.test(src) && /let IT = \{\};/.test(src));

  check('no uncaught JS errors', errors.length === 0, errors.slice(0, 3).join('\n'));

  if (SHOTS) console.log('shots in ' + path.dirname(await shot(page, 'app.png')));
  console.log(`\n${failures ? failures + ' FAILED' : 'all checks passed'}`);

  await browser.close();
  server.close();
  process.exit(failures ? 1 : 0);
})();
