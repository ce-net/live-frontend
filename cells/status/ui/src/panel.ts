// status.panel: which nodes are alive, answered by any of three replicas. When the replica answering it stops,
// the next answer comes from another one, with no reload.
import { cell } from '../../../../web/cell-rt';
import { clear, h } from '../../../../web/vendor/ui-cells/kit/dom';
import { client, slots, type Board } from '../gen/status_1_0';

cell(async (cx) => {
  const api = client(cx);
  const by = h('span', { class: 'tag', text: '' });
  const table = h('table');
  const hist = h('div', { class: 'small muted', text: '' });
  const holders: string[] = [];
  let seq = 0;
  const draw = (b: Board) => {
    seq = b.seq;
    by.textContent = `answered by ${b.holder}`;
    if (holders[holders.length - 1] !== b.holder) holders.push(b.holder);
    hist.textContent = 'holders so far: ' + holders.slice(-8).join(' → ');
    clear(table, h('tr', {}, h('th', { text: 'node' }), h('th', { text: 'state' }), h('th', { text: 'last beat' }), h('th', { text: 'calls served' })),
      ...b.nodes.map((n) => h('tr', {}, h('td', { class: 'mono', text: n.node }), h('td', {}, h('i', { class: 'dot ' + (n.alive ? 'g' : 'r') }), n.alive ? 'alive' : 'stopped'),
        h('td', { class: 'mono', text: n.age_ms > 9_000_000 ? 'never' : `${(n.age_ms / 1000).toFixed(1)} s ago` }), h('td', { class: 'mono', text: String(n.calls) }))));
  };
  cx.root.append(h('h2', {}, 'Nodes', h('span', { class: 'tag acc', text: `status@${cx.init.version}` }), by, h('span', { class: 'tag', text: `frame boot ${cx.init.boot}` })), table, hist);
  draw(await api.Read());
  cx.every(1000, async () => draw(await api.Watch({ seq })));
  slots(cx).trend({ series: 'calls' });
});
