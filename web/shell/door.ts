// The one way the page reaches the nodes: frames to fe.door. Over HTTP to a real door (local processes or live
// nodes), or over postMessage to the door node of the in-browser sim.
export interface DoorReply {
  ok: boolean;
  body: any;
  refused?: string | null;
  node?: string;
  /** Set when the frame never reached a node (unreachable, timed out): the bus then tries the next holder. */
  transport?: string;
  ms?: number;
}

export interface Door {
  req(op: string, body: unknown): Promise<DoorReply>;
}

export class HttpDoor implements Door {
  constructor(private url: string) {}
  async req(op: string, body: unknown): Promise<DoorReply> {
    const ctl = new AbortController();
    const t = setTimeout(() => ctl.abort(), 2500);
    try {
      const r = await fetch(this.url, { method: 'POST', body: JSON.stringify({ op, body }), headers: { 'content-type': 'application/json' }, signal: ctl.signal });
      return (await r.json()) as DoorReply;
    } catch (e) {
      return { ok: false, body: null, transport: 'door: ' + String((e as Error).message || e) };
    } finally {
      clearTimeout(t);
    }
  }
}

export function b64decode(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function b64encode(b: Uint8Array): string {
  let s = '';
  for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000));
  return btoa(s);
}

export const hex = (b: ArrayBuffer | Uint8Array) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, '0')).join('');
export const unhex = (s: string) => new Uint8Array((s.match(/../g) ?? []).map((x) => parseInt(x, 16)));

export async function sha256(b: Uint8Array | string): Promise<Uint8Array> {
  const bytes = typeof b === 'string' ? new TextEncoder().encode(b) : b;
  return new Uint8Array(await crypto.subtle.digest('SHA-256', bytes as BufferSource));
}
