// Vaults that live somewhere else: a KeePass file where it is, or a Bitwarden-compatible server.
// The list holds no secret. An offline copy of a server account is encrypted as that server's is.

import { host } from '../engine/host';
import { pass } from '../engine/pass.svelte';
import { overlays } from '../parts/floating.svelte';
import { keepassAnswer } from './hardware';
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

/**
 * Where this runs, so Bitwarden's sign-in emails name the right kind of device. The desktop app's
 * engine knows its system already; in a browser it is wasm and cannot tell.
 */
function platform() {
  if (host().kind === 'desktop') return '';
  // Most of these also say Chrome or Safari, so the more particular ones come first.
  const browsers: [string, RegExp][] = [
    ['edge', /Edg\//],
    ['opera', /OPR\//],
    ['vivaldi', /Vivaldi/],
    ['duckduckgo', /DuckDuckGo|Ddg\//],
    ['firefox', /Firefox\//],
    ['chrome', /Chrome\//],
    ['safari', /Safari\//],
  ];
  const browser = browsers.find(([, test]) => test.test(navigator.userAgent))?.[0] ?? 'other';
  return `${browser}-${host().kind === 'extension' ? 'extension' : 'browser'}`;
}

class Accounts {
  list = $state.raw<AccountRef[]>([]);
  /** How the last trip to a server went: null for fine, or words to show. */
  syncProblem = $state<string | null>(null);
  private device = '';
  /** The account opened last, to have it chosen at the door when this device has no vault of its own. */
  private last = '';
  /** The account just added, for the door to have ready the next time it shows. */
  private signInNextId: string | null = null;

  signInNext(id: string) {
    this.signInNextId = id;
  }

  takeSignInNext(): AccountRef | null {
    const id = this.signInNextId;
    this.signInNextId = null;
    return this.list.find((a) => a.id === id) ?? null;
  }

  /** The account the door starts on when there is no vault here to unlock: the last one opened. */
  preferred(): AccountRef | null {
    return this.list.find((a) => a.id === this.last) ?? this.list[0] ?? null;
  }

  async load() {
    const raw = await host().loadSettings(SETTINGS);
    if (!raw) return;
    try {
      const parsed = JSON.parse(raw);
      this.list = parsed.list ?? [];
      this.device = parsed.device ?? '';
      this.last = parsed.last ?? '';
    } catch {
      // A damaged list costs the names, not the vaults.
    }
  }

  private async store() {
    await host().saveSettings(SETTINGS, JSON.stringify({ list: this.list, device: this.device, last: this.last }));
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
    this.last = '';
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
  private async openKdbx(ref: AccountRef, password: Uint8Array, keyFile: Uint8Array | null, slot: 0 | 1 | 2) {
    const data = await this.readKdbx(ref);
    // A YubiKey answers this file's challenge; the answer then goes in with the password.
    const answer = slot ? await keepassAnswer(data, slot) : null;
    const blobs = [data, ...(keyFile ? [keyFile] : []), ...(answer ? [answer] : [])];
    const reply = await pass
      .engine('kdbx.open', { name: ref.label, response: !!answer }, { secret: password, blobs })
      .finally(() => answer?.fill(0));
    await pass.adoptAccount(ref.id, reply, async (bytes) => {
      // The last good copy stays here, encrypted as the file itself is, in case the write is cut off.
      await host().writeFile(`${FOLDER}/${ref.id}.bak`, data);
      await this.writeKdbx(ref, bytes);
    });
    if (reply.json.kdbx_upgrade) {
      overlays.toast(
        'This file is in the older KDBX 3 format. The first change saved rewrites it as KDBX 4, which KeePass 2.35 or later, KeePassXC 2.3 or later, KeePassDX and Strongbox open. WM Pass keeps a copy of the file as it was.',
        { ms: 12000 },
      );
    }
  }

  /**
   * Signs in to a Bitwarden-compatible server, or opens the offline copy if there is one. `code` is
   * a two-step code, or with `deviceCode` the one Bitwarden emails to confirm a new device.
   */
  private async openBitwarden(ref: AccountRef, password: Uint8Array, code: string | null, deviceCode: boolean) {
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
            { server: ref.server, region: ref.region, email: ref.email, device: this.deviceId(), platform: platform(), ...(code ? { [deviceCode ? 'device_code' : 'code']: code } : {}) },
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
      this.syncProblem = failure?.message ?? 'The server could not be reached.';
    }
  }

  async open(ref: AccountRef, password: Uint8Array, options: { keyFile?: Uint8Array | null; slot?: 0 | 1 | 2; code?: string | null; deviceCode?: boolean } = {}) {
    await (ref.kind === 'kdbx'
      ? this.openKdbx(ref, password, options.keyFile ?? null, options.slot ?? 0)
      : this.openBitwarden(ref, password, options.code ?? null, options.deviceCode ?? false));
    if (this.last !== ref.id) {
      this.last = ref.id;
      await this.store();
    }
  }
}

export const accounts = new Accounts();

/** Nothing to unlock at all: no vault in this folder and no KeePass file or server account either. */
export function nothingToUnlock() {
  return pass.state.kind === 'none' && accounts.list.length === 0;
}
pass.onWipe.push(() => void accounts.forget());
