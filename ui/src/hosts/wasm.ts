// The session as a wasm module in this page, with files in IndexedDB. The design preview runs on
// this. Nothing here can reach a network: those calls are the desktop's and the phone's.

import init, { call, lock_all, start } from '../wasm/wmpass_wasm.js';
import type { Host } from '../engine/host';
import { webHost } from './web';

export async function wasmHost(wasmUrl?: string): Promise<Host> {
  await init(wasmUrl ? { module_or_path: wasmUrl } : undefined);
  start(crypto.getRandomValues(new Uint32Array(1))[0]);
  return webHost({ call: async (frame) => call(frame), lockAll: async () => lock_all() });
}
