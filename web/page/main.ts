// The page around the shell: which mode it runs in, the nodes, the controls (each one real action on the nodes),
// and the recorded proof with its in-browser check.
import { HttpDoor, type Door } from '../shell/door';
import { Identity } from '../shell/identity';
import { propsMismatch, Shell } from '../shell/shell';
import { HttpController, SimController, type Controller } from '../sim/control';
import { SimNet } from '../sim/net';

const $ = (id: string) => document.getElementById(id) as HTMLElement;
const el = (tag: string, cls = '', text = '') => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
};
const said = (s: string) => ($('said').textContent = s);
const INDEX = document.body.dataset.index === 'systems';

async function kitCss(): Promise<string> {
  const parts = await Promise.all(['kit/tokens.css', 'kit/components.css', 'cell.css'].map((u) => fetch(u).then((r) => r.text())));
  return parts.join('\n').replace(/@font-face\{[^}]*\}/g, '');
}

async function main(): Promise<void> {
  $('theme').onclick = () => {
    const d = document.documentElement;
    d.dataset.theme = d.dataset.theme === 'dark' ? 'light' : 'dark';
    try {
      localStorage.setItem('ce-theme', d.dataset.theme);
    } catch {
      /* private window */
    }
  };
  if (!(await Identity.supported())) {
    $('empty').textContent = 'This browser has no WebCrypto Ed25519, which the visitor key needs. Recent Chrome, Edge, Firefox and Safari have it.';
    return;
  }
  let door: Door;
  let ctl: Controller;
  let net: SimNet | null = null;
  const local = await fetch('door.json').then((r) => (r.ok ? r.json() : null), () => null);
  if (local) {
    door = new HttpDoor(local.door);
    ctl = new HttpController(`${location.protocol}//${location.hostname}:21202`);
    $('mode').textContent = local.mode === 'live' ? 'Live nodes: four processes on a cloud node, keys only in their memory' : 'Sim on four local processes';
  } else {
    net = new SimNet();
    $('mode').textContent = 'Sim in this browser: four nodes, each a Web Worker running the wasm node';
    await net.start('sim.wasm', 'worker.js', 'page-' + Date.now());
    const [plan, bundles] = await Promise.all([fetch('plan.json').then((r) => r.json()), fetch('bundles.json').then((r) => r.json())]);
    const sc = new SimController(net, plan, bundles, '');
    if (INDEX) {
      // the index of live systems: one systems.card cell instance per system, served by a and b
      const systems: { slug: string }[] = await fetch('systems.json').then((r) => r.json());
      plan.base = systems.map((x, i) => ({ op: 'install', node: i % 2 ? 'b' : 'a', view: 'systems.card', api: 'systems.catalog', version: '1.0', dir: 'systems', instance: x.slug, peers: ['a', 'b'] }));
    }
    await sc.base_();
    ctl = sc;
    door = net;
  }
  $('mode').classList.add('acc');
  const shell = new Shell(door, { cards: $('r-cards'), main: $('r-main'), side: $('r-side'), bottom: $('r-bottom') }, await kitCss());
  (window as any).__lf = { shell, ctl, net };
  (window as any).__lfPropsMismatch = propsMismatch;
  const welcome = await shell.hello();

  const drawNodes = () => {
    const up = ctl.up();
    const box = $('nodes');
    box.replaceChildren();
    for (const n of welcome.nodes) {
      const d = el('div', 'node' + (up[n.name] === false ? ' down' : ''));
      d.append(el('span', 'dot'), el('b', '', `${n.name}`), el('span', 'fp', `  ${n.class} · ${n.fp}`));
      const ul = el('ul');
      const here = shell.entries.filter((e) => e.holders.some((h) => h.node === n.name));
      for (const e of here) ul.append(el('li', '', `${e.body.view}${e.body.instance !== 'main' ? ':' + e.body.instance : ''} ${e.body.api.split('@')[0]}@${e.body.version}`));
      if (n.name === 'd') ul.append(el('li', '', 'http.serve page.compose fe.shell fe.visitor fe.history fe.explain'));
      if (up[n.name] === false) ul.append(el('li', 'bad', 'stopped'));
      d.append(ul);
      box.append(d);
    }
    $('empty').style.display = shell.mounted.size ? 'none' : '';
    // the first paint (Render.Html of each public view, inlined by http.serve) gives way to the live frames
    if (shell.mounted.size) document.getElementById('first-paint')?.remove();
    $('chain').textContent = shell.chainId ? `chain ${shell.chainId} · key ${shell.id?.pub.slice(0, 10)}` : 'no chain';
  };
  shell.onChange = drawNodes;
  setInterval(drawNodes, 1500);
  drawNodes();

  const busy = new Set<string>();
  const button = (label: string, f: () => Promise<unknown>) => {
    const b = el('button', '', label) as HTMLButtonElement;
    b.onclick = async () => {
      if (busy.has(label)) return;
      busy.add(label);
      b.disabled = true;
      said(label + ' ...');
      try {
        const r = await f();
        said(label + ': done' + (r ? '\n' + JSON.stringify(r).slice(0, 400) : ''));
      } catch (e) {
        said(label + ': ' + String((e as Error).message || e));
      } finally {
        busy.delete(label);
        b.disabled = false;
        drawNodes();
      }
    };
    $('controls').append(b);
  };
  if (INDEX) {
    button('Stop node b (its cards leave)', () => ctl.action('stop-b'));
    button('Start node b', () => ctl.action('start-b'));
    button('Revoke my chain', () => shell.revoke());
    await shell.join().catch((e) => said(String(e)));
    return;
  }
  button('Join as a visitor', () => shell.join());
  if (welcome.mode === 'live') {
    // live nodes are not changed from the page: the visitor's own actions only
    button('Let notes.board try to escape its sandbox', () => shell.probe('notes.board'));
    button('Revoke my chain', () => shell.revoke());
    await proof(net);
    await shell.join().catch((e) => said(String(e)));
    return;
  }
  button('Start the faults cell on b', () => ctl.action('start-faults'));
  button('Deploy notes 1.1, node by node', () => ctl.action('deploy-notes-1.1'));
  button('Deploy notes 1.2 (render ops)', () => ctl.action('deploy-notes-1.2'));
  button('Move trend.chart to node a', () => ctl.action('move-trend'));
  button('Deploy notes 2.1 beside 1', () => ctl.action('deploy-notes-2.1'));
  button('Change one byte of the notes bundle on c', () => ctl.action('tamper-c'));
  button('Stop node b', () => ctl.action('stop-b'));
  button('Start node b', () => ctl.action('start-b'));
  button('Let notes.board try to escape its sandbox', () => shell.probe('notes.board'));
  button('Revoke my chain', () => shell.revoke());
  await proof(net);
  if (new URLSearchParams(location.search).get('join') !== '0') await shell.join().catch((e) => said(String(e)));
}

/** The recorded run: signed steps checked in this browser, and every logged input re-executed by the wasm nodes. */
async function proof(net: SimNet | null): Promise<void> {
  const box = $('proof');
  const sum = await fetch('proof/summary.json').then((r) => (r.ok ? r.json() : null), () => null);
  if (!sum) {
    box.append(el('h3', '', 'Recorded run'), el('p', 'mono', 'No recorded run next to this page (run `cargo cell record`).'));
    return;
  }
  const roster = await fetch('proof/roster.json').then((r) => r.json());
  const jsonl = await fetch('proof/run.jsonl').then((r) => r.text());
  box.append(el('h3', '', `Recorded run: real processes and a headless browser, ${sum.recorded ?? ''}`));
  const p = el('p', 'mono', `${jsonl.trim().split('\n').length} signed steps. Checking in this browser...`);
  box.append(p);
  const t = el('table');
  t.innerHTML = '<tr><th>step</th><th>what</th><th>result</th><th>verdict</th></tr>';
  for (const line of jsonl.trim().split('\n')) {
    const s = JSON.parse(line);
    const tr = el('tr');
    tr.append(el('td', 'mono', `P${s.step}`), el('td', '', s.note), el('td', 'mono', JSON.stringify(s.response).slice(0, 220)), el('td', s.verdict === 'pass' ? 'ok' : 'bad', s.verdict));
    t.append(tr);
  }
  box.append(t);
  const wasm = await (await fetch('sim.wasm')).arrayBuffer();
  const w = new Worker('worker.js');
  w.onmessage = (ev) => {
    if (ev.data.t === 'verified') {
      const r = ev.data.r;
      p.textContent = r.ok ? `${r.steps} signed steps: every signature and the hash chain check out in this browser (signer ${roster.recorder.slice(0, 22)}...).` : `Check FAILED: ${r.error}`;
      p.className = 'mono ' + (r.ok ? 'ok' : 'bad');
    }
  };
  w.postMessage({ t: 'verify', wasm, jsonl, keys: [roster.recorder] });
  const rb = el('button', 'btn', 'Replay the recorded run: re-execute every input the four processes logged, in four wasm nodes');
  const rr = el('div', 'mono');
  rb.onclick = async () => {
    rb.setAttribute('disabled', '');
    const cfgs = await fetch('proof/configs.json').then((r) => r.json());
    const res: string[] = [];
    await Promise.all(
      cfgs.map(async (cfg: { name: string }) => {
        const lines = (await fetch(`proof/inputs/${cfg.name}.jsonl`).then((r) => r.text())).trim().split('\n').filter(Boolean);
        const wk = new Worker('worker.js');
        await new Promise<void>((ok) => {
          wk.onmessage = (ev) => {
            if (ev.data.t === 'progress') rr.textContent = `node ${cfg.name}: ${ev.data.i} of ${ev.data.n}...`;
            if (ev.data.t === 'replayed') {
              res.push(`node ${cfg.name}: ${ev.data.same} of ${ev.data.n} outputs byte-identical${ev.data.firstBad.length ? ' (differs at ' + ev.data.firstBad.join(', ') + ')' : ''}`);
              wk.terminate();
              ok();
            }
          };
          wk.postMessage({ t: 'replay', wasm, cfg, lines });
        });
      }),
    );
    rr.textContent = res.sort().join('\n');
    rr.className = 'mono ' + (res.every((x) => !x.includes('differs')) ? 'ok' : 'bad');
  };
  box.append(rb, rr);
  const ff = await fetch('proof/firefox/summary.json').then((r) => (r.ok ? r.json() : null), () => null);
  if (ff) {
    const loop = ff.tight_loop_firefox?.shell_stall_ms;
    box.append(el('p', 'mono', `The same run in Firefox (proof/firefox/): ${ff.steps}. A 4 s tight loop in a cell paused the whole page there for ${loop} ms (Firefox runs a sandboxed frame on the page's thread); in Chromium the shell kept running.`));
  }
  void net;
}

void main();
