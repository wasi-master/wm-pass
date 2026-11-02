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
} catch (failure) {
  say('FAILED', failure.message.slice(0, 300));
} finally {
  await context?.close();
  rmSync(profile, { recursive: true, force: true });
}
