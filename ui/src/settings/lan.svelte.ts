// This computer's identity for the local network, and who it trusts there. Both are held with the
// places, sealed by the system's keychain. Everything else about a sync is in the Rust side.

import { host } from '../engine/host';
import { consumeText, pass } from '../engine/pass.svelte';
import { overlays } from '../parts/floating.svelte';

/** How the last sync or merge with a device went. */
export interface LanStatus {
  at: number;
  ok: boolean;
  text: string;
  /** The engine's code for a failure, such as `not_this_vault`. */
  code?: string;
}

export interface LanPeer {
  key: string;
  /** What the device calls itself. */
  name: string;
  /** What the person calls it here, if they renamed it. */
  nickname?: string;
  last?: LanStatus;
}

/** A sync or merge that stopped, as the engine reports it. */
interface Failed {
  code: string;
  message: string;
  key: string | null;
}

/** One device's part in a Sync now. */
export interface LanResult {
  peer: LanPeer;
  ok: boolean;
  text: string;
}

/** The name to show for a device. */
export function shown(peer: LanPeer) {
  return peer.nickname?.trim() || peer.name;
}

/** Words for a failure, said about the device it happened with. */
export function describe(failed: Failed) {
  switch (failed.code) {
    case 'not_this_vault':
      return 'Has a different vault. Merge the two to sync them.';
    case 'io':
      return 'The connection was lost.';
    case 'version':
      return 'Update WM Pass on both devices.';
    case 'declined':
      return 'Declined on that device.';
    case 'unknown_peer':
      return 'It does not know this computer any more. Pair the two again.';
    default: {
      const text = failed.message.charAt(0).toUpperCase() + failed.message.slice(1);
      return text.endsWith('.') ? text : `${text}.`;
    }
  }
}

function syncedText(json: any) {
  return json.sent === 0 && json.received === 0 ? 'Synced. Nothing had changed.' : `Synced: ${json.sent} sent, ${json.received} received.`;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

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
  /** Whether this computer is waiting for its devices on its own, while the vault is open. */
  listening = $state(false);
  /** A merge under way, for the screens to show. */
  merging = $state<string | null>(null);
  private background = false;
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
    // Pairing again keeps the name it was given here.
    const before = this.peers.find((p) => p.key === peer.key);
    this.peers = [...this.peers.filter((p) => p.key !== peer.key), { key: peer.key, name: peer.name, nickname: before?.nickname }];
    await this.persist();
  }

  async rename(key: string, nickname: string) {
    this.peers = this.peers.map((p) => (p.key === key ? { ...p, nickname: nickname.trim() || undefined } : p));
    await this.persist();
  }

  private async note(key: string, ok: boolean, text: string, code?: string) {
    const last: LanStatus = { at: Date.now(), ok, text, code };
    this.peers = this.peers.map((p) => (p.key === key ? { ...p, last } : p));
    await this.persist();
  }

  private peer(key: string | null | undefined) {
    return key ? this.peers.find((p) => p.key === key) : undefined;
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

  /** What the engine needs for a sync: the devices to accept, and where attachments live. */
  private async args(peers: LanPeer[] = this.peers) {
    const folder = pass.blobFolder();
    return { peers: peers.map((p) => ({ key: p.key, name: p.name })), blob_dir: folder ? await host().lan!.blobDir(folder) : '' };
  }

  // ---- a new device ----

  /**
   * Starts waiting as a device with no vault, and gives the code to show. `address` is where this
   * device can be reached; without one, only the digits work.
   */
  async joinListen(address: string | null) {
    const reply = await pass.engine('lan.join_listen', { address: address ?? '' }, {});
    return consumeText(reply.secret);
  }

  /** Takes the next connection: the vault itself, or six digits to compare first. */
  async joinWait(blobDir: string): Promise<JoinReply> {
    return this.withIdentity(async (identity) => joinReply(await pass.engine('lan.join_wait', { name: await this.name(), blob_dir: blobDir }, { secret: identity })));
  }

  async joinFinish(accepted: boolean, blobDir: string): Promise<JoinReply> {
    return joinReply(await pass.engine('lan.join_finish', { accepted, blob_dir: blobDir }, {}));
  }

  // ---- sending this vault to a new device ----

  async joinFind(): Promise<string[]> {
    return (await pass.engine('lan.join_find', { ms: 4000 }, {})).json;
  }

  async joinScan(text: string): Promise<LanPeer> {
    return this.withIdentity(async (identity) => {
      const reply = await pass.engine('lan.join_scan', { text, name: await this.name(), ...(await this.args()) }, { handle: pass.handle, secret: identity });
      return reply.json.peer;
    });
  }

  async joinConnect(address: string): Promise<{ code: string; peer: LanPeer }> {
    return this.withIdentity(async (identity) => (await pass.engine('lan.join_connect', { address, name: await this.name() }, { secret: identity })).json);
  }

  async joinSend(accepted: boolean): Promise<LanPeer> {
    return (await pass.engine('lan.join_send', { accepted, ...(await this.args()) }, { handle: pass.handle })).json.peer;
  }

  /**
   * Syncs with every paired device waiting on this network, or only `only`. Each device's
   * outcome is kept with it, so the screen can say which one failed and why.
   */
  async sync(only?: string): Promise<LanResult[]> {
    const found = await this.browse();
    if (found.length === 0) throw new Error('No paired device is waiting on this network. On your phone, choose Wait for one, or open WM Pass on your other computer.');
    let left = this.peers.filter((p) => !only || p.key === only);
    const results: LanResult[] = [];
    await this.withIdentity(async (identity) => {
      for (const address of found) {
        if (left.length === 0) break;
        let json: any;
        try {
          json = (await pass.engine('lan.sync', { address, ...(await this.args(left)) }, { handle: pass.handle, secret: identity })).json;
        } catch {
          continue; // Nothing answered there.
        }
        const peer = this.peer(json.failed ? json.failed.key : json.key);
        // Some other device answered: one not paired, or not the one asked for.
        if (!peer || !left.some((p) => p.key === peer.key) || json.failed?.code === 'unknown_peer') continue;
        left = left.filter((p) => p.key !== peer.key);
        if (json.failed) {
          const text = describe(json.failed);
          await this.note(peer.key, false, text, json.failed.code);
          results.push({ peer, ok: false, text });
        } else {
          await pass.reload();
          await pass.flush();
          const text = syncedText(json);
          await this.note(peer.key, true, text);
          results.push({ peer, ok: true, text });
        }
      }
    });
    if (results.length === 0) {
      const which = only ? shown(this.peer(only)!) : 'None of your paired devices';
      throw new Error(`${which} answered. On a phone, choose Wait for one first.`);
    }
    return results;
  }

  /** Waits on this network for a paired device to sync with, from the screen. */
  async serve() {
    return this.withIdentity(async (identity) => {
      try {
        const port = (await pass.engine('lan.listen', {}, {})).json.port;
        await pass.engine('lan.advertise', { port }, {});
        const reply = await pass.engine('lan.serve', await this.args(), { handle: pass.handle, secret: identity });
        return await this.served(reply.json, false);
      } finally {
        await this.stop();
      }
    });
  }

  /**
   * Keeps waiting for paired devices for as long as the vault stays open, so a phone only has
   * to choose Sync now. Stops with [`stopListening`].
   */
  listen() {
    if (this.background) return;
    this.background = true;
    this.listening = true;
    void this.listenLoop();
  }

  async stopListening() {
    this.background = false;
    this.listening = false;
    await pass.engine('lan.stop_background', {}, {}).catch(() => undefined);
  }

  private async listenLoop() {
    let failures = 0;
    while (this.background) {
      const handle = pass.handle;
      try {
        await pass.engine('lan.listen_background', {}, {});
        const json = await this.withIdentity(async (identity) => (await pass.engine('lan.serve', { background: true, ...(await this.args()) }, { handle, secret: identity })).json);
        if (!this.background || handle !== pass.handle) break;
        await this.served(json, true);
        failures = 0;
      } catch (failure: any) {
        if (failure?.code === 'cancelled' || !this.background) break;
        // A network that comes and goes: try again, more slowly each time.
        failures++;
        await sleep(Math.min(60_000, 2000 * failures));
      }
    }
    this.listening = this.background;
  }

  /** What to do with what a waiting device received. `quiet` is for the background. */
  private async served(json: any, quiet: boolean): Promise<string | null> {
    if (json.failed) {
      const peer = this.peer(json.failed.key);
      // No key, or one not paired: some device looking for another. Nothing to say.
      if (!peer || json.failed.code === 'unknown_peer') return quiet ? null : 'A device that is not paired with this computer tried to connect.';
      const text = describe(json.failed);
      await this.note(peer.key, false, text, json.failed.code);
      return `${shown(peer)}: ${text}`;
    }
    if (json.merge_asked) {
      const peer = this.peer(json.merge_asked.peer.key) ?? json.merge_asked.peer;
      const name = shown(peer);
      const theyKeep = json.merge_asked.they_keep === true;
      const yes = await overlays.ask({
        title: `Merge vaults with ${name}?`,
        body: theyKeep
          ? `${name} keeps its vault. This computer’s items move into it, then this computer uses ${name}’s vault, and you unlock it with that vault’s master password.`
          : `This computer keeps its vault, and ${name}’s items move into it. ${name} then uses this vault.`,
        action: 'Merge',
        danger: theyKeep,
      });
      return this.answerMerge(yes, peer);
    }
    await pass.reload();
    await pass.flush();
    const peer = this.peer(json.key);
    const text = syncedText(json);
    if (peer) await this.note(peer.key, true, text);
    if (quiet && json.received > 0) overlays.toast(`Synced with ${peer ? shown(peer) : json.peer}: ${json.received} received.`);
    return `${peer ? shown(peer) : json.peer}: ${text}`;
  }

  // ---- merging two different vaults ----

  /** Where a vault that may arrive from a merge goes, and its attachments with it. */
  private async newHome() {
    const name = pass.freshVaultName();
    return { name, dir: await host().lan!.blobDir(pass.blobFolderFor(name)) };
  }

  private async answerMerge(accepted: boolean, peer: LanPeer): Promise<string | null> {
    const home = await this.newHome();
    this.merging = accepted ? shown(peer) : null;
    try {
      const reply = await pass.engine('lan.merge_answer', { accepted, ...(await this.args()), new_blob_dir: home.dir }, { handle: pass.handle });
      return accepted ? await this.merged(reply, home.name) : null;
    } finally {
      this.merging = null;
    }
  }

  /**
   * Merges this vault and a paired device's into one. `keepMine` says which is kept; the other
   * device gives its items and then uses the kept vault. The other device must be waiting, and
   * its person says yes there.
   */
  async merge(key: string, keepMine: boolean): Promise<string> {
    const peer = this.peer(key)!;
    const found = await this.browse();
    if (found.length === 0) throw new Error(`${shown(peer)} is not waiting on this network. On it, choose Wait for one.`);
    const home = await this.newHome();
    this.merging = shown(peer);
    try {
      return await this.withIdentity(async (identity) => {
        for (const address of found) {
          let reply;
          try {
            reply = await pass.engine('lan.merge', { address, keep_mine: keepMine, ...(await this.args([peer])), new_blob_dir: home.dir }, { handle: pass.handle, secret: identity });
          } catch {
            continue;
          }
          const failed: Failed | undefined = reply.json.failed;
          if (failed && (failed.key !== peer.key || failed.code === 'unknown_peer')) continue;
          return this.merged(reply, home.name);
        }
        throw new Error(`${shown(peer)} did not answer. On it, choose Wait for one.`);
      });
    } finally {
      this.merging = null;
    }
  }

  private async merged(reply: { json: any; blobs: Uint8Array[] }, name: string): Promise<string> {
    const json = reply.json;
    const peer = this.peer(json.failed ? json.failed.key : json.peer?.key);
    const label = peer ? shown(peer) : (json.peer?.name ?? 'The other device');
    if (json.failed) {
      const text = describe(json.failed);
      if (peer) await this.note(peer.key, false, text, json.failed.code);
      throw new Error(`${label}: ${text}`);
    }
    if (json.merged === 'kept') {
      await pass.reload();
      await pass.flush();
      const text = `Merged: ${json.added} added, ${json.matched} already here.`;
      if (peer) await this.note(peer.key, true, text);
      overlays.toast(`${label}’s items are in this vault now. ${label} uses this vault from here on.`);
      return text;
    }
    // This computer gave its items, and takes the kept vault in place of its own.
    if (peer) await this.note(peer.key, true, 'Merged. Both now use the same vault.');
    await this.stopListening();
    await pass.replaceWith(reply.blobs[0], name);
    overlays.toast(`This computer now uses ${label}’s vault, with your items in it. Unlock it with that vault’s master password.`, { ms: 12_000 });
    return 'Merged.';
  }
}

export type JoinReply = { done: true; peer: LanPeer; file: Uint8Array } | { done?: false; code: string; peer: LanPeer };

function joinReply(reply: { json: any; blobs: Uint8Array[] }): JoinReply {
  return reply.json.done ? { done: true, peer: reply.json.peer, file: reply.blobs[0] } : { code: reply.json.code, peer: reply.json.peer };
}

export const lan = new Lan();
pass.onWipe.push(() => void lan.forget());
