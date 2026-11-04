// Places outside the browser, through the same Rust sink code the desktop app runs, built into
// the session's WebAssembly. Only ciphertext goes through here. Requests leave by `fetch` without
// cookies and without following redirects; plain http is refused except on this computer, by the
// Rust policy check. A browser cannot open raw connections, so SFTP, FTP, SMB, NFS and IMAP are
// the desktop app's and the phone's. A folder on this computer goes through the File System Access
// API where the browser has it.
//
// The extension and the web vault differ only in where the WebAssembly comes from and how a
// service's sign-in page hands its answer back.

import type { PlaceFile, PlacesHost, SignInAnswer } from '../engine/host';
import init, { place_authorize, place_exchange, place_kinds, place_run, place_sign_in } from '../wasm/wmpass_wasm.js';
import { wasmFetch } from './fetch';
import { browserFolders, fileAccess } from './folders';

export interface BrowserSignIn {
  /** Where the service sends its answer, as registered with it. */
  redirect: string;
  /** Opens the service's page and resolves to the address it came back to, or null if cancelled. */
  open(url: string): Promise<string | null>;
}

const folder = (place: any) => place?.kind === 'folder';
const withFolder = (kinds: string[]) => (fileAccess ? ['folder', ...kinds] : kinds);

// The pages ask only when the owner has network features on; the Rust check is the backstop.
const ONLINE = { online: true };

export function browserPlaces(wasmUrl: string, signIn: BrowserSignIn): PlacesHost {
  let ready: Promise<unknown> | null = null;
  const wasm = () => (ready ??= init({ module_or_path: wasmUrl }));

  async function run(place: any, request: Record<string, unknown>, bytes: Uint8Array = new Uint8Array()) {
    await wasm();
    const online = !!request.online;
    const answer: any = await place_run(JSON.stringify(place), JSON.stringify({ ...request, online, now_ms: Date.now() }), new Uint8Array(bytes), wasmFetch);
    const update = JSON.parse(answer.update || '{}');
    if (place?.id && Object.keys(update).length) places.onUpdate?.(place.id, update);
    if (answer.problem) throw new Error(answer.problem);
    return answer.value;
  }

  async function accountSignIn(place: any): Promise<SignInAnswer> {
    await wasm();
    const start = JSON.parse(place_authorize(JSON.stringify(place), signIn.redirect));
    const back = await signIn.open(start.url);
    if (!back) throw new Error('The sign-in was cancelled.');
    const query = new URL(back).searchParams;
    const code = query.get('code');
    if (!code) throw new Error('The sign-in was cancelled.');
    // The state ties the answer to this request: a code from anywhere else is refused.
    if (query.get('state') !== start.state) throw new Error('The sign-in came back with the wrong state, so it was refused.');
    const refresh = await place_exchange(JSON.stringify(place), code, start.verifier, signIn.redirect, wasmFetch);
    return { extra: { refresh_token: refresh } };
  }

  const places: PlacesHost = {
    kinds: withFolder(['webdav', 's3']),
    list: async (place: any) => (folder(place) ? browserFolders.list(place.path) : (JSON.parse(await run(place, { op: 'list', ...ONLINE })) as PlaceFile[])),
    read: async (place: any, file) => (folder(place) ? browserFolders.read(place.path, file) : ((await run(place, { op: 'read', file, ...ONLINE })) as Uint8Array)),
    write: async (place: any, name, bytes) => void (folder(place) ? await browserFolders.write(place.path, name, bytes) : await run(place, { op: 'write', name, ...ONLINE }, bytes)),
    remove: async (place: any, file) => void (folder(place) ? await browserFolders.remove(place.path, file) : await run(place, { op: 'delete', file, ...ONLINE })),
    chooseFolder: async () => (fileAccess ? browserFolders.choose() : null),
    async signIn(place: any, step) {
      if (['drive', 'dropbox', 'onedrive', 'box'].includes(place.kind)) return accountSignIn(place);
      await wasm();
      return JSON.parse(await place_sign_in(JSON.stringify(place), JSON.stringify(step), Date.now(), wasmFetch)) as SignInAnswer;
    },
  };

  // The kinds this build can reach, once the WebAssembly is in: some need an app id it was built with.
  void wasm().then(() => {
    places.kinds = withFolder(JSON.parse(place_kinds()).kinds);
  });
  return places;
}
