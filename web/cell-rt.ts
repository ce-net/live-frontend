// The runtime inside one UI cell's frame. The frame is `<iframe sandbox="allow-scripts">` with a CSP that lets
// only this bundle's hash run and allows no network, so this module's only link to anything is the one
// MessagePort the shell hands it. It never sees the viewer's chain, its rights, or another cell.

export interface Init {
  view: string;
  instance: string;
  api: string;
  version: string;
  label: string;
  boot: number;
  /** The state the previous frame of this cell saved (hot swap, crash restart), and which version saved it. */
  state: unknown;
  from: string | null;
  props: unknown;
}

/** A call the node (or the bus) refused: `refused` is the gate's text, verbatim. */
export class Refusal extends Error {
  constructor(public refused: string, public body: unknown) {
    super(refused);
  }
}

export interface Ctx {
  readonly init: Init;
  readonly root: HTMLElement;
  call<T = unknown>(op: string, body: unknown): Promise<T>;
  emit(event: string, data: unknown): void;
  slot(name: string, props: unknown): void;
  /** What to hand over when this frame is replaced (hot swap) or restarted. */
  keep(f: () => unknown): void;
  /** Hands the kept state to the shell now (it also asks every 2 s and before a swap). */
  flush(): void;
  every(ms: number, f: () => void | Promise<void>): void;
}

let port: MessagePort | null = null;
let seq = 0;
const waiting = new Map<number, { ok: (v: unknown) => void; no: (e: unknown) => void }>();
let keeper: () => unknown = () => null;

function post(m: unknown): void {
  port?.postMessage(m);
}

addEventListener('error', (e) => post({ t: 'crash', why: String((e as ErrorEvent).message || e) }));
addEventListener('unhandledrejection', (e) => {
  const r = (e as PromiseRejectionEvent).reason;
  if (r instanceof Refusal) return; // a refused call is an answer, not a crash
  post({ t: 'crash', why: String(r && r.message ? r.message : r) });
});

/** The sandbox probe (proof step P7): three things a cell must not be able to do. */
async function probe(cx: Ctx): Promise<Record<string, string>> {
  const out: Record<string, string> = {};
  try {
    await cx.call('status.Read', null);
    out.undeclared = 'allowed (bad)';
  } catch (e) {
    out.undeclared = e instanceof Refusal ? e.refused : String(e);
  }
  try {
    await fetch('https://example.com/');
    out.fetch = 'allowed (bad)';
  } catch (e) {
    out.fetch = 'blocked: ' + String((e as Error).message || e);
  }
  try {
    const d = (window.parent as Window).document;
    out.parent = 'allowed (bad): ' + String(d.title);
  } catch (e) {
    out.parent = 'blocked: ' + String((e as Error).name || e);
  }
  try {
    localStorage.setItem('x', '1');
    out.storage = 'allowed (bad)';
  } catch (e) {
    out.storage = 'blocked: ' + String((e as Error).name || e);
  }
  return out;
}

export function cell(main: (cx: Ctx) => void | Promise<void>): void {
  addEventListener('message', (ev: MessageEvent) => {
    if (port || !ev.data || ev.data.t !== 'init' || !ev.ports[0]) return;
    port = ev.ports[0];
    const init = ev.data.init as Init;
    const root = document.getElementById('cell') as HTMLElement;
    const cx: Ctx = {
      init,
      root,
      call: (op, body) =>
        new Promise((ok, no) => {
          const id = ++seq;
          waiting.set(id, { ok: ok as (v: unknown) => void, no });
          post({ t: 'call', id, op, body });
        }),
      emit: (event, data) => post({ t: 'emit', event, data }),
      slot: (name, props) => post({ t: 'slot', name, props }),
      keep: (f) => {
        keeper = f;
      },
      flush: () => post({ t: 'state', id: 0, state: keeper() }),
      every: (ms, f) => {
        const run = async () => {
          try {
            await f();
          } catch (e) {
            if (!(e instanceof Refusal)) throw e;
          }
        };
        setInterval(run, ms);
      },
    };
    port.onmessage = async (m: MessageEvent) => {
      const d = m.data;
      if (d.t === 'reply') {
        const w = waiting.get(d.id);
        waiting.delete(d.id);
        if (!w) return;
        if (d.ok) w.ok(d.body);
        else w.no(new Refusal(d.refused || 'refused', d.body));
      } else if (d.t === 'save') {
        post({ t: 'state', id: d.id, state: keeper() });
      } else if (d.t === 'probe') {
        post({ t: 'probe', id: d.id, result: await probe(cx) });
      }
    };
    let n = 0;
    setInterval(() => post({ t: 'beat', n: ++n }), 500);
    new ResizeObserver(() => post({ t: 'size', h: Math.ceil(root.getBoundingClientRect().bottom + 14) })).observe(root);
    Promise.resolve(main(cx)).then(
      () => post({ t: 'ready' }),
      (e) => post({ t: 'crash', why: String(e && e.message ? e.message : e) }),
    );
  });
}
