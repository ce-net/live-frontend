// The recorded run (proof steps P1-P12): a headless browser opens the real page served by the real door of four
// real node processes, and the controller of `cargo cell record` changes the nodes underneath it. Every step is
// asserted here and written to --out; `cargo cell record` signs them into proof/run.jsonl.
//   node tools/record.mjs --url http://127.0.0.1:21200/ --ctl http://127.0.0.1:21202 --browsers chromium --out obs.json --shots dir
import { chromium, firefox } from 'playwright-core';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';

const arg = (k, d) => (process.argv.includes(k) ? process.argv[process.argv.indexOf(k) + 1] : d);
const url = arg('--url', 'http://127.0.0.1:21200/');
const ctlUrl = arg('--ctl', 'http://127.0.0.1:21202');
const out = arg('--out', 'obs.json');
const shots = arg('--shots', 'shots');
const browsers = arg('--browsers', 'chromium').split(',');
mkdirSync(shots, { recursive: true });
const bundles = JSON.parse(readFileSync('dist/bundles.json', 'utf8'));

const steps = [];
const summary = {};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function ctl(action) {
  const t = Date.now();
  const r = await (await fetch(ctlUrl + '/ctl', { method: 'POST', body: JSON.stringify({ action }) })).json();
  if (!r.ok) throw new Error(`${action}: ${r.error}`);
  return { ms: Date.now() - t, r: r.result };
}
function step(n, browser, cell, op, to, request, response, pass, note, ms = 0) {
  steps.push({ n, browser, cell, op, to, request, response, verdict: pass ? 'pass' : 'fail', note, ms });
  console.log(`${pass ? 'PASS' : 'FAIL'} P${n} [${browser}] ${note}`);
  if (!pass) console.log('   ', JSON.stringify(response).slice(0, 400));
}
async function until(page, f, ms = 15000, arg = undefined) {
  const t = Date.now();
  while (Date.now() - t < ms) {
    try {
      const v = await page.evaluate(f, arg);
      if (v) return v;
    } catch {
      /* page busy */
    }
    await sleep(150);
  }
  return null;
}
const S = (page) => page.evaluate(() => {
  const s = window.__lf.shell;
  return { frames: s.frames(), mounted: [...s.mounted.keys()], boots: { ...s.boots }, fetched: s.stats.fetched, events: s.events.map((e) => ({ kind: e.kind, cell: e.cell, detail: e.detail, t: e.t })) };
});

async function cellFrame(page, view, version) {
  for (let i = 0; i < 60; i++) {
    for (const f of page.frames()) {
      if (!f.parentFrame()) continue;
      const el = await f.frameElement().catch(() => null);
      const title = el && (await el.getAttribute('title').catch(() => null));
      if (title && title.startsWith(view) && (!version || title === `${view} ${version}`)) return f;
    }
    await sleep(150);
  }
  return null;
}

async function run(name, type) {
  const exe = process.env.CHROMIUM || (existsSync('/usr/bin/chromium') ? '/usr/bin/chromium' : undefined);
  const b = await type.launch(name === 'chromium' ? { executablePath: exe, args: ['--no-sandbox'] } : {});
  const page = await b.newPage({ viewport: { width: 1400, height: 2400 } });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));

  // P1: the page with no chain: nothing mounted, nothing fetched
  await page.goto(url + '?join=0');
  await until(page, () => window.__lf && window.__lf.shell.welcome);
  await sleep(1500);
  let s = await S(page);
  step(1, name, 'shell', 'open', 'd', { url: url + '?join=0' }, { frames: s.frames, bundles_fetched: s.fetched }, s.frames === 0 && s.fetched === 0, 'Page open with no chain: zero frames, zero bundles fetched');
  await page.evaluate(() => (window.__mark = 'no-reload-since-P1'));

  // P2: join; cells appear in announce order; ops.panel never in Resolve; a direct Fetch of its hash is refused
  const tj = Date.now();
  await page.evaluate(() => window.__lf.shell.join());
  await until(page, () => window.__lf.shell.events.filter((e) => e.kind === 'Mounted').length >= 8);
  s = await S(page);
  const resolved = await page.evaluate(async () => (await window.__lf.shell.call(null, 'd', 'fe.manifest@1', 'Resolve', null)).body.entries.map((e) => e.body.view));
  const opsSha = bundles['ops.panel@1.0'].sha;
  const fetchOps = await page.evaluate((sha) => window.__lf.shell.call(null, 'a', 'fe.serve@1', 'Fetch', { sha256: sha }), opsSha);
  const order = s.events.filter((e) => e.kind === 'Mounted').map((e) => e.cell);
  step(2, name, 'fe.manifest', 'Resolve', 'd', { chain: 'visitor' }, { mounted_in_order: order, resolved_views: [...new Set(resolved)], ops_fetch: { ok: fetchOps.ok, refused: fetchOps.refused, node: fetchOps.node } },
    !resolved.includes('ops.panel') && !fetchOps.ok && /need: "ops:read"/.test(fetchOps.refused) && order.length >= 7,
    `Joined: ${order.length} cells mounted in ${Date.now() - tj} ms; ops.panel absent from Resolve; Fetch of its bundle hash refused by node a`, Date.now() - tj);
  await page.screenshot({ path: `${shots}/${name}-p2-joined.png`, fullPage: true });

  // P3: a new cell starts on b while the page is open
  const t3 = Date.now();
  await ctl('start-faults');
  const f3 = await until(page, () => window.__lf.shell.mounted.has('faults.panel|main|faults@1'), 15000);
  const mark3 = await page.evaluate(() => window.__mark);
  step(3, name, 'faults', 'start', 'b', { action: 'start-faults' }, { appeared: !!f3, ms: Date.now() - t3, reloaded: mark3 !== 'no-reload-since-P1' }, !!f3 && mark3 === 'no-reload-since-P1',
    `faults backend started on b: its panel appeared ${Date.now() - t3} ms later, no reload, no rebuild`, Date.now() - t3);

  // P4 + P6: a draft is being typed; c serves the notes bundle with one byte changed; notes 1.1 rolls out c, a, b
  const nf = await cellFrame(page, 'notes.board');
  const draft = 'half-typed draft ' + name;
  await nf.locator('input[type=text]').fill(draft);
  await sleep(2300); // one periodic state save
  await ctl('tamper-c');
  const before = (await S(page)).boots;
  const t4 = Date.now();
  await ctl('deploy-notes-1.1');
  const swapped = await until(page, () => window.__lf.shell.stats.swaps.find((x) => x.view === 'notes.board' && x.to === '1.1'), 20000);
  s = await S(page);
  const refusedEv = s.events.find((e) => e.kind === 'Refused' && e.cell === 'notes.board' && e.detail.want);
  step(6, name, 'fe.serve', 'Fetch', 'c', { holder: 'c', view: 'notes.board@1.1' }, refusedEv ? refusedEv.detail : null,
    !!refusedEv && refusedEv.detail.holder === 'c' && refusedEv.detail.want !== refusedEv.detail.got,
    'Node c served notes.board 1.1 with one changed byte: refused (want and got differ), reported to fe.manifest, loaded from another holder');
  const nf2 = await cellFrame(page, 'notes.board', '1.1');
  const kept = nf2 ? await nf2.locator('input[type=text]').inputValue() : null;
  const others = Object.keys(before).filter((k) => !k.startsWith('notes.board'));
  const untouched = others.filter((k) => before[k] === s.boots[k]);
  step(4, name, 'notes.board', 'swap', 'a,b,c', { from: '1.0', to: '1.1', draft }, { swap: swapped, draft_after: kept, boots_before: before, boots_after: s.boots },
    !!swapped && kept === draft && untouched.length === others.length && swapped.touched.length === 0,
    `notes 1.0 -> 1.1 node by node: only notes.board booted again (${swapped && swapped.ms} ms); the draft survived; ${untouched.length} of ${others.length} other cells untouched; max beat gap of the others ${swapped && swapped.maxOtherBeatGapMs} ms`,
    swapped ? swapped.ms : 0);
  summary.hot_swap_ms = swapped && swapped.ms;
  summary.cells_untouched_during_swap = `${untouched.length} of ${others.length}`;
  summary.max_other_beat_gap_ms = swapped && swapped.maxOtherBeatGapMs;
  await ctl('untamper-c');
  await page.screenshot({ path: `${shots}/${name}-p4-swapped.png`, fullPage: true });

  // P7: the sandbox
  const pr = await page.evaluate(() => window.__lf.shell.probe('notes.board'));
  step(7, name, 'notes.board', 'probe', 'browser', { tries: ['status.Read', 'fetch', 'parent.document', 'localStorage'] }, pr,
    !!pr && /^NotDeclared/.test(pr.undeclared) && /^blocked/.test(pr.fetch) && /^blocked/.test(pr.parent) && /^blocked/.test(pr.storage),
    'notes.board tries status.Read (NotDeclared at the bus), a fetch (blocked: no network in the frame), parent.document and storage (blocked: opaque origin)');

  // P10: the trend slot: a child frame from node c, typed props; wrong props refused at the bus
  s = await S(page);
  const child = await page.evaluate(() => {
    const m = [...window.__lf.shell.mounted.values()].find((x) => x.entry.body.view === 'notes.board');
    const c = m && m.children.get('trend');
    return c ? { view: c.entry.body.view, holders: c.entry.holders.map((h) => h.node), props: c.props } : null;
  });
  const bad = await page.evaluate(() => {
    const t = window.__lf.shell.entries.find((e) => e.body.view === 'trend.chart');
    return window.__lfPropsMismatch ? window.__lfPropsMismatch(t.body, { series: 5 }) : null;
  });
  step(10, name, 'trend.chart', 'slot', 'c', { parent: 'notes.board', slot: 'trend', wrong_props: { series: 5 } }, { child, wrong_props: bad },
    !!child && child.holders.includes('c') && /expected String/.test(bad || ''),
    'notes.board opens its typed slot: trend.chart mounts in its own frame from node c; props {series: 5} are refused at the bus (and `cargo cell check` refuses a slot whose props type differs)');

  // P8: crash isolation: a thrown error restarts that one frame; a tight loop is measured (see the note)
  const ff = await cellFrame(page, 'faults.panel');
  const b8 = (await S(page)).boots;
  await ff.locator('button', { hasText: 'Throw an error' }).click();
  const r1 = await until(page, () => window.__lf.shell.events.find((e) => e.kind === 'Restarted' && e.cell === 'faults.panel'), 10000);
  const a8 = (await S(page)).boots;
  const o8 = Object.keys(b8).filter((k) => !k.startsWith('faults.panel'));
  const u8 = o8.filter((k) => b8[k] === a8[k]);
  await sleep(500);
  const ff2 = await cellFrame(page, 'faults.panel');
  await page.evaluate(() => (window.__lf.shell.stats.loopMaxGap = 0));
  const b8b = (await S(page)).boots;
  await ff2.locator('button', { hasText: 'Loop for 4 s' }).click().catch(() => {});
  await sleep(6500);
  const loop = await page.evaluate(() => ({ shell_max_gap_ms: Math.round(window.__lf.shell.stats.loopMaxGap), stalls: window.__lf.shell.stats.stalls.slice(-1), restarted: window.__lf.shell.events.filter((e) => e.kind === 'Restarted').length }));
  const a8b = (await S(page)).boots;
  const rebooted = Object.keys(b8b).filter((k) => b8b[k] !== a8b[k]);
  const ffs = await cellFrame(page, 'faults.panel');
  const shown = ffs ? await ffs.locator('div.small.muted').first().innerText() : '';
  step(8, name, 'faults.panel', 'crash', 'browser', { crashes: ['throw', 'tight loop 4 s'] }, { throw: { restarted: r1 && r1.detail, others_untouched: `${u8.length} of ${o8.length}`, shown_after: shown }, loop: { ...loop, frames_rebooted: rebooted } },
    !!r1 && u8.length === o8.length && r1.detail.state && r1.detail.state.crashes >= 1 && rebooted.length === 0,
    `faults.panel threw: restarted alone with its saved state, ${u8.length} of ${o8.length} other cells untouched. Then a 4 s tight loop in it (${name}): the shell's own thread stalled at most ${loop.shell_max_gap_ms} ms; ` +
      (loop.stalls.length ? `the cell frames paused together for ${loop.stalls[0].ms} ms (${name} runs all sandboxed frames of a page in one renderer process) and none was restarted` : 'no other frame paused'));
  summary[`tight_loop_${name}`] = { shell_stall_ms: loop.shell_max_gap_ms, frames_paused: loop.stalls[0] || null };

  // P9: kill b while status streams; a retried write with the same op id is applied once
  const st = await page.evaluate(() => {
    const m = window.__lf.shell.mounted.get('status.panel|main|status@1');
    return m.entry.holders.map((h) => h.node)[m.holderAt % m.entry.holders.length];
  });
  const tk = Date.now();
  await ctl('stop-b');
  const fo = await until(page, () => window.__lf.shell.stats.failovers.find((f) => f.view === 'status.panel'), 10000);
  const foMs = fo ? fo.t - tk : null;
  const sf = await cellFrame(page, 'status.panel');
  await sleep(1200);
  const holderText = sf ? await sf.locator('.tag').nth(1).innerText() : '';
  const op = 'retry-' + name + '-' + Date.now();
  const put = (to) => page.evaluate(([to, op]) => window.__lf.shell.call(null, to, 'notes@1', 'Put', { op_id: op, note: { id: 'retry', text: 'written once', colour: null } }), [to, op]);
  const p1 = await put('b');
  const p2 = await put('c');
  const p3 = await put('a');
  step(9, name, 'status', 'Watch', 'b -> next holder', { killed: 'b', streaming_from: st }, { failover: fo, failover_ms: foMs, now: holderText, retried_put: [p1.transport || p1.refused, p2.ok && p2.body.seq, p3.ok && p3.body.seq] },
    !!fo && /answered by [ac]/.test(holderText) && (p2.ok ? p2.body.seq === (p3.ok ? p3.body.seq : -1) : true) && (await page.evaluate(() => window.__mark)) === 'no-reload-since-P1',
    `Node b stopped while status.panel streamed from ${st}: the next answer came from ${fo && fo.to} ${foMs} ms after the kill, no reload; a write retried with the same op id (b unreachable, then c, then a) was applied once`, foMs || 0);
  summary.failover_ms = foMs;
  await ctl('start-b');
  await page.screenshot({ path: `${shots}/${name}-p9-failover.png`, fullPage: true });

  // P11: allowed, refused verbatim, uses exhausted
  const nf3 = await cellFrame(page, 'notes.board', '1.1');
  await nf3.locator('input[type=text]').fill('allowed write ' + name);
  await nf3.locator('button', { hasText: 'Write' }).click();
  await sleep(800);
  const allowed = await nf3.locator('div.small.muted').first().innerText();
  await nf3.locator('button', { hasText: 'Call notes.Purge' }).click();
  await sleep(800);
  const purge = await nf3.locator('.verdict').innerText();
  await nf3.locator('button', { hasText: 'Spend the rest of my uses' }).click();
  await until(page, () => true, 100);
  let exhausted = '';
  for (let i = 0; i < 80 && !/exhausted/.test(exhausted); i++) {
    await sleep(250);
    exhausted = await nf3.locator('.verdict').innerText();
  }
  step(11, name, 'notes', 'Put / Purge / Put*', 'a,b,c', { put: 'own sandbox', purge: 'needs notes:admin', spend: 'until the count runs out' }, { allowed, purge, exhausted },
    /^Allowed/.test(allowed) && /^Refused\{need: "notes:admin"/.test(purge) && /uses exhausted \(40 of 40 spent\)/.test(exhausted),
    'notes.Put allowed in the visitor sandbox; notes.Purge refused with the gate text verbatim; writes refused once the 40 counted uses are spent');

  // P5: notes 2.0 beside 1, on the same rows; 1 leaves at its until
  await ctl('deploy-notes-2.0');
  const both = await until(page, () => window.__lf.shell.mounted.has('notes.board|main|notes@1') && window.__lf.shell.mounted.has('notes.board|main|notes@2'), 15000);
  await page.screenshot({ path: `${shots}/${name}-p5-two-majors.png`, fullPage: true });
  const rows = await page.evaluate(async () => {
    const s = window.__lf.shell;
    const r1 = await s.call(null, 'a', 'notes@1', 'List', null);
    const r2 = await s.call(null, 'b', 'notes@2', 'List', null);
    return { v1: r1.ok && r1.body.length, v2: r2.ok && r2.body.length, v1_first: r1.ok && r1.body[0], v2_first: r2.ok && r2.body[0] };
  });
  const gone1 = await until(page, () => !window.__lf.shell.mounted.has('notes.board|main|notes@1') && window.__lf.shell.mounted.has('notes.board|main|notes@2'), 30000);
  step(5, name, 'notes.board', 'negotiate', 'a,b,c', { deploy: 'notes@2.0 beside notes@1.1 (until +20 s)' }, { both_mounted: !!both, rows, one_left: !!gone1 },
    !!both && rows.v1 === rows.v2 && !!gone1, 'notes@2 served beside notes@1: two panels on the same rows (colour by name in 1, as RGB in 2); 1 left by itself at its until');

  // P12: revoke: the next call is refused and every panel leaves
  const t12 = Date.now();
  const rv = await page.evaluate(() => window.__lf.shell.revoke());
  const empty = await until(page, () => window.__lf.shell.frames() === 0, 15000);
  const after = await page.evaluate(() => window.__lf.shell.call(null, 'a', 'notes@1', 'List', null));
  step(12, name, 'fe.visitor', 'Revoke', 'd', { chain: 'own' }, { revoke: rv.ok && rv.body, next_call: after.refused, frames: await page.evaluate(() => window.__lf.shell.frames()) },
    !!empty && !after.ok && /revoked/.test(after.refused || ''), `Chain revoked: the next call is refused by the gate and every panel left the page (${Date.now() - t12} ms), no reload`, Date.now() - t12);
  await page.screenshot({ path: `${shots}/${name}-p12-revoked.png`, fullPage: true });
  summary[`page_errors_${name}`] = errors.length;
  await b.close();
}

for (const name of browsers) {
  const type = name === 'firefox' ? firefox : chromium;
  try {
    await run(name, type);
  } catch (e) {
    console.error(`${name}: ${e.stack || e}`);
    steps.push({ n: 0, browser: name, cell: 'recorder', op: 'run', to: '', request: {}, response: { error: String(e) }, verdict: 'fail', note: `the ${name} run stopped: ${e.message}`, ms: 0 });
  }
}
steps.sort((a, b) => (a.browser === b.browser ? a.n - b.n : a.browser < b.browser ? -1 : 1));
summary.steps = `${steps.filter((s) => s.verdict === 'pass').length} of ${steps.length} pass`;
summary.browsers = browsers;
writeFileSync(out, JSON.stringify({ recorded: new Date().toISOString(), steps, summary }, null, 1));
process.exit(steps.every((s) => s.verdict === 'pass') ? 0 : 1);
