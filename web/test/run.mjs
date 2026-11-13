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

// Two tabs on one vault: what one saves shows in the other, and the other's next save keeps it
// rather than writing its older copy over it.
const second = await context.newPage();
second.on('pageerror', (e) => errors.push(e.message));
await second.goto(`${ORIGIN}/vault/`);
await second.locator('input[type=password]').first().fill(password);
await second.keyboard.press('Enter');
await second.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 });
await second.getByRole('button', { name: 'Not now' }).click({ timeout: 3000 }).catch(() => {});
async function addLogin(tab, title) {
  await tab.bringToFront();
  await tab.getByRole('button', { name: 'Add an item' }).click();
  await tab.getByRole('menuitem', { name: 'Login', exact: true }).click({ timeout: 2000 }).catch(() => {});
  await tab.getByRole('textbox', { name: 'Name', exact: true }).fill(title);
  await tab.getByRole('button', { name: 'Save', exact: true }).click();
  await tab.locator('.list-pane').getByText(title, { exact: true }).waitFor({ timeout: 10_000 });
}
const shows = (tab, title) => tab.locator('.list-pane').getByText(title, { exact: true }).waitFor({ timeout: 10_000 }).then(() => true, () => false);
await addLogin(page, 'From the first tab');
check('a change in one tab shows in another on the same vault', await shows(second, 'From the first tab'));
await addLogin(second, 'From the second tab');
check('and the other tab’s change comes back', await shows(page, 'From the second tab'));
await second.close();
await page.reload();
await page.locator('input[type=password]').first().fill(password);
await page.keyboard.press('Enter');
await page.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 });
await page.getByRole('button', { name: 'Not now' }).click({ timeout: 3000 }).catch(() => {});
check('neither tab’s save lost the other’s', (await shows(page, 'From the first tab')) && (await shows(page, 'From the second tab')));

// The list from the keyboard alone. Shift and the arrows choose a range, as Shift-click does.
await addLogin(page, 'Range three');
const list = page.locator('.list-body[role=listbox]');
await list.focus();
await page.keyboard.press('Home');
await page.keyboard.press('Shift+ArrowDown');
await page.keyboard.press('Shift+ArrowDown');
const chosen = await page.locator('.list-body [role=option][aria-selected=true]').count();
check('Shift and the arrow keys choose a range in the list', chosen === 3 && (await page.getByText('3 selected').isVisible()), `${chosen} chosen`);
await page.keyboard.press('ArrowUp');

// A site's logins fold into one stack, which the arrows reach and open and close.
async function addSiteLogin(title, site) {
  await page.getByRole('button', { name: 'Add an item' }).click();
  await page.getByRole('menuitem', { name: 'Login', exact: true }).click({ timeout: 2000 }).catch(() => {});
  const name = page.getByRole('textbox', { name: 'Name', exact: true });
  // The form puts the focus in the name a moment after it opens; typing elsewhere before then would land there.
  await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === 'Name', null, { timeout: 3000 }).catch(() => {});
  await name.fill(title);
  const website = page.getByRole('textbox', { name: 'Website', exact: true });
  if (!(await website.isVisible())) await page.getByRole('button', { name: /^Websites and apps/ }).click();
  await website.fill(site);
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await page.locator('.detail h1', { hasText: title }).waitFor({ timeout: 10_000 });
}
await addSiteLogin('Stacked one', 'stack.example');
await addSiteLogin('Stacked two', 'stack.example');
await list.focus();
await page.keyboard.press('Home');
let onStack = false;
for (let i = 0; i < 8 && !onStack; i++) {
  onStack = ((await list.getAttribute('aria-activedescendant')) ?? '').startsWith('stack-');
  if (!onStack) await page.keyboard.press('ArrowDown');
}
const options = () => page.locator('.list-body [role=option]').count();
const stackSays = () => page.locator('.list-body [id^="stack-"]').first().textContent();
const closedCount = await options();
const closedWords = await stackSays();
await page.keyboard.press('ArrowRight');
const openCount = await options();
const openWords = await stackSays();
await page.keyboard.press('ArrowLeft');
check(
  'the arrow keys reach a site’s stack, and open and close it',
  onStack && openCount === closedCount + 2 && (await options()) === closedCount && /closed/.test(closedWords ?? '') && /open/.test(openWords ?? ''),
  `${onStack} ${closedCount}/${openCount}`,
);

// A message with an Undo waits while the keyboard is on it, and goes once the keyboard leaves.
await page.locator('.list-pane').getByText('Range three', { exact: true }).click();
await page.getByRole('button', { name: 'More', exact: true }).click();
await page.getByRole('menuitem', { name: 'Move to trash' }).click();
const undo = page.locator('.toast button', { hasText: 'Undo' });
await undo.waitFor({ timeout: 5000 });
await undo.focus();
await sleep(7000);
const stayed = await undo.isVisible();
await page.getByRole('textbox', { name: 'Search' }).focus();
await sleep(6800);
check('a message stays while the keyboard is on its button, and goes after', stayed && (await page.locator('.toast').count()) === 0);

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
await page.getByPlaceholder('The same password again').fill(keyPassword);
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

// A rekey in one tab: the other tab locks and says why, rather than writing its copy under the old
// key back over the file, and the new password opens it there.
const other = await context.newPage();
other.on('pageerror', (e) => errors.push(e.message));
await other.goto(`${ORIGIN}/vault/`);
await other.locator('input[type=password]').first().fill(password);
await other.keyboard.press('Enter');
await other.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 });
await other.getByRole('button', { name: 'Not now' }).click({ timeout: 3000 }).catch(() => {});
await page.bringToFront();
await page.getByRole('button', { name: 'Settings', exact: true }).click();
await page.getByRole('button', { name: 'Security', exact: true }).first().click();
await page.getByRole('button', { name: /^Change it and rekey/ }).click();
const rekeyed = crypto.randomUUID() + crypto.randomUUID();
await page.getByPlaceholder('Current password').fill(password);
await page.getByPlaceholder('New password', { exact: true }).fill(rekeyed);
await page.getByPlaceholder('New password again').fill(rekeyed);
await page.getByRole('button', { name: 'Change', exact: true }).click();
check('a rekey turns passkey unlock off, since the passkey kept the old key', await page.waitForFunction(() => document.querySelector('[role=switch][aria-label="Unlock with a passkey"]')?.getAttribute('aria-checked') === 'false', null, { timeout: 15_000 }).then(() => true, () => false));
check('a tab locks when another rekeys the vault', await other.waitForSelector('.door-card', { timeout: 15_000 }).then(() => true, () => false));
check('and says the password changed elsewhere', await other.getByText(/master password was changed in another window/).first().waitFor({ timeout: 5000 }).then(() => true, () => false));
await other.locator('input[type=password]').first().fill(rekeyed);
await other.keyboard.press('Enter');
check('the new password opens it there, the file not written back under the old key', await other.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 }).then(() => true, () => false), (await other.locator('.danger-text').allTextContents()).join(' '));
await other.close();

// A bug in the session. A wasm module traps on a panic, so the trap is made here, as one looks from
// the page: a call that never answers, and a RuntimeError thrown from the module's own queue. The
// release build has no way to panic on purpose. The page's own scripts are as shipped.
const trap = await context.newPage();
await trap.addInitScript(() => {
  const instantiate = WebAssembly.instantiateStreaming;
  WebAssembly.instantiateStreaming = async (...args) => {
    const { instance, module } = await instantiate(...args);
    const exports = { ...instance.exports };
    const real = exports.call_async;
    exports.call_async = (...inner) => {
      if (!window.__wmpassTrap) return real(...inner);
      queueMicrotask(() => {
        throw new WebAssembly.RuntimeError('unreachable');
      });
      return new Promise(() => {});
    };
    window.__wmpassMemory = instance.exports.memory;
    return { instance: { exports }, module };
  };
});
await trap.goto(`${ORIGIN}/vault/`);
await trap.locator('input[type=password]').first().fill(rekeyed);
await trap.keyboard.press('Enter');
await trap.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 });
await trap.getByRole('button', { name: 'Not now' }).click({ timeout: 3000 }).catch(() => {});
await trap.getByRole('button', { name: 'Add an item' }).click();
await trap.getByRole('menuitem', { name: 'Login', exact: true }).click({ timeout: 2000 }).catch(() => {});
const halfTyped = trap.getByRole('textbox', { name: 'Name', exact: true });
await halfTyped.fill('Typed before the error');
await trap.evaluate(() => (window.__wmpassTrap = true));
await trap.getByRole('button', { name: 'Save', exact: true }).click();
const notice = trap.getByRole('alertdialog', { name: 'Something went wrong' });
check('an internal error says so over the screen', await notice.waitFor({ timeout: 10_000 }).then(() => true, () => false));
check('with the words it should', /locked the vault to keep it safe\. Anything you were typing is still on this screen/.test((await notice.textContent()) ?? ''));
check('what was typed is still there, and can be selected to copy', (await halfTyped.inputValue().catch(() => '')) === 'Typed before the error' && (await trap.evaluate(() => !document.querySelector('input[aria-label="Name"]')?.closest('[inert]') && !document.querySelector('dialog:modal'))));
check('no message besides it', (await trap.locator('.toast').count()) === 0);
check('the module’s memory, keys and all, is wiped', await trap.evaluate(() => new Uint8Array(window.__wmpassMemory.buffer).every((byte) => byte === 0)));
await trap.keyboard.press('Escape');
await trap.waitForSelector('.door-card', { timeout: 15_000 }).catch(() => {});
check('Escape goes to the door, with a fresh session', !(await notice.isVisible().catch(() => false)) && (await trap.locator('input[type=password]').count()) > 0);
await trap.locator('input[type=password]').first().fill(rekeyed);
await trap.keyboard.press('Enter');
check('and the vault opens again, nothing saved lost', await trap.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 }).then(() => true, () => false) && (await shows(trap, 'From the first tab')));
await trap.close();

check('no script errors and nothing the policy blocked', errors.length === 0, errors.slice(0, 3).join(' | ').slice(0, 300));
await context.close();
console.log(failures ? `${failures} failed` : 'all passed');
process.exit(failures ? 1 : 0);
