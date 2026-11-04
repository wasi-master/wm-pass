// Drives the built extension in a real Chromium against the pages in fixtures/. Run it with
// `node extension/test/run.mjs` after `npm run extension:build`. Pictures go to WMPASS_SHOTS if set.
import { execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { createServer as createTlsServer } from 'node:https';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const here = dirname(fileURLToPath(import.meta.url));
const extension = join(here, '../dist/chrome');
const shots = process.env.WMPASS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
let failures = 0;
function check(name, ok, detail = '') {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? ` (${detail})` : ''}`);
  if (!ok) failures++;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const serve = (req, res) => {
  const path = new URL(req.url, 'http://x').pathname.replace(/^\//, '') || 'login.html';
  // A stand-in Inbucket server, with no CORS headers: only the extension's background can read it.
  if (path.startsWith('api/v1/mailbox/')) {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify([{ id: '1', from: 'a@example.com', subject: 'Hello from the fixture', 'posix-millis': 1 }]));
    return;
  }
  try {
    const body = readFileSync(join(here, 'fixtures', path.replace(/[^a-z-.]/g, '')));
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end(body);
  } catch {
    res.writeHead(404).end();
  }
};
const server = createServer(serve).listen(0);
await new Promise((r) => server.once('listening', r));
const port = server.address().port;
const site = (page) => `http://localhost:${port}/${page}`;

// Throwaway, made fresh for this run, for a throwaway vault in a throwaway profile.
const testPassword = crypto.randomUUID() + crypto.randomUUID();
const entryPassword = crypto.randomUUID();
const profile = mkdtempSync(join(tmpdir(), 'wmpass-ext-'));

// Passkeys need https, so a certificate only for this run.
const certs = mkdtempSync(join(tmpdir(), 'wmpass-cert-'));
execFileSync('openssl', ['req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:prime256v1', '-nodes', '-days', '1', '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost', '-keyout', join(certs, 'k.pem'), '-out', join(certs, 'c.pem')], { stdio: 'ignore' });
const tls = createTlsServer({ key: readFileSync(join(certs, 'k.pem')), cert: readFileSync(join(certs, 'c.pem')) }, serve).listen(0);
await new Promise((r) => tls.once('listening', r));
const context = await chromium.launchPersistentContext(profile, {
  channel: 'chromium',
  headless: true,
  ignoreHTTPSErrors: true,
  viewport: { width: 1100, height: 760 },
  args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`],
});
const errors = [];
context.on('weberror', (e) => errors.push(e.error().message));

let worker = context.serviceWorkers()[0] ?? (await context.waitForEvent('serviceworker'));
const id = new URL(worker.url()).host;
check('the id comes from the manifest key', id === (process.env.WMPASS_EXT_ID ?? id), id);

const vault = await context.newPage();
vault.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
vault.on('pageerror', (e) => errors.push(e.message));
await vault.goto(`chrome-extension://${id}/vault.html`);
await vault.waitForSelector('input[type=password]');
const fields = vault.locator('input[type=password]');
await fields.nth(0).fill(testPassword);
await fields.nth(1).fill(testPassword);
await vault.getByRole('button', { name: 'Create vault' }).click();
await vault.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 90_000 });
check('a vault can be made in a tab', true);

// The guide: the welcome comes once after the first vault, and a topic opens from Settings, Guide.
const welcome = vault.getByRole('button', { name: 'Not now' });
check('the welcome shows after the first vault', await welcome.waitFor({ timeout: 5000 }).then(() => true, () => false));
await welcome.click();
await vault.getByRole('button', { name: 'Settings', exact: true }).click();
await vault.getByRole('button', { name: 'Guide', exact: true }).click();
await vault.getByRole('button', { name: 'Main window' }).click();
check('a guide topic opens from Settings, Guide', await vault.locator('dialog.help-sheet[open]').getByText('three panes').first().waitFor({ timeout: 5000 }).then(() => true, () => false));
if (shots) await vault.screenshot({ path: join(shots, 'guide-topic.png') });
await vault.keyboard.press('Escape');
check('the help sheet closes with Escape', await vault.locator('dialog.help-sheet').waitFor({ state: 'detached', timeout: 3000 }).then(() => true, () => false));

// Calls the background the way the pages do.
const call = async (page, method, args, secret, online = false) =>
  page.evaluate(
    async ({ method, args, secret, handle, online }) => {
      const text = new TextEncoder().encode(JSON.stringify({ method, handle, args, now_ms: Date.now(), online }));
      const secretBytes = new TextEncoder().encode(secret ?? '');
      const out = new Uint8Array(4 + text.length + 4 + secretBytes.length + 4);
      const view = new DataView(out.buffer);
      view.setUint32(0, text.length, true);
      out.set(text, 4);
      view.setUint32(4 + text.length, secretBytes.length, true);
      out.set(secretBytes, 8 + text.length);
      const b64 = btoa(String.fromCharCode(...out));
      const reply = await chrome.runtime.sendMessage({ type: 'call', frame: b64 });
      if (reply.failed) throw new Error(reply.failed);
      const raw = Uint8Array.from(atob(reply.ok.frame), (c) => c.charCodeAt(0));
      const length = new DataView(raw.buffer).getUint32(0, true);
      return JSON.parse(new TextDecoder().decode(raw.subarray(4, 4 + length)));
    },
    { method, args, secret, online, handle: await page.evaluate(async () => (await chrome.runtime.sendMessage({ type: 'attach' })).ok.handle) },
  );

const made = await call(vault, 'entry.create', { kind: 'login', title: 'Fixture', fields: { username: 'sample@example.com' }, sites: [{ Url: `http://localhost:${port}` }, { Url: `http://127.0.0.1:${port}` }] });
await call(vault, 'entry.set_secret', { id: made.id, field: 'password' }, entryPassword);
await vault.evaluate(async () => {
  // What a page does after a change.
  const { file, handle } = (await chrome.runtime.sendMessage({ type: 'attach' })).ok;
  const text = new TextEncoder().encode(JSON.stringify({ method: 'vault.save', handle, args: {}, now_ms: Date.now() }));
  const out = new Uint8Array(4 + text.length + 8);
  new DataView(out.buffer).setUint32(0, text.length, true);
  out.set(text, 4);
  const reply = await chrome.runtime.sendMessage({ type: 'call', frame: btoa(String.fromCharCode(...out)) });
  const raw = Uint8Array.from(atob(reply.ok.frame), (c) => c.charCodeAt(0));
  const view = new DataView(raw.buffer);
  let at = 4 + view.getUint32(0, true);
  at += 1; // no secret
  at += 4; // one blob
  const length = view.getUint32(at, true);
  const blob = raw.slice(at + 4, at + 4 + length);
  await new Promise((resolve, reject) => {
    const open = indexedDB.open('wmpass', 1);
    open.onerror = () => reject(open.error);
    open.onsuccess = () => {
      const put = open.result.transaction('files', 'readwrite').objectStore('files').put(blob, file);
      put.onsuccess = resolve;
      put.onerror = () => reject(put.error);
    };
  });
});
await vault.reload();
await vault.waitForFunction(() => document.body.innerText.includes('Fixture'), null, { timeout: 15_000 });
check('a tab reattaches to the session after a reload', true);

// Network calls go out from the background, where the host permissions lift CORS.
const inbox = await call(vault, 'inbox.open', { provider: 'inbucket', base: `http://localhost:${port}`, domain: 'example.com', label: 'Fixture' });
const mail = await call(vault, 'inbox.list', { id: inbox.id }, '', true);
check('a network call runs from the extension', Array.isArray(mail) && mail[0]?.subject === 'Hello from the fixture', JSON.stringify(mail).slice(0, 160));
const offline = await call(vault, 'inbox.list', { id: inbox.id }, '', false);
check('with network features off it reaches nothing', offline?.error?.code === 'network', JSON.stringify(offline).slice(0, 160));
await call(vault, 'inbox.close', { id: inbox.id });
if (shots) await vault.screenshot({ path: join(shots, 'vault.png') });

async function menuClick(page, field = '#user', wait = 600) {
  await page.click(field);
  const box = await page.locator(field).boundingBox();
  await sleep(100);
  await page.mouse.click(box.x + box.width - 15, box.y + box.height / 2);
  await sleep(wait);
  return box;
}

const value = (page, selector) => page.locator(selector).inputValue();

const page = await context.newPage();
await page.goto(site('login.html'));
await sleep(1200);
check('nothing fills on load', (await value(page, '#pass')) === '' && (await value(page, '#user')) === '');
check('the page cannot read the menu', await page.evaluate(() => {
  document.querySelector('#user').focus();
  return new Promise((r) => setTimeout(() => r(document.querySelector('wm-pass-menu')?.shadowRoot === null), 200));
}));
const box = await menuClick(page);
if (shots) await page.screenshot({ path: join(shots, 'menu.png') });
await page.mouse.click(box.x + 60, box.y + box.height + 24);
await sleep(400);
if (shots) await page.screenshot({ path: join(shots, 'after-fill.png') });
check('a click on the menu fills the login', (await value(page, '#user')) === 'sample@example.com' && (await value(page, '#pass')) === entryPassword, `${(await value(page, '#user')).length} ${(await value(page, '#pass')).length}`);

await page.goto(site('login.html'));
await page.waitForSelector('#user');
const fast = await menuClick(page, '#user', 30);
await page.mouse.click(fast.x + 60, fast.y + fast.height + 24);
await sleep(300);
check('a click too soon after the menu opens is ignored', (await value(page, '#pass')) === '');

await page.goto(site('jack-opacity.html'));
await page.waitForSelector('#user');
const faded = await menuClick(page);
await page.mouse.click(faded.x + 60, faded.y + faded.height + 24);
await sleep(300);
check('a faded page gets nothing', (await value(page, '#pass')) === '');

await page.goto(site('jack-cover.html'));
await page.waitForSelector('#user');
const covered = await menuClick(page);
await page.mouse.click(covered.x + 60, covered.y + covered.height + 24);
await sleep(300);
check('a covered field gets nothing', (await value(page, '#pass')) === '');

// The form inside another site's frame.
await page.goto(site('framed.html'));
const frame = page.frameLocator('#inner');
await frame.locator('#user').click();
const frameBox = await frame.locator('#user').boundingBox();
await page.mouse.click(frameBox.x + frameBox.width - 15, frameBox.y + frameBox.height / 2);
await sleep(700);
if (shots) await page.screenshot({ path: join(shots, 'foreign-frame.png') });
await page.mouse.click(frameBox.x + 60, frameBox.y + frameBox.height + 70);
await sleep(400);
check('a form in a foreign frame fills only from a deliberate click', (await frame.locator('#pass').inputValue()) === entryPassword);

// A stopped worker comes back with the vault still open.
const session = await context.newCDPSession(page);
const targets = (await session.send('Target.getTargets')).targetInfos.filter((t) => t.type === 'service_worker' && t.url.includes(id));
await session.send('Target.closeTarget', { targetId: targets[0].targetId });
await sleep(500);
await page.goto(site('login.html'));
await page.waitForSelector('#user');
const again = await menuClick(page);
await page.mouse.click(again.x + 60, again.y + again.height + 24);
await sleep(600);
check('the session survives the worker being stopped', (await value(page, '#pass')) === entryPassword);

// Multi-step.
await page.goto(site('steps.html'));
await page.waitForSelector('#user');
const step = await menuClick(page);
await page.mouse.click(step.x + 60, step.y + step.height + 24);
await sleep(400);
check('the first step of a two-step form fills', (await value(page, '#user')) === 'sample@example.com');

// Offers to save.
await page.goto(site('login.html'));
await page.fill('#user', 'new@example.com');
await page.fill('#pass', 'a-new-password');
await page.click('#go');
await page.waitForURL(/done=1/);
await sleep(800);
if (shots) await page.screenshot({ path: join(shots, 'offer.png') });
await page.mouse.click(1100 - 12 - 12 - 24, 12 + 10 + 18 + 8 + 13);
await sleep(800);
const listed = await call(vault, 'entry.list', { sort: 'title' });
check('a new login is offered and saved', listed.some((e) => e.subtitle === 'new@example.com'), `${listed.length} entries`);

await page.goto(site('login.html'));
await page.fill('#user', 'sample@example.com');
await page.fill('#pass', entryPassword);
await page.click('#go');
await page.waitForURL(/done=1/);
await sleep(800);
const offered = await page.evaluate(() => !!document.querySelector('wm-pass-menu'));
check('nothing is offered for the password already saved', !offered);

// Passkeys, checked by the page itself the way a site's server would.
const keyPage = await context.newPage();
await keyPage.goto(`https://localhost:${tls.address().port}/passkey.html`);
const confirmed = async (button, trigger, withPassword = false) => {
  const opening = context.waitForEvent('page');
  await keyPage.click(trigger);
  const box = await opening;
  await box.waitForSelector(`button:has-text("${button}")`);
  if (shots) await box.screenshot({ path: join(shots, `confirm-${button.replace(' ', '-')}.png`) });
  if (withPassword) await box.locator('input[type=password]').fill(testPassword);
  await box.click(`button:has-text("${button}")`);
  await keyPage.waitForFunction(() => document.querySelector('#out').textContent.length > 0, null, { timeout: 20_000 });
  const text = await keyPage.locator('#out').textContent();
  await keyPage.evaluate(() => (document.querySelector('#out').textContent = ''));
  return JSON.parse(text);
};
const created = await confirmed('Save passkey', '#create');
check('a site can make a passkey', created.created === true && created.origin === true && created.type === 'webauthn.create', JSON.stringify(created));
const signedIn = await confirmed('Sign in', '#get');
check('its signature verifies, without claiming verification', signedIn.signed === true && signedIn.challenge === true && signedIn.origin === true && signedIn.rp === true && signedIn.uv === false, JSON.stringify(signedIn));
const verified = await confirmed('Sign in', '#get', true);
check('the password makes it a verified signature', verified.signed === true && verified.uv === true, JSON.stringify(verified));
await keyPage.close();

// The popup as a page, for a look.
const popup = await context.newPage();
await popup.setViewportSize({ width: 380, height: 560 });
await popup.goto(`chrome-extension://${id}/popup.html`);
await sleep(800);
if (shots) await popup.screenshot({ path: join(shots, 'popup-unlocked.png') });
check('the popup opens unlocked', await popup.locator('input').first().isVisible());
await popup.locator('input').first().fill('fixture');
check('the popup searches the whole vault', await popup.locator('.row.chosen', { hasText: 'Fixture' }).waitFor({ timeout: 5000 }).then(() => true, () => false));
await popup.close();

// Lock.
await vault.evaluate(() => chrome.runtime.sendMessage({ type: 'closed' }));
await sleep(400);
const stored = await vault.evaluate(() => chrome.runtime.sendMessage({ type: 'attach' }));
check('locking forgets the session', stored.ok === null);
check('the page follows the lock', await vault.waitForSelector('input[type=password]', { timeout: 5000 }).then(() => true, () => false));
await page.goto(site('login.html'));
await page.waitForSelector('#user');
const closed = await menuClick(page);
if (shots) await page.screenshot({ path: join(shots, 'menu-locked.png') });
await page.mouse.click(closed.x + 60, closed.y + closed.height + 24);
await sleep(300);
check('a locked vault fills nothing', (await value(page, '#pass')) === '');
const lockedPopup = await context.newPage();
await lockedPopup.setViewportSize({ width: 380, height: 560 });
await lockedPopup.goto(`chrome-extension://${id}/popup.html`);
await sleep(500);
if (shots) await lockedPopup.screenshot({ path: join(shots, 'popup-locked.png') });
await lockedPopup.close();

// Security keys and passkey unlock, through WebAuthn's PRF, on Chromium's own test authenticators.
// WebAuthn answers only the page in front, so the site's tab goes.
await page.close();
await vault.locator('input[type=password]').first().fill(testPassword);
await vault.keyboard.press('Enter');
await vault.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 });
const authenticators = await context.newCDPSession(vault);
await authenticators.send('WebAuthn.enable');
const virtual = (transport) => authenticators.send('WebAuthn.addVirtualAuthenticator', {
  options: { protocol: 'ctap2', ctap2Version: 'ctap2_1', transport, hasResidentKey: true, hasUserVerification: true, isUserVerified: true, hasPrf: true, automaticPresenceSimulation: true },
});
await virtual('internal');
await virtual('usb');
await vault.reload();
await vault.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 15_000 });
// Making a credential needs the page's view to have focus, which headless Chromium hands over
// some seconds after a reload. Until then it is refused for that reason alone, so this waits,
// making throwaway credentials on the test key until one is not refused for it.
await vault.bringToFront();
for (let i = 0; i < 40; i++) {
  const refusal = await vault.evaluate(() =>
    navigator.credentials
      .create({ publicKey: { rp: { name: 'Focus' }, user: { id: new Uint8Array(16), name: 'f', displayName: 'f' }, challenge: new Uint8Array(32), pubKeyCredParams: [{ type: 'public-key', alg: -7 }], authenticatorSelection: { authenticatorAttachment: 'cross-platform', userVerification: 'discouraged' } } })
      .then(() => '', (e) => e.message),
  );
  if (!/focus/i.test(refusal)) break;
  await sleep(500);
}
await vault.getByRole('button', { name: 'Settings', exact: true }).click();
await vault.getByRole('button', { name: 'Security', exact: true }).first().click();
const keyPassword = crypto.randomUUID();
await vault.getByRole('button', { name: /^Add a security key/ }).click();
await vault.getByRole('button', { name: /^A FIDO2 security key/ }).click();
check('a browser offers no YubiKey slot of its own', (await vault.getByText('A YubiKey with challenge-response').count()) === 0);
await vault.getByPlaceholder('A name for the key').fill('Test key');
await vault.getByPlaceholder('Password to use with it').fill(keyPassword);
await vault.getByRole('button', { name: 'Next' }).click();
check('a security key is added through the browser', await vault.getByText('The key is added').waitFor({ timeout: 15_000 }).then(() => true, () => false), (await vault.locator('.danger-text').allTextContents()).join(' '));
await vault.getByRole('button', { name: 'Done' }).click();
const passkeySwitch = vault.getByRole('switch', { name: 'Unlock with a passkey' });
check('passkey unlock is offered', await passkeySwitch.waitFor({ timeout: 5000 }).then(() => true, () => false));
await passkeySwitch.click();
check('passkey unlock turns on', await vault.waitForFunction(() => document.querySelector('[role=switch][aria-label="Unlock with a passkey"]')?.getAttribute('aria-checked') === 'true', null, { timeout: 10_000 }).then(() => true, () => false), (await vault.locator('.toast').allTextContents()).join(' '));
await vault.evaluate(() => chrome.runtime.sendMessage({ type: 'closed' }));
await vault.waitForSelector('input[type=password]', { timeout: 5000 });
await vault.getByRole('button', { name: 'Unlock with a passkey' }).click();
check('a passkey opens the vault', await vault.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 15_000 }).then(() => true, () => false));
await vault.evaluate(() => chrome.runtime.sendMessage({ type: 'closed' }));
await vault.waitForSelector('input[type=password]', { timeout: 5000 });
await vault.locator('input[type=password]').first().fill(keyPassword);
await vault.keyboard.press('Enter');
check('the key and its password open the vault', await vault.waitForFunction(() => !document.querySelector('.door-card'), null, { timeout: 60_000 }).then(() => true, () => false), (await vault.locator('.danger-text').allTextContents()).join(' '));
if (shots) await vault.screenshot({ path: join(shots, 'after-key.png') });

// The web vault hands over to the extension: its page asks, and the tab becomes the extension's own.
const webDist = join(here, '../../web/dist');
if (existsSync(webDist)) {
  const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.png': 'image/png' };
  await context.route('https://wmpass.pages.dev/vault/**', (route) => {
    const rest = new URL(route.request().url()).pathname.slice('/vault/'.length);
    const file = join(webDist, !rest || rest.endsWith('/') ? `${rest}index.html` : rest);
    if (!file.startsWith(webDist) || !existsSync(file)) return route.fulfill({ status: 404, body: '' });
    return route.fulfill({ status: 200, headers: { 'content-type': types[extname(file)] ?? 'application/octet-stream' }, body: readFileSync(file) });
  });
  const site = await context.newPage();
  await site.goto('https://wmpass.pages.dev/vault/');
  await site.getByRole('button', { name: 'Open in the extension' }).click({ timeout: 15_000 });
  check('the web vault hands over to the extension', await site.waitForURL(`chrome-extension://${id}/vault.html`, { timeout: 10_000 }).then(() => true, () => false), site.url());
  await site.close();
} else {
  console.log('skip the web vault hand-over: run `npm run web:build` first');
}

check('no script errors', errors.length === 0, errors.slice(0, 3).join(' | ').slice(0, 300));
await context.close();
server.close();
tls.close();
rmSync(certs, { recursive: true, force: true });
rmSync(profile, { recursive: true, force: true });
console.log(failures ? `${failures} failed` : 'all passed');
process.exit(failures ? 1 : 0);
