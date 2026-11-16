// ops.panel: never in a visitor's manifest, and its bundle is refused to a visitor's chain.
import { cell } from '../../../../web/cell-rt';
import { h } from '../../../../web/vendor/ui-cells/kit/dom';
import { client } from '../gen/ops_1_0';

cell(async (cx) => {
  const r = await client(cx).Read();
  cx.root.append(h('h2', { text: 'Operator' }), h('p', { text: `nodes: ${r.nodes.join(', ')}` }));
});
