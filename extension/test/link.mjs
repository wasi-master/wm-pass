// The extension, the relay program and a running debug build of the app, all real. Run the app's
// self-test build first and pass its run folder, then this fills a login from the app's vault.
//   WMPASS_RUN_DIR=<folder> node extension/test/link.mjs <relay program> <page address>
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const here = dirname(fileURLToPath(import.meta.url));
const extension = join(here, '../dist/chrome');
const [relay, address] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const say = (name, value) => console.log(`${name}: ${typeof value === 'string' ? value : JSON.stringify(value)}`);

const ID = 'blpbdcnicidapbjjopepnaofggclabom';
const profile = mkdtempSync(join(tmpdir(), 'wmpass-link-'));
// A browser looks for relay programs in its own data folder, so this stays in the throwaway profile.
const manifest = JSON.stringify({ name: 'com.wasimaster.wmpass', description: 'test', path: relay, type: 'stdio', allowed_origins: [`chrome-extension://${ID}/`] });
mkdirSync(join(profile, 'NativeMessagingHosts'), { recursive: true });
writeFileSync(join(profile, 'NativeMessagingHosts/com.wasimaster.wmpass.json'), manifest);
let context;
try {
  context = await chromium.launchPersistentContext(profile, {
    channel: 'chromium',
    headless: true,
    env: { ...process.env },
    viewport: { width: 1100, height: 700 },
    args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`],
  });
  const host = new URL(address).host;
  await context.route(`https://${host}/**`, (route) =>
    route.fulfill({
      contentType: 'text/html',
      body: '<!doctype html><form><input id="user" name="email" type="email" style="width:280px;height:32px;margin:40px"><input id="pass" type="password" style="width:280px;height:32px;margin:40px"></form>',
    }),
  );
  const worker = context.serviceWorkers()[0] ?? (await context.waitForEvent('serviceworker'));
  say('extension', new URL(worker.url()).host);

  const page = await context.newPage();
  await page.goto(`chrome-extension://${ID}/vault.html`);
  await sleep(800);
  const send = (message) => page.evaluate((m) => chrome.runtime.sendMessage(m), message);
  say('state before', (await send({ type: 'link.state' })).ok.status);
  await send({ type: 'link.connect' });
  let state;
  for (let i = 0; i < 40; i++) {
    state = (await send({ type: 'link.state' })).ok;
    if (state.status === 'compare' || state.status === 'failed' || state.status === 'absent') break;
    await sleep(150);
  }
  say('after connect', { status: state.status, code: state.code?.length ?? 0, problem: state.problem });
  await send({ type: 'link.answer', match: true });
  for (let i = 0; i < 40; i++) {
    state = (await send({ type: 'link.state' })).ok;
    if (state.status === 'ready' || state.status === 'failed') break;
    await sleep(150);
  }
  say('after answer', { status: state.status, unlocked: state.unlocked });

  // What only the app can reach, offered to the extension's own vault while connected.
  say('places through the app', (await send({ type: 'link.places.kinds' })).ok);

  const site = (await send({ type: 'site', url: address })).ok;
  say('site', { source: site.source, locked: site.locked, rows: site.rows.length });

  const tab = await context.newPage();
  await tab.goto(address);
  await tab.waitForSelector('#user');
  await tab.click('#user');
  const box = await tab.locator('#user').boundingBox();
  await sleep(100);
  await tab.mouse.click(box.x + box.width - 15, box.y + box.height / 2);
  await sleep(700);
  await tab.mouse.click(box.x + 60, box.y + box.height + 24);
  await sleep(700);
  say('filled', { username: (await tab.locator('#user').inputValue()).length, password: (await tab.locator('#pass').inputValue()).length });

  // The app's vault in the extension's own screens: calls go over the link, the app saves.
  const attached = (await send({ type: 'link.vault.attach' })).ok;
  say('vault attached', { handle: attached?.handle > 0, name: attached?.name });
  const call = (method, args = {}, secret = '', handle = attached?.handle ?? 0) =>
    page.evaluate(
      async ({ method, handle, args, secret }) => {
        const encoder = new TextEncoder();
        const head = encoder.encode(JSON.stringify({ method, handle, args, now_ms: Date.now(), online: false }));
        const sealed = encoder.encode(secret);
        const frame = new Uint8Array(4 + head.length + 4 + sealed.length + 4);
        const view = new DataView(frame.buffer);
        view.setUint32(0, head.length, true);
        frame.set(head, 4);
        view.setUint32(4 + head.length, sealed.length, true);
        frame.set(sealed, 8 + head.length);
        const reply = await chrome.runtime.sendMessage({ type: 'link.vault.call', frame: btoa(String.fromCharCode(...frame)) });
        if (!reply.ok?.frame) return reply;
        const bytes = Uint8Array.from(atob(reply.ok.frame), (c) => c.charCodeAt(0));
        const length = new DataView(bytes.buffer).getUint32(0, true);
        return JSON.parse(new TextDecoder().decode(bytes.subarray(4, 4 + length)));
      },
      { method, handle, args, secret },
    );
  const before = await call('entry.list', { sort: 'title' });
  say('listed over the link', before.map?.((e) => e.title) ?? before);
  const made = await call('entry.create', { kind: 'login', title: 'Made in the browser', fields: { username: 'browser@example.com' } });
  say('created over the link', !!made.id);
  const after = await call('entry.list', { sort: 'title' });
  say('count after create', `${before.length} -> ${after.length}`);
  say('key stays in the app', await call('vault.export_key', {}, 'anything'));
  say('a stale handle answers locked', await call('entry.list', { sort: 'title' }, '', (attached?.handle ?? 0) + 1000));

  // A fresh vault tab opens the app's vault by itself.
  const screen = await context.newPage();
  await screen.goto(`chrome-extension://${ID}/vault.html`);
  await sleep(2500);
  const text = await screen.evaluate(() => document.body.innerText);
  say('vault tab shows the app vault', { github: text.includes('GitHub'), made: text.includes('Made in the browser'), door: text.includes('Enter your password') || text.includes('Create your vault') });
  if (process.env.WMPASS_SHOTS) await screen.screenshot({ path: join(process.env.WMPASS_SHOTS, 'linked-vault.png') });
} catch (failure) {
  say('FAILED', failure.message.slice(0, 300));
} finally {
  await context?.close();
  rmSync(profile, { recursive: true, force: true });
}
