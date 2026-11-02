// Sealing for what must rest on disk here: logins for backup places and the key that talks to the
// desktop app. The sealing key cannot be read out of the browser by a script, though anyone who
// can run code as this extension could ask it to decrypt, so this guards against copied files.

import { database, request } from '../../ui/src/hosts/web';

const STORE = 'settings';
const KEY_NAME = 'sealing-key';

async function sealingKey(): Promise<CryptoKey> {
  const db = await database();
  const found = await request(db.transaction(STORE, 'readonly').objectStore(STORE).get(KEY_NAME));
  if (found) return found as CryptoKey;
  const key = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
  await request(db.transaction(STORE, 'readwrite').objectStore(STORE).put(key, KEY_NAME));
  return key;
}

export async function seal(name: string, text: string) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const data = new TextEncoder().encode(text);
  const sealed = new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv, additionalData: new TextEncoder().encode(name) }, await sealingKey(), data));
  data.fill(0);
  const out = new Uint8Array(iv.length + sealed.length);
  out.set(iv);
  out.set(sealed, iv.length);
  const db = await database();
  await request(db.transaction(STORE, 'readwrite').objectStore(STORE).put(out, `sealed/${name}`));
}

export async function unseal(name: string): Promise<string | null> {
  const db = await database();
  const stored = (await request(db.transaction(STORE, 'readonly').objectStore(STORE).get(`sealed/${name}`))) as Uint8Array | undefined;
  if (!stored) return null;
  const plain = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: stored.slice(0, 12), additionalData: new TextEncoder().encode(name) },
    await sealingKey(),
    stored.slice(12),
  );
  return new TextDecoder().decode(plain);
}
