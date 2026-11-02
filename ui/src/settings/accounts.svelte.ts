// Vaults that live somewhere else: a KeePass file where it is, or a Bitwarden-compatible server.
// The list holds no secret. An offline copy of a server account is encrypted as that server's is.

import { host } from '../engine/host';
import { pass } from '../engine/pass.svelte';
import { online } from './net.svelte';

export interface AccountRef {
  id: string;
  kind: 'kdbx' | 'bitwarden';
  label: string;
  server: string;
  region: string;
  email: string;
  /** Where a KeePass file is. Empty when the app keeps the only copy itself. */
  uri: string;
}

const SETTINGS = 'accounts';
const FOLDER = 'accounts';

function base64(bytes: Uint8Array) {
  let text = '';
  const step = 0x8000;
  for (let i = 0; i < bytes.length; i += step) text += String.fromCharCode(...bytes.subarray(i, i + step));
  return btoa(text);
}

class Accounts {
  list = $state.raw<AccountRef[]>([]);
  /** How the last trip to a server went: null for fine, or words to show. */
  syncProblem = $state<string | null>(null);
  private device = '';

  async load() {
    const raw = await host().loadSettings(SETTINGS);
    if (!raw) return;
    try {
      const parsed = JSON.parse(raw);
      this.list = parsed.list ?? [];
      this.device = parsed.device ?? '';
    } catch {
      // A damaged list costs the names, not the vaults.
    }
  }

  private async store() {
    await host().saveSettings(SETTINGS, JSON.stringify({ list: this.list, device: this.device }));
  }

  private deviceId() {
    if (!this.device) {
      this.device = crypto.randomUUID();
      void this.store();
    }
    return this.device;
  }

  async add(ref: AccountRef) {
    this.list = [...this.list.filter((a) => a.id !== ref.id), ref];
    await this.store();
  }

  async remove(id: string) {
    this.list = this.list.filter((a) => a.id !== id);
    await this.store();
    await host().removeFile(`${FOLDER}/${id}.cache`);
    await host().removeFile(`${FOLDER}/${id}.bak`);
    await host().removeFile(`${FOLDER}/${id}.kdbx`);
  }

  /** On a wipe: no sign of them remains, offline copies included. */
  async forget() {
    this.list = [];
    this.device = '';
    await host().saveSettings(SETTINGS, '');
    await host().removeFolder(FOLDER);
  }

  newId() {
    return crypto.randomUUID().slice(0, 12);
  }

  /** Keeps a chosen file for a shell that cannot reach it again later. */
  async keepFile(id: string, bytes: Uint8Array) {
    await host().writeFile(`${FOLDER}/${id}.kdbx`, bytes);
  }

  private async readKdbx(ref: AccountRef) {
    const outside = host().outside;
    return ref.uri && outside ? outside.read(ref.uri) : host().readFile(`${FOLDER}/${ref.id}.kdbx`);
  }

  private async writeKdbx(ref: AccountRef, bytes: Uint8Array) {
    const outside = host().outside;
    if (ref.uri && outside) await outside.write(ref.uri, bytes);
    else await host().writeFile(`${FOLDER}/${ref.id}.kdbx`, bytes);
  }

  /** Opens a KeePass file in place. It stays where it is and is written back on each save. */
  private async openKdbx(ref: AccountRef, password: Uint8Array, keyFile: Uint8Array | null) {
    const data = await this.readKdbx(ref);
    const reply = await pass.engine('kdbx.open', { name: ref.label }, { secret: password, blobs: keyFile ? [data, keyFile] : [data] });
    await pass.adoptAccount(ref.id, reply, async (bytes) => {
      // The last good copy stays here, encrypted as the file itself is, in case the write is cut off.
      await host().writeFile(`${FOLDER}/${ref.id}.bak`, data);
      await this.writeKdbx(ref, bytes);
    });
  }

  /** Signs in to a Bitwarden-compatible server, or opens the offline copy if there is one. */
  private async openBitwarden(ref: AccountRef, password: Uint8Array, code: string | null) {
    if (!host().network) throw Object.assign(new Error('This shell cannot reach a server.'), { code: 'network' });
    const cachePath = `${FOLDER}/${ref.id}.cache`;
    const hasCache = (await host().listFiles(FOLDER)).some((f) => f.name === `${ref.id}.cache`);
    const reply =
      hasCache && !code
        ? await pass.engine(
            'bitwarden.open_cached',
            { server: ref.server, region: ref.region, cache: base64(await host().readFile(cachePath)) },
            { secret: password, online: online() },
          )
        : await pass.engine(
            'bitwarden.login',
            { server: ref.server, region: ref.region, email: ref.email, device: this.deviceId(), ...(code ? { code } : {}) },
            { secret: password, online: online() },
          );
    await pass.adoptAccount(ref.id, reply, (bytes) => host().writeFile(cachePath, bytes));
    pass.afterAccountSave = () => this.syncNow();
    if (hasCache) void this.syncNow();
    else await pass.flush();
  }

  /** Takes local changes to the server and brings its changes back. Failing leaves them queued. */
  async syncNow() {
    try {
      await pass.net('remote.sync', online());
      await pass.reload();
      this.syncProblem = null;
    } catch (failure: any) {
      this.syncProblem = failure.message ?? 'The server could not be reached.';
    }
  }

  open(ref: AccountRef, password: Uint8Array, options: { keyFile?: Uint8Array | null; code?: string | null } = {}) {
    return ref.kind === 'kdbx' ? this.openKdbx(ref, password, options.keyFile ?? null) : this.openBitwarden(ref, password, options.code ?? null);
  }
}

export const accounts = new Accounts();
pass.onWipe.push(() => void accounts.forget());
