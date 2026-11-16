// The in-browser network: four nodes, each a Web Worker running the wasm node, and a router that delivers
// what each node sends. The page's door talks to node d exactly as it talks to a real door.
import type { Door, DoorReply } from '../shell/door';
import { load } from './wasm';

interface Out { to: string; api: string; op: string; body: unknown; tag?: string }
interface Output { reply: { ok: boolean; body: unknown; refused?: string } | null; out: Out[]; forward: { to: string; api: string; op: string; body: unknown } | null }

export class SimNet implements Door {
  workers = new Map<string, Worker>();
  up = new Map<string, boolean>();
  cfgs: any[] = [];
  private id = 0;
  private waits = new Map<number, (o: Output) => void>();
  wasm: ArrayBuffer | null = null;
  onChange: () => void = () => {};

  async start(wasmUrl: string, workerUrl: string, seed: string): Promise<void> {
    this.wasm = await (await fetch(wasmUrl)).arrayBuffer();
    const w = await load(this.wasm);
    this.cfgs = JSON.parse(w.call('lf_configs', JSON.stringify({ seed, now: Date.now(), commit: '' })));
    this.workerUrl = workerUrl;
    await Promise.all(this.cfgs.map((c) => this.boot(c.name)));
  }
  private workerUrl = '';

  async boot(name: string): Promise<void> {
    const cfg = this.cfgs.find((c) => c.name === name);
    const wk = new Worker(this.workerUrl);
    this.workers.set(name, wk);
    await new Promise<void>((ok) => {
      wk.onmessage = (ev) => {
        const d = ev.data;
        if (d.t === 'booted') ok();
        else if (d.t === 'out') this.route(name, d.out);
        else if (d.t === 'res') {
          const w = this.waits.get(d.id);
          this.waits.delete(d.id);
          w?.(d.o);
        }
      };
      wk.postMessage({ t: 'boot', wasm: this.wasm, cfg });
    });
    this.up.set(name, true);
    this.onChange();
  }

  kill(name: string): void {
    this.workers.get(name)?.terminate();
    this.workers.delete(name);
    this.up.set(name, false);
    this.onChange();
  }

  /** One input to one node; null when the node is down or does not answer in time. */
  input(name: string, input: Record<string, unknown>, ms = 1500): Promise<Output | null> {
    const wk = this.workers.get(name);
    if (!wk) return Promise.resolve(null);
    const id = ++this.id;
    return new Promise((ok) => {
      const t = setTimeout(() => {
        this.waits.delete(id);
        ok(null);
      }, ms);
      this.waits.set(id, (o) => {
        clearTimeout(t);
        ok(o);
      });
      wk.postMessage({ t: 'in', id, input });
    });
  }

  private route(from: string, outs: Out[]): void {
    for (const m of outs) {
      void this.input(m.to, { kind: 'frame', from, api: m.api, op: m.op, body: m.body }).then((o) => {
        if (!o) return;
        this.route(m.to, o.out);
        if (m.tag && o.reply) void this.input(from, { kind: 'reply', tag: m.tag, ok: o.reply.ok, body: o.reply.body }).then((o2) => o2 && this.route(from, o2.out));
      });
    }
  }

  async ctl(node: string, op: string, body: unknown): Promise<{ ok: boolean; body: unknown; refused?: string }> {
    const o = await this.input(node, { kind: 'frame', from: 'root', api: 'sim.ctl', op, body }, 5000);
    if (!o) return { ok: false, body: null, refused: `${node} is down` };
    this.route(node, o.out);
    return o.reply!;
  }

  async req(op: string, body: unknown): Promise<DoorReply> {
    const o = await this.input('d', { kind: 'frame', from: 'browser', api: 'fe.door@1', op, body });
    if (!o) return { ok: false, body: null, transport: 'door: no answer' };
    this.route('d', o.out);
    if (!o.forward) return { ...(o.reply as DoorReply), node: 'd' };
    const f = o.forward;
    const r = await this.input(f.to, { kind: 'frame', from: 'd', api: f.api, op: f.op, body: f.body });
    if (!r) return { ok: false, body: null, transport: `${f.to}: unreachable`, node: f.to };
    this.route(f.to, r.out);
    return { ...(r.reply as DoorReply), node: f.to };
  }
}
