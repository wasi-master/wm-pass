// Folders and files on this computer for the browser builds, through the File System Access API,
// which Chrome and Edge have and Firefox does not. The browser hands over a handle, never a path.
// Handles are kept in their own IndexedDB store, and what WM Pass saves is a name for one,
// `browser:<id>:<shown name>`. In each new browser session the browser asks once more before a
// handle can be used, and only a click on the page can start that question.

import type { OutsideFiles, PlaceFile } from '../engine/host';
import { request } from './web';

const NAMES = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' }).compare;

const PREFIX = 'browser:';
const DB_NAME = 'wmpass-handles';
const STORE = 'handles';

/** Whether this browser can reach folders and files where they are. */
export const fileAccess = typeof (globalThis as any).showDirectoryPicker === 'function';

function handles(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open(DB_NAME, 1);
    open.onupgradeneeded = () => open.result.createObjectStore(STORE);
    open.onsuccess = () => resolve(open.result);
    open.onerror = () => reject(open.error);
  });
}

async function keep(handle: any): Promise<string> {
  const id = Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) => b.toString(16).padStart(2, '0')).join('');
  await request((await handles()).transaction(STORE, 'readwrite').objectStore(STORE).put(handle, id));
  return `${PREFIX}${id}:${handle.name}`;
}

async function find(path: string): Promise<any> {
  const id = path.startsWith(PREFIX) ? path.slice(PREFIX.length).split(':')[0] : '';
  const handle = id ? await request((await handles()).transaction(STORE, 'readonly').objectStore(STORE).get(id)) : null;
  if (!handle) throw new Error('This browser no longer knows that folder. Choose it again.');
  return handle;
}

/** What to show for a kept handle: the name it had when chosen. */
export function shownName(path: string) {
  return path.startsWith(PREFIX) ? path.slice(path.indexOf(':', PREFIX.length) + 1) : path;
}

async function allowed(handle: any, write: boolean) {
  const mode = write ? 'readwrite' : 'read';
  if ((await handle.queryPermission({ mode })) === 'granted') return;
  // Asking needs a click on this page. A run on a timer has none, so it says what to do instead.
  const answer = await handle.requestPermission({ mode }).catch(() => 'prompt');
  if (answer !== 'granted') {
    throw Object.assign(new Error(`The browser needs your yes again to use “${handle.name}”. Run it once from Settings.`), { code: 'permission' });
  }
}

// A name read from elsewhere must not walk out of the folder, the same rule as the desktop's.
function checked(name: string) {
  if (!name || /[\\/]/.test(name) || name === '.' || name === '..') throw new Error('That file name is not allowed.');
  return name;
}

const gone = (failure: any) => failure?.name === 'NotFoundError';

/** Writes and closes; a write that fails aborts, so the half-written copy is dropped and the file is let go. */
async function finish(writer: any, bytes: Uint8Array) {
  try {
    await writer.write(bytes);
  } catch (failure) {
    await writer.abort().catch(() => {});
    throw failure;
  }
  await writer.close();
}

/** A folder place: any folder the person picks, which another app may sync elsewhere. */
export const browserFolders = {
  async choose(): Promise<string | null> {
    try {
      return await keep(await (globalThis as any).showDirectoryPicker({ id: 'wmpass-folder', mode: 'readwrite' }));
    } catch (failure: any) {
      if (failure?.name === 'AbortError') return null;
      throw failure;
    }
  },
  async list(path: string): Promise<PlaceFile[]> {
    const folder = await find(path);
    await allowed(folder, false);
    const out: PlaceFile[] = [];
    for await (const [name, entry] of folder.entries()) {
      if (entry.kind !== 'file' || name.startsWith('.')) continue;
      const file: File = await entry.getFile();
      out.push({ id: name, name, size: file.size, modified_ms: file.lastModified });
    }
    // As a file manager shows them: "backup 2" before "backup 10".
    return out.sort((a, b) => NAMES(a.name, b.name));
  },
  async read(path: string, file: PlaceFile): Promise<Uint8Array> {
    const folder = await find(path);
    await allowed(folder, false);
    const handle = await folder.getFileHandle(checked(file.id));
    return new Uint8Array(await (await handle.getFile()).arrayBuffer());
  },
  async write(path: string, name: string, bytes: Uint8Array) {
    const folder = await find(path);
    await allowed(folder, true);
    const handle = await folder.getFileHandle(checked(name), { create: true });
    // The browser writes beside the file and swaps it in on close, so a cut-off write leaves the old one.
    const writer = await handle.createWritable();
    await finish(writer, bytes);
  },
  async remove(path: string, file: PlaceFile) {
    const folder = await find(path);
    await allowed(folder, true);
    try {
      await folder.removeEntry(checked(file.id));
    } catch (failure) {
      if (!gone(failure)) throw failure;
    }
  },
};

/** A file kept where it is, such as a KeePass database, read and written back in place. */
export const browserFiles: OutsideFiles = {
  async choose(title, extensions) {
    try {
      const types = extensions?.length ? [{ description: title, accept: { 'application/octet-stream': extensions.map((e) => `.${e}`) } }] : undefined;
      const [handle] = await (globalThis as any).showOpenFilePicker({ id: 'wmpass-file', types });
      return { path: await keep(handle), name: handle.name };
    } catch (failure: any) {
      if (failure?.name === 'AbortError') return null;
      throw failure;
    }
  },
  async read(path) {
    const handle = await find(path);
    // Asked for writing too, while the click that opened it still counts: saves come later, with none.
    await allowed(handle, true);
    return new Uint8Array(await (await handle.getFile()).arrayBuffer());
  },
  async write(path, bytes) {
    const handle = await find(path);
    await allowed(handle, true);
    const writer = await handle.createWritable();
    await finish(writer, bytes);
  },
};
