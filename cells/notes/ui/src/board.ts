// notes.board: the visitor's own notes. Shared by the 1.0, 1.1 and 2.0 bundles; each passes its typed client.
import { Refusal, type Ctx } from '../../../../web/cell-rt';
import { clear, h } from '../../../../web/vendor/ui-cells/kit/dom';

export interface Row { id: string; text: string; seq: number; colour?: string | null; rgb?: number | null }
export interface Draft { id: string; text: string; colour?: string | null; rgb?: number | null }
export interface Ops {
  list(): Promise<Row[]>;
  put(op_id: string, d: Draft): Promise<Row>;
  since(seq: number): Promise<{ seq: number; rows: Row[] }>;
  purge(): Promise<number>;
  trend(series: string): void;
  colours: 'none' | 'name' | 'rgb';
}

const NAMES: [string, string][] = [['amber', '#f5a524'], ['green', '#3f9d4a'], ['blue', '#3b6fd8'], ['red', '#d64545'], ['grey', '#8a8f87']];
const hex = (n: number) => '#' + n.toString(16).padStart(6, '0');

export function board(cx: Ctx, ops: Ops): void {
  const prev = (cx.init.state ?? null) as Draft | null;
  const draft: Draft = { id: prev?.id ?? 'n' + (cx.init.boot % 1000), text: prev?.text ?? '', colour: prev?.colour ?? null, rgb: prev?.rgb ?? null };
  if (ops.colours === 'rgb' && draft.rgb == null && draft.colour) draft.rgb = parseInt((NAMES.find((n) => n[0] === draft.colour) ?? ['', '#8a8f87'])[1].slice(1), 16);
  if (ops.colours === 'name' && !draft.colour && draft.rgb != null) draft.colour = 'grey';
  cx.keep(() => ({ ...draft }));
  const rows = new Map<string, Row>();
  let seq = 0;
  const list = h('ul', { class: 'notes' });
  const said = h('div', { class: 'small muted', text: '' });
  const input = h('input', { attrs: { type: 'text', placeholder: 'Write a note (stays in your sandbox)', maxlength: '200', 'aria-label': 'note text' } });
  input.value = draft.text;
  input.addEventListener('input', () => (draft.text = input.value));
  const swatch = (c: string | null) => h('span', { class: 'sw', attrs: { style: `background:${c ?? 'transparent'}` } });
  const draw = () => {
    clear(list, ...[...rows.values()].sort((a, b) => a.seq - b.seq).map((r) =>
      h('li', {}, swatch(r.rgb != null ? hex(r.rgb) : r.colour ? (NAMES.find((n) => n[0] === r.colour) ?? ['', null])[1] : null), h('span', { text: r.text }), h('span', { class: 'mono muted', text: `#${r.seq}` }))));
    if (rows.size === 0) list.append(h('li', { class: 'muted', text: 'No notes in your sandbox yet.' }));
  };
  const picker = h('span', { class: 'row' });
  if (ops.colours === 'name') {
    for (const [n, c] of NAMES) {
      const b = h('button', { class: 'sw', label: n, pressed: draft.colour === n, attrs: { style: `background:${c};padding:0;width:16px;height:16px` } });
      b.onclick = () => {
        draft.colour = draft.colour === n ? null : n;
        for (const x of picker.querySelectorAll('button')) x.setAttribute('aria-pressed', String(x.getAttribute('aria-label') === draft.colour));
      };
      picker.append(b);
    }
  } else if (ops.colours === 'rgb') {
    const c = h('input', { attrs: { type: 'color', value: draft.rgb != null ? hex(draft.rgb) : '#8a8f87', 'aria-label': 'colour (RGB)' } });
    c.addEventListener('input', () => (draft.rgb = parseInt(c.value.slice(1), 16)));
    picker.append(h('span', { class: 'small muted', text: 'RGB' }), c);
  }
  const write = h('button', { class: 'primary', text: 'Write' });
  write.onclick = async () => {
    try {
      const r = await ops.put('op-' + draft.id + '-' + Date.now(), { ...draft });
      rows.set(r.id, r);
      draw();
      said.textContent = `Allowed: notes.Put, note #${r.seq} in your sandbox`;
      cx.emit('NoteAdded', { id: r.id });
    } catch (e) {
      said.textContent = e instanceof Refusal ? e.refused : String(e);
    }
  };
  const verdict = h('div', { class: 'verdict', text: 'Not tried yet.' });
  const purge = h('button', { text: 'Call notes.Purge' });
  purge.onclick = async () => {
    try {
      const n = await ops.purge();
      verdict.textContent = `allowed (${n} rows): this should not happen`;
      verdict.className = 'verdict yes';
    } catch (e) {
      verdict.textContent = e instanceof Refusal ? e.refused : String(e);
      verdict.className = 'verdict no';
    }
  };
  const spend = h('button', { text: 'Spend the rest of my uses' });
  spend.onclick = async () => {
    let n = 0;
    for (;;) {
      try {
        await ops.put('spend-' + Date.now() + '-' + n, { id: 'spend', text: `use ${n + 1}`, colour: null, rgb: null });
        n++;
        verdict.textContent = `allowed: ${n} writes so far`;
        await new Promise((r) => setTimeout(r, 120)); // under the chain's rate caveat
      } catch (e) {
        verdict.textContent = `after ${n} more writes:\n` + (e instanceof Refusal ? e.refused : String(e));
        verdict.className = 'verdict no';
        break;
      }
    }
  };
  const v = cx.init.version;
  cx.root.append(
    h('h2', {}, 'Notes', h('span', { class: 'tag acc', text: `notes@${v}` }), cx.init.label ? h('span', { class: 'tag', text: cx.init.label }) : null, h('span', { class: 'tag', text: `frame boot ${cx.init.boot}` }),
      cx.init.from ? h('span', { class: 'tag', text: `state from ${cx.init.from}` }) : null),
    h('div', { class: 'row' }, input, picker, write),
    said,
    list,
    h('div', { class: 'row' }, purge, spend),
    verdict,
  );
  draw();
  ops.list().then((l) => {
    for (const r of l) rows.set(r.id, r), (seq = Math.max(seq, r.seq));
    draw();
  });
  cx.every(1500, async () => {
    const d = await ops.since(seq);
    for (const r of d.rows) rows.set(r.id, r);
    seq = Math.max(seq, d.seq);
    if (d.rows.length) draw();
  });
  ops.trend('notes');
}
