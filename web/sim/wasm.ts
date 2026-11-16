// The node as wasm: the same fe-host and cell cores as the native processes, through a C ABI with JSON in memory.
export interface Wasm {
  call(fn: 'lf_new' | 'lf_input' | 'lf_configs' | 'lf_verify', arg: string): string;
}

export async function load(bytes: ArrayBuffer): Promise<Wasm> {
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const ex = instance.exports as Record<string, any>;
  const mem = ex.memory as WebAssembly.Memory;
  const enc = new TextEncoder();
  const dec = new TextDecoder();
  return {
    call(fn, arg) {
      const b = enc.encode(arg);
      const p = ex.lf_alloc(b.length) as number;
      new Uint8Array(mem.buffer, p, b.length).set(b);
      const r = BigInt.asUintN(64, BigInt(ex[fn](p, b.length)));
      const ptr = Number(r >> 32n);
      const len = Number(r & 0xffffffffn);
      return dec.decode(new Uint8Array(mem.buffer, ptr, len));
    },
  };
}
