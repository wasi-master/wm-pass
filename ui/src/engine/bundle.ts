// A backup: every vault file and sealed attachment in one zip, the same on every platform. All of
// it is ciphertext. Entries are stored rather than compressed, since sealed bytes do not shrink.

import { host } from './host';

const FILE_TIME = dosTime(1_600_000_000_000);

function dosTime(ms: number) {
  const d = new Date(ms);
  const time = (d.getUTCHours() << 11) | (d.getUTCMinutes() << 5) | (d.getUTCSeconds() >> 1);
  const date = ((d.getUTCFullYear() - 1980) << 9) | ((d.getUTCMonth() + 1) << 5) | d.getUTCDate();
  return { time, date };
}

const TABLE = (() => {
  const out = new Uint32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    out[i] = c >>> 0;
  }
  return out;
})();

function crc32(bytes: Uint8Array) {
  let crc = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) crc = TABLE[(crc ^ bytes[i]) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

export interface BundleFile {
  name: string;
  bytes: Uint8Array;
}

/** Makes a zip of stored entries. */
export function zipStored(files: BundleFile[]): Uint8Array {
  const encoder = new TextEncoder();
  let size = 22;
  const names = files.map((f) => encoder.encode(f.name));
  files.forEach((f, i) => (size += 30 + names[i].length + f.bytes.length + 46 + names[i].length));
  const out = new Uint8Array(size);
  const view = new DataView(out.buffer);
  let at = 0;
  const offsets: number[] = [];
  const crcs = files.map((f) => crc32(f.bytes));
  files.forEach((f, i) => {
    offsets.push(at);
    view.setUint32(at, 0x04034b50, true);
    view.setUint16(at + 4, 20, true);
    view.setUint16(at + 6, 0x0800, true);
    view.setUint16(at + 8, 0, true);
    view.setUint16(at + 10, FILE_TIME.time, true);
    view.setUint16(at + 12, FILE_TIME.date, true);
    view.setUint32(at + 14, crcs[i], true);
    view.setUint32(at + 18, f.bytes.length, true);
    view.setUint32(at + 22, f.bytes.length, true);
    view.setUint16(at + 26, names[i].length, true);
    view.setUint16(at + 28, 0, true);
    out.set(names[i], at + 30);
    at += 30 + names[i].length;
    out.set(f.bytes, at);
    at += f.bytes.length;
  });
  const central = at;
  files.forEach((f, i) => {
    view.setUint32(at, 0x02014b50, true);
    view.setUint16(at + 4, 20, true);
    view.setUint16(at + 6, 20, true);
    view.setUint16(at + 8, 0x0800, true);
    view.setUint16(at + 10, 0, true);
    view.setUint16(at + 12, FILE_TIME.time, true);
    view.setUint16(at + 14, FILE_TIME.date, true);
    view.setUint32(at + 16, crcs[i], true);
    view.setUint32(at + 20, f.bytes.length, true);
    view.setUint32(at + 24, f.bytes.length, true);
    view.setUint16(at + 28, names[i].length, true);
    view.setUint32(at + 42, offsets[i], true);
    out.set(names[i], at + 46);
    at += 46 + names[i].length;
  });
  view.setUint32(at, 0x06054b50, true);
  view.setUint16(at + 8, files.length, true);
  view.setUint16(at + 10, files.length, true);
  view.setUint32(at + 12, at - central, true);
  view.setUint32(at + 16, central, true);
  return out;
}

async function inflate(raw: Uint8Array): Promise<Uint8Array> {
  const stream = new Blob([raw as BlobPart]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

/** Whether an entry's name is one a backup may hold. An entry called `../../x` must go nowhere. */
function nameIsFine(name: string) {
  const parts = name.split('/');
  if (parts.some((p) => !p || p === '.' || p === '..' || p.startsWith('.') || p.includes('\\'))) return false;
  if (parts[0] === 'vaults' || parts[0] === 'linked') return parts.length === 2;
  if (parts[0] === 'blobs') return parts.length === 3;
  return false;
}

/** The files in a backup, with hostile names refused. A backup comes from somewhere else. */
export async function unzip(bytes: Uint8Array): Promise<BundleFile[]> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let end = -1;
  for (let i = bytes.length - 22; i >= Math.max(0, bytes.length - 66_000); i--) {
    if (view.getUint32(i, true) === 0x06054b50) {
      end = i;
      break;
    }
  }
  if (end < 0) return [];
  const count = view.getUint16(end + 10, true);
  let at = view.getUint32(end + 16, true);
  const decoder = new TextDecoder();
  const out: BundleFile[] = [];
  for (let i = 0; i < count; i++) {
    if (at + 46 > bytes.length || view.getUint32(at, true) !== 0x02014b50) break;
    const method = view.getUint16(at + 10, true);
    const packed = view.getUint32(at + 20, true);
    const plain = view.getUint32(at + 24, true);
    const nameLength = view.getUint16(at + 28, true);
    const extra = view.getUint16(at + 30, true);
    const comment = view.getUint16(at + 32, true);
    const offset = view.getUint32(at + 42, true);
    const name = decoder.decode(bytes.subarray(at + 46, at + 46 + nameLength));
    at += 46 + nameLength + extra + comment;
    if (!nameIsFine(name) || plain > 2 ** 31 || offset + 30 > bytes.length) continue;
    const start = offset + 30 + view.getUint16(offset + 26, true) + view.getUint16(offset + 28, true);
    const stored = bytes.subarray(start, start + packed);
    if (method === 0) out.push({ name, bytes: stored.slice() });
    else if (method === 8) out.push({ name, bytes: await inflate(stored) });
  }
  return out;
}

/** Every vault file and sealed attachment on this device, as one file. Runs while locked too. */
export async function makeBundle(): Promise<Uint8Array> {
  const files: BundleFile[] = [];
  for (const prefix of ['vaults', 'linked']) {
    const listed = (await host().listFiles(prefix)).filter((f) => !f.name.startsWith('.')).sort((a, b) => (a.name < b.name ? -1 : 1));
    for (const f of listed) files.push({ name: `${prefix}/${f.name}`, bytes: await host().readFile(`${prefix}/${f.name}`) });
  }
  const groups = (await host().listFiles('blobs')).sort((a, b) => (a.name < b.name ? -1 : 1));
  for (const group of groups) {
    const listed = (await host().listFiles(`blobs/${group.name}`)).sort((a, b) => (a.name < b.name ? -1 : 1));
    for (const f of listed) files.push({ name: `blobs/${group.name}/${f.name}`, bytes: await host().readFile(`blobs/${group.name}/${f.name}`) });
  }
  return zipStored(files);
}

export function isVaultFile(bytes: Uint8Array) {
  return bytes.length > 4 && bytes[0] === 0x57 && bytes[1] === 0x4d && bytes[2] === 0x50 && bytes[3] === 0x56;
}

export async function bundleLooksRight(bytes: Uint8Array) {
  return (await unzip(bytes)).some((f) => f.name.startsWith('vaults/'));
}
