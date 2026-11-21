// cap.inspect: the viewer's own chain, as the issuer's node sees it. Links, rights, caveats, uses left, expiry,
// which views it holds the needs of (and which it does not), and the last decisions the gates made on it.
import { cell } from '../../../../web/cell-rt';
import { clear, h } from '../../../../web/vendor/ui-cells/kit/dom';
import { client, type MyChain } from '../gen/fe_explain_1_1';

cell(async (cx) => {
  const api = client(cx);
  const body = h('div');
  const draw = (m: MyChain) => {
    const left = Math.max(0, Math.round((m.until - Date.now()) / 1000));
    clear(body,
      h('div', { class: 'row' }, h('span', { class: 'tag acc', text: `chain ${m.id}` }), h('span', { class: 'tag', text: `${m.uses_left} of ${m.uses} uses left` }),
        h('span', { class: 'tag', text: m.revoked ? 'revoked' : `${Math.floor(left / 60)} min ${left % 60} s left` })),
      ...m.links.map((l, i) => h('div', { class: 'verdict' },
        `link ${i}: ${l.issuer} → ${l.holder}\n` +
        `rights: ${l.rights.join(', ')}\n` +
        `until ${new Date(l.until).toISOString().slice(0, 19)}Z` + (l.count != null ? ` · count ${l.count}` : '') + (l.rate != null ? ` · rate ${l.rate}/s` : '') +
        (l.rows ? ` · where ${l.rows[0]} == ${l.rows[1]}` : '') + (l.mint != null ? ` · mint ${l.mint}/day` : ''))),
      h('table', {}, h('tr', {}, h('th', { text: 'view' }), h('th', { text: 'needs held' }), h('th', { text: 'missing' }), h('th', { text: 'holders' })),
        ...m.views.map((v) => h('tr', {}, h('td', { class: 'mono', text: v.view }), h('td', { class: 'mono ok', text: v.held.join(' ') }),
          h('td', { class: 'mono bad', text: v.missing.join(' ') || '-' }), h('td', { class: 'mono', text: v.holders.join(' ') })))),
      h('div', { class: 'small muted', text: 'Last gate decisions on this chain:' }),
      ...m.gates.map(([k, node, need, why]) => h('div', { class: 'mono small ' + (k === 'Allowed' ? 'ok' : 'bad'), text: `${k} by ${node}: ${need}${why ? ' (' + why + ')' : ''}` })),
    );
  };
  cx.root.append(h('h2', {}, 'Your capability', h('span', { class: 'tag acc', text: `fe.explain@${cx.init.version}` }), h('span', { class: 'tag', text: `frame boot ${cx.init.boot}` })), body);
  draw(await api.Mine());
  cx.every(2000, async () => draw(await api.Mine()));
});
