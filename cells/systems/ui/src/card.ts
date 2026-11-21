// systems.card: one live system. Each card is its own cell instance, announced by the node that serves it.
import { cell } from '../../../../web/cell-rt';
import { h } from '../../../../web/vendor/ui-cells/kit/dom';
import { client } from '../gen/systems_catalog_1_1';

cell(async (cx) => {
  const all = await client(cx).List();
  const s = all.find((x) => x.slug === cx.init.instance);
  if (!s) {
    cx.root.append(h('p', { class: 'muted', text: `no system ${cx.init.instance}` }));
    return;
  }
  const open = (url: string) => {
    const b = h('button', { text: url.includes('github.com') ? 'Code' : 'Open' });
    b.onclick = () => cx.emit('Open', { url });
    return b;
  };
  cx.root.append(
    h('h2', {}, s.name, h('span', { class: 'tag ' + (s.status === 'works' ? 'acc' : ''), text: s.status })),
    h('p', { class: 'small', text: s.line }),
    h('div', { class: 'row' }, s.page ? open(s.page) : null, s.repo ? open(s.repo) : null),
  );
});
