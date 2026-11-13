// Somewhere outside this device. Only the encrypted file goes there, so a server cannot read it.
// Backups, file sync and shared vaults all run here, so the order they do things in is one order.

import { bundleLooksRight } from '../engine/bundle';
import { host, type PlaceFile } from '../engine/host';
import { pass } from '../engine/pass.svelte';
import { prefs } from '../engine/prefs.svelte';
import { byName } from '../engine/text';
import { online } from './net.svelte';

export interface Place {
  id: string;
  kind: string;
  label: string;
  path: string;
  url: string;
  user: string;
  password: string;
  endpoint: string;
  region: string;
  bucket: string;
  prefix: string;
  access_key: string;
  secret_key: string;
  backup: boolean;
  sync: boolean;
  /**
   * What each kind needs beyond the fields above: hosts and ports, folders inside an account,
   * and the tokens and sessions a sign-in hands back. The shell keeps these current when they
   * change during a run (see `PlacesHost.onUpdate`).
   */
  extra?: Record<string, any>;
}

/** Whether a place has everything a run needs. An account is not ready until it is signed in. */
export function placeReady(place: Place): boolean {
  const x = place.extra ?? {};
  switch (place.kind) {
    case 'folder':
      return !!place.path;
    case 'webdav':
      return !!place.url.trim();
    case 's3':
      return !!place.bucket.trim() && !!place.access_key.trim() && !!place.secret_key;
    case 'sftp':
    case 'ftp':
      return !!x.host && !!place.user;
    case 'smb':
      return !!x.host && !!x.share && !!place.user;
    case 'nfs':
      return !!x.host && !!x.export;
    case 'git':
      return String(x.repository ?? '').includes('/') && !!x.token;
    case 'imap':
      return !!x.host && !!place.user && !!place.password;
    case 'drive':
    case 'dropbox':
    case 'onedrive':
    case 'box':
      return !!x.refresh_token;
    case 'pcloud':
      return !!x.auth;
    case 'mega':
      return !!x.session_id && !!x.master_key;
    case 'proton':
      return !!x.uid && !!x.refresh_token && !!x.salted_key_pass;
    case 'icloud':
      return !!x.trust_token && !!place.password;
    case 'synology':
    case 'qnap':
      return !!x.server && !!place.user && !!place.password;
    default:
      return false;
  }
}

export function blankPlace(kind: string): Place {
  return {
    id: crypto.randomUUID(),
    kind,
    label: '',
    path: '',
    url: '',
    user: '',
    password: '',
    endpoint: '',
    region: 'us-east-1',
    bucket: '',
    prefix: '',
    access_key: '',
    secret_key: '',
    backup: true,
    sync: false,
    extra: {},
  };
}

export interface RunResult {
  at_ms: number;
  problem: string | null;
}

const PREFIX = 'wmpass-backup-';
const EXTENSION = '.wmbackup';
const SYNC_EXTENSION = '.wmsync';
const BLOB_EXTENSION = '.wmblob';
const SHARED_EXTENSION = '.wmshared';
const MANIFEST_EXTENSION = '.wmmanifest';
const STATE = 'places';

function stamp() {
  const d = new Date();
  const two = (n: number) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}${two(d.getUTCMonth() + 1)}${two(d.getUTCDate())}-${two(d.getUTCHours())}${two(d.getUTCMinutes())}${two(d.getUTCSeconds())}`;
}

/** When a copy was made, from its name. */
function stampOf(name: string) {
  const m = /(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})/.exec(name);
  return m ? Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]) : 0;
}

/** Words for a failure at a place. The shell has already said what kind it was. */
export function placeProblem(failure: any): string {
  return failure?.message || 'The connection failed. WM Pass will try again.';
}

interface Saved {
  every_days: number;
  keep: number;
  last_ms: number;
  runs: Record<string, RunResult>;
  shared: Record<string, string>;
}

/** How often an open vault looks at its sync places, with no edit to set it off. */
const AUTO_SYNC_MS = 5 * 60_000;

async function digest(bytes: Uint8Array) {
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes as BufferSource));
  return Array.from(hash, (b) => b.toString(16).padStart(2, '0')).join('');
}

class Places {
  list = $state.raw<Place[]>([]);
  everyDays = $state(0);
  keep = $state(7);
  runs = $state<Record<string, RunResult>>({});
  /** The last file sync's trouble, in words. */
  syncProblem = $state<string | null>(null);
  private lastMs = 0;
  private shared: Record<string, string> = {};
  private syncTimer: ReturnType<typeof setTimeout> | null = null;
  private clock: ReturnType<typeof setInterval> | null = null;
  /** Off where a page can vanish mid-run, as the extension's popup does. */
  autoSync = true;
  /** The sync under way, so a timer, a focus and a button never run two at once. */
  private syncing: Promise<void> | null = null;
  private lastSyncMs = 0;
  /** The vault last seen open, to notice an unlock. */
  private seenHandle = 0;
  /** Per place, the other devices' copies as last merged, by name: size and time. */
  private merged: Record<string, Record<string, string>> = {};
  /** Per place, a hash of this device's copy as last written there. */
  private written: Record<string, string> = {};

  get available() {
    return !!host().places;
  }

  /** A token, session, host key or certificate that changed during a run, kept with its place. */
  private async absorbUpdate(id: string, extra: Record<string, unknown>) {
    const place = this.list.find((p) => p.id === id);
    if (!place) return;
    await this.put({ ...place, extra: { ...(place.extra ?? {}), ...extra } });
  }

  async load() {
    if (!host().places) return;
    host().places!.onUpdate = (id, extra) => void this.absorbUpdate(id, extra);
    try {
      const text = await host().loadSealed?.('places');
      this.list = text ? JSON.parse(text) : [];
    } catch {
      this.list = [];
    }
    const raw = await host().loadSettings(STATE);
    if (raw) {
      try {
        const saved: Saved = JSON.parse(raw);
        this.everyDays = saved.every_days ?? 0;
        this.keep = saved.keep ?? 7;
        this.lastMs = saved.last_ms ?? 0;
        this.runs = saved.runs ?? {};
        this.shared = saved.shared ?? {};
      } catch {
        // The schedule starts again.
      }
    }
    this.startClock();
    pass.afterSave = () => this.syncSoon();
    if (this.autoSync) this.startAutoSync();
  }

  /**
   * Other devices' changes come in without an edit here: just after an unlock, when the window
   * comes back after a minute away, and every five minutes while the vault is open.
   */
  private startAutoSync() {
    const due = (gapMs: number) => Date.now() - this.lastSyncMs >= gapMs;
    setInterval(() => {
      const opened = pass.state.kind === 'unlocked' ? pass.handle : 0;
      if (opened !== this.seenHandle) {
        this.seenHandle = opened;
        // A different vault: what was merged and written before says nothing about this one.
        this.merged = {};
        this.written = {};
        if (opened) void this.syncNow(true);
      } else if (opened && due(AUTO_SYNC_MS)) {
        void this.syncNow(true);
      }
    }, 5_000);
    const back = () => {
      if (document.visibilityState === 'visible' && pass.state.kind === 'unlocked' && due(60_000)) void this.syncNow(true);
    };
    document.addEventListener('visibilitychange', back);
    window.addEventListener('focus', back);
  }

  private persistState() {
    const saved: Saved = { every_days: this.everyDays, keep: this.keep, last_ms: this.lastMs, runs: this.runs, shared: this.shared };
    void host().saveSettings(STATE, JSON.stringify(saved));
  }

  private async persist() {
    await host().saveSealed?.('places', JSON.stringify(this.list));
  }

  async put(place: Place) {
    this.list = [...this.list.filter((p) => p.id !== place.id), place].sort((a, b) => byName(a.label, b.label));
    // Setting up a remote place is the owner's yes to reaching it.
    if (place.kind !== 'folder') prefs.set('network_allowed', true);
    await this.persist();
  }

  async remove(id: string) {
    this.list = this.list.filter((p) => p.id !== id);
    delete this.runs[id];
    this.persistState();
    await this.persist();
  }

  /** On a wipe. */
  async forget() {
    this.list = [];
    this.runs = {};
    this.shared = {};
    this.everyDays = 0;
    this.persistState();
    if (host().places) await this.persist();
  }

  setSchedule(days: number) {
    this.everyDays = days;
    this.persistState();
  }

  setKeep(count: number) {
    this.keep = count;
    this.persistState();
  }

  private guard(place: Place) {
    if (place.kind !== 'folder' && !online()) throw new Error('Network features are turned off.');
  }

  private note(id: string, problem: string | null) {
    this.runs[id] = { at_ms: Date.now(), problem };
    this.persistState();
  }

  /**
   * One backup to one place: write, read back, check, and only then remove older copies. A failed
   * run can leave one file too many, never one too few.
   */
  async backupTo(place: Place) {
    const shell = host().places!;
    try {
      this.guard(place);
      const payload = await pass.bundle();
      const name = `${PREFIX}${stamp()}${EXTENSION}`;
      await shell.write(place, name, payload);
      const listed = await shell.list(place);
      const stored = listed.find((f) => f.name === name);
      if (!stored) throw new Error('The copy could not be read back.');
      const back = await shell.read(place, stored);
      if (back.length !== payload.length || !back.every((b, i) => b === payload[i]) || !(await bundleLooksRight(back))) {
        throw new Error('The copy could not be read back.');
      }
      const ours = listed.filter((f) => f.name.startsWith(PREFIX) && f.name.endsWith(EXTENSION)).sort((a, b) => (a.name > b.name ? -1 : 1));
      for (const old of ours.slice(this.keep)) if (old.name !== name) await shell.remove(place, old);
      this.note(place.id, null);
      return null;
    } catch (failure) {
      const problem = placeProblem(failure);
      this.note(place.id, problem);
      return problem;
    }
  }

  /** The copies at a place, newest first. */
  async copies(place: Place): Promise<PlaceFile[]> {
    this.guard(place);
    const all = await host().places!.list(place);
    return all
      .filter((f) => f.name.startsWith(PREFIX) && f.name.endsWith(EXTENSION))
      .sort((a, b) => (a.name > b.name ? -1 : 1))
      .map((f) => ({ ...f, modified_ms: f.modified_ms || stampOf(f.name) }));
  }

  read(place: Place, file: PlaceFile) {
    return host().places!.read(place, file);
  }

  /** Scheduled copies run while the vault is locked too, since they only move ciphertext. */
  private startClock() {
    if (this.clock) clearInterval(this.clock);
    const check = () => {
      if (this.everyDays <= 0 || Date.now() - this.lastMs < this.everyDays * 86_400_000) return;
      this.lastMs = Date.now();
      this.persistState();
      for (const place of this.list.filter((p) => p.backup)) void this.backupTo(place);
    };
    this.clock = setInterval(check, 3_600_000);
    setTimeout(check, 60_000);
  }

  // ---- file sync ----

  /** A little after the last change, so a burst of edits makes one trip. */
  syncSoon() {
    if (!this.list.some((p) => p.sync) || !online()) return;
    if (this.syncTimer) clearTimeout(this.syncTimer);
    this.syncTimer = setTimeout(() => void this.syncNow(), 20_000);
  }

  /**
   * `auto` is a run nobody asked for: it passes over places that need the network while network
   * use is off, rather than reporting them.
   */
  async syncNow(auto = false): Promise<void> {
    if (this.syncing) {
      if (auto) return this.syncing;
      // A button pressed during a timed run gets a full run of its own after it.
      await this.syncing;
    }
    this.syncing = this.runSync(auto).finally(() => (this.syncing = null));
    return this.syncing;
  }

  private async runSync(auto: boolean) {
    // The desktop app's vault, seen through the extension, is synced by the app.
    if (pass.state.kind !== 'unlocked' || pass.account || pass.child || pass.desktop) return;
    const places = this.list.filter((p) => p.sync && (!auto || p.kind === 'folder' || online()));
    if (!places.length) return;
    this.lastSyncMs = Date.now();
    let failure: string | null = null;
    for (const place of places) {
      try {
        await this.syncOne(place);
        this.note(place.id, null);
      } catch (problem) {
        failure = placeProblem(problem);
        this.note(place.id, failure);
      }
    }
    this.syncProblem = failure;
  }

  /**
   * Each device leaves its own copy and merges the others', which cannot lose a value. A file
   * under another key fails to open and is left alone.
   */
  private async syncOne(place: Place) {
    this.guard(place);
    const shell = host().places!;
    const mine = (await pass.view('vault.sync_name')).name as string;
    const listing = await shell.list(place);
    // A copy unchanged since it was last merged has nothing new, so a timed run reads only what moved.
    const merged = (this.merged[place.id] ??= {});
    const listedAt = Date.now();
    // This device's own copy is merged too, unless it is just what this device wrote there: after
    // a restore from an older backup, or with two installs sharing a device number, it may hold
    // changes found nowhere else, and writing over it unread would lose them.
    for (const other of listing.filter((f) => f.name.endsWith(SYNC_EXTENSION))) {
      // Copies are padded to fixed sizes and some disks keep times to the second or two, so a copy
      // written again within that time can carry the same stamp: one that recent is read anyway.
      const recent = !other.modified_ms || listedAt - other.modified_ms < 5000;
      const stamp = recent ? '' : `${other.size}:${other.modified_ms}`;
      if (stamp && merged[other.name] === stamp) continue;
      try {
        const bytes = await shell.read(place, other);
        const own = other.name === `${mine}${SYNC_EXTENSION}`;
        if (!own || (await digest(bytes)) !== this.written[place.id]) await pass.mergeCopy(bytes);
      } catch {
        // Some other vault's file.
      }
      merged[other.name] = stamp;
    }
    await pass.flush();
    const group = ((await pass.view('vault.chaff_name')).name as string).slice(0, 16);
    // Attachments are sealed already. Each goes up once and stays, named under this vault.
    const blobPrefix = `${group}-`;
    const have = listing.filter((f) => f.name.startsWith(blobPrefix) && f.name.endsWith(BLOB_EXTENSION));
    const short = (name: string) => name.slice(blobPrefix.length, -BLOB_EXTENSION.length);
    const haveNames = new Set(have.map((f) => short(f.name)));
    const local = await pass.blobNames();
    for (const name of local.filter((n) => !haveNames.has(n))) {
      await shell.write(place, `${blobPrefix}${name}${BLOB_EXTENSION}`, await pass.readBlob(name));
    }
    for (const entry of have.filter((f) => !local.includes(short(f.name)))) {
      await pass.writeBlob(short(entry.name), await shell.read(place, entry));
    }
    // Unchanged since this device last wrote it there, there is nothing to send.
    const sealed = await pass.sealedFile();
    const hash = await digest(sealed);
    const there = listing.some((f) => f.name === `${mine}${SYNC_EXTENSION}`);
    if (there && this.written[place.id] === hash) return;
    await shell.write(place, `${mine}${SYNC_EXTENSION}`, sealed);
    this.written[place.id] = hash;
  }

  // ---- shared vaults ----

  placeOf(file: string) {
    return this.list.find((p) => p.id === this.shared[file]) ?? null;
  }

  setPlace(file: string, place: Place) {
    this.shared[file] = place.id;
    this.persistState();
  }

  private async readNamed(place: Place, name: string) {
    this.guard(place);
    const shell = host().places!;
    const entry = (await shell.list(place)).find((f) => f.name === name);
    if (!entry) throw new Error(`That file is not at ${place.label} yet.`);
    return shell.read(place, entry);
  }

  async createShared(place: Place, name: string, myName: string) {
    const reply = await pass.change('shared.create', { name, my_name: myName });
    const file = reply.json.file as string;
    const shell = host().places!;
    await shell.write(place, `${file}${SHARED_EXTENSION}`, reply.blobs[0]);
    await shell.write(place, `${file}${MANIFEST_EXTENSION}`, reply.blobs[1]);
    this.setPlace(file, place);
    await pass.flush();
    return file;
  }

  manifest(place: Place, file: string) {
    return this.readNamed(place, `${file}${MANIFEST_EXTENSION}`);
  }

  async addMember(place: Place, file: string, code: string) {
    const current = await this.manifest(place, file);
    const reply = await pass.engine('shared.add_member', { file, code }, { handle: pass.handle, blobs: [current] });
    await host().places!.write(place, `${file}${MANIFEST_EXTENSION}`, reply.blobs[0]);
    return reply.json.invite as string;
  }

  async removeMember(place: Place, file: string, key: string) {
    const reply = await pass.change('shared.remove_member', { file, key }, { blobs: [await this.manifest(place, file), await this.readNamed(place, `${file}${SHARED_EXTENSION}`)] });
    // The vault first: a manifest pointing at a key the file does not use yet would lock everyone
    // out if this stopped halfway.
    const shell = host().places!;
    await shell.write(place, `${file}${SHARED_EXTENSION}`, reply.blobs[1]);
    await shell.write(place, `${file}${MANIFEST_EXTENSION}`, reply.blobs[0]);
    await pass.flush();
  }

  /** Opens it through the normal screens. Saving merges with the copy at the place first. */
  async openShared(place: Place, file: string) {
    const manifest = await this.manifest(place, file);
    const sealed = await this.readNamed(place, `${file}${SHARED_EXTENSION}`);
    const reply = await pass.change('shared.open', { file }, { blobs: [manifest, sealed] });
    await pass.enterChild(
      file,
      reply,
      (bytes) => host().places!.write(place, `${file}${SHARED_EXTENSION}`, bytes),
      async () => pass.absorb(await this.readNamed(place, `${file}${SHARED_EXTENSION}`)),
    );
  }
}

export const places = new Places();
pass.onWipe.push(() => void places.forget());
