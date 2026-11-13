// An internal error: a bug in the session (a Rust panic) stopped a call part way. The shell has
// already thrown that session away, keys and all, as what it held may be half changed; the files
// on disk are whole. The screens say so over whatever they show (stopped.svelte.ts), keeping
// anything typed there to be copied, and go to the door once the person has read it. Not the panic
// lock a person asks for, which is an ordinary lock with a wait after it.

import { EngineError } from './frame';

export const INTERNAL = 'internal';

/** Whether a failure is an internal error, from any shell. */
export function isInternal(failure: unknown): boolean {
  // `panic` is what the session said before it said `internal`.
  if (failure instanceof EngineError) return failure.code === INTERNAL || failure.code === 'panic';
  // A WebAssembly module traps on a panic, as it cannot unwind.
  return typeof WebAssembly !== 'undefined' && failure instanceof WebAssembly.RuntimeError;
}

/** What every call is told while the screens say so, without asking the session. */
export function stoppedError() {
  return new EngineError(INTERNAL, 'WM Pass hit an internal error and locked the vault.');
}

/** A reply frame that carries only this error, as the session writes one. */
export function internalFrame(): Uint8Array {
  const text = new TextEncoder().encode(JSON.stringify({ error: { code: INTERNAL, message: stoppedError().message } }));
  const out = new Uint8Array(4 + text.length + 1 + 4);
  new DataView(out.buffer).setUint32(0, text.length, true);
  out.set(text, 4);
  return out;
}
