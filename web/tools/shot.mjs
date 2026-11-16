// node tools/shot.mjs <url> <out.png> [waitMs] [width]: a screenshot of the page with headless Chromium.
import { chromium } from 'playwright-core';
const [url, out, wait = '6000', width = '1400'] = process.argv.slice(2);
import { existsSync } from 'node:fs';
const exe = process.env.CHROMIUM || (existsSync('/usr/bin/chromium') ? '/usr/bin/chromium' : undefined);
const b = await chromium.launch({ executablePath: exe, args: ['--no-sandbox'] });
const p = await b.newPage({ viewport: { width: Number(width), height: 1000 } });
const logs = [];
p.on('console', (m) => logs.push(m.type() + ': ' + m.text()));
p.on('pageerror', (e) => logs.push('pageerror: ' + e.message));
await p.goto(url);
await p.waitForTimeout(Number(wait));
await p.screenshot({ path: out, fullPage: true });
const st = await p.evaluate(() => { const l = window.__lf; return l ? { frames: l.shell.frames(), mounted: [...l.shell.mounted.keys()], events: l.shell.events.slice(-12).map((e) => e.kind + ' ' + e.cell + ' ' + JSON.stringify(e.detail).slice(0, 120)) } : null; });
console.log(JSON.stringify(st, null, 1));
console.log(logs.slice(-15).join('\n'));
await b.close();
