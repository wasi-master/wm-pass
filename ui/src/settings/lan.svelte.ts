// This computer's identity for the local network, and who it trusts there. Both are held with the
// places, sealed by the system's keychain. Everything else about a sync is in the Rust side.

import { host } from '../engine/host';
import { pass } from '../engine/pass.svelte';

export interface LanPeer {
  key: string;
  name: string;
}

const FILE = 'lan';

function hex(bytes: Uint8Array) {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

function unhex(text: string) {
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(text.slice(i * 2, i * 2 + 2), 16);
  return out;
}

class Lan {
  peers = $state.raw<LanPeer[]>([]);
  private identityHex = '';
  private loaded = false;

  async load() {
    if (this.loaded) return;
    const raw = await host().loadSealed?.(FILE);
    if (raw) {
      try {
        const parsed = JSON.parse(raw);
        this.peers = parsed.peers ?? [];
        this.identityHex = parsed.identity ?? '';
      } catch {
        this.peers = [];
      }
    }
    this.loaded = true;
  }

  private async persist() {
    await host().saveSealed?.(FILE, JSON.stringify({ peers: this.peers, identity: this.identityHex }));
  }

  async addPeer(peer: LanPeer) {
    this.peers = [...this.peers.filter((p) => p.key !== peer.key), peer];
    await this.persist();
  }

  async removePeer(key: string) {
    this.peers = this.peers.filter((p) => p.key !== key);
    await this.persist();
  }

  /** On a wipe. */
  async forget() {
    this.peers = [];
    this.identityHex = '';
    await this.persist();
  }

  async withIdentity<T>(run: (identity: Uint8Array) => Promise<T>): Promise<T> {
    await this.load();
    if (!this.identityHex) {
      const made = (await pass.engine('lan.new_identity', {}, {})).secret!;
      this.identityHex = hex(made);
      made.fill(0);
      await this.persist();
    }
    const bytes = unhex(this.identityHex);
    try {
      return await run(bytes);
    } finally {
      bytes.fill(0);
    }
  }

  name() {
    return host().lan!.deviceName();
  }

  async browse(): Promise<string[]> {
    return (await pass.engine('lan.browse', { ms: 4000 }, {})).json;
  }

  stop() {
    return pass.engine('lan.stop', {}, {}).catch(() => undefined);
  }

  private async args() {
    const folder = pass.blobFolder();
    return { peers: this.peers, blob_dir: folder ? await host().lan!.blobDir(folder) : '' };
  }

  /** Finds a paired device and syncs. The key exchange decides who it really is. */
  async sync() {
    const found = await this.browse();
    if (found.length === 0) throw new Error('No paired device is waiting on this network. Choose Wait for one on it.');
    return this.withIdentity(async (identity) => {
      let last: unknown;
      for (const address of found) {
        try {
          const reply = await pass.engine('lan.sync', { address, ...(await this.args()) }, { handle: pass.handle, secret: identity });
          await pass.reload();
          await pass.flush();
          return reply.json;
        } catch (failure) {
          last = failure;
        }
      }
      throw last;
    });
  }

  /** Waits on this network for a paired device to sync with. */
  async serve() {
    return this.withIdentity(async (identity) => {
      try {
        const port = (await pass.engine('lan.listen', {}, {})).json.port;
        await pass.engine('lan.advertise', { port }, {});
        const reply = await pass.engine('lan.serve', await this.args(), { handle: pass.handle, secret: identity });
        await pass.reload();
        await pass.flush();
        return reply.json;
      } finally {
        await this.stop();
      }
    });
  }
}

export const lan = new Lan();
pass.onWipe.push(() => void lan.forget());
