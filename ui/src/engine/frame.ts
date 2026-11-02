// One call as one run of bytes. The format is in crates/wmpass-session/src/wire.rs.

export interface Reply<T = any> {
  json: T;
  secret: Uint8Array | null;
  blobs: Uint8Array[];
}

export class EngineError extends Error {
  code: string;
  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

export interface Head {
  method: string;
  handle?: number;
  args?: unknown;
  now_ms?: number;
  online?: boolean;
}

export function encodeRequest(head: Head, secret?: Uint8Array | null, blobs: Uint8Array[] = []): Uint8Array {
  const text = encoder.encode(JSON.stringify(head));
  const secretLength = secret?.length ?? 0;
  let size = 4 + text.length + 4 + secretLength + 4;
  for (const blob of blobs) size += 4 + blob.length;
  const out = new Uint8Array(size);
  const view = new DataView(out.buffer);
  let at = 0;
  const run = (bytes: Uint8Array | null | undefined) => {
    const length = bytes?.length ?? 0;
    view.setUint32(at, length, true);
    at += 4;
    if (bytes && length) out.set(bytes, at);
    at += length;
  };
  run(text);
  run(secret);
  view.setUint32(at, blobs.length, true);
  at += 4;
  for (const blob of blobs) run(blob);
  return out;
}

export function decodeReply<T = any>(frame: Uint8Array): Reply<T> {
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  let at = 0;
  const length = () => {
    const value = view.getUint32(at, true);
    at += 4;
    return value;
  };
  const run = () => {
    const size = length();
    // A copy, so wiping the frame does not take the caller's value with it.
    const value = frame.slice(at, at + size);
    at += size;
    return value;
  };
  const json = JSON.parse(decoder.decode(run()));
  if (json && typeof json === 'object' && json.error && typeof json.error.code === 'string') {
    throw new EngineError(json.error.code, json.error.message);
  }
  const hasSecret = frame[at++] === 1;
  const secret = hasSecret ? run() : null;
  const count = length();
  const blobs: Uint8Array[] = [];
  for (let i = 0; i < count; i++) blobs.push(run());
  frame.fill(0);
  return { json, secret, blobs };
}

/** Text from a secret, and the bytes wiped. The text itself cannot be wiped, so use it only to show it. */
export function consumeText(bytes: Uint8Array | null): string {
  if (!bytes) return '';
  const text = decoder.decode(bytes);
  bytes.fill(0);
  return text;
}

export function bytesOf(text: string): Uint8Array {
  return encoder.encode(text);
}
