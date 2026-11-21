// faults.panel: a cell that crashes when asked. The shell restarts only this frame and hands back its state.
import { cell } from '../../../../web/cell-rt';
import { h } from '../../../../web/vendor/ui-cells/kit/dom';
import { client } from '../gen/faults_1_1';

cell((cx) => {
  const api = client(cx);
  const kept = (cx.init.state as { crashes: number } | null) ?? { crashes: 0 };
  cx.keep(() => kept);
  const out = h('div', { class: 'small muted', text: kept.crashes ? `Restarted after ${kept.crashes} crash(es); this count survived in the saved state.` : 'Running.' });
  const crash = h('button', { text: 'Throw an error' });
  crash.onclick = async () => {
    const a = await api.Arm({ mode: 'throw' });
    kept.crashes = a.n;
    cx.flush();
    setTimeout(() => {
      throw new Error(`faults.panel: crash ${a.n} on request`);
    });
  };
  const hang = h('button', { text: 'Loop for 4 s' });
  hang.onclick = async () => {
    const a = await api.Arm({ mode: 'hang' });
    kept.crashes = a.n;
    cx.flush();
    const t = Date.now();
    while (Date.now() - t < 4000) {
      /* a tight loop: no beats */
    }
  };
  cx.root.append(h('h2', {}, 'Faults', h('span', { class: 'tag acc', text: `faults@${cx.init.version}` }), h('span', { class: 'tag', text: `frame boot ${cx.init.boot}` })),
    h('p', { class: 'small', text: 'Crash this cell. Only this frame restarts; every other panel keeps running (watch their boot counters).' }),
    h('div', { class: 'row' }, crash, hang), out);
});
