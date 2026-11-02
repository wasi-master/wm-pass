// Somewhere outside this device. Only the encrypted file goes there, so a server cannot read it.
// Backups, file sync and shared vaults all run here, so the order they do things in is one order.

import { bundleLooksRight } from '../engine/bundle';
import { host, type PlaceFile } from '../engine/host';
import { pass } from '../engine/pass.svelte';
import { prefs } from '../engine/prefs.svelte';
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

  get available() {
    return !!host().places;
  }

  async load() {
    if (!host().places) return;
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
  }

  private persistState() {
    const saved: Saved = { every_days: this.everyDays, keep: this.keep, last_ms: this.lastMs, runs: this.runs, shared: this.shared };
    void host().saveSettings(STATE, JSON.stringify(saved));
  }

  private async persist() {
    await host().saveSealed?.('places', JSON.stringify(this.list));
  }

  async put(place: Place) {
    this.list = [...this.list.filter((p) => p.id !== place.id), place].sort((a, b) => a.label.localeCompare(b.label));
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

  async syncNow() {
    if (pass.state.kind !== 'unlocked' || pass.account || pass.child) return;
    let failure: string | null = null;
    for (const place of this.list.filter((p) => p.sync)) {
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
    for (const other of listing.filter((f) => f.name.endsWith(SYNC_EXTENSION) && f.name !== `${mine}${SYNC_EXTENSION}`)) {
      try {
        await pass.mergeCopy(await shell.read(place, other));
      } catch {
        // Some other vault's file.
      }
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
    await shell.write(place, `${mine}${SYNC_EXTENSION}`, await pass.sealedFile());
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
