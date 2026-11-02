// The vaults on this device, for every screen. A port of android/core/.../Pass.kt, so a folder of
// vault files means the same thing on every platform.

import { host } from './host';
import { bytesOf, consumeText, decodeReply, encodeRequest, EngineError, type Reply } from './frame';
import { isVaultFile, makeBundle, unzip } from './bundle';
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
} from './types';

const VAULTS = 'vaults';
const LINKED = 'linked';
const BLOBS = 'blobs';
const DOOR = 'door';

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
  /** The account behind what is open, if it is not a vault in this folder. */
  account = $state<string | null>(null);
  /** The name of the nested or shared vault in use. */
  child = $state<string | null>(null);

  handle = 0;
  file: string | null = null;
  private door: DoorState = { device: 0, fails: 0, retry_at: 0, seen: {} };
  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  private writing: Promise<unknown> = Promise.resolve();
  private accountWriter: ((bytes: Uint8Array) => Promise<void>) | null = null;
  private childWriter: ((bytes: Uint8Array) => Promise<void>) | null = null;
  private childBefore: (() => Promise<void>) | null = null;
  private outer: { handle: number; file: string | null; state: LockState } | null = null;
  /** Called after an account has saved, to take its changes to a server if it has one. */
  afterAccountSave: (() => Promise<void>) | null = null;
  /** Called after the vault in this folder has saved, for a sync that follows it. */
  afterSave: (() => void) | null = null;
  /** Run on a wipe, so what the app keeps beside the vault goes too. */
  onWipe: (() => void)[] = [];
  /** How long to wait after a change before writing. The extension's popup can vanish at any moment. */
  saveDelayMs = 350;

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
  }

  private async writeDoor() {
    await host().saveSettings(DOOR, JSON.stringify(this.door));
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
    const frame = encodeRequest(
      { method, handle: options.handle ?? 0, args: args ?? {}, now_ms: Date.now(), online: options.online ?? false },
      options.secret,
      options.blobs ?? [],
    );
    const reply = await host().call(frame);
    frame.fill(0);
    return decodeReply<T>(reply);
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
    await this.refresh();
    this.scheduleSave();
    return reply;
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
    await this.saveNow();
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
    this.state = { kind: 'unlocked', name, file, readOnly };
    host().stateChanged?.({ unlocked: true, name });
  }

  /** When typing a password next becomes allowed, or 0. */
  retryAt() {
    return this.door.retry_at > Date.now() ? this.door.retry_at : 0;
  }

  private async noteFailure() {
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
        return { kind: 'wrong', retryAtMs: await this.noteFailure() };
    }
  }

  /** Opens with a key the system's own store holds. The key never reaches this page. */
  async unlockQuick() {
    const desktop = host().desktop;
    if (!desktop) return false;
    for (const f of await this.vaultFiles()) {
      if (!(await desktop.quickEnrolled(f.name))) continue;
      const path = `${VAULTS}/${f.name}`;
      const obj = decodeReply(await desktop.quickOpen(f.name, await host().readFile(path))).json;
      await this.noteSuccess();
      this.adopt(obj.handle, path, obj.name, obj.read_only === true);
      await this.afterOpen(f.name, obj.generation);
      return true;
    }
    return false;
  }

  /** Whether quick unlock could work for a file on this device. */
  async quickFiles() {
    const desktop = host().desktop;
    if (!desktop || !(await desktop.quickAvailable())) return [];
    const out: string[] = [];
    for (const f of await this.vaultFiles()) if (await desktop.quickEnrolled(f.name)) out.push(f.name);
    return out;
  }

  private async afterOpen(name: string, generation: number) {
    const seen = this.door.seen[name] ?? 0;
    this.rollbackWarning = generation < seen;
    if (generation > seen) {
      this.door.seen[name] = generation;
      await this.writeDoor();
    }
    const swept = (await this.engine('vault.sweep', {}, { handle: this.handle })).json.delete_blobs ?? [];
    for (const blob of swept) await this.deleteBlob(blob);
    if (swept.length) this.scheduleSave();
    await this.refresh();
  }

  /** Saves anything pending and forgets every key. */
  async lock() {
    await this.leaveChild();
    await this.flush();
    await host().lockAll();
    this.handle = 0;
    this.file = null;
    this.accountWriter = null;
    this.afterAccountSave = null;
    this.account = null;
    this.entries = [];
    this.folders = [];
    await this.refreshDoorState();
    host().stateChanged?.({ unlocked: false, name: '' });
  }

  /** A panic: locked at once, and every password refused for an hour. */
  async panic() {
    await this.lock();
    this.door.retry_at = Date.now() + 3_600_000;
    await this.writeDoor();
  }

  async wipe() {
    await host().lockAll();
    await host().removeFolder(VAULTS);
    await host().removeFolder(BLOBS);
    await host().removeFolder(LINKED);
    this.door = { device: this.door.device, fails: 0, retry_at: 0, seen: {} };
    await this.writeDoor();
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
    this.child = null;
    this.account = null;
    this.entries = [];
    this.folders = [];
    this.state = { kind: 'none' };
    host().stateChanged?.({ unlocked: false, name: '' });
  }

  // ---- saving ----

  private scheduleSave() {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => void this.queueSave(), this.saveDelayMs);
  }

  private queueSave() {
    this.writing = this.writing.then(() => this.saveNow()).catch(() => undefined);
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
    if (!this.file) return;
    const reply = await this.engine('vault.save', {}, { handle: active });
    await host().writeFile(this.file, reply.blobs[0]);
    this.door.seen[this.file.split('/').pop()!] = reply.json.generation;
    await this.writeDoor();
    await this.coverTracks(reply.blobs[0].length);
    this.afterSave?.();
  }

  /** Writes pending changes now, before the window or process may go away. */
  async flush() {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = null;
    await this.queueSave();
  }

  private async refresh() {
    if (!this.handle) return;
    this.entries = await this.view<Summary[]>('entry.list', { sort: 'title' });
    this.folders = await this.view<FolderView[]>('folder.list');
    this.revision += 1;
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
  health(breached: Record<string, number> = {}) {
    return this.view<HealthView>('health.report', { breached });
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
    const frame = encodeRequest({ method, handle: this.handle, args, now_ms: Date.now() });
    const reply = await host().callToClipboard(frame, clearAfterSeconds);
    frame.fill(0);
    decodeReply(reply);
  }

  // ---- attachments ----

  private blobFolderOf(file: string) {
    return `${BLOBS}/${file.split('/').pop()}`;
  }

  blobFolder() {
    return this.file ? this.blobFolderOf(this.file) : null;
  }

  private async deleteBlob(name: string) {
    const folder = this.blobFolder();
    if (folder) await host().removeFile(`${folder}/${name}`);
  }

  async addAttachment(id: string, name: string, mime: string, plain: Uint8Array) {
    const reply = await this.change('attachment.add', { id, name, mime }, { blobs: [plain] });
    await host().writeFile(`${this.blobFolder()}/${reply.json.blob}`, reply.blobs[0]);
  }

  async openAttachment(id: string, blob: string): Promise<Uint8Array> {
    const sealed = await host().readFile(`${this.blobFolder()}/${blob}`);
    return (await this.engine('attachment.open', { id, blob }, { handle: this.handle, blobs: [sealed] })).secret!;
  }

  // ---- files coming and going ----

  /** The saved file, for a backup or a sync. It is ciphertext. */
  async sealedFile() {
    await this.flush();
    return host().readFile(this.file!);
  }

  async verify(password: Uint8Array) {
    return (await this.view('vault.verify', {}, { secret: password })).ok === true;
  }

  async rekey(password: Uint8Array) {
    const before = (await this.view('vault.chaff_name')).name;
    await this.change('vault.rekey', {}, { secret: password });
    await this.flush();
    await host().removeFile(`${VAULTS}/${before}`);
  }

  /** Takes a vault file from elsewhere as this device's vault. */
  async adoptFile(bytes: Uint8Array) {
    await host().writeFile(`${VAULTS}/${randomHex(16)}`, bytes);
    // A chaff file of its own, or a lone file would say which one is the vault.
    const chaff = await this.engine('vault.chaff', { file_len: bytes.length, kdf: { m_kib: await this.kdfMemoryKib() } });
    await host().writeFile(`${VAULTS}/${randomHex(16)}`, chaff.blobs[0]);
    for (const f of await this.vaultFiles()) await host().coverFile(`${VAULTS}/${f.name}`);
    if (!this.handle) this.state = { kind: 'locked' };
  }

  /** Every vault file and attachment on this device as one file, for a backup. */
  async bundle() {
    if (this.handle) await this.flush();
    return makeBundle();
  }

  /** Puts a vault file or a backup in place at the door. The password decides what opens. */
  async restore(bytes: Uint8Array) {
    if (isVaultFile(bytes)) {
      await this.adoptFile(bytes);
      return;
    }
    const files = await unzip(bytes);
    if (!files.some((f) => f.name.startsWith('vaults/'))) throw new EngineError('invalid', 'That is not a WM Pass backup.');
    for (const f of files) await host().writeFile(f.name, f.bytes);
    for (const f of await this.vaultFiles()) await host().coverFile(`${VAULTS}/${f.name}`);
    if (!this.handle) this.state = { kind: 'locked' };
  }

  /**
   * Merges a vault file or a backup into the open vault, and says how many of its files were
   * copies of this vault. Nothing is lost: a value that loses moves to history.
   */
  async mergeIn(bytes: Uint8Array) {
    const files = isVaultFile(bytes) ? [{ name: 'vaults/x', bytes }] : await unzip(bytes);
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
    this.adopt(reply.json.handle, `${VAULTS}/${randomHex(16)}`, reply.json.name, false);
    await this.engine('vault.change_password', { index: 0 }, { handle: this.handle, secret: password });
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
    host().stateChanged?.({ unlocked: true, name: reply.json.name });
    await this.refresh();
  }

  noteWrongAccountPassword() {
    return this.noteFailure();
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
    await this.refresh();
  }
}

export const pass = new PassStore();
export { bytesOf, consumeText };
