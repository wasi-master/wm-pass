// The session as a wasm module in this page, with files in IndexedDB. The design preview runs on
// this. Calls that reach a network go out through this page's own fetch, so a service that does
// not allow requests from a web page answers as unreachable.

import init, { call_async, lock_all, start } from '../wasm/wmpass_wasm.js';
import type { Host } from '../engine/host';
import { wasmFetch } from './fetch';
import { webHost } from './web';

export async function wasmHost(wasmUrl?: string): Promise<Host> {
  await init(wasmUrl ? { module_or_path: wasmUrl } : undefined);
  start(crypto.getRandomValues(new Uint32Array(1))[0]);
  return { ...(await webHost({ call: (frame) => call_async(frame, wasmFetch), lockAll: async () => lock_all() })), network: true };
}
