// WebDAV and S3 from an extension page. Only ciphertext goes through here, and a backup or sync
// bundle fits in memory. Plain http is refused except on this computer, as in the Rust sinks.

import type { PlaceFile, PlacesHost } from '../../ui/src/engine/host';

function checked(url: string) {
  const parsed = new URL(url);
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(parsed.hostname);
  if (parsed.protocol !== 'https:' && !(parsed.protocol === 'http:' && local)) throw new Error('This place needs an https address.');
  return parsed;
}

function failed(status: number) {
  if (status === 401 || status === 403) return new Error('The server refused that login.');
  return new Error(`The server answered ${status}.`);
}

// ---- WebDAV ----

function davBase(place: any) {
  const url = checked(place.url);
  if (!url.pathname.endsWith('/')) url.pathname += '/';
  return url.href;
}

function davAuth(place: any) {
  const bytes = new TextEncoder().encode(`${place.user}:${place.password}`);
  return `Basic ${btoa(String.fromCharCode(...bytes))}`;
}

async function davList(place: any): Promise<PlaceFile[]> {
  const base = davBase(place);
  const reply = await fetch(base, { method: 'PROPFIND', headers: { Authorization: davAuth(place), Depth: '1', 'Content-Type': 'application/xml' }, credentials: 'omit' });
  if (reply.status === 404) return [];
  if (!reply.ok) throw failed(reply.status);
  const doc = new DOMParser().parseFromString(await reply.text(), 'application/xml');
  const out: PlaceFile[] = [];
  for (const item of Array.from(doc.getElementsByTagNameNS('DAV:', 'response'))) {
    const href = item.getElementsByTagNameNS('DAV:', 'href')[0]?.textContent ?? '';
    if (href.endsWith('/')) continue;
    const name = decodeURIComponent(href.split('/').pop() ?? '');
    if (!name || name.includes('..')) continue;
    out.push({
      id: name,
      name,
      size: Number(item.getElementsByTagNameNS('DAV:', 'getcontentlength')[0]?.textContent ?? 0),
      modified_ms: Date.parse(item.getElementsByTagNameNS('DAV:', 'getlastmodified')[0]?.textContent ?? '') || 0,
    });
  }
  return out;
}

async function davSend(place: any, method: string, name: string, body?: Uint8Array) {
  const reply = await fetch(davBase(place) + encodeURIComponent(name), {
    method,
    headers: { Authorization: davAuth(place) },
    body: body as BodyInit | undefined,
    credentials: 'omit',
  });
  if (!reply.ok && !(method === 'DELETE' && reply.status === 404)) throw failed(reply.status);
  return reply;
}

// ---- S3 ----

const encoder = new TextEncoder();

async function sha256Hex(data: Uint8Array | string) {
  const bytes = typeof data === 'string' ? encoder.encode(data) : data;
  return hex(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes as BufferSource)));
}

function hex(bytes: Uint8Array) {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

async function hmac(key: Uint8Array, text: string) {
  const imported = await crypto.subtle.importKey('raw', key as BufferSource, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return new Uint8Array(await crypto.subtle.sign('HMAC', imported, encoder.encode(text)));
}

function encodeKey(path: string) {
  return path.split('/').map(encodeURIComponent).join('/');
}

async function s3Send(place: any, method: string, name: string | null, body?: Uint8Array) {
  const endpoint = checked(place.endpoint || `https://s3.${place.region}.amazonaws.com`);
  const prefix = place.prefix ? `${place.prefix.replace(/\/+$/, '')}/` : '';
  const path = `/${place.bucket}/${name === null ? '' : encodeKey(prefix + name)}`;
  const query = name === null ? `list-type=2&prefix=${encodeURIComponent(prefix)}` : '';
  const stamp = new Date().toISOString().replace(/[-:]|\.\d{3}/g, '');
  const day = stamp.slice(0, 8);
  const hash = await sha256Hex(body ?? '');
  const signed = 'host;x-amz-content-sha256;x-amz-date';
  const canonical = [method, path, query, `host:${endpoint.host}`, `x-amz-content-sha256:${hash}`, `x-amz-date:${stamp}`, '', signed, hash].join('\n');
  const scope = `${day}/${place.region}/s3/aws4_request`;
  const toSign = ['AWS4-HMAC-SHA256', stamp, scope, await sha256Hex(canonical)].join('\n');
  let key = await hmac(encoder.encode(`AWS4${place.secret_key}`), day);
  key = await hmac(key, place.region);
  key = await hmac(key, 's3');
  key = await hmac(key, 'aws4_request');
  const signature = hex(await hmac(key, toSign));
  const reply = await fetch(`${endpoint.origin}${path}${query ? `?${query}` : ''}`, {
    method,
    headers: {
      'x-amz-content-sha256': hash,
      'x-amz-date': stamp,
      Authorization: `AWS4-HMAC-SHA256 Credential=${place.access_key}/${scope}, SignedHeaders=${signed}, Signature=${signature}`,
    },
    body: body as BodyInit | undefined,
    credentials: 'omit',
  });
  if (!reply.ok && !(method === 'DELETE' && reply.status === 404)) throw failed(reply.status);
  return reply;
}

async function s3List(place: any): Promise<PlaceFile[]> {
  const prefix = place.prefix ? `${place.prefix.replace(/\/+$/, '')}/` : '';
  const doc = new DOMParser().parseFromString(await (await s3Send(place, 'GET', null)).text(), 'application/xml');
  const out: PlaceFile[] = [];
  for (const item of Array.from(doc.getElementsByTagName('Contents'))) {
    const key = item.getElementsByTagName('Key')[0]?.textContent ?? '';
    const name = key.slice(prefix.length);
    if (!name || name.includes('/')) continue;
    out.push({
      id: name,
      name,
      size: Number(item.getElementsByTagName('Size')[0]?.textContent ?? 0),
      modified_ms: Date.parse(item.getElementsByTagName('LastModified')[0]?.textContent ?? '') || 0,
    });
  }
  return out;
}

export const extensionPlaces: PlacesHost = {
  kinds: ['webdav', 's3'],
  list: (place: any) => (place.kind === 's3' ? s3List(place) : davList(place)),
  async read(place: any, file) {
    const reply = place.kind === 's3' ? await s3Send(place, 'GET', file.name) : await davSend(place, 'GET', file.name);
    return new Uint8Array(await reply.arrayBuffer());
  },
  async write(place: any, name, bytes) {
    if (place.kind === 's3') await s3Send(place, 'PUT', name, bytes);
    else await davSend(place, 'PUT', name, bytes);
  },
  async remove(place: any, file) {
    if (place.kind === 's3') await s3Send(place, 'DELETE', file.name);
    else await davSend(place, 'DELETE', file.name);
  },
  chooseFolder: async () => null,
};
