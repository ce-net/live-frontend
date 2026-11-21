// trend.chart: a small line chart. It only ever lives in another view's typed slot, with typed props.
import { cell } from '../../../../web/cell-rt';
import { h } from '../../../../web/vendor/ui-cells/kit/dom';
import { client, type Points, type TrendProps } from '../gen/trend_1_1';

const NS = 'http://www.w3.org/2000/svg';
const COLS = ['var(--acc)', 'var(--ok)', '#3b6fd8', '#8a8f87'];

cell(async (cx) => {
  const props = cx.init.props as TrendProps;
  const api = client(cx);
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 300 60');
  svg.setAttribute('role', 'img');
  const legend = h('div', { class: 'small muted', text: '' });
  const draw = (p: Points) => {
    svg.replaceChildren();
    const all = p.lines.flatMap(([, pts]) => pts);
    if (!all.length) {
      legend.textContent = 'no data yet';
      return;
    }
    const t1 = Math.max(...all.map((x) => x[0]));
    const vmax = Math.max(1, ...all.map((x) => x[1]));
    p.lines.forEach(([k, pts], i) => {
      const d = pts.map(([t, v], j) => `${j ? 'L' : 'M'}${(300 - (t1 - t) * 5).toFixed(1)},${(56 - (v / vmax) * 50).toFixed(1)}`).join(' ');
      const path = document.createElementNS(NS, 'path');
      path.setAttribute('d', d);
      path.setAttribute('fill', 'none');
      path.setAttribute('stroke', COLS[i % COLS.length]);
      path.setAttribute('stroke-width', '1.5');
      svg.append(path);
    });
    legend.textContent = p.lines.map(([k, pts]) => `${k}: ${pts.length ? pts[pts.length - 1][1] : 0}/s`).join('   ') + `   (max ${vmax}/s, last 60 s)`;
  };
  cx.root.append(h('div', { class: 'small', text: `trend.chart · ${props.series} · from node c` }), svg, legend);
  draw(await api.Series(props));
  cx.every(2000, async () => draw(await api.Series(props)));
});
