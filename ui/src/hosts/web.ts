// Files in IndexedDB and the rest of a browser page's abilities. The session itself comes from
// whoever calls this: wasm in the same page, or the extension's background.

import type { Host, PickedFile } from '../engine/host';
import { decodeReply } from '../engine/frame';

const DB_NAME = 'wmpass';
const FILES = 'files';
const SETTINGS = 'settings';

let opened: Promise<IDBDatabase> | null = null;

/** The one connection this page keeps to its storage, rather than a new one for every call. */
export function database(): Promise<IDBDatabase> {
  opened ??= new Promise<IDBDatabase>((resolve, reject) => {
    const open = indexedDB.open(DB_NAME, 1);
    open.onupgradeneeded = () => {
      open.result.createObjectStore(FILES);
      open.result.createObjectStore(SETTINGS);
    };
    open.onsuccess = () => {
      const db = open.result;
      // A newer page asking to change the database's shape is let through; the next call reopens.
      db.onversionchange = () => {
        db.close();
        opened = null;
      };
      db.onclose = () => (opened = null);
      resolve(db);
    };
    open.onerror = () => reject(open.error);
  }).catch((failure) => {
    opened = null;
    throw failure;
  });
  return opened;
}

export function request<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(storageError(req.error));
  });
}

/** What a failed write says. A full disk or a site out of its share is told as that. */
export function storageError(error: DOMException | null): Error {
  if (error?.name === 'QuotaExceededError') {
    return new Error('This browser has no room left for WM Pass. Free up disk space, or clear other sites’ data, and try again.');
  }
  return error ?? new Error('The browser could not write to its storage.');
}

/**
 * Settles once a write is on disk, not when it was only accepted: a full disk shows only as the
 * transaction commits, and a save that reported success then would be lost unseen.
 */
export function committed(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = tx.onabort = () => reject(storageError(tx.error));
  });
}

const FILE_NEWS = 'wmpass-files';
/** This page, so it can tell its own news from another tab's: a channel hears its page's other channels too. */
const PAGE = Math.random().toString(36).slice(2) + Date.now().toString(36);

/**
 * Tells this site's other tabs which file just changed, and calls `run` when one of theirs does.
 * Two tabs on the same vault would otherwise each keep their own copy and save over the other's.
 */
export function fileChanges(run: (path: string) => void): () => void {
  if (typeof BroadcastChannel === 'undefined') return () => {};
  const channel = new BroadcastChannel(FILE_NEWS);
  channel.onmessage = (event) => {
    if (typeof event.data?.path === 'string' && event.data.from !== PAGE) run(event.data.path);
  };
  return () => channel.close();
}

function pickWithInput(extensions?: string[]): Promise<PickedFile | null> {
  return new Promise((resolve, reject) => {
    const input = document.createElement('input');
    input.type = 'file';
    if (extensions?.length) input.accept = extensions.map((e) => `.${e}`).join(',');
    input.onchange = async () => {
      const file = input.files?.[0];
      // A file that cannot be read, such as one moved meanwhile, says so rather than never answering.
      try {
        resolve(file ? { name: file.name, bytes: new Uint8Array(await file.arrayBuffer()) } : null);
      } catch {
        reject(new Error('That file could not be read.'));
      }
    };
    input.oncancel = () => resolve(null);
    input.click();
  });
}

/**
 * `shared`: the session lives in this page and every write to these files comes from a page of
 * this site, so they can tell each other of it. Not so in the extension, whose background writes too.
 */
export async function webHost(session: { call(frame: Uint8Array): Promise<Uint8Array>; lockAll(): Promise<void> }, shared = false): Promise<Host> {
  // Opened now, so a browser that keeps no storage for this page says so before anything else.
  await database();
  const store = async (name: string, mode: IDBTransactionMode) => (await database()).transaction(name, mode).objectStore(name);
  const under = (prefix: string) => IDBKeyRange.bound(prefix, `${prefix}\uffff`);

  const keys = async (prefix: string) => (await request((await store(FILES, 'readonly')).getAllKeys(under(prefix)))).map(String);
  /**
   * Each file's size, as last written or read here. Listing a folder would otherwise read every
   * file in it whole, and a save lists the vault's folder twice. Kept only where every other
   * writer says what it changed.
   */
  const sizes = new Map<string, number>();
  const news = shared && typeof BroadcastChannel !== 'undefined' ? new BroadcastChannel(FILE_NEWS) : null;
  if (news) news.onmessage = (event) => sizes.delete(String(event.data?.path ?? ''));
  const tell = (path: string) => news?.postMessage({ path, from: PAGE });
  const write = async (name: string, change: (store: IDBObjectStore) => void) => {
    const tx = (await database()).transaction(name, 'readwrite');
    change(tx.objectStore(name));
    await committed(tx);
  };
  let clearTimer: ReturnType<typeof setTimeout> | undefined;

  const host: Host = {
    kind: 'web',
    os: /Mac/.test(navigator.platform) ? 'mac' : /Win/.test(navigator.platform) ? 'windows' : /Linux/.test(navigator.platform) ? 'linux' : 'other',
    call: session.call,
    async callToClipboard(frame, clearAfterSeconds) {
      // A page has no way around this: the secret passes through here on its way to the clipboard.
      const raw = await session.call(frame);
      const reply = decodeReply(raw);
      const text = new TextDecoder().decode(reply.secret ?? new Uint8Array());
      try {
        await host.copyText(text, true, clearAfterSeconds);
      } finally {
        // Wiped even when the page may not write the clipboard, such as without focus.
        reply.secret?.fill(0);
      }
      return encodeReplyEmpty();
    },
    async copyText(text, _sensitive, clearAfterSeconds) {
      await navigator.clipboard.writeText(text);
      clearTimeout(clearTimer);
      if (clearAfterSeconds > 0) {
        clearTimer = setTimeout(async () => {
          try {
            if ((await navigator.clipboard.readText()) === text) await navigator.clipboard.writeText('');
          } catch {
            // Not allowed to read it back without focus, so leave it rather than guess.
          }
        }, clearAfterSeconds * 1000);
      }
    },
    async clearClipboard() {
      clearTimeout(clearTimer);
      await navigator.clipboard.writeText('');
    },
    lockAll: session.lockAll,
    async listFiles(folder) {
      const prefix = `${folder}/`;
      const seen = new Map<string, number>();
      for (const key of await keys(prefix)) {
        const rest = key.slice(prefix.length);
        const slash = rest.indexOf('/');
        if (slash >= 0) seen.set(rest.slice(0, slash), 0);
        else {
          let size = news ? sizes.get(key) : undefined;
          if (size === undefined) {
            size = (((await request((await store(FILES, 'readonly')).get(key))) as Uint8Array | undefined)?.length ?? 0);
            if (news) sizes.set(key, size);
          }
          seen.set(rest, size);
        }
      }
      return [...seen].map(([name, size]) => ({ name, size }));
    },
    async readFile(path) {
      const value = await request((await store(FILES, 'readonly')).get(path));
      if (!value) throw new Error('That file is gone.');
      return value as Uint8Array;
    },
    async writeFile(path, bytes) {
      sizes.delete(path);
      await write(FILES, (files) => files.put(bytes, path));
      if (news) sizes.set(path, bytes.length);
      tell(path);
    },
    async removeFile(path) {
      sizes.delete(path);
      await write(FILES, (files) => files.delete(path));
      tell(path);
    },
    async removeFolder(path) {
      // One transaction: a folder half gone would leave a vault without some of its attachments.
      const gone = await keys(`${path}/`);
      for (const key of gone) sizes.delete(key);
      await write(FILES, (files) => files.delete(under(`${path}/`)));
      for (const key of gone) tell(key);
    },
    async coverFile() {
      // IndexedDB keeps no file times.
    },
    async loadSettings(name) {
      return ((await request((await store(SETTINGS, 'readonly')).get(name))) as string | undefined) ?? null;
    },
    async saveSettings(name, text) {
      await write(SETTINGS, (settings) => settings.put(text, name));
    },
    pickFile(_title, extensions) {
      return pickWithInput(extensions);
    },
    async saveFile(_title, name, bytes) {
      const url = URL.createObjectURL(new Blob([bytes as BlobPart]));
      const link = document.createElement('a');
      link.href = url;
      link.download = name;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 10_000);
      return true;
    },
    async openUrl(url) {
      // As in the desktop app: a saved address such as javascript: must not run in this page's name.
      const lower = url.trim().toLowerCase();
      if (!(lower.startsWith('https://') || lower.startsWith('http://') || lower.startsWith('mailto:')) || /\p{Cc}/u.test(url)) {
        throw new Error('That is not a web address.');
      }
      window.open(url.trim(), '_blank', 'noopener');
    },
    async memoryGib() {
      return (navigator as any).deviceMemory ?? 4;
    },
  };
  return host;
}

function encodeReplyEmpty(): Uint8Array {
  // json "{}", no secret, no blobs.
  return new Uint8Array([2, 0, 0, 0, 0x7b, 0x7d, 0, 0, 0, 0, 0]);
}

