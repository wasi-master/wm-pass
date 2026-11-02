// The session as a wasm module, with files in IndexedDB. The extension and the design preview run
// on this. Nothing here can reach a network: those calls are the desktop's and the phone's.

import init, { call, lock_all, start } from '../wasm/wmpass_wasm.js';
import type { Host, PickedFile } from '../engine/host';
import { decodeReply } from '../engine/frame';

const DB_NAME = 'wmpass';
const FILES = 'files';
const SETTINGS = 'settings';

function database(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open(DB_NAME, 1);
    open.onupgradeneeded = () => {
      open.result.createObjectStore(FILES);
      open.result.createObjectStore(SETTINGS);
    };
    open.onsuccess = () => resolve(open.result);
    open.onerror = () => reject(open.error);
  });
}

function request<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function pickWithInput(extensions?: string[]): Promise<PickedFile | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    if (extensions?.length) input.accept = extensions.map((e) => `.${e}`).join(',');
    input.onchange = async () => {
      const file = input.files?.[0];
      resolve(file ? { name: file.name, bytes: new Uint8Array(await file.arrayBuffer()) } : null);
    };
    input.oncancel = () => resolve(null);
    input.click();
  });
}

export async function wasmHost(wasmUrl?: string): Promise<Host> {
  await init(wasmUrl ? { module_or_path: wasmUrl } : undefined);
  const db = await database();
  const store = (name: string, mode: IDBTransactionMode) => db.transaction(name, mode).objectStore(name);

  const keys = async (prefix: string) => (await request(store(FILES, 'readonly').getAllKeys())).map(String).filter((k) => k.startsWith(prefix));
  let clearTimer: ReturnType<typeof setTimeout> | undefined;

  const host: Host = {
    kind: 'web',
    os: /Mac/.test(navigator.platform) ? 'mac' : /Win/.test(navigator.platform) ? 'windows' : /Linux/.test(navigator.platform) ? 'linux' : 'other',
    async call(frame) {
      return call(frame);
    },
    async callToClipboard(frame, clearAfterSeconds) {
      // A page has no way around this: the secret passes through here on its way to the clipboard.
      const raw = call(frame);
      const reply = decodeReply(raw);
      const text = new TextDecoder().decode(reply.secret ?? new Uint8Array());
      await host.copyText(text, true, clearAfterSeconds);
      reply.secret?.fill(0);
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
    async lockAll() {
      lock_all();
    },
    async listFiles(folder) {
      const prefix = `${folder}/`;
      const seen = new Map<string, number>();
      for (const key of await keys(prefix)) {
        const rest = key.slice(prefix.length);
        const slash = rest.indexOf('/');
        if (slash >= 0) seen.set(rest.slice(0, slash), 0);
        else seen.set(rest, ((await request(store(FILES, 'readonly').get(key))) as Uint8Array).length);
      }
      return [...seen].map(([name, size]) => ({ name, size }));
    },
    async readFile(path) {
      const value = await request(store(FILES, 'readonly').get(path));
      if (!value) throw new Error('That file is gone.');
      return value as Uint8Array;
    },
    async writeFile(path, bytes) {
      await request(store(FILES, 'readwrite').put(bytes, path));
    },
    async removeFile(path) {
      await request(store(FILES, 'readwrite').delete(path));
    },
    async removeFolder(path) {
      for (const key of await keys(`${path}/`)) await request(store(FILES, 'readwrite').delete(key));
    },
    async coverFile() {
      // IndexedDB keeps no file times.
    },
    async loadSettings(name) {
      return ((await request(store(SETTINGS, 'readonly').get(name))) as string | undefined) ?? null;
    },
    async saveSettings(name, text) {
      await request(store(SETTINGS, 'readwrite').put(text, name));
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
      window.open(url, '_blank', 'noopener');
    },
    async memoryGib() {
      return (navigator as any).deviceMemory ?? 4;
    },
  };
  start(crypto.getRandomValues(new Uint32Array(1))[0]);
  return host;
}

function encodeReplyEmpty(): Uint8Array {
  // json "{}", no secret, no blobs.
  return new Uint8Array([2, 0, 0, 0, 0x7b, 0x7d, 0, 0, 0, 0, 0]);
}

