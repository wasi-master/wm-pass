// Places outside the browser, through the same Rust sink code the desktop app runs, built into
// the session's WebAssembly. Only ciphertext goes through here. Requests leave by `fetch` without
// cookies and without following redirects; plain http is refused except on this computer, by the
// Rust policy check. A browser cannot open raw connections, so SFTP, FTP, SMB, NFS and IMAP are
// the desktop app's and the phone's.

import type { PlaceFile, PlacesHost, SignInAnswer } from '../../ui/src/engine/host';
import init, { place_authorize, place_exchange, place_kinds, place_run, place_sign_in } from '../../ui/src/wasm/wmpass_wasm.js';

declare const chrome: any;

let ready: Promise<unknown> | null = null;

function wasm() {
  return (ready ??= init({ module_or_path: chrome.runtime.getURL('wmpass_wasm_bg.wasm') }));
}

async function wasmFetch(method: string, url: string, headers: [string, string][], body: Uint8Array | null, timeoutMs: number) {
  const stop = new AbortController();
  const timer = setTimeout(() => stop.abort(), timeoutMs || 120_000);
  try {
    const reply = await fetch(url, {
      method,
      headers,
      body: body as BodyInit | null,
      credentials: 'omit',
      redirect: 'manual',
      cache: 'no-store',
      signal: stop.signal,
    });
    return {
      status: reply.status,
      headers: Array.from(reply.headers.entries()),
      body: new Uint8Array(await reply.arrayBuffer()),
      retry_after: Number(reply.headers.get('retry-after')) || undefined,
    };
  } finally {
    clearTimeout(timer);
  }
}

async function run(place: any, request: Record<string, unknown>, bytes: Uint8Array = new Uint8Array()) {
  await wasm();
  const online = !!request.online;
  const answer: any = await place_run(JSON.stringify(place), JSON.stringify({ ...request, online, now_ms: Date.now() }), new Uint8Array(bytes), wasmFetch);
  const update = JSON.parse(answer.update || '{}');
  if (place?.id && Object.keys(update).length) extensionPlaces.onUpdate?.(place.id, update);
  if (answer.problem) throw new Error(answer.problem);
  return answer.value;
}

// The pages ask only when the owner has network features on; the Rust check is the backstop.
const ONLINE = { online: true };

async function browserSignIn(place: any): Promise<SignInAnswer> {
  await wasm();
  const redirect = chrome.identity.getRedirectURL('oauth');
  const start = JSON.parse(place_authorize(JSON.stringify(place), redirect));
  const back = await chrome.identity.launchWebAuthFlow({ url: start.url, interactive: true });
  if (!back) throw new Error('The sign-in was cancelled.');
  const query = new URL(back).searchParams;
  const code = query.get('code');
  if (!code) throw new Error('The sign-in was cancelled.');
  // The state ties the answer to this request: a code from anywhere else is refused.
  if (query.get('state') !== start.state) throw new Error('The sign-in came back with the wrong state, so it was refused.');
  const refresh = await place_exchange(JSON.stringify(place), code, start.verifier, redirect, wasmFetch);
  return { extra: { refresh_token: refresh } };
}

export const extensionPlaces: PlacesHost = {
  kinds: ['webdav', 's3'],
  list: async (place) => JSON.parse(await run(place, { op: 'list', ...ONLINE })) as PlaceFile[],
  read: async (place, file) => (await run(place, { op: 'read', file, ...ONLINE })) as Uint8Array,
  write: async (place, name, bytes) => void (await run(place, { op: 'write', name, ...ONLINE }, bytes)),
  remove: async (place, file) => void (await run(place, { op: 'delete', file, ...ONLINE })),
  chooseFolder: async () => null,
  async signIn(place: any, step) {
    if (['drive', 'dropbox', 'onedrive', 'box'].includes(place.kind)) return browserSignIn(place);
    await wasm();
    return JSON.parse(await place_sign_in(JSON.stringify(place), JSON.stringify(step), Date.now(), wasmFetch)) as SignInAnswer;
  },
};

// The kinds this build can reach, once the WebAssembly is in: some need an app id it was built with.
void wasm().then(() => {
  extensionPlaces.kinds = JSON.parse(place_kinds()).kinds;
});
