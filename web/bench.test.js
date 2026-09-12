// Drives bench.html with the REAL engine.
//
// The wasm target is not installable in every environment, and a page that is only ever
// eyeballed is a page that is not tested. So the harness replaces `Grimoire.load` with a
// shim that pipes each request to `grimoire dispatch` — the same `grimoire_wasm::dispatch`
// function the wasm module wraps. The transport differs; the code answering does not.
//
//   node web/bench.test.js
//
// Needs: cargo build --release, a corpus at web/corpus.grim, and playwright.

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

const ROOT = path.join(__dirname);
/* GRIMOIRE_BIN lets the gate point at whichever binary it already built. A git worktree has no
   target/ at all (it is gitignored), so hard-coding the release path made this file unrunnable in
   exactly the place the harness runs it. Default is unchanged for a hand run. */
const BIN = process.env.GRIMOIRE_BIN || path.join(__dirname, '..', 'target', 'release', 'grimoire');

const engine = (json) =>
  execFileSync(BIN, ['dispatch'], { input: json + '\n', maxBuffer: 1 << 28 })
    .toString().trim();

function serve() {
  return http.createServer((req, res) => {
    const f = path.join(ROOT, decodeURIComponent(req.url.split('?')[0]).replace(/^\/+/, ''));
    fs.readFile(f, (err, body) => {
      if (err) { res.writeHead(404); return res.end('no'); }
      res.writeHead(200, { 'content-type': f.endsWith('.js') ? 'text/javascript' : 'text/html' });
      res.end(body);
    });
  });
}

let failures = 0;
function check(name, ok, detail = '') {
  console.log(`${ok ? '  ok  ' : ' FAIL '} ${name}${ok || !detail ? '' : '\n         ' + detail}`);
  if (!ok) failures++;
}

(async () => {
  const server = serve();
  const port = await listen(server);
  console.log(`serving web/ on http://127.0.0.1:${port}`);
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });

  const errors = [];
  page.on('pageerror', e => errors.push(String(e)));

  await page.exposeFunction('__engine', engine);

  // Swap the wasm loader for the shim before any of the page's own script runs.
  await page.addInitScript(() => {
    window.__patch = () => {
      Grimoire.load = async () => ({
        call: r => {
          // The shim is async by nature; the page is synchronous by design. Requests are
          // pre-resolved into a cache below, so this stays a plain function.
          const key = JSON.stringify(r);
          if (!(key in window.__cache)) throw new Error('uncached: ' + key.slice(0, 90));
          return window.__cache[key];
        },
        chance(s, t, m = 0) { return this.call({ op: 'chance', skill: s, trivial: t, mastery: m }); },
        quote(rec, q, h, b = false) { return this.call({ op: 'quote', recipe: rec, qty: q, hand: h, buyer_supplies: b }); },
        recipe(c, k) { return this.call({ op: 'recipe', corpus: Array.from(c), key: k }); },
        catalogue(c) { return this.call({ op: 'catalogue', corpus: Array.from(c) }); },
      });
    };
  });

  // Pre-resolve every request the page will make, through the real engine.
  const corpus = Array.from(fs.readFileSync(path.join(ROOT, 'corpus.grim')));
  const cache = {};
  const put = (req) => { cache[JSON.stringify(req)] = JSON.parse(engine(JSON.stringify(req))); };

  put({ op: 'catalogue', corpus });
  const cat = cache[JSON.stringify({ op: 'catalogue', corpus })];

  // The recipe the harness will click: one whose trivial the log pinned.
  const target = cat.find(r => r.name === 'Gold Malachite Bracelet');
  put({ op: 'recipe', corpus, key: target.key });
  const t = target.trivial;
  for (const skill of [t, t - 60, t - 40, t - 20, t + 20].filter(s => s >= 0)) {
    put({ op: 'chance', skill, trivial: t, mastery: 0 });
    put({ op: 'quote', recipe: cache[JSON.stringify({ op: 'recipe', corpus, key: target.key })],
          qty: 10, hand: { skill, mastery: 0, courtesy: 0, owns_tools: true }, buyer_supplies: false });
  }

  await page.addInitScript(c => { window.__cache = c; }, cache);
  await page.goto(`http://127.0.0.1:${port}/bench.html`);
  await page.evaluate(() => { window.__patch(); return boot(); });

  // --- the page is now running on real engine output ---
  check('app is shown, boot banner is not',
    !(await page.$eval('#app', e => e.hidden)) && (await page.$eval('#boot', e => e.hidden)));

  const count = await page.textContent('#count');
  check('catalogue counts the real corpus', /294 recipes/.test(count), count);

  await page.fill('#q', 'Gold Malachite Bracelet');
  await page.click('.item');
  const out = await page.textContent('#out');

  check('names the recipe and its measured trivial',
    out.includes('Gold Malachite Bracelet') && out.includes('trivial 146'), out.slice(0, 120));

  // 146 at trivial 146 is 88% by the classic formula, and grey — which is the warning case.
  check('shows the chance beside the con colour', /88%/.test(out), out.slice(0, 200));
  check('warns that grey is not safe', /grey means no skill-up, not safe/.test(out));

  const totals = await page.$$eval('#out table:last-of-type tr td:last-child',
    tds => tds.map(td => td.textContent.trim()));
  check('the skill band is priced worst-to-best', totals.length >= 4, totals.join(' | '));

  const asCopper = s => {
    let n = 0;
    for (const [v, u] of [[1000, 'p'], [100, 'g'], [10, 's'], [1, 'c']]) {
      const m = s.match(new RegExp(`(\\d+)${u}`));
      if (m) n += +m[1] * v;
    }
    return n;
  };
  const nums = totals.map(asCopper);
  check('a worse hand costs strictly more',
    nums.every((n, i) => i === 0 || n <= nums[i - 1]), totals.join(' | '));

  check('no uncaught JS errors', errors.length === 0, errors.join('\n'));

  const png = await shot(page, 'bench.png', { fullPage: true });
  console.log(`\n${failures ? failures + ' FAILED' : 'all checks passed'} — screenshot ${png}`);

  await browser.close();
  server.close();
  process.exit(failures ? 1 : 0);
})();
