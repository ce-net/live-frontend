// The shell. It has no list of cells: it watches fe.manifest with the viewer's chain and mounts whatever it is
// sent, each UI cell in its own sandboxed frame, joined to the world by one MessagePort to this bus. The bus lets
// a cell make only the calls, events and slot openings its signed manifest entry declares; the node's gate checks
// every call again. A changed entry swaps that one cell, with its state; a crashed cell restarts alone; a call to a
// holder that stopped moves to the next holder.
import { b64decode, hex, sha256, type Door, type DoorReply } from './door';
import { Identity, verifyEd25519 } from './identity';

export interface SlotDecl { name: string; view: string; props: string }
export interface EntryBody {
  view: string; instance: string; api: string; version: string; schema: string; bundle: string; bundle_ref: string;
  needs: string[]; place: string; data: string[]; tries: string[]; streams: string[]; props: string;
  props_fields: [string, string][]; state: string; slots: SlotDecl[]; speaks: string[]; hears: string[];
  supersedes: string | null; until: number | null; label: string;
}
export interface Holder { node: string; sig: string; lease_until: number }
export interface Entry { body: EntryBody; holders: Holder[] }
export interface NodeInfo { name: string; class: string; key: string; fp: string }
export interface Welcome { root: string; nodes: NodeInfo[]; door: string; mode: string }

interface Mounted {
  key: string;
  entry: Entry;
  panel: HTMLElement;
  frame: HTMLIFrameElement | null;
  port: MessagePort | null;
  boot: number;
  ready: boolean;
  lastBeat: number;
  maxGap: number;
  state: unknown;
  holderAt: number;
  frozen: { op: string; body: unknown; id: number }[] | null;
  frozenPort: MessagePort | null;
  waits: Map<number, (v: unknown) => void>;
  slots: Map<string, HTMLElement>;
  children: Map<string, Mounted>;
  parent: Mounted | null;
  props: unknown;
  restarts: number;
  readyWait: ((ok: boolean) => void) | null;
}

export interface ShellEvent { t: number; kind: string; cell: string; detail: Record<string, unknown> }

const ALLOWED_OPEN = [/^https:\/\/github\.com\/ce-net\//, /^https:\/\/ce-net\.github\.io\//, /^https:\/\/(www\.)?ce-net\.com\//];

export class Shell {
  id: Identity | null = null;
  chain: unknown = null;
  chainId = '';
  welcome: Welcome | null = null;
  entries: Entry[] = [];
  mounted = new Map<string, Mounted>();
  boots: Record<string, number> = {};
  events: ShellEvent[] = [];
  bad = new Set<string>(); // holder|bundle pairs that served wrong bytes
  kept = new Map<string, { state: unknown; from: string; t: number }>();
  stats = { fetched: 0, refusedBundles: 0, swaps: [] as Record<string, unknown>[], failovers: [] as Record<string, unknown>[], loopMaxGap: 0, calls: 0, stalls: [] as Record<string, unknown>[] };
  private loopLast = performance.now();
  private resolving = false;
  private timer: number | null = null;
  private nonce = 0;
  onChange: () => void = () => {};

  constructor(private door: Door, private regions: Record<string, HTMLElement>, private kitCss: string) {
    setInterval(() => {
      const t = performance.now();
      this.stats.loopMaxGap = Math.max(this.stats.loopMaxGap, t - this.loopLast - 100);
      this.loopLast = t;
    }, 100);
    setInterval(() => this.watchdog(), 500);
    setInterval(() => this.saveAll(), 2000);
  }

  event(kind: string, cell: string, detail: Record<string, unknown> = {}, report = false): void {
    this.events.push({ t: Date.now(), kind, cell, detail });
    if (this.events.length > 500) this.events.shift();
    if (report && this.chain) void this.call(null, 'd', 'fe.history@1', 'Observe', { kind, cell, detail });
    this.onChange();
  }

  async hello(): Promise<Welcome> {
    const r = await this.door.req('Hello', null);
    if (!r.ok) throw new Error(r.refused || r.transport || 'no door');
    this.welcome = r.body as Welcome;
    return this.welcome;
  }

  /** Join as a visitor: a fresh key in this browser, a proof of work, and the chain fe.visitor signs for it. */
  async join(): Promise<void> {
    this.id = await Identity.make();
    const pow = await this.id.pow(this.welcome?.mode === 'live' ? 3 : 2);
    const r = await this.door.req('Join', { holder: this.id.pub, pow });
    if (!r.ok) throw new Error('join refused: ' + (r.refused ?? r.transport));
    this.chain = r.body.chain;
    this.chainId = r.body.id;
    this.event('Joined', 'fe.visitor', { chain: this.chainId, until: r.body.until, uses: r.body.uses });
    this.start();
  }

  start(): void {
    if (this.timer === null) this.timer = window.setInterval(() => void this.resolve(), 1000);
    void this.resolve();
  }

  /** One signed visitor call through the door to `to`. */
  async call(m: Mounted | null, to: string, api: string, op: string, body: unknown): Promise<DoorReply> {
    if (!this.id || !this.chain) return { ok: false, body: null, refused: 'no chain: join first' };
    const call = { api, op, body: JSON.stringify(body ?? null), ts: Date.now(), nonce: `${this.chainId.slice(0, 6)}-${++this.nonce}-${Math.random().toString(36).slice(2, 8)}` };
    const hsig = await this.id.sign(`lf-call\n${call.api}\n${call.op}\n${call.body}\n${call.ts}\n${call.nonce}`);
    this.stats.calls++;
    const t = performance.now();
    const r = await Promise.race([
      this.door.req('Call', { to, vc: { call, chain: this.chain, hsig } }),
      new Promise<DoorReply>((ok) => setTimeout(() => ok({ ok: false, body: null, transport: `${to}: no answer in 1.5 s` }), 1500)),
    ]);
    r.ms = r.ms ?? performance.now() - t;
    void m;
    return r;
  }

  /** A call to a cell's backend: the entry's holders in turn; a holder that does not answer is skipped. */
  async callCell(m: Mounted, op: string, body: unknown): Promise<DoorReply> {
    const hs = m.entry.holders.map((h) => h.node);
    let last: DoorReply = { ok: false, body: null, transport: 'no holders' };
    for (let i = 0; i < hs.length; i++) {
      const k = (m.holderAt + i) % hs.length;
      const r = await this.call(m, hs[k], m.entry.body.api, op, body);
      if (!r.transport) {
        if (i > 0) {
          const f = { view: m.entry.body.view, op, from: hs[m.holderAt % hs.length], to: hs[k], why: last.transport, t: Date.now() };
          this.stats.failovers.push(f);
          this.event('FailedOver', m.entry.body.view, f, true);
          m.holderAt = k;
        }
        return r;
      }
      last = r;
    }
    return last;
  }

  async resolve(): Promise<void> {
    if (this.resolving || !this.chain) return;
    this.resolving = true;
    try {
      const r = await this.call(null, 'd', 'fe.manifest@1', 'Resolve', null);
      if (r.transport) return;
      if (!r.ok) {
        this.event('Refused', 'fe.manifest', { why: r.refused });
        await this.leaveAll(r.refused ?? 'refused');
        return;
      }
      const entries = r.body.entries as Entry[];
      this.entries = entries;
      const want = new Map(entries.filter((e) => e.body.place !== 'slot').map((e) => [keyOf(e), e]));
      for (const [k, m] of this.mounted) {
        const e = want.get(k);
        if (!e) {
          this.unmount(m, 'gone');
          continue;
        }
        if (e.body.bundle !== m.entry.body.bundle || e.body.version !== m.entry.body.version) {
          void this.swap(m, e);
        } else {
          m.entry = { ...m.entry, holders: e.holders, body: e.body };
        }
      }
      for (const [k, e] of want) if (!this.mounted.has(k)) void this.mount(e, null, null, null);
      // slot children follow their entry too
      for (const m of this.mounted.values())
        for (const c of m.children.values()) {
          const e = entries.find((x) => x.body.view === c.entry.body.view);
          if (!e) this.unmount(c, 'gone');
          else if (e.body.bundle !== c.entry.body.bundle) void this.swap(c, e);
          else c.entry = e;
        }
      this.onChange();
    } finally {
      this.resolving = false;
    }
  }

  async leaveAll(why: string): Promise<void> {
    const all = [...this.mounted.values()];
    for (const m of all) {
      this.unmount(m, why);
      await new Promise((r) => setTimeout(r, 150));
    }
    this.entries = [];
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
    this.onChange();
  }

  /** Fetches a bundle by hash from the entry's holders and checks it against the signed entry. */
  async bundle(e: Entry): Promise<Uint8Array | null> {
    const signed = await this.entrySigned(e);
    if (!signed) {
      this.event('Refused', e.body.view, { why: 'no holder signature on this entry verifies' });
      return null;
    }
    for (const h of e.holders) {
      const k = `${h.node}|${e.body.bundle}`;
      if (this.bad.has(k)) continue;
      const r = await this.call(null, h.node, 'fe.serve@1', 'Fetch', { sha256: e.body.bundle });
      if (r.transport || !r.ok) continue;
      const bytes = b64decode(r.body.b64);
      const got = hex(await sha256(bytes));
      this.stats.fetched++;
      if (got !== e.body.bundle) {
        this.bad.add(k);
        this.stats.refusedBundles++;
        const d = { holder: h.node, want: e.body.bundle, got };
        this.event('Refused', e.body.view, { ...d, why: 'bundle sha256 does not match the signed entry' });
        void this.call(null, 'd', 'fe.manifest@1', 'Report', { view: e.body.view, bundle: e.body.bundle, holder: h.node, why: `sha256 ${got.slice(0, 12)} != ${e.body.bundle.slice(0, 12)}` });
        continue;
      }
      return bytes;
    }
    return null;
  }

  private async entrySigned(e: Entry): Promise<boolean> {
    const msg = 'lf-entry\n' + JSON.stringify(e.body);
    for (const h of e.holders) {
      const key = this.welcome?.nodes.find((n) => n.name === h.node)?.key;
      if (key && (await verifyEd25519(key, msg, h.sig))) return true;
    }
    return false;
  }

  private panelFor(e: Entry, parent: Mounted | null, slot: string | null): HTMLElement {
    if (parent && slot) return parent.slots.get(slot)!;
    const p = document.createElement('section');
    p.className = 'cellpanel';
    p.dataset.view = e.body.view;
    const head = document.createElement('div');
    head.className = 'cellhead';
    p.append(head);
    const body = document.createElement('div');
    body.className = 'cellbody';
    p.append(body);
    for (const s of e.body.slots) {
      const r = document.createElement('div');
      r.className = 'slot';
      r.dataset.slot = s.name;
      r.dataset.state = 'empty';
      p.append(r);
    }
    const region = this.regions[e.body.place] ?? this.regions.main;
    const order = (x: Element) => (x as HTMLElement).dataset.view ?? '';
    const after = [...region.children].find((c) => order(c) > e.body.view);
    region.insertBefore(p, after ?? null);
    return p;
  }

  private headText(m: Mounted): void {
    const head = m.panel.querySelector(':scope > .cellhead');
    if (!head) return;
    const hs = m.entry.holders.map((h) => h.node).join(' ');
    head.textContent = `${m.entry.body.view}${m.entry.body.instance !== 'main' ? ' · ' + m.entry.body.instance : ''} · ${m.entry.body.api.split('@')[0]}@${m.entry.body.version} · on ${hs} · sha256 ${m.entry.body.bundle.slice(0, 10)}`;
  }

  async mount(e: Entry, parent: Mounted | null, slot: string | null, props: unknown, state?: { state: unknown; from: string }): Promise<Mounted | null> {
    const key = parent && slot ? `${parent.key}/${slot}` : keyOf(e);
    const bytes = await this.bundle(e);
    if (!bytes) return null;
    if (!parent && this.mounted.has(key)) return this.mounted.get(key)!;
    const panel = this.panelFor(e, parent, slot);
    const m: Mounted = {
      key, entry: e, panel, frame: null, port: null, boot: 0, ready: false, lastBeat: 0, maxGap: 0, state: null, holderAt: hashIdx(key, e.holders.length),
      frozen: null, frozenPort: null, waits: new Map(), slots: new Map(), children: new Map(), parent, props, restarts: 0, readyWait: null,
    };
    for (const s of e.body.slots) m.slots.set(s.name, panel.querySelector(`:scope > .slot[data-slot="${s.name}"]`) as HTMLElement);
    const keep = state ?? this.kept.get(key);
    if (parent && slot) parent.children.set(slot, m);
    else this.mounted.set(key, m);
    await this.boot(m, bytes, keep ? keep.state : null, keep ? keep.from : null);
    this.headText(m);
    this.event('Mounted', e.body.view, { version: e.body.version, boot: m.boot, holders: e.holders.map((h) => h.node) });
    return m;
  }

  /** Starts a frame for `m` with `bytes`. Resolves when the cell says it is ready (or after 2 s, false). */
  private boot(m: Mounted, bytes: Uint8Array, state: unknown, from: string | null, hidden = false): Promise<boolean> {
    return this.makeFrame(m, bytes, state, from, hidden).then((f) => f !== null);
  }

  private async makeFrame(m: Mounted, bytes: Uint8Array, state: unknown, from: string | null, hidden: boolean): Promise<HTMLIFrameElement | null> {
    const script = new TextDecoder().decode(bytes);
    const shaB64 = btoa(String.fromCharCode(...(await sha256(bytes))));
    const theme = document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light';
    const csp = `default-src 'none'; script-src 'sha256-${shaB64}'; style-src 'unsafe-inline'; img-src data:`;
    const frame = document.createElement('iframe');
    frame.setAttribute('sandbox', 'allow-scripts');
    frame.setAttribute('title', `${m.entry.body.view} ${m.entry.body.version}`);
    frame.className = 'cellframe';
    frame.srcdoc = `<!doctype html><html data-theme="${theme}"><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${csp}"><style>${this.kitCss}</style></head><body><div id="cell"></div><script>${script}</script></body></html>`;
    if (hidden) frame.style.display = 'none';
    const host = m.parent ? m.panel : (m.panel.querySelector(':scope > .cellbody') as HTMLElement);
    host.append(frame);
    const boot = (this.boots[m.key] = (this.boots[m.key] ?? 0) + 1);
    const ch = new MessageChannel();
    const prev = { frame: m.frame, port: m.port };
    await new Promise<void>((ok) => frame.addEventListener('load', () => ok(), { once: true }));
    const init = { view: m.entry.body.view, instance: m.entry.body.instance, api: m.entry.body.api, version: m.entry.body.version, label: m.entry.body.label, boot, state, from, props: m.props };
    const ready = new Promise<boolean>((ok) => {
      const t = setTimeout(() => ok(false), 2500);
      m.readyWait = (v) => {
        clearTimeout(t);
        ok(v);
      };
    });
    // the new frame's port is live only once it is the cell's frame (a swap keeps the old one until then)
    const port = ch.port1;
    port.onmessage = (ev) => this.bus(m, port, ev.data);
    frame.contentWindow!.postMessage({ t: 'init', init }, '*', [ch.port2]);
    m.frame = frame;
    m.port = port;
    m.boot = boot;
    m.ready = false;
    m.lastBeat = performance.now();
    const ok = await ready;
    if (!ok) {
      frame.remove();
      m.frame = prev.frame;
      m.port = prev.port;
      return null;
    }
    m.ready = true;
    if (m.panel.dataset) m.panel.dataset.state = 'mounted';
    if (m.parent) m.panel.dataset.state = 'mounted';
    return frame;
  }

  private reply(port: MessagePort, id: number, r: DoorReply): void {
    port.postMessage({ t: 'reply', id, ok: r.ok, body: r.body, refused: r.refused ?? r.transport ?? null });
  }

  /** Every message a cell sends goes through here, and only what its entry declares gets past. */
  private async bus(m: Mounted, port: MessagePort, d: any): Promise<void> {
    const e = m.entry.body;
    switch (d?.t) {
      case 'beat': {
        if (port !== m.port) return;
        const t = performance.now();
        if (m.ready) m.maxGap = Math.max(m.maxGap, t - m.lastBeat);
        m.lastBeat = t;
        return;
      }
      case 'ready':
        m.readyWait?.(true);
        return;
      case 'size':
        if (port === m.port && m.frame) m.frame.style.height = Math.min(Number(d.h) || 60, 900) + 'px';
        return;
      case 'state':
        if (d.id === 0) {
          if (port === m.port) m.state = d.state;
          return;
        }
      // falls through
      case 'probe': {
        const w = m.waits.get(d.id);
        m.waits.delete(d.id);
        w?.(d.t === 'state' ? d.state : d.result);
        return;
      }
      case 'crash':
        if (port === m.port) void this.crashed(m, String(d.why));
        return;
      case 'call': {
        const allowed = [...e.data, ...e.tries, ...e.streams];
        if (typeof d.op !== 'string' || !allowed.includes(d.op)) {
          this.reply(port, d.id, { ok: false, body: { NotDeclared: { op: d.op, view: e.view } }, refused: `NotDeclared: ${e.view} may call ${allowed.join(', ')} of ${e.api}; not ${d.op}` });
          this.event('NotDeclared', e.view, { op: d.op });
          return;
        }
        if (m.frozen && port === m.frozenPort) {
          m.frozen.push({ op: d.op, body: d.body, id: d.id });
          return;
        }
        this.reply(port, d.id, await this.callCell(m, d.op, d.body));
        return;
      }
      case 'emit': {
        if (!e.speaks.includes(d.event)) {
          this.event('NotDeclared', e.view, { event: d.event });
          return;
        }
        if (d.event === 'Open' && typeof d.data?.url === 'string' && ALLOWED_OPEN.some((r) => r.test(d.data.url))) window.open(d.data.url, '_blank', 'noopener');
        this.event('Event', e.view, { event: d.event });
        return;
      }
      case 'slot': {
        const decl = e.slots.find((s) => s.name === d.name);
        if (!decl) {
          this.event('NotDeclared', e.view, { slot: d.name });
          return;
        }
        const child = this.entries.find((x) => x.body.view === decl.view);
        const region = m.slots.get(decl.name);
        if (!child || !region) {
          if (region) region.dataset.state = 'empty';
          return;
        }
        const why = propsMismatch(child.body, d.props);
        if (why) {
          this.event('SlotTypeMismatch', e.view, { slot: d.name, why });
          return;
        }
        if (m.children.has(decl.name)) return;
        await this.mount(child, m, decl.name, d.props);
        return;
      }
    }
  }

  private ask(m: Mounted, t: 'save' | 'probe', ms = 1000): Promise<unknown> {
    return new Promise((ok) => {
      if (!m.port) return ok(null);
      const id = ++this.nonce;
      const timer = setTimeout(() => {
        m.waits.delete(id);
        ok(undefined);
      }, ms);
      m.waits.set(id, (v) => {
        clearTimeout(timer);
        ok(v);
      });
      m.port.postMessage({ t, id });
    });
  }

  private async saveAll(): Promise<void> {
    for (const m of this.all()) {
      if (!m.ready || m.frozen) continue;
      const s = await this.ask(m, 'save', 800);
      if (s !== undefined) m.state = s;
    }
  }

  all(): Mounted[] {
    const out: Mounted[] = [];
    for (const m of this.mounted.values()) out.push(m, ...m.children.values());
    return out;
  }

  /** Hot swap of one cell: save, freeze, verify, mount, restore, replay, remove. Nothing else is touched. */
  async swap(m: Mounted, e: Entry): Promise<void> {
    if (m.frozen) return;
    const t0 = performance.now();
    const from = m.entry.body.version;
    const before = { ...this.boots };
    for (const x of this.all()) x.maxGap = 0;
    m.frozen = [];
    m.frozenPort = m.port;
    const saved = await this.ask(m, 'save', 1000);
    const state = saved === undefined ? m.state : saved;
    const bytes = await this.bundle(e);
    if (!bytes) {
      const q = m.frozen, port = m.frozenPort;
      m.frozen = null;
      m.frozenPort = null;
      for (const c of q) if (port) this.reply(port, c.id, await this.callCell(m, c.op, c.body));
      this.event('SwapRefused', e.body.view, { from, to: e.body.version, why: 'no holder served the signed bundle' }, true);
      return;
    }
    const oldFrame = m.frame;
    const oldEntry = m.entry;
    m.entry = e;
    const frame = await this.makeFrame(m, bytes, state, from, false);
    if (!frame) {
      m.entry = oldEntry;
      const q = m.frozen, port = m.frozenPort;
      m.frozen = null;
      m.frozenPort = null;
      for (const c of q) if (port) this.reply(port, c.id, await this.callCell(m, c.op, c.body));
      this.event('SwapRefused', e.body.view, { from, to: e.body.version, why: 'the new frame did not start' }, true);
      return;
    }
    oldFrame?.remove();
    const q = m.frozen;
    m.frozen = null;
    m.frozenPort = null;
    // calls the old frame made while frozen still reach the backend (a write is not lost); the old frame is gone
    for (const c of q) await this.callCell(m, c.op, c.body);
    this.headText(m);
    const ms = performance.now() - t0;
    const others = Object.keys(this.boots).filter((k) => k !== m.key && !k.startsWith(m.key + '/'));
    const touched = others.filter((k) => before[k] !== undefined && before[k] !== this.boots[k]);
    const maxGap = Math.max(0, ...this.all().filter((x) => x !== m && x.parent !== m).map((x) => x.maxGap));
    const s = { view: e.body.view, from, to: e.body.version, ms: Math.round(ms), state: state ?? null, others: others.length, touched, maxOtherBeatGapMs: Math.round(maxGap) };
    this.stats.swaps.push(s);
    this.event('Swapped', e.body.view, s, true);
  }

  private async crashed(m: Mounted, why: string): Promise<void> {
    if (!m.ready) return;
    m.ready = false;
    m.frame?.remove();
    m.frame = null;
    m.port = null;
    this.event('Crashed', m.entry.body.view, { why, boot: m.boot }, true);
    const delays = [500, 1000, 2000];
    const note = document.createElement('div');
    note.className = 'restarting';
    note.textContent = `${m.entry.body.view} crashed (${why}); restarting`;
    const host = m.parent ? m.panel : (m.panel.querySelector(':scope > .cellbody') as HTMLElement);
    host.append(note);
    for (const d of delays) {
      await new Promise((r) => setTimeout(r, d));
      const bytes = await this.bundle(m.entry);
      if (bytes && (await this.makeFrame(m, bytes, m.state, m.entry.body.version, false))) {
        note.remove();
        m.restarts++;
        this.event('Restarted', m.entry.body.view, { boot: m.boot, after_ms: d, state: m.state ?? null }, true);
        return;
      }
    }
    note.textContent = `${m.entry.body.view} did not restart after three tries`;
  }

  /**
   * A frame that misses three beats while the others keep beating is hung: it is restarted alone. When most
   * frames miss them together, the browser paused the one process that runs them (Chromium puts every sandboxed
   * frame of a page in one renderer process), so nothing is restarted; the pause is measured and reported.
   */
  private watchdog(): void {
    const t = performance.now();
    const live = this.all().filter((m) => m.ready && !m.frozen);
    const late = live.filter((m) => t - m.lastBeat > 1600);
    if (!this.stall && late.length >= 2 && late.length * 2 > live.length) {
      this.stall = { from: Math.min(...late.map((m) => m.lastBeat)), frames: late.length, resumed: null };
    }
    if (this.stall) {
      this.stall.frames = Math.max(this.stall.frames, late.length);
      if (late.length === 0 || (this.stall.resumed !== null && t - this.stall.resumed > 4000)) {
        const s = { ms: Math.round((this.stall.resumed ?? t) - this.stall.from), frames: this.stall.frames, of: live.length };
        this.stats.stalls.push(s);
        this.event('FramesPaused', 'shell', s, true);
        this.stall = null;
        // a frame still silent gets one more full window; if it stays silent the normal check restarts it
        for (const m of late) m.lastBeat = t;
      } else if (this.stall.resumed === null && late.length < this.stall.frames) {
        this.stall.resumed = t;
      }
      return;
    }
    for (const m of late) void this.crashed(m, 'stopped answering (three beats missed)');
  }
  private stall: { from: number; frames: number; resumed: number | null } | null = null;

  unmount(m: Mounted, why: string): void {
    this.kept.set(m.key, { state: m.state, from: m.entry.body.version, t: Date.now() });
    for (const c of m.children.values()) this.unmount(c, why);
    m.frame?.remove();
    m.port?.close();
    m.ready = false;
    if (m.parent) {
      m.panel.dataset.state = 'empty';
      for (const [k, v] of m.parent.children) if (v === m) m.parent.children.delete(k);
    } else {
      m.panel.remove();
      this.mounted.delete(m.key);
    }
    this.event('Unmounted', m.entry.body.view, { why });
  }

  /** P7: ask one mounted cell to try what it must not do; returns what happened. */
  probe(view: string): Promise<unknown> {
    const m = this.all().find((x) => x.entry.body.view === view);
    return m ? this.ask(m, 'probe', 4000) : Promise.resolve(null);
  }

  /** Revoke this browser's chain: the gates refuse the next call and every panel leaves. */
  async revoke(): Promise<DoorReply> {
    const r = await this.call(null, 'd', 'fe.visitor@1', 'Revoke', null);
    await this.resolve();
    return r;
  }

  frames(): number {
    return document.querySelectorAll('iframe.cellframe').length;
  }
}

export function keyOf(e: Entry): string {
  return `${e.body.view}|${e.body.instance}|${e.body.api}`;
}

function hashIdx(s: string, n: number): number {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return n ? h % n : 0;
}

/** The props a parent passes must decode against the child's declared props type. */
export function propsMismatch(child: EntryBody, props: unknown): string | null {
  if (child.props === '()') return props === null || props === undefined ? null : 'this view takes no props';
  if (typeof props !== 'object' || props === null) return `expected ${child.props}`;
  const p = props as Record<string, unknown>;
  for (const [k, ty] of child.props_fields) {
    const v = p[k];
    const opt = ty.startsWith('Option<');
    if (v === undefined || v === null) {
      if (!opt) return `${child.props}.${k} missing`;
      continue;
    }
    const base = opt ? ty.slice(7, -1) : ty;
    const js = base === 'String' ? 'string' : base === 'bool' ? 'boolean' : /^[ui](8|16|32|64)|f(32|64)|usize$/.test(base) ? 'number' : 'object';
    if (typeof v !== js) return `${child.props}.${k}: expected ${base}, got ${typeof v}`;
  }
  for (const k of Object.keys(p)) if (!child.props_fields.some(([f]) => f === k)) return `${child.props} has no field ${k}`;
  return null;
}
