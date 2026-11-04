// Drives the built web vault in a real Chromium, at its real address: requests for
// https://wmpass.pages.dev/vault/ are answered from web/dist with the headers docs/public/_headers
// gives them, so the page runs under the same Content-Security-Policy it gets in production.
// Run it with `node web/test/run.mjs` after `npm run web:build`.
import { mkdtempSync, readFileSync, existsSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const here = dirname(fileURLToPath(import.meta.url));
const dist = join(here, '../dist');
const shots = process.env.WMPASS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const ORIGIN = 'https://wmpass.pages.dev';
let failures = 0;
function check(name, ok, detail = '') {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? ` (${detail})` : ''}`);
  if (!ok) failures++;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The headers file, as Cloudflare Pages reads it: a path pattern, then indented `Name: value` lines.
const rules = [];
for (const line of readFileSync(join(here, '../../docs/public/_headers'), 'utf8').split('\n')) {
  if (!line.trim() || line.trim().startsWith('#')) continue;
  if (!/^\s/.test(line)) rules.push({ pattern: line.trim(), headers: {} });
  else {
    const at = line.indexOf(':');
    rules.at(-1).headers[line.slice(0, at).trim().toLowerCase()] = line.slice(at + 1).trim();
  }
}
const matches = (pattern, path) => (pattern.endsWith('*') ? path.startsWith(pattern.slice(0, -1)) : path === pattern);
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.png': 'image/png', '.svg': 'image/svg+xml', '.webmanifest': 'application/manifest+json', '.json': 'application/json' };

const context = await chromium.launchPersistentContext(mkdtempSync(join(tmpdir(), 'wmpass-web-')), {
  channel: 'chromium',
  headless: true,
  viewport: { width: 1100, height: 760 },
  // Requests a service worker makes are not all routed, so it stays out of the way here.
  serviceWorkers: 'block',
});
await context.route(`${ORIGIN}/vault/**`, (route) => {
  const path = new URL(route.request().url()).pathname;
  const rest = path.slice('/vault/'.length);
  const file = join(dist, !rest || rest.endsWith('/') ? `${rest}index.html` : rest);
  if (!file.startsWith(dist) || !existsSync(file)) return route.fulfill({ status: 404, body: 'not found' });
  const headers = { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' };
  for (const rule of rules) if (matches(rule.pattern, path)) Object.assign(headers, rule.headers);
  return route.fulfill({ status: 200, headers, body: readFileSync(file) });
});
const errors = [];
const page = await context.newPage();
page.on('console', (m) => {
  if (m.type() === 'error') errors.push(m.text());
});
page.on('pageerror', (e) => errors.push(e.message));
await page.addInitScript(() => {
  document.addEventListener('securitypolicyviolation', (e) => console.error(`CSP blocked ${e.blockedURI} (${e.violatedDirective})`));
});

const password = crypto.randomUUID() + crypto.randomUUID();
await page.goto(`${ORIGIN}/vault/`);
await page.waitForSelector('input[type=password]', { timeout: 30_000 });
const csp = await page.evaluate(async () => (await fetch(location.href)).headers.get('content-security-policy'));
check('the page is served under its policy', !!csp && csp.includes("script-src 'self' 'wasm-unsafe-eval'"), csp?.slice(0, 60));
const fields = page.locator('input[type=password]');
await fields.nth(0).fill(password);
await fields.nth(1).fill(password);
await page.getByRole('button', { name: 'Create vault' }).click();
await page.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 90_000 });
check('a vault can be made in the web vault', true);
await page.getByRole('button', { name: 'Not now' }).click({ timeout: 5000 }).catch(() => {});

// The vault stays in this site's storage: a reload comes back to the door, and the password opens it.
await page.reload();
await page.locator('input[type=password]').first().fill(password);
await page.keyboard.press('Enter');
check('the vault is still here after a reload', await page.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 }).then(() => true, () => false));
await page.getByRole('button', { name: 'Not now' }).click({ timeout: 3000 }).catch(() => {});

// A folder place, on a folder handle from the browser's private file system: the same handle type
// the folder picker gives, without the picker. It is kept as the app keeps one, and the place is
// sealed as the app seals it.
await page.evaluate(async () => {
  const folder = await (await navigator.storage.getDirectory()).getDirectoryHandle('backups', { create: true });
  const open = (name, version, upgrade) =>
    new Promise((resolve, reject) => {
      const request = indexedDB.open(name, version);
      request.onupgradeneeded = () => upgrade?.(request.result);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  const put = (db, store, value, key) =>
    new Promise((resolve, reject) => {
      const request = db.transaction(store, 'readwrite').objectStore(store).put(value, key);
      request.onsuccess = resolve;
      request.onerror = () => reject(request.error);
    });
  const get = (db, store, key) =>
    new Promise((resolve, reject) => {
      const request = db.transaction(store, 'readonly').objectStore(store).get(key);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  await put(await open('wmpass-handles', 1, (db) => db.createObjectStore('handles')), 'handles', folder, 'fixture');
  const db = await open('wmpass', 1);
  let key = await get(db, 'settings', 'sealing-key');
  if (!key) {
    key = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
    await put(db, 'settings', key, 'sealing-key');
  }
  const place = { id: 'fixture-place', kind: 'folder', label: 'Fixture folder', path: 'browser:fixture:backups', url: '', user: '', password: '', endpoint: '', region: '', bucket: '', prefix: '', access_key: '', secret_key: '', backup: true, sync: false, extra: {} };
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const sealed = new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv, additionalData: new TextEncoder().encode('places') }, key, new TextEncoder().encode(JSON.stringify([place]))));
  const out = new Uint8Array(12 + sealed.length);
  out.set(iv);
  out.set(sealed, 12);
  await put(db, 'settings', out, 'sealed/places');
});
await page.reload();
await page.locator('input[type=password]').first().fill(password);
await page.keyboard.press('Enter');
await page.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 });
await page.getByRole('button', { name: 'Not now' }).click({ timeout: 3000 }).catch(() => {});
await page.getByRole('button', { name: 'Settings', exact: true }).click();
await page.getByRole('button', { name: 'Backups', exact: true }).first().click();
check('a folder place shows by its folder’s name', await page.getByText('backups', { exact: true }).first().waitFor({ timeout: 5000 }).then(() => true, () => false));
await page.getByRole('button', { name: 'Back up to Fixture folder now' }).click();
const copies = async () =>
  page.evaluate(async () => {
    const folder = await (await navigator.storage.getDirectory()).getDirectoryHandle('backups');
    const names = [];
    for await (const name of folder.keys()) names.push(name);
    return names;
  });
let written = [];
for (let i = 0; i < 40 && !written.some((n) => n.startsWith('wmpass-backup-')); i++) {
  await sleep(250);
  written = await copies();
}
check('a backup is written to a folder and read back', written.some((n) => n.startsWith('wmpass-backup-') && n.endsWith('.wmbackup')), written.join(', '));

// Security keys and passkey unlock, belonging to wmpass.pages.dev like the extension's.
const authenticators = await context.newCDPSession(page);
await authenticators.send('WebAuthn.enable');
for (const transport of ['internal', 'usb']) {
  await authenticators.send('WebAuthn.addVirtualAuthenticator', {
    options: { protocol: 'ctap2', ctap2Version: 'ctap2_1', transport, hasResidentKey: true, hasUserVerification: true, isUserVerified: true, hasPrf: true, automaticPresenceSimulation: true },
  });
}
await page.reload();
await page.locator('input[type=password]').first().fill(password);
await page.keyboard.press('Enter');
await page.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 });
await page.getByRole('button', { name: 'Not now' }).click({ timeout: 3000 }).catch(() => {});
// Making a credential needs the view to have focus, which headless Chromium hands over late.
await page.bringToFront();
for (let i = 0; i < 40; i++) {
  const refusal = await page.evaluate(() =>
    navigator.credentials
      .create({ publicKey: { rp: { name: 'Focus' }, user: { id: new Uint8Array(16), name: 'f', displayName: 'f' }, challenge: new Uint8Array(32), pubKeyCredParams: [{ type: 'public-key', alg: -7 }], authenticatorSelection: { authenticatorAttachment: 'cross-platform', userVerification: 'discouraged' } } })
      .then(() => '', (e) => e.message),
  );
  if (!/focus/i.test(refusal)) break;
  await sleep(500);
}
await page.getByRole('button', { name: 'Settings', exact: true }).click();
await page.getByRole('button', { name: 'Security', exact: true }).first().click();
const keyPassword = crypto.randomUUID();
await page.getByRole('button', { name: /^Add a security key/ }).click();
await page.getByRole('button', { name: /^A FIDO2 security key/ }).click();
await page.getByPlaceholder('A name for the key').fill('Test key');
await page.getByPlaceholder('Password to use with it').fill(keyPassword);
await page.getByRole('button', { name: 'Next' }).click();
check('a security key is added in the web vault', await page.getByText('The key is added').waitFor({ timeout: 15_000 }).then(() => true, () => false), (await page.locator('.danger-text').allTextContents()).join(' '));
await page.getByRole('button', { name: 'Done' }).click();
const passkeySwitch = page.getByRole('switch', { name: 'Unlock with a passkey' });
await passkeySwitch.click();
check('passkey unlock turns on in the web vault', await page.waitForFunction(() => document.querySelector('[role=switch][aria-label="Unlock with a passkey"]')?.getAttribute('aria-checked') === 'true', null, { timeout: 10_000 }).then(() => true, () => false));
await page.reload();
await page.getByRole('button', { name: 'Unlock with a passkey' }).click();
check('a passkey opens the web vault', await page.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 15_000 }).then(() => true, () => false));
await page.reload();
await page.locator('input[type=password]').first().fill(keyPassword);
await page.keyboard.press('Enter');
check('the key and its password open the web vault', await page.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 }).then(() => true, () => false), (await page.locator('.danger-text').allTextContents()).join(' '));
if (shots) await page.screenshot({ path: join(shots, 'web-vault.png') });

check('no script errors and nothing the policy blocked', errors.length === 0, errors.slice(0, 3).join(' | ').slice(0, 300));
await context.close();
console.log(failures ? `${failures} failed` : 'all passed');
process.exit(failures ? 1 : 0);
