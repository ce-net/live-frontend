// timeline.view: the system changing over time. One lane per node, one mark per event of fe.history (announce,
// change, refusal, crash, kill, failover, join, revoke), time left to right. The scrubber shows which cell and
// version each node served at any step of the log.
import { cell } from '../../../../web/cell-rt';
import { clear, h } from '../../../../web/vendor/ui-cells/kit/dom';
import { client, type Event } from '../gen/fe_history_1_1';

const NS = 'http://www.w3.org/2000/svg';
const LANES = ['a', 'b', 'c', 'd', 'browser'];
const KIND: Record<string, string> = {
  Announced: 'var(--ok)', Changed: 'var(--acc)', Installed: 'var(--ink-3)', Gone: '#8a8f87', Refused: 'var(--bad)', Tampered: 'var(--bad)',
  Killed: 'var(--bad)', Started: 'var(--ok)', Joined: '#3b6fd8', Revoked: 'var(--bad)', Allowed: 'var(--ok)', Exhausted: 'var(--bad)',
  Mounted: '#3b6fd8', Swapped: 'var(--acc)', Crashed: 'var(--bad)', Restarted: 'var(--acc)', FailedOver: 'var(--acc)',
};

/** Which view@version each node served after event `upto`, from Announced / Changed / Gone. */
function servedAt(evs: Event[], upto: number): Map<string, Map<string, string>> {
  const m = new Map<string, Map<string, string>>();
  for (const e of evs) {
    if (e.seq > upto) break;
    const d = e.detail as Record<string, string>;
    const holder = d && d.holder;
    if (!holder) continue;
    const per = m.get(holder) ?? new Map<string, string>();
    if (e.kind === 'Announced') per.set(e.cell, d.version);
    if (e.kind === 'Changed') per.set(e.cell, d.to);
    if (e.kind === 'Gone' && per.get(e.cell) === d.version) per.delete(e.cell);
    m.set(holder, per);
  }
  return m;
}

cell(async (cx) => {
  const api = client(cx);
  const evs: Event[] = [];
  let at = -1;
  const svg = document.createElementNS(NS, 'svg');
  const scrub = h('input', { attrs: { type: 'range', min: '0', max: '0', value: '0', 'aria-label': 'step of the log', style: 'width:100%' } });
  const now = h('div', { class: 'small' });
  const list = h('div', { class: 'mono small', attrs: { style: 'max-height:120px;overflow:auto' } });
  scrub.addEventListener('input', () => {
    at = Number(scrub.value) >= evs.length ? -1 : Number(scrub.value);
    draw();
  });
  const draw = () => {
    if (!evs.length) return;
    const t0 = evs[0].t;
    const t1 = Math.max(evs[evs.length - 1].t, t0 + 1);
    const W = 640;
    svg.setAttribute('viewBox', `0 0 ${W} ${LANES.length * 16 + 4}`);
    svg.replaceChildren();
    LANES.forEach((l, i) => {
      const t = document.createElementNS(NS, 'text');
      t.setAttribute('x', '0');
      t.setAttribute('y', String(i * 16 + 13));
      t.setAttribute('font-size', '10');
      t.setAttribute('fill', 'var(--ink-2)');
      t.textContent = l;
      svg.append(t);
      const ln = document.createElementNS(NS, 'line');
      ln.setAttribute('x1', '48');
      ln.setAttribute('x2', String(W));
      ln.setAttribute('y1', String(i * 16 + 9));
      ln.setAttribute('y2', String(i * 16 + 9));
      ln.setAttribute('stroke', 'var(--line)');
      svg.append(ln);
    });
    const upto = at < 0 ? evs.length : at + 1;
    evs.forEach((e, k) => {
      const lane = LANES.indexOf(e.node);
      if (lane < 0) return;
      const c = document.createElementNS(NS, 'rect');
      const x = 50 + ((e.t - t0) / (t1 - t0)) * (W - 56);
      c.setAttribute('x', x.toFixed(1));
      c.setAttribute('y', String(lane * 16 + 3));
      c.setAttribute('width', '3');
      c.setAttribute('height', '12');
      c.setAttribute('fill', KIND[e.kind] ?? 'var(--ink-3)');
      c.setAttribute('opacity', k < upto ? '1' : '0.15');
      const tt = document.createElementNS(NS, 'title');
      tt.textContent = `#${e.seq} ${e.kind} ${e.cell} on ${e.node}`;
      c.append(tt);
      svg.append(c);
    });
    const step = at < 0 ? evs.length - 1 : at;
    const served = servedAt(evs, evs[step].seq);
    clear(now, h('b', { text: at < 0 ? 'Now: ' : `At event #${evs[step].seq}: ` }),
      ...['a', 'b', 'c', 'd'].map((n) => h('span', { class: 'tag', text: `${n}: ${[...(served.get(n) ?? new Map()).entries()].map(([v, ver]) => `${v.split('.')[0]}@${ver}`).join(' ') || '-'}` })));
    clear(list, ...evs.slice(Math.max(0, upto - 14), upto).reverse().map((e) =>
      h('div', {}, h('span', { attrs: { style: `color:${KIND[e.kind] ?? 'inherit'}` }, text: `#${e.seq} ${e.kind}` }), ` ${e.cell} @${e.node} ${JSON.stringify(e.detail).slice(0, 90)}`)));
  };
  cx.root.append(h('h2', {}, 'The system over time', h('span', { class: 'tag acc', text: `fe.history@${cx.init.version}` }), h('span', { class: 'tag', text: `frame boot ${cx.init.boot}` })), svg, scrub, now, list);
  const pull = async () => {
    const r = await api.Since({ seq: evs.length });
    if (!r.events.length) return;
    evs.push(...r.events);
    scrub.max = String(evs.length);
    if (at < 0) scrub.value = String(evs.length);
    draw();
  };
  await pull();
  cx.every(1500, pull);
});
