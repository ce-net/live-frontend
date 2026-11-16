// The viewer's key: a WebCrypto Ed25519 key pair made in this browser, with the private half not extractable.
// Every call is signed with it, so a chain copied off the wire is useless.
import { hex, sha256, unhex } from './door';

export class Identity {
  private constructor(private key: CryptoKeyPair, public pub: string) {}

  static supported(): Promise<boolean> {
    return crypto.subtle.generateKey({ name: 'Ed25519' }, false, ['sign', 'verify']).then(() => true, () => false);
  }

  static async make(): Promise<Identity> {
    const k = (await crypto.subtle.generateKey({ name: 'Ed25519' }, false, ['sign', 'verify'])) as CryptoKeyPair;
    const raw = await crypto.subtle.exportKey('raw', k.publicKey);
    return new Identity(k, hex(raw));
  }

  async sign(msg: string): Promise<string> {
    return hex(await crypto.subtle.sign({ name: 'Ed25519' }, this.key.privateKey, new TextEncoder().encode(msg)));
  }

  /** A small proof of work for Join: sha256("<pub>:<n>") starting with `zeros` hex zeros. */
  async pow(zeros: number): Promise<string> {
    const want = '0'.repeat(zeros);
    for (let i = 0; ; i++) {
      if (hex(await sha256(`${this.pub}:${i}`)).startsWith(want)) return String(i);
    }
  }
}

export async function verifyEd25519(pubHex: string, msg: string, sigHex: string): Promise<boolean> {
  try {
    const k = await crypto.subtle.importKey('raw', unhex(pubHex) as BufferSource, { name: 'Ed25519' }, false, ['verify']);
    return await crypto.subtle.verify({ name: 'Ed25519' }, k, unhex(sigHex) as BufferSource, new TextEncoder().encode(msg));
  } catch {
    return false;
  }
}
