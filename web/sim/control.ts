// The operator's plan (web/plan.json) carried out on the nodes: on the in-browser sim directly, or by asking the
// native controller of `cargo cell sim` over HTTP. Each control on the page is one action of the plan.
import { b64encode } from '../shell/door';
import type { SimNet } from './net';

export interface Controller {
  action(name: string): Promise<unknown>;
  up(): Record<string, boolean>;
}

export class HttpController implements Controller {
  private state: Record<string, boolean> = { a: true, b: true, c: true, d: true };
  constructor(private url: string) {}
  async action(name: string): Promise<unknown> {
    const r = await (await fetch(this.url + '/ctl', { method: 'POST', body: JSON.stringify({ action: name }) })).json();
    try {
      this.state = (await (await fetch(this.url + '/ctl/state')).json()).up;
    } catch {
      /* keep */
    }
    if (!r.ok) throw new Error(r.error);
    return r.result;
  }
  up(): Record<string, boolean> {
    return this.state;
  }
}

export class SimController implements Controller {
  installed = new Map<string, Record<string, unknown>[]>();
  constructor(private net: SimNet, private plan: any, private bundles: Record<string, { file: string | null; sha: string }>, private base: string) {}

  async base_(): Promise<void> {
    for (const s of this.plan.base) await this.step(s);
  }

  async step(s: Record<string, any>): Promise<unknown> {
    const node = s.node as string;
    switch (s.op) {
      case 'install': {
        const b = this.bundles[`${s.view}@${s.version}`];
        const body: Record<string, unknown> = { ...s };
        if (b.file) body.bundle_b64 = b64encode(new Uint8Array(await (await fetch(this.base + b.file)).arrayBuffer()));
        else body.bundle_sha = b.sha;
        const r = await this.net.ctl(node, 'Install', body);
        const l = (this.installed.get(node) ?? []).filter((x) => !(x.view === s.view && x.api === s.api && x.instance === s.instance));
        l.push(s);
        this.installed.set(node, l);
        return r;
      }
      case 'tamper':
      case 'untamper':
        return this.net.ctl(node, 'Tamper', { view: s.view, on: s.op === 'tamper' });
      case 'kill':
        this.net.kill(node);
        await this.net.ctl('d', 'Note', { kind: 'Killed', cell: '', detail: { node } });
        return null;
      case 'start': {
        await this.net.boot(node);
        for (const x of this.installed.get(node) ?? []) await this.step(x);
        await this.net.ctl('d', 'Note', { kind: 'Started', cell: '', detail: { node } });
        return null;
      }
      case 'wait':
        return new Promise((r) => setTimeout(r, s.ms));
    }
    return null;
  }

  async action(name: string): Promise<unknown> {
    for (const s of this.plan.actions[name]) await this.step(s);
    return null;
  }
  up(): Record<string, boolean> {
    return Object.fromEntries(this.net.up);
  }
}
