// The vaults on this device, for every screen. A port of android/core/.../Pass.kt, so a folder of
// vault files means the same thing on every platform.

import { host } from './host';
import { prefs } from './prefs.svelte';
import { bytesOf, consumeText, decodeReply, encodeRequest, EngineError, type Reply } from './frame';
import { isVaultFile, makeBundle, unzip } from './bundle';
import { isInternal, stopped, stoppedError } from './stopped.svelte';
import type {
  CompareView,
  Detail,
  FolderView,
  HealthView,
  KindSpec,
  LockState,
  MatchRow,
  NeededFactor,
  OtpRow,
  PresetView,
  SlotView,
  StrengthView,
  Summary,
  UnlockResult,
  VaultInfo,
  VaultNotices,
} from './types';

const VAULTS = 'vaults';
const LINKED = 'linked';
const BLOBS = 'blobs';
const DOOR = 'door';
/**
 * Copies a backup brought in at the door, held aside rather than written over this device's
 * vault: an older backup must not replace newer items. They merge in when the vault next opens.
 */
const HELD = 'restored';

/** Thrown out of a save that stopped because the file is now under another key. */
class KeyChanged extends Error {
  constructor() {
    super('The master password was changed elsewhere.');
  }
}

function randomHex(bytes: number) {
  const raw = crypto.getRandomValues(new Uint8Array(bytes));
  return Array.from(raw, (b) => b.toString(16).padStart(2, '0')).join('');
}

interface DoorState {
  device: number;
  fails: number;
  retry_at: number;
  seen: Record<string, number>;
}

class PassStore {
  state = $state<LockState>({ kind: 'none' });
  entries = $state.raw<Summary[]>([]);
  folders = $state.raw<FolderView[]>([]);
  /** Rises on every change, for screens that hold a detail view. */
  revision = $state(0);
  rollbackWarning = $state(false);
  /** What the open vault has to tell its owner: a clock that looks wrong, a password changed on two devices at once. */
  notices = $state<VaultNotices>({ clock_warning: null, password_conflict: null });
  /** Told when the notices are read: after opening (`opened`), and after every merge or change. */
  onNotices: ((notices: VaultNotices, opened: boolean) => void) | null = null;
  /** The account behind what is open, if it is not a vault in this folder. */
  account = $state<string | null>(null);
  /** The name of the nested or shared vault in use. */
  child = $state<string | null>(null);
  /** Whether what is open is the desktop app's vault, which the app saves. */
  desktop = $state(false);

  handle = 0;
  file: string | null = null;
  private door: DoorState = { device: 0, fails: 0, retry_at: 0, seen: {} };
  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  /** Sites off the list asked this run, by domain: the page they named, or null for none. */
  private changeAsked = new Map<string, string | null>();
  private writing: Promise<unknown> = Promise.resolve();
  private accountWriter: ((bytes: Uint8Array) => Promise<void>) | null = null;
  private childWriter: ((bytes: Uint8Array) => Promise<void>) | null = null;
  private childBefore: (() => Promise<void>) | null = null;
  private outer: { handle: number; file: string | null; state: LockState } | null = null;
  /** Called after an account has saved, to take its changes to a server if it has one. */
  afterAccountSave: (() => Promise<void>) | null = null;
  /** Called after the vault in this folder has saved, for a sync that follows it. */
  afterSave: (() => void) | null = null;
  /** Also called after the vault in this folder has saved, for more than one listener. */
  onSaved: (() => void)[] = [];
  /** Told when a save fails, so the person hears of it rather than losing the change unseen. */
  onSaveError: ((message: string) => void) | null = null;
  /** Called after the lists reload, which follows every change and every merge. */
  afterRefresh: (() => void) | null = null;
  /** Run on a wipe, so what the app keeps beside the vault goes too. */
  onWipe: (() => void)[] = [];
  /** How long to wait after a change before writing. The extension's popup can vanish at any moment. */
  saveDelayMs = 350;
  /** The vault file as last read or written, by its hash, to notice another program writing it. */
  private known: string | null = null;
  private watch: ReturnType<typeof setInterval> | null = null;
  private unlisten: (() => void) | null = null;

  get device() {
    return this.door.device;
  }

  async init() {
    const raw = await host().loadSettings(DOOR);
    if (raw) {
      try {
        this.door = { ...this.door, ...JSON.parse(raw) };
      } catch {
        // A damaged file costs the throttle's count, nothing else.
      }
    }
    if (!this.door.device) {
      this.door.device = crypto.getRandomValues(new Uint32Array(1))[0] || 1;
      await this.writeDoor();
    }
    await this.refreshDoorState();
    const open = await host().attach?.();
    if (open) {
      this.adopt(open.handle, open.file, open.name, open.read_only);
      await this.refresh();
    }
  }

  /**
   * `keepOthers`: another tab of the web vault may have noted a newer save since this one read
   * the door, and the higher count stays, or a rollback to an older copy would pass unnoticed.
   */
  private async writeDoor(keepOthers = true) {
    if (keepOthers) {
      const kept = await this.storedDoor();
      for (const [name, generation] of Object.entries(kept?.seen ?? {})) {
        if (typeof generation === 'number' && generation > (this.door.seen[name] ?? 0)) this.door.seen[name] = generation;
      }
    }
    await host().saveSettings(DOOR, JSON.stringify(this.door));
  }

  private async storedDoor(): Promise<Partial<DoorState> | null> {
    try {
      const raw = await host().loadSettings(DOOR);
      return raw ? JSON.parse(raw) : null;
    } catch {
      return null;
    }
  }

  private async vaultFiles() {
    const files = await host().listFiles(VAULTS);
    return files.filter((f) => !f.name.startsWith('.')).sort((a, b) => (a.name < b.name ? -1 : 1));
  }

  private async refreshDoorState() {
    this.state = (await this.vaultFiles()).length === 0 ? { kind: 'none' } : { kind: 'locked' };
  }

  // ---- calls ----

  async engine<T = any>(
    method: string,
    args?: unknown,
    options: { handle?: number; secret?: Uint8Array | null; blobs?: Uint8Array[]; online?: boolean } = {},
  ): Promise<Reply<T>> {
    // Nothing reaches the session while the screens say it stopped: each call would fail the same
    // way, and say so in a message of its own.
    if (stopped.on) throw stoppedError();
    const frame = encodeRequest(
      { method, handle: options.handle ?? 0, args: args ?? {}, now_ms: Date.now(), online: options.online ?? false },
      options.secret,
      options.blobs ?? [],
    );
    // Wiped whether or not the call goes through: it may carry a password.
    try {
      return decodeReply<T>(await host().call(frame));
    } catch (failure) {
      if (isInternal(failure)) {
        this.internalError();
        throw stoppedError();
      }
      throw failure;
    } finally {
      frame.fill(0);
    }
  }

  async view<T = any>(method: string, args?: unknown, options: { secret?: Uint8Array | null; blobs?: Uint8Array[] } = {}): Promise<T> {
    return (await this.engine<T>(method, args, { handle: this.handle, ...options })).json;
  }

  /** A call on the open vault that may use the network, if the person has allowed that. */
  async net<T = any>(method: string, online: boolean, args?: unknown, secret?: Uint8Array | null): Promise<Reply<T>> {
    return this.engine<T>(method, args, { handle: this.handle, secret, online });
  }

  /** Any call that changes the vault. Refreshes the lists and schedules a save. */
  async change<T = any>(method: string, args?: unknown, options: { secret?: Uint8Array | null; blobs?: Uint8Array[] } = {}): Promise<Reply<T>> {
    const reply = await this.engine<T>(method, args, { handle: this.handle, ...options });
    const gone = (reply.json as any)?.delete_blobs;
    if (Array.isArray(gone)) for (const name of gone) await this.deleteBlob(name);
    if (this.batching) this.refreshOwed = true;
    else await this.refresh();
    this.scheduleSave();
    return reply;
  }

  private batching = 0;
  private refreshOwed = false;

  /**
   * Runs several changes with one reload of the lists at the end, not one after each: saving an
   * item with fifty fields would otherwise list the whole vault fifty times over.
   */
  async batch<T>(run: () => Promise<T>): Promise<T> {
    this.batching += 1;
    try {
      return await run();
    } finally {
      this.batching -= 1;
      if (this.batching === 0 && this.refreshOwed) {
        this.refreshOwed = false;
        await this.refresh().catch(() => undefined);
      }
    }
  }

  private kdfMemoryCache = 0;

  /** Memory for the password hash, by what this machine can spare. */
  private async kdfMemoryKib() {
    if (!this.kdfMemoryCache) {
      const gib = await host().memoryGib();
      this.kdfMemoryCache = host().kind === 'desktop' ? (gib >= 6 ? 256 : gib >= 4 ? 128 : 64) * 1024 : 64 * 1024;
    }
    return this.kdfMemoryCache;
  }

  // ---- the door ----

  /** Makes the first vault, with chaff files beside it so the folder gives nothing away. */
  async create(name: string, password: Uint8Array) {
    const memory = await this.kdfMemoryKib();
    const reply = await this.engine('vault.create', { name, kdf: { m_kib: memory } }, { secret: password });
    this.adopt(reply.json.handle, `${VAULTS}/${randomHex(16)}`, name, false);
    try {
      await this.saveNow();
    } catch (failure) {
      // Written, the vault is kept: what failed after it (the decoys, the door's notes) can wait for
      // the next save. Never written, it must not stay open as if it were kept, since its items
      // would be lost on quitting: the door comes back with the reason.
      const kept = await host()
        .readFile(this.file!)
        .then((bytes) => bytes.length > 0, () => false);
      if (!kept) {
        this.stopWatching();
        await this.engine('vault.lock', {}, { handle: reply.json.handle }).catch(() => undefined);
        this.handle = 0;
        this.file = null;
        await this.refreshDoorState().catch(() => (this.state = { kind: 'none' }));
        host().stateChanged?.({ unlocked: false, name: '' });
        throw failure;
      }
    }
    // One decoy that never changes, so a hidden vault's unchanging file is nothing unusual. Its
    // partner is made by saveNow and keeps pace with this vault.
    const length = (await host().readFile(this.file!)).length;
    await this.writeChaff(`${VAULTS}/${randomHex(16)}`, length);
    await this.refresh();
  }

  /** Adds a vault with its own password. The door shows no sign of it. */
  async createHidden(name: string, password: Uint8Array) {
    const reply = await this.engine('vault.create', { name, same_kdf_as: this.handle }, { secret: password });
    const hiddenHandle = reply.json.handle;
    const saved = await this.engine('vault.save', {}, { handle: hiddenHandle });
    const path = `${VAULTS}/${randomHex(16)}`;
    await host().writeFile(path, saved.blobs[0]);
    await this.engine('vault.lock', {}, { handle: hiddenHandle });
    await host().coverFile(path);
  }

  private async writeChaff(path: string, fileLength: number) {
    const reply = await this.engine('vault.chaff', { file_len: fileLength, same_kdf_as: this.handle }, { handle: this.handle });
    await host().writeFile(path, reply.blobs[0]);
  }

  /** Keeps the folder from saying which file is the vault. */
  private async coverTracks(savedLength: number) {
    const { name } = (await this.engine('vault.chaff_name', {}, { handle: this.handle })).json;
    const files = await this.vaultFiles();
    const decoy = files.find((f) => f.name === name);
    if (decoy?.size !== savedLength) await this.writeChaff(`${VAULTS}/${name}`, savedLength);
    for (const f of await this.vaultFiles()) await host().coverFile(`${VAULTS}/${f.name}`);
  }

  private adopt(handle: number, file: string, name: string, readOnly: boolean) {
    this.handle = handle;
    this.file = file;
    this.known = null;
    this.keyLost = false;
    this.savedChanges = this.changes;
    this.state = { kind: 'unlocked', name, file, readOnly };
    host().stateChanged?.({ unlocked: true, name, handle, file, read_only: readOnly });
    this.watchFile();
  }

  // ---- other programs writing the same file ----

  private async hashOf(bytes: Uint8Array) {
    const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes as BufferSource));
    return Array.from(digest, (b) => b.toString(16).padStart(2, '0')).join('');
  }

  /**
   * On a computer, the terminal app and `wmpass` open this same folder. Every file here keeps
   * one time on purpose, so a change shows only in what the file holds: it is read every few
   * seconds and merged in when it differs from what this app last read or wrote.
   */
  private watchFile() {
    this.stopWatching();
    // A second tab of the web vault on the same file says when it writes, and its change comes in
    // here at once, rather than this tab showing what it had and its next save being the first to see it.
    const listen = host().onFileChanged;
    if (listen) {
      this.unlisten = listen((path) => {
        if (path === this.file) this.writing = this.writing.then(() => this.takeOutsideChanges()).catch(() => undefined);
      });
    }
    if (host().kind !== 'desktop') return;
    this.watch = setInterval(() => {
      this.writing = this.writing.then(() => this.takeOutsideChanges()).catch(() => undefined);
    }, 3000);
  }

  private stopWatching() {
    if (this.watch) clearInterval(this.watch);
    this.watch = null;
    this.unlisten?.();
    this.unlisten = null;
  }

  /** Merges in the file if another program wrote it since. Returns whether anything came in. */
  private async takeOutsideChanges() {
    const file = this.file;
    if (!this.handle || !file || this.childWriter || this.accountWriter) return false;
    let bytes: Uint8Array;
    try {
      bytes = await host().readFile(file);
    } catch {
      return false;
    }
    const hash = await this.hashOf(bytes);
    if (this.known === null) {
      this.known = hash;
      return false;
    }
    if (hash === this.known || file !== this.file) return false;
    const name = file.split('/').pop()!;
    try {
      await this.engine('vault.merge', { seen_generation: this.door.seen[name] ?? 0 }, { handle: this.handle, blobs: [bytes] });
    } catch (failure) {
      // The file is there and whole, but not under this vault's key: another window or program
      // changed the master password and rekeyed it.
      if (failure instanceof EngineError && failure.code === 'integrity' && file === this.file) {
        this.keyChangedElsewhere();
        throw new KeyChanged();
      }
      throw failure;
    }
    this.known = hash;
    await this.refresh();
    return true;
  }

  /** Set once the file on disk is under a key this window no longer has: nothing more is written. */
  private keyLost = false;
  /** Changes asked to be saved, and how many of those are on disk, to tell whether any would be lost. */
  private changes = 0;
  private savedChanges = 0;
  /**
   * Told when this window is about to lock because the vault's key changed elsewhere. `lost` when
   * changes made here since its last save cannot be kept.
   */
  onKeyChanged: ((lost: boolean) => void) | null = null;

  /**
   * Another window rekeyed the vault. Saving now would write this window's copy, under the old
   * key, over the new file, and take the vault back to the old password. Nothing more is saved
   * and this window locks, for the person to open it again with the new password.
   *
   * What changed here since the last save cannot be merged into the new file, which only the new
   * key opens, and is not kept aside either: a copy under the old key would open with the old
   * password, which a rekey is meant to retire. The person is told it was lost.
   */
  private keyChangedElsewhere() {
    if (this.keyLost) return;
    this.keyLost = true;
    this.stopWatching();
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = null;
    // Told before the lock, while the screens still say what was open, such as an edit under way.
    this.onKeyChanged?.(this.changes > this.savedChanges);
    // Not awaited: this runs inside the queue of writes, which the lock waits on.
    void Promise.resolve()
      .then(() => this.lock())
      .catch(() => undefined);
  }

  /** When typing a password next becomes allowed, or 0. */
  retryAt() {
    return this.door.retry_at > Date.now() ? this.door.retry_at : 0;
  }

  private async noteFailure() {
    // Counted with the other tabs' tries, so opening more tabs is no way around the wait.
    const kept = await this.storedDoor();
    this.door.fails = Math.max(this.door.fails, kept?.fails ?? 0);
    this.door.fails += 1;
    const fails = this.door.fails;
    // Five free tries, then thirty seconds doubling up to fifteen minutes.
    const wait = fails <= 5 ? 0 : Math.min(30_000 * 2 ** Math.min(fails - 6, 5), 900_000);
    this.door.retry_at = wait === 0 ? 0 : Date.now() + wait;
    await this.writeDoor();
    return this.door.retry_at;
  }

  private async noteSuccess() {
    this.door.fails = 0;
    this.door.retry_at = 0;
    await this.writeDoor();
  }

  async unlock(password: Uint8Array, factors: { kind: string; bytes: Uint8Array }[] = []): Promise<UnlockResult> {
    const wait = this.retryAt();
    if (wait) return { kind: 'waiting', retryAtMs: wait };
    const files = await this.vaultFiles();
    const blobs = [...factors.map((f) => f.bytes)];
    for (const f of files) blobs.push(await host().readFile(`${VAULTS}/${f.name}`));
    const reply = await this.engine('vault.open', { factors: factors.map((f) => f.kind) }, { secret: password, blobs });
    const obj = reply.json;
    switch (obj.outcome) {
      case 'opened': {
        await this.noteSuccess();
        const target = `${VAULTS}/${files[obj.index].name}`;
        this.adopt(obj.handle, target, obj.name, obj.read_only === true);
        this.known = await this.hashOf(blobs[factors.length + obj.index]);
        await this.afterOpen(files[obj.index].name, obj.generation);
        return { kind: 'opened' };
      }
      case 'duress':
        if (obj.action === 'wipe') {
          await this.wipe();
          // Indistinguishable from a mistyped password.
          return { kind: 'wrong', retryAtMs: 0 };
        }
        this.door.retry_at = Date.now() + 3_600_000;
        await this.writeDoor();
        return { kind: 'hard_locked' };
      case 'needs_factor':
        return { kind: 'needs_factor', factors: obj.factors as NeededFactor[] };
      default:
        if (await this.openHeld(password, factors)) return { kind: 'opened' };
        return { kind: 'wrong', retryAtMs: await this.noteFailure() };
    }
  }

  /**
   * A password that opens nothing here may open a restored copy: the vault file here is damaged,
   * or its master password changed since the backup. That copy becomes a vault of its own, under
   * a new name, so the file already here stays as it is.
   */
  private async openHeld(password: Uint8Array, factors: { kind: string; bytes: Uint8Array }[]) {
    const held = await this.heldFiles(VAULTS);
    if (!held.length) return false;
    const blobs = [...factors.map((f) => f.bytes)];
    for (const f of held) blobs.push(await host().readFile(`${HELD}/${VAULTS}/${f.name}`));
    let obj: any;
    try {
      obj = (await this.engine('vault.open', { factors: factors.map((f) => f.kind) }, { secret: password, blobs })).json;
    } catch {
      return false;
    }
    if (obj?.outcome !== 'opened') return false;
    const chosen = held[obj.index].name;
    const bytes = blobs[factors.length + obj.index];
    const taken = new Set((await this.vaultFiles()).map((f) => f.name));
    const name = taken.has(chosen) ? randomHex(16) : chosen;
    await host().writeFile(`${VAULTS}/${name}`, bytes);
    await host().removeFile(`${HELD}/${VAULTS}/${chosen}`);
    await this.noteSuccess();
    this.adopt(obj.handle, `${VAULTS}/${name}`, obj.name, obj.read_only === true);
    this.known = await this.hashOf(bytes);
    await this.afterOpen(name, obj.generation);
    return true;
  }

  private async heldFiles(folder: string) {
    return (await host().listFiles(`${HELD}/${folder}`).catch(() => [])).filter((f) => !f.name.startsWith('.') && f.size > 0);
  }

  /**
   * Merges the restored copies of the vault just opened into it. Nothing is lost: a value that
   * loses moves to history. A copy that is no part of this vault keeps waiting.
   */
  private async absorbHeld() {
    const held = await this.heldFiles(VAULTS);
    if (!held.length) return;
    // The backup's copy of this vault's decoy holds nothing to merge.
    const chaff = (await this.view('vault.chaff_name').catch(() => null))?.name;
    let merged = 0;
    for (const f of held) {
      const path = `${HELD}/${VAULTS}/${f.name}`;
      if (f.name === chaff) {
        await host().removeFile(path);
        continue;
      }
      const bytes = await host().readFile(path);
      try {
        await this.engine('vault.merge', {}, { handle: this.handle, blobs: [bytes] });
        merged++;
        await host().removeFile(path);
      } catch {
        // Another vault's copy, or a decoy. Put in place now, an older copy of a vault here could
        // open before the newer one with the same password: it waits until its own vault opens,
        // or until a password opens nothing here but it.
      }
    }
    if (merged > 0) this.scheduleSave();
  }

  /** The same for a nested or shared vault, when it opens. */
  private async absorbHeldLinked(id: string) {
    const path = `${HELD}/${LINKED}/${id}`;
    if (!(await this.heldFiles(LINKED)).some((f) => f.name === id)) return;
    try {
      await this.engine('vault.merge', {}, { handle: this.handle, blobs: [await host().readFile(path)] });
      await host().removeFile(path);
      this.scheduleSave();
    } catch {
      // Kept for the next time this vault opens.
    }
  }

  /** Opens with a key the system's keyring or a passkey holds. The key never reaches this page. */
  async unlockQuick() {
    const quick = host().quick;
    if (!quick) return false;
    for (const f of await this.vaultFiles()) {
      if (!(await quick.enrolled(f.name))) continue;
      const path = `${VAULTS}/${f.name}`;
      const bytes = await host().readFile(path);
      const obj = decodeReply(await quick.open(f.name, bytes)).json;
      if (obj?.error) throw new EngineError(obj.error.code, obj.error.message);
      await this.noteSuccess();
      this.adopt(obj.handle, path, obj.name, obj.read_only === true);
      this.known = await this.hashOf(bytes);
      await this.afterOpen(f.name, obj.generation);
      return true;
    }
    return false;
  }

  /** Whether quick unlock could work for a file on this device. */
  async quickFiles() {
    const quick = host().quick;
    if (!quick || !(await quick.available())) return [];
    const out: string[] = [];
    for (const f of await this.vaultFiles()) if (await quick.enrolled(f.name)) out.push(f.name);
    return out;
  }

  private async afterOpen(name: string, generation: number) {
    const seen = this.door.seen[name] ?? 0;
    this.rollbackWarning = generation < seen;
    if (generation > seen) {
      this.door.seen[name] = generation;
      await this.writeDoor();
    }
    // A restored copy that cannot be merged now is tried again the next time.
    await this.absorbHeld().catch(() => undefined);
    // This device's name, for a warning on another device about its clock.
    if (this.state.kind === 'unlocked' && !this.state.readOnly) {
      const named = await this.engine<{ changed: boolean }>('vault.device_name', { name: await this.deviceName() }, { handle: this.handle }).catch(() => null);
      if (named?.json.changed) this.scheduleSave();
    }
    const swept = (await this.engine('vault.sweep', {}, { handle: this.handle })).json.delete_blobs ?? [];
    for (const blob of swept) await this.deleteBlob(blob);
    if (swept.length) this.scheduleSave();
    this.justOpened = true;
    await this.refresh();
  }

  private locking: Promise<void> | null = null;

  /** Saves anything pending and forgets every key. */
  lock() {
    // The idle timer, a hidden window and the menu can all ask at once: one lock serves them all.
    this.locking ??= this.lockOnce().finally(() => (this.locking = null));
    return this.locking;
  }

  private async lockOnce() {
    // The session is gone already, and the screens stay under the message until the person
    // leaves it: a timer or a hidden window must not take what they typed away first.
    if (stopped.on) return;
    if (this.desktop) {
      this.leaveDesktop();
      return;
    }
    this.stopWatching();
    // A nested vault that cannot be left cleanly must not keep every key open: lockAll still runs.
    await this.leaveChild().catch(() => undefined);
    // The desktop app's own timer locks the engine before telling this page: with no change
    // waiting there is nothing to save, and trying would only report the vault as locked.
    const engineClosed = (await host().desktop?.currentHandle().catch(() => -1)) === 0;
    if (!engineClosed || this.saveTimer) await this.flush();
    // The screens leave the vault even when the shell answers with an error: the door shows rather
    // than items whose keys may be gone, and the failure still reaches the caller.
    let failure: unknown = null;
    try {
      await host().lockAll();
    } catch (error) {
      failure = error;
    }
    this.forgetOpen();
    await this.refreshDoorState().catch(() => (this.state = { kind: 'locked' }));
    host().stateChanged?.({ unlocked: false, name: '' });
    if (failure) throw failure;
  }

  /** Drops everything about the vault that was open, for a lock. */
  private forgetOpen() {
    this.handle = 0;
    this.file = null;
    this.keyLost = false;
    this.outer = null;
    this.childWriter = null;
    this.childBefore = null;
    this.child = null;
    this.accountWriter = null;
    this.afterAccountSave = null;
    this.account = null;
    this.rollbackWarning = false;
    this.notices = { clock_warning: null, password_conflict: null };
    // The sites asked about are the vault's own sites: they go with it.
    this.changeAsked.clear();
    this.entries = [];
    this.folders = [];
  }

  // ---- an internal error ----

  /**
   * A bug in the session stopped a call, from this page or, on the desktop, from anything else
   * that uses the session. The session is gone already, keys and all. Nothing is saved, as what it
   * held may be half changed, and nothing more reaches it. The screens stay as they are, with
   * anything typed there, under a message ([`stopped`]), until the person goes to the door with
   * [`leaveInternal`]. The shell's menus, tray and quick search hear of the lock now.
   */
  internalError() {
    if (stopped.on) return;
    stopped.on = true;
    this.stopWatching();
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = null;
    host().internalError?.();
    // What the shell keeps beside the session, such as the extension's note of what is open.
    void host()
      .lockAll()
      .catch(() => undefined);
    host().link?.detachVault();
    host().stateChanged?.({ unlocked: false, name: '' });
  }

  /** From the message about an internal error to the door. */
  async leaveInternal() {
    this.desktop = false;
    this.forgetOpen();
    this.locking = null;
    // A browser has to load the session's module again, which this page's reload does.
    if (host().restart) {
      await host().restart!();
      return;
    }
    stopped.on = false;
    await this.refreshDoorState().catch(() => (this.state = { kind: 'locked' }));
  }

  /** A panic: locked at once, and every password refused for an hour. */
  async panic() {
    // A panic locks the desktop app's vault too: a browser showing it is no reason to keep it open.
    if (this.desktop) await host().link?.lockDesktop().catch(() => undefined);
    // The wait first, so the door that opens on locking already shows it.
    this.door.retry_at = Date.now() + 3_600_000;
    await this.writeDoor();
    await this.lock();
  }

  async wipe() {
    this.stopWatching();
    // A save still waiting would fail on the keys just forgotten, and say so on screen.
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = null;
    await host().lockAll();
    await host().removeFolder(VAULTS);
    await host().removeFolder(BLOBS);
    await host().removeFolder(LINKED);
    this.door = { device: this.door.device, fails: 0, retry_at: 0, seen: {} };
    await this.writeDoor(false);
    for (const run of this.onWipe) {
      try {
        run();
      } catch {
        // Everything else still has to go.
      }
    }
    this.handle = 0;
    this.file = null;
    this.outer = null;
    this.childWriter = null;
    this.childBefore = null;
    this.accountWriter = null;
    this.afterAccountSave = null;
    this.child = null;
    this.account = null;
    this.rollbackWarning = false;
    this.notices = { clock_warning: null, password_conflict: null };
    this.changeAsked.clear();
    this.entries = [];
    this.folders = [];
    this.state = { kind: 'none' };
    host().stateChanged?.({ unlocked: false, name: '' });
  }

  // ---- saving ----

  private scheduleSave() {
    this.changes += 1;
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => void this.queueSave(), this.saveDelayMs);
  }

  private queueSave() {
    this.writing = this.writing
      .then(() => this.saveNow())
      // A key changed elsewhere is told once, by onKeyChanged, not as a failed save.
      .catch((failure: any) => {
        if (!(failure instanceof KeyChanged)) this.onSaveError?.(failure?.message ?? String(failure));
      });
    return this.writing;
  }

  private async saveNow() {
    const active = this.handle;
    if (!active) return;
    if (this.childWriter) {
      if (this.childBefore) await this.childBefore().catch(() => undefined);
      await this.childWriter((await this.engine('vault.save', {}, { handle: active })).blobs[0]);
      await this.refresh();
      return;
    }
    if (this.accountWriter) {
      const reply = await this.engine('remote.save', {}, { handle: active });
      await this.accountWriter(reply.blobs[0]);
      await this.afterAccountSave?.();
      return;
    }
    if (!this.file || this.keyLost) return;
    const file = this.file;
    const save = async () => {
      // Whatever another program wrote since goes in first, so this write keeps it. A file under
      // another key stops the save here, before anything is written over it.
      await this.takeOutsideChanges().catch((failure) => {
        if (failure instanceof KeyChanged) throw failure;
        return false;
      });
      if (this.keyLost || file !== this.file) return;
      const upTo = this.changes;
      const reply = await this.engine('vault.save', {}, { handle: active });
      await host().writeFile(file, reply.blobs[0]);
      this.savedChanges = Math.max(this.savedChanges, upTo);
      this.known = await this.hashOf(reply.blobs[0]);
      this.door.seen[file.split('/').pop()!] = reply.json.generation;
      await this.writeDoor();
      await this.coverTracks(reply.blobs[0].length);
    };
    // Two tabs of the web vault saving at once could each read the file before the other wrote
    // it, and the second write would drop the first's change: they take turns.
    const locks = (globalThis.navigator as any)?.locks;
    if (host().onFileChanged && locks?.request) await locks.request(`wmpass-save:${file}`, save);
    else await save();
    this.afterSave?.();
    for (const run of this.onSaved) run();
  }

  /** Saves soon, after a change that needs no lists reloaded. */
  flushSoon() {
    this.scheduleSave();
  }

  /** Writes pending changes now, before the window or process may go away. */
  async flush() {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = null;
    await this.queueSave();
  }

  private async refresh() {
    const handle = this.handle;
    if (!handle) return;
    const entries = (await this.engine<Summary[]>('entry.list', { sort: 'title' }, { handle })).json;
    const folders = (await this.engine<FolderView[]>('folder.list', {}, { handle })).json;
    // A lock or a switch to another vault while these were asked: their lists are not for it.
    if (this.handle !== handle) return;
    this.entries = entries;
    this.folders = folders;
    this.revision += 1;
    this.afterRefresh?.();
    await this.readNotices();
  }

  /** Set between opening and the first read of the notices, which then count as said on opening. */
  private justOpened = false;

  /** Reads what the vault has to say, and passes it on. A vault that cannot say keeps quiet. */
  private async readNotices() {
    const handle = this.handle;
    const opened = this.justOpened;
    this.justOpened = false;
    if (!handle) return;
    try {
      const notices = (await this.engine<VaultNotices>('vault.notices', {}, { handle })).json;
      if (this.handle !== handle) return;
      this.notices = { clock_warning: notices.clock_warning ?? null, password_conflict: notices.password_conflict ?? null };
      this.onNotices?.(this.notices, opened);
    } catch {
      // An older desktop app lending its vault does not know the call.
    }
  }

  /** What this device is called in another device's warning about its clock. */
  private async deviceName(): Promise<string> {
    try {
      const name = await host().lan?.deviceName();
      if (name) return name;
    } catch {
      // Falls back to a plain description.
    }
    const os = { mac: 'Mac', windows: 'Windows', linux: 'Linux', other: '' }[host().os];
    if (host().kind === 'desktop') return os ? `${os} computer` : 'Computer';
    return os ? `Browser on ${os}` : 'Browser';
  }

  /** Marks a lost change of password, which the owner has now read about, as told on every device. */
  async passwordLossSeen() {
    await this.change('slot.conflict_seen');
  }

  /** Reloads the lists, after something changed the vault from outside these calls. */
  reload() {
    return this.refresh();
  }

  // ---- reading ----

  search(query: string, sort = 'title') {
    return this.view<Summary[]>('entry.list', { query, sort });
  }
  detail(id: string) {
    return this.view<Detail>('entry.get', { id });
  }
  trashed() {
    return this.search('trashed', 'modified');
  }
  codes(ids?: string[]) {
    return this.view<OtpRow[]>('otp.codes', ids ? { ids } : {});
  }
  health(breached: Record<string, number> = {}, ignored: Record<string, string[]> = {}, skipLocal = false) {
    return this.view<HealthView>('health.report', { breached, ignored, skip_local: skipLocal });
  }
  /**
   * Where a saved address lets you change the password, and only for a site that has such a page:
   * the core's list at once, and for any other website, when the network may be used, the site's
   * own answer to whether its `/.well-known/change-password` address leads anywhere. Null for no
   * page, an app or a local address.
   */
  async changePasswordUrl(site: string | null | undefined): Promise<string | null> {
    if (!site) return null;
    const { url, ask } = await this.view<{ url: string | null; ask: string | null }>('site.change_password', { site });
    if (url || !ask) return url;
    const kept = this.changeAsked.get(ask);
    if (kept !== undefined) return kept;
    if (!prefs.v.network_allowed) return null;
    try {
      const found = (await this.net<{ url: string | null }>('site.probe_change_password', true, { host: ask })).json.url ?? null;
      this.changeAsked.set(ask, found);
      return found;
    } catch {
      // No answer, such as no network: asked again next time.
      return null;
    }
  }
  presets() {
    return this.view<PresetView[]>('gen.presets');
  }
  kinds() {
    return this.view<KindSpec[]>('kinds');
  }
  info() {
    return this.view<VaultInfo>('vault.info');
  }
  slots() {
    return this.view<SlotView[]>('slot.list');
  }
  matchWeb(url: string) {
    return this.view<MatchRow[]>('match.web', { url });
  }
  strength(password: Uint8Array, context: string[] = []) {
    return this.view<StrengthView>('strength.assess', { context }, { secret: password });
  }
  compare(first: Uint8Array, second: Uint8Array) {
    return this.view<CompareView>('strength.compare', {}, { blobs: [first, second] });
  }

  /** A secret field as text, to show it on screen for a moment. */
  async reveal(method: string, args: unknown): Promise<string> {
    return consumeText((await this.engine(method, args, { handle: this.handle })).secret);
  }

  /**
   * A secret goes straight to the clipboard. On the desktop it never reaches this page at all.
   */
  async copySecret(method: string, args: unknown, clearAfterSeconds: number) {
    if (stopped.on) throw stoppedError();
    const frame = encodeRequest({ method, handle: this.handle, args, now_ms: Date.now() });
    try {
      decodeReply(await host().callToClipboard(frame, clearAfterSeconds));
    } catch (failure) {
      if (isInternal(failure)) {
        this.internalError();
        throw stoppedError();
      }
      throw failure;
    } finally {
      frame.fill(0);
    }
  }

  // ---- attachments ----

  private blobFolderOf(file: string) {
    return `${BLOBS}/${file.split('/').pop()}`;
  }

  /** A name for a vault file that is about to arrive, so its attachments can arrive first. */
  freshVaultName() {
    return randomHex(16);
  }

  /** Where the attachments of the vault file `name` live. */
  blobFolderFor(name: string) {
    return this.blobFolderOf(name);
  }

  blobFolder() {
    return this.file ? this.blobFolderOf(this.file) : null;
  }

  private async deleteBlob(name: string) {
    const folder = this.blobFolder();
    if (folder) await host().removeFile(`${folder}/${name}`);
  }

  private noAttachmentsHere() {
    if (this.desktop) throw new Error('Attachments stay in the desktop app. Add or open them there.');
  }

  async addAttachment(id: string, name: string, mime: string, plain: Uint8Array) {
    this.noAttachmentsHere();
    const reply = await this.change('attachment.add', { id, name, mime }, { blobs: [plain] });
    await host().writeFile(`${this.blobFolder()}/${reply.json.blob}`, reply.blobs[0]);
  }

  /** Adds the import just read, and writes the attachments it brought along. */
  async applyImport() {
    const reply = await this.change<{ added: number; duplicates: number; blob_names?: string[] }>('import.apply');
    const names = reply.json.blob_names ?? [];
    const folder = this.blobFolder();
    if (folder) for (const [i, name] of names.entries()) await host().writeFile(`${folder}/${name}`, reply.blobs[i]);
    return reply.json;
  }

  async openAttachment(id: string, blob: string): Promise<Uint8Array> {
    this.noAttachmentsHere();
    const sealed = await host().readFile(`${this.blobFolder()}/${blob}`);
    return (await this.engine('attachment.open', { id, blob }, { handle: this.handle, blobs: [sealed] })).secret!;
  }

  // ---- files coming and going ----

  /** The saved file, for a backup or a sync. It is ciphertext. */
  async sealedFile() {
    await this.flush();
    return host().readFile(this.file!);
  }

  async verify(password: Uint8Array, factors: { kind: string; bytes: Uint8Array }[] = []) {
    return (await this.view('vault.verify', { factors: factors.map((f) => f.kind) }, { secret: password, blobs: factors.map((f) => f.bytes) })).ok === true;
  }

  /**
   * Which slot this password opens, or null: a change then replaces that one. A slot that also
   * needs its key file or security key says which, and is asked again with the answer, as at the
   * door; the engine keeps that factor for the change that follows.
   */
  async passwordSlot(
    password: Uint8Array,
    factors: { kind: string; bytes: Uint8Array }[] = [],
  ): Promise<{ slot: number } | { needs: NeededFactor[] } | null> {
    const reply = await this.view('vault.verify', { factors: factors.map((f) => f.kind) }, { secret: password, blobs: factors.map((f) => f.bytes) });
    if (reply.ok === true) {
      if (typeof reply.slot === 'number') return { slot: reply.slot };
      // An older desktop app, lending its vault to the browser, does not say which: the first, as before.
      return { slot: (await this.slots())[0]?.index ?? 0 };
    }
    if (Array.isArray(reply.needs_factor) && reply.needs_factor.length > 0) return { needs: reply.needs_factor as NeededFactor[] };
    return null;
  }

  /**
   * A new vault key and a new password. Says whether quick unlock was turned off: what the
   * keyring or a passkey keeps is the old key, which opens the file no more, so it goes, as on
   * Android, to be turned on again.
   */
  async rekey(password: Uint8Array): Promise<{ quickOff: boolean }> {
    const before = (await this.view('vault.chaff_name')).name;
    await this.change('vault.rekey', {}, { secret: password });
    await this.flush();
    await host().removeFile(`${VAULTS}/${before}`);
    const quick = host().quick;
    const file = this.fileName;
    if (!quick || !file || this.account !== null || this.child !== null) return { quickOff: false };
    try {
      if (!(await quick.enrolled(file))) return { quickOff: false };
      await quick.forget(file);
      return { quickOff: true };
    } catch {
      // Left enrolled, it fails at the door and the password is asked for instead: nothing lost.
      return { quickOff: false };
    }
  }

  /** Takes a vault file from elsewhere as this device's vault. */
  async adoptFile(bytes: Uint8Array, name = randomHex(16)) {
    await host().writeFile(`${VAULTS}/${name}`, bytes);
    // A chaff file of its own, or a lone file would say which one is the vault.
    const chaff = await this.engine('vault.chaff', { file_len: bytes.length, kdf: { m_kib: await this.kdfMemoryKib() } });
    await host().writeFile(`${VAULTS}/${randomHex(16)}`, chaff.blobs[0]);
    for (const f of await this.vaultFiles()) await host().coverFile(`${VAULTS}/${f.name}`);
    if (!this.handle) this.state = { kind: 'locked' };
  }

  /**
   * Puts another copy of a vault in place of the one open, after a merge gave this one's items
   * to it. The old file and its decoy go, so one password cannot open the wrong vault, and the
   * door opens the new file with its own master password.
   */
  async replaceWith(bytes: Uint8Array, name: string) {
    const old = this.file;
    const chaff = (await this.view('vault.chaff_name')).name;
    await this.lock();
    await this.adoptFile(bytes, name);
    if (old && old !== `${VAULTS}/${name}`) await host().removeFile(old);
    await host().removeFile(`${VAULTS}/${chaff}`);
    await this.refreshDoorState();
  }

  /** Every vault file and attachment on this device as one file, for a backup. */
  async bundle() {
    if (this.handle) await this.flush();
    return makeBundle();
  }

  /**
   * Puts a vault file or a backup in place at the door. The password decides what opens. On a
   * device that has a vault already, the copies are held aside and merged into it when it next
   * opens, so an older backup cannot replace newer items. Says whether they were held.
   */
  async restore(bytes: Uint8Array): Promise<{ held: boolean }> {
    const hasVault = (await this.vaultFiles()).length > 0;
    if (isVaultFile(bytes)) {
      if (!hasVault) {
        await this.adoptFile(bytes);
        return { held: false };
      }
      await host().writeFile(`${HELD}/${VAULTS}/${randomHex(16)}`, bytes);
      return { held: true };
    }
    const files = await unzip(bytes);
    if (!files.some((f) => f.name.startsWith('vaults/'))) throw new EngineError('invalid', 'That is not a WM Pass backup.');
    let held = false;
    for (const f of files) {
      const [folder, name] = f.name.split('/');
      const here = await this.sameFile(f.name, f.bytes);
      if (here === 'same') continue;
      if (hasVault && folder === VAULTS) {
        await host().writeFile(`${HELD}/${VAULTS}/${name}`, f.bytes);
        held = true;
      } else if (here === 'differs' && folder === LINKED) {
        // A nested or shared vault here may be newer: the copy merges in when that vault opens.
        await host().writeFile(`${HELD}/${LINKED}/${name}`, f.bytes);
        held = true;
      } else if (here === 'missing') {
        // Attachments are named for what they hold, so one here already is the same.
        await host().writeFile(f.name, f.bytes);
      }
    }
    for (const f of await this.vaultFiles()) await host().coverFile(`${VAULTS}/${f.name}`);
    if (!this.handle) this.state = { kind: 'locked' };
    return { held };
  }

  private async sameFile(path: string, bytes: Uint8Array): Promise<'missing' | 'same' | 'differs'> {
    let here: Uint8Array;
    try {
      here = await host().readFile(path);
    } catch {
      return 'missing';
    }
    if (here.length !== bytes.length) return 'differs';
    for (let i = 0; i < here.length; i++) if (here[i] !== bytes[i]) return 'differs';
    return 'same';
  }

  /**
   * Merges a vault file or a backup into the open vault, and says how many of its files were
   * copies of this vault. Nothing is lost: a value that loses moves to history.
   */
  async mergeIn(bytes: Uint8Array) {
    const files = isVaultFile(bytes) ? [{ name: 'vaults/x', bytes }] : await unzip(bytes);
    // Any other file holds no vault at all, which is not the same as holding someone else's.
    if (!files.some((f) => f.name.startsWith('vaults/'))) throw new EngineError('invalid', 'That is not a WM Pass backup or vault file.');
    let merged = 0;
    for (const f of files.filter((f) => f.name.startsWith('vaults/'))) {
      try {
        await this.engine('vault.merge', {}, { handle: this.handle, blobs: [f.bytes] });
        merged++;
      } catch {
        // Under another key, so it holds some other vault.
      }
    }
    if (merged > 0) {
      const folder = this.blobFolder();
      if (folder) {
        const have = new Set((await host().listFiles(folder)).map((f) => f.name));
        for (const f of files.filter((f) => f.name.startsWith('blobs/'))) {
          const name = f.name.split('/').pop()!;
          if (!have.has(name)) await host().writeFile(`${folder}/${name}`, f.bytes);
        }
      }
      await this.refresh();
      await this.flush();
    }
    return merged;
  }

  /** Merges a copy in during a save, when the lists are about to be refreshed anyway. */
  async absorb(bytes: Uint8Array) {
    await this.engine('vault.merge', {}, { handle: this.handle, blobs: [bytes] });
  }

  async blobNames() {
    const folder = this.blobFolder();
    return folder ? (await host().listFiles(folder)).map((f) => f.name).filter((n) => !n.startsWith('.')) : [];
  }

  readBlob(name: string) {
    return host().readFile(`${this.blobFolder()}/${name}`);
  }

  writeBlob(name: string, bytes: Uint8Array) {
    if (name.includes('/') || name.startsWith('.')) throw new EngineError('invalid', 'bad name');
    return host().writeFile(`${this.blobFolder()}/${name}`, bytes);
  }

  /** The vault's name on disk, for keys that belong to one file. */
  get fileName() {
    return this.file?.split('/').pop() ?? null;
  }

  async readLinked(id: string) {
    return host().readFile(this.linkedPath(id));
  }

  async writeLinked(id: string, bytes: Uint8Array) {
    await host().writeFile(this.linkedPath(id), bytes);
  }

  /** Merges one saved copy of the open vault, as a sync brings it in. */
  async mergeCopy(bytes: Uint8Array) {
    const name = this.file?.split('/').pop() ?? '';
    const reply = await this.engine('vault.merge', { seen_generation: this.door.seen[name] ?? 0 }, { handle: this.handle, blobs: [bytes] });
    if (reply.json.rollback) this.rollbackWarning = true;
    await this.refresh();
  }

  /** Opens with emergency shares and a vault file, sets a new password and keeps the file. */
  async recover(file: Uint8Array, shares: string[], password: Uint8Array) {
    const reply = await this.engine('emergency.recover', { shares }, { blobs: [file] });
    // The new password first: a vault opened here with only its old owner's password would be
    // saved to this device under a password nobody here knows.
    try {
      await this.engine('vault.change_password', { index: 0 }, { handle: reply.json.handle, secret: password });
    } catch (failure) {
      await this.engine('vault.lock', {}, { handle: reply.json.handle }).catch(() => undefined);
      throw failure;
    }
    this.adopt(reply.json.handle, `${VAULTS}/${randomHex(16)}`, reply.json.name, false);
    await this.saveNow();
    const length = (await host().readFile(this.file!)).length;
    await this.writeChaff(`${VAULTS}/${randomHex(16)}`, length);
    await this.refresh();
  }

  /** Shows an account through the same screens as a vault. `writer` keeps what it saves. */
  async adoptAccount(accountId: string, reply: Reply, writer: (bytes: Uint8Array) => Promise<void>) {
    this.handle = reply.json.handle;
    this.file = null;
    this.accountWriter = writer;
    this.account = accountId;
    await this.noteSuccess();
    this.state = { kind: 'unlocked', name: reply.json.name, file: accountId, readOnly: false };
    host().stateChanged?.({ unlocked: true, name: reply.json.name, handle: reply.json.handle, writes: true });
    await this.refresh();
  }

  noteWrongAccountPassword() {
    return this.noteFailure();
  }

  // ---- the desktop app's vault ----

  /**
   * Shows the vault open in the desktop app through these screens. Calls go to the app over its
   * link, and the app saves. Attachments stay there: their files sit beside its vault. False when
   * the app is locked or not connected.
   */
  async openDesktop() {
    const link = host().link;
    const vault = await link?.attachVault();
    if (!vault) return false;
    this.handle = vault.handle;
    this.file = null;
    this.desktop = true;
    this.state = { kind: 'unlocked', name: vault.name, file: 'desktop', readOnly: false };
    try {
      await this.refresh();
      this.desktopSeen = (await this.view<{ hash: string }>('vault.state_hash')).hash;
    } catch (failure) {
      this.leaveDesktop();
      throw failure;
    }
    // Changes made in the app show here, and a lock there brings the door: the next call answers locked.
    // One watch only, even if a second open raced the first.
    this.stopWatching();
    this.watch = setInterval(() => {
      this.writing = this.writing.then(() => this.desktopChanges()).catch(() => undefined);
    }, 3000);
    return true;
  }

  private desktopSeen = '';

  private async desktopChanges() {
    if (!this.desktop) return;
    const { hash } = await this.view<{ hash: string }>('vault.state_hash');
    if (hash === this.desktopSeen) return;
    this.desktopSeen = hash;
    await this.refresh();
  }

  /** Back to the door. The vault in the desktop app stays open there. */
  private leaveDesktop() {
    this.stopWatching();
    host().link?.detachVault();
    this.desktop = false;
    this.handle = 0;
    this.file = null;
    this.entries = [];
    this.folders = [];
    this.rollbackWarning = false;
    this.notices = { clock_warning: null, password_conflict: null };
    this.changeAsked.clear();
    void this.refreshDoorState().catch(() => (this.state = { kind: 'locked' }));
  }

  // ---- nested and shared vaults ----

  linkedPath(id: string) {
    return `${LINKED}/${id}`;
  }

  /** Shows a nested or shared vault through the same screens. The one behind it stays open. */
  async enterChild(id: string, reply: Reply, writer: (bytes: Uint8Array) => Promise<void>, before?: () => Promise<void>) {
    await this.flush();
    this.outer ??= { handle: this.handle, file: this.file, state: this.state };
    this.handle = reply.json.handle;
    this.file = this.linkedPath(id);
    this.childWriter = writer;
    this.childBefore = before ?? null;
    this.child = reply.json.name;
    this.state = { kind: 'unlocked', name: reply.json.name, file: id, readOnly: false };
    host().stateChanged?.({ unlocked: true, name: reply.json.name, handle: reply.json.handle, writes: true });
    await this.absorbHeldLinked(id);
    await this.refresh();
  }

  async leaveChild() {
    const back = this.outer;
    if (!back) return;
    await this.flush();
    await this.engine('vault.lock', {}, { handle: this.handle });
    this.outer = null;
    this.childWriter = null;
    this.childBefore = null;
    this.handle = back.handle;
    this.file = back.file;
    this.child = null;
    this.state = back.state;
    // Said as it was when the vault behind first opened: the extension keeps the file to open again
    // after its worker stops, and without it would bring back a vault with no file.
    const name = back.state.kind === 'unlocked' ? back.state.name : '';
    const readOnly = back.state.kind === 'unlocked' && back.state.readOnly;
    // An account behind it is written from this window; the desktop app's vault is written there.
    host().stateChanged?.({ unlocked: true, name, handle: back.handle, ...(back.file ? { file: back.file, read_only: readOnly } : { writes: this.account !== null }) });
    await this.refresh();
  }
}

export const pass = new PassStore();
export { bytesOf, consumeText };
