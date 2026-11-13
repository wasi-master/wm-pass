// The session as a wasm module in this page, with files in IndexedDB. The design preview runs on
// this. Calls that reach a network go out through this page's own fetch, so a service that does
// not allow requests from a web page answers as unreachable.

import init, { call_async, lock_all, start } from '../wasm/wmpass_wasm.js';
import type { Host } from '../engine/host';
import { internalFrame } from '../engine/internal';
import { wasmFetch } from './fetch';
import { fileChanges, webHost } from './web';

export async function wasmHost(wasmUrl?: string): Promise<Host> {
  const module = await init(wasmUrl ? { module_or_path: wasmUrl } : undefined);
  start(crypto.getRandomValues(new Uint32Array(1))[0]);
  const trapped = trapWatch(module.memory);
  // Every page of this site keeps its own session over the same files: each says what it wrote.
  const base = await webHost(
    {
      call: (frame) => trapped.run(() => call_async(frame, wasmFetch)),
      lockAll: async () => {
        if (!trapped.dead) lock_all();
      },
    },
    true,
  );
  return {
    ...base,
    network: true,
    onFileChanged: fileChanges,
    internalError: () => trapped.wipe(),
    // A module that panicked cannot be used again: the page loads a fresh one, at the door.
    restart: async () => location.reload(),
  };
}

/**
 * A panic in the module traps, as stable wasm32 cannot unwind. A call that traps at once throws a
 * `RuntimeError`; one that traps after its first wait, inside the module's own queue of tasks,
 * never answers, and the trap shows only as an error on the page. Either way the calls waiting
 * are answered with an internal error, and the module's whole memory, the keys it held among it,
 * is filled with zeros. Nothing reaches the module after that.
 */
function trapWatch(memory: WebAssembly.Memory) {
  const waiting = new Set<() => void>();
  const watch = {
    dead: false,
    wipe() {
      if (watch.dead) return;
      watch.dead = true;
      new Uint8Array(memory.buffer).fill(0);
      for (const answer of [...waiting]) answer();
      waiting.clear();
    },
    run(call: () => Promise<Uint8Array>): Promise<Uint8Array> {
      if (watch.dead) return Promise.resolve(internalFrame());
      return new Promise((resolve, reject) => {
        const answer = () => resolve(internalFrame());
        waiting.add(answer);
        const done = (run: () => void) => {
          waiting.delete(answer);
          run();
        };
        const failed = (failure: unknown) => {
          if (!(failure instanceof WebAssembly.RuntimeError)) return done(() => reject(failure));
          watch.wipe();
          done(() => resolve(internalFrame()));
        };
        try {
          call().then((reply) => done(() => resolve(reply)), failed);
        } catch (failure) {
          failed(failure);
        }
      });
    },
  };
  const seen = (failure: unknown) => {
    if (failure instanceof WebAssembly.RuntimeError) watch.wipe();
  };
  window.addEventListener('error', (event) => seen(event.error));
  window.addEventListener('unhandledrejection', (event) => seen(event.reason));
  return watch;
}
