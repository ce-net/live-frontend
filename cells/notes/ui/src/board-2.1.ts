import { cell } from '../../../../web/cell-rt';
import { client, slots } from '../gen/notes_2_1';
import { board } from './board';

cell((cx) => {
  const api = client(cx);
  const s = slots(cx);
  board(cx, {
    list: () => api.List(),
    put: (op_id, d) => api.Put({ op_id, note: { id: d.id, text: d.text, rgb: d.rgb ?? null } }),
    since: (seq) => api.Since({ seq }),
    purge: () => api.Purge(),
    trend: (series) => s.trend({ series }),
    colours: 'rgb',
  });
});
