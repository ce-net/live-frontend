// One sim node: one wasm instance in its own Web Worker, with its own key. It ticks itself; everything it wants
// sent goes back to the page's router, which delivers it like a network would.
import { load, type Wasm } from './wasm';

let w: Wasm | null = null;
const post = (m: unknown) => (self as unknown as Worker).postMessage(m);

async function sha(s: string): Promise<string> {
  const d = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(s));
  return [...new Uint8Array(d)].map((x) => x.toString(16).padStart(2, '0')).join('');
}

self.onmessage = async (ev: MessageEvent) => {
  const d = ev.data;
  if (d.t === 'boot') {
    w = await load(d.wasm);
    const r = JSON.parse(w.call('lf_new', JSON.stringify(d.cfg)));
    post({ t: 'booted', r });
    setInterval(() => {
      const o = JSON.parse(w!.call('lf_input', JSON.stringify({ kind: 'tick', now: Date.now() })));
      if (o.out.length) post({ t: 'out', out: o.out });
    }, 250);
  } else if (d.t === 'in') {
    const inp = { ...d.input, now: Date.now() };
    const o = JSON.parse(w!.call('lf_input', JSON.stringify(inp)));
    post({ t: 'res', id: d.id, o });
  } else if (d.t === 'replay') {
    // re-execute a recorded node's inputs; compare the sha256 of every output with the recorded one
    let ww = await load(d.wasm);
    ww.call('lf_new', JSON.stringify(d.cfg));
    let same = 0;
    let n = 0;
    const lines: string[] = d.lines;
    const firstBad: string[] = [];
    for (let i = 0; i < lines.length; i++) {
      const v = JSON.parse(lines[i]);
      if (v.boot !== undefined) {
        ww = await load(d.wasm); // the process was (re)started here: a fresh node
        ww.call('lf_new', JSON.stringify(d.cfg));
        continue;
      }
      n++;
      const out = ww.call('lf_input', JSON.stringify(v.in));
      if ((await sha(out)) === v.out) same++;
      else if (firstBad.length < 3) firstBad.push(`input ${i + 1}`);
      if (i % 200 === 0) post({ t: 'progress', i, n: lines.length });
    }
    post({ t: 'replayed', n, same, firstBad });
  } else if (d.t === 'verify') {
    const ww = await load(d.wasm);
    post({ t: 'verified', r: JSON.parse(ww.call('lf_verify', JSON.stringify({ jsonl: d.jsonl, keys: d.keys }))) });
  }
};
