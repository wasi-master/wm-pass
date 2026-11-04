// Holds the session for every page of the extension and answers the content scripts. The browser
// can stop this worker at any quiet moment, so what it needs to come back lives in storage.session,
// which stays in memory, is gone when the browser closes, and is out of content scripts' reach.
//
// A message's sender is set by the browser. Content scripts run next to hostile pages, so they get
// only the `cs.` calls here, and those use the sender's address, never one the message claims.

import init, { call, call_async, lock_all, start } from '../../ui/src/wasm/wmpass_wasm.js';
import { bytesOf, decodeReply, encodeRequest } from '../../ui/src/engine/frame';
import { wasmFetch } from '../../ui/src/hosts/fetch';
import { database, request as idb } from '../../ui/src/hosts/web';
import { answerLink, ask, connectLink, forgetLink, hasLink, linkReady, linkState, watchLink } from './bridge';
import { fromB64, toB64, toB64Url, type SiteAnswer, type SiteRow } from './shared';

declare const chrome: any;

const OWN_ORIGIN = chrome.runtime.getURL('').replace(/\/$/, '');
const VAULTS = 'vaults';
const OFFER_TTL_MS = 120_000;

interface Open {
  key: string;
  file: string;
  handle: number;
  name: string;
  read_only: boolean;
}

let open: Open | null = null;

const ready = (async () => {
  await init({ module_or_path: chrome.runtime.getURL('wmpass_wasm_bg.wasm') });
  let { device } = await chrome.storage.local.get('device');
  if (!device) {
    device = crypto.getRandomValues(new Uint32Array(1))[0] || 1;
    await chrome.storage.local.set({ device });
  }
  start(device);
  const saved = (await chrome.storage.session.get('open')).open as Open | undefined;
  if (saved) {
    try {
      const bytes = await readFile(saved.file);
      engine('vault.open_key', { handle: saved.handle }, { secret: fromB64(saved.key), blobs: [bytes], handle: 0 });
      open = saved;
    } catch {
      await chrome.storage.session.remove('open');
    }
  }
  if (await hasLink()) void connectLink();
})();

async function files() {
  return (await database()).transaction('files', 'readwrite').objectStore('files');
}

async function readFile(path: string): Promise<Uint8Array> {
  const value = await idb((await files()).get(path));
  if (!value) throw new Error('That file is gone.');
  return value as Uint8Array;
}

function engine(method: string, args: unknown, options: { secret?: Uint8Array; blobs?: Uint8Array[]; handle?: number } = {}) {
  const frame = encodeRequest({ method, handle: options.handle ?? open?.handle ?? 0, args, now_ms: Date.now() }, options.secret, options.blobs ?? []);
  const reply = call(frame);
  frame.fill(0);
  return decodeReply(reply);
}

/** After the background changed the vault, puts it on disk and tells any open page. */
async function saveAndTell() {
  if (!open) return;
  const reply = engine('vault.save', {});
  await idb((await files()).put(reply.blobs[0], open.file));
  broadcast({ type: 'changed' });
}

function broadcast(message: unknown) {
  chrome.runtime.sendMessage(message).catch(() => {});
}

// ---- locking ----

async function lockEverything() {
  lock_all();
  open = null;
  await chrome.storage.session.remove(['open', 'offers']);
  await chrome.alarms.clear('lock');
  broadcast({ type: 'locked' });
}

let lastPush = 0;
async function active() {
  const now = Date.now();
  if (now - lastPush < 10_000) return;
  lastPush = now;
  const { lockAfter = 300 } = await chrome.storage.local.get('lockAfter');
  if (lockAfter < 0) await chrome.alarms.clear('lock');
  // Chrome will not fire an alarm sooner than thirty seconds.
  else await chrome.alarms.create('lock', { delayInMinutes: Math.max(lockAfter / 60, 0.5) });
}

chrome.alarms.onAlarm.addListener((alarm: any) => {
  void ready.then(async () => {
    if (alarm.name === 'lock') await lockEverything();
    if (alarm.name === 'clip') await wipeClipboard();
  });
});

chrome.idle.onStateChanged.addListener((idle: string) => {
  void ready.then(async () => {
    const { lockOnSleep = true } = await chrome.storage.local.get('lockOnSleep');
    if (idle === 'locked' && lockOnSleep) await lockEverything();
  });
});

// ---- clipboard ----

async function armClipboard(seconds: number) {
  await chrome.alarms.clear('clip');
  if (seconds > 0) await chrome.alarms.create('clip', { delayInMinutes: Math.max(seconds / 60, 0.5) });
}

/** A worker has no clipboard. Chrome lends a hidden page for it; Firefox's background is a page. */
async function wipeClipboard() {
  if (chrome.offscreen) {
    if (!(await chrome.offscreen.hasDocument())) {
      await chrome.offscreen.createDocument({ url: 'offscreen.html', reasons: ['CLIPBOARD'], justification: 'Clears a copied password when its time is up.' });
    }
    await chrome.runtime.sendMessage({ type: 'offscreen.clear' });
    await chrome.offscreen.closeDocument();
    return;
  }
  const box = document.createElement('textarea');
  box.value = ' ';
  document.body.append(box);
  box.select();
  document.execCommand('copy');
  box.remove();
}

// ---- sites ----

function looksForeign(frameUrl: string, tabUrl: string | undefined) {
  if (!tabUrl) return false;
  const frame = new URL(frameUrl).hostname;
  const top = new URL(tabUrl).hostname;
  return !(frame === top || frame.endsWith(`.${top}`) || top.endsWith(`.${frame}`));
}

function rowOf(entry: any): SiteRow {
  return {
    id: entry.id,
    title: entry.title,
    subtitle: entry.subtitle ?? '',
    site: entry.site ?? null,
    icon: entry.icon ?? null,
    has_password: entry.has_password === true,
    has_otp: entry.has_otp === true,
  };
}

async function siteAnswer(url: string, foreign: string | null): Promise<SiteAnswer> {
  if (open) {
    const rows = engine('match.web', { url }).json as any[];
    return { source: 'vault', locked: false, rows: rows.filter((r) => r.entry.kind === 'login').map((r) => rowOf(r.entry)), foreign };
  }
  if (linkReady()) {
    const reply = await ask({ method: 'match', url });
    if (reply.locked || reply.error) return { source: 'desktop', locked: true, rows: [], foreign };
    return { source: 'desktop', locked: false, rows: reply.entries.map((r: any) => rowOf(r.entry)), foreign };
  }
  return { source: 'none', locked: true, rows: [], foreign };
}

async function fillFor(url: string, id: string) {
  if (open) {
    await active();
    const reply = engine('match.fill', { id, url });
    const text = new TextDecoder().decode(reply.secret!);
    reply.secret!.fill(0);
    return JSON.parse(text);
  }
  if (linkReady()) {
    const reply = await ask({ method: 'fill', url, entry: id });
    if (reply.error) throw new Error(reply.error);
    return reply.fill;
  }
  throw new Error('WM Pass is locked.');
}

// ---- offers to save ----

interface Offer {
  url: string;
  username: string;
  password: string;
  kind: 'new' | 'update';
  id: string | null;
  at: number;
}

async function offerFor(tab: number) {
  const { offers = {} } = await chrome.storage.session.get('offers');
  const offer = offers[tab] as Offer | undefined;
  if (!offer || Date.now() - offer.at > OFFER_TTL_MS) return null;
  return offer;
}

async function setOffer(tab: number, offer: Offer | null) {
  const { offers = {} } = await chrome.storage.session.get('offers');
  if (offer) offers[tab] = offer;
  else delete offers[tab];
  await chrome.storage.session.set({ offers });
}

async function considerOffer(tab: number, url: string, username: string, password: string) {
  if (!open || !password) return;
  const site = new URL(url).hostname;
  const { offerSave = true, neverSave = [] } = await chrome.storage.local.get(['offerSave', 'neverSave']);
  if (!offerSave || neverSave.includes(site)) return;
  const rows = engine('match.web', { url }).json as any[];
  const same = rows.map((r) => r.entry).find((e) => e.kind === 'login' && e.subtitle === username);
  if (same) {
    const reply = engine('entry.reveal', { id: same.id, field: 'password' });
    const stored = new TextDecoder().decode(reply.secret!);
    reply.secret!.fill(0);
    if (stored === password) return;
    await setOffer(tab, { url, username, password, kind: 'update', id: same.id, at: Date.now() });
    return;
  }
  await setOffer(tab, { url, username, password, kind: 'new', id: null, at: Date.now() });
}

async function acceptOffer(offer: Offer) {
  const origin = new URL(offer.url).origin;
  let id = offer.id;
  if (!id) {
    const title = new URL(offer.url).hostname.replace(/^www\./, '');
    id = engine('entry.create', { kind: 'login', title, fields: { username: offer.username }, sites: [{ Url: origin }] }).json.id as string;
  }
  const bytes = bytesOf(offer.password);
  engine('entry.set_secret', { id, field: 'password' }, { secret: bytes });
  bytes.fill(0);
  await saveAndTell();
}

// ---- passkeys ----

interface Pending {
  op: 'get' | 'create';
  site: string;
  rp: string;
  user: string;
  choices: { id: string; title: string; credential_id: string; user_name: string }[];
  finish: (answer: { choice: number; verified: boolean } | null) => void;
}

const asking = new Map<string, Pending>();

function askInWindow(pending: Omit<Pending, 'finish'>): Promise<{ choice: number; verified: boolean } | null> {
  return new Promise((resolve) => {
    const id = crypto.randomUUID();
    // The worker must not be stopped while a person is deciding.
    const keepAwake = setInterval(() => chrome.runtime.getPlatformInfo(() => {}), 20_000);
    let windowId: number | undefined;
    const finish = (answer: { choice: number; verified: boolean } | null) => {
      clearInterval(keepAwake);
      chrome.windows.onRemoved.removeListener(gone);
      asking.delete(id);
      resolve(answer);
    };
    const gone = (closed: number) => {
      if (closed === windowId) finish(null);
    };
    asking.set(id, { ...pending, finish });
    chrome.windows.onRemoved.addListener(gone);
    chrome.windows.create({ url: `confirm.html?id=${id}`, type: 'popup', width: 400, height: 500 }).then((w: any) => {
      windowId = w.id;
    });
  });
}

async function passkey(sender: any, op: 'get' | 'create', options: any) {
  if (!open) return { fallback: true };
  const url = new URL(sender.url);
  const origin = url.origin;
  const host = url.hostname;
  if (url.protocol !== 'https:') return { fallback: true };

  if (op === 'get') {
    const rpId = options.rpId ?? host;
    let list: any[];
    try {
      list = engine('passkey.list', { rp_id: rpId, origin_host: host }).json;
    } catch {
      return { fallback: true };
    }
    const allowed: string[] = (options.allowCredentials ?? []).map((c: any) => c.id);
    if (allowed.length) list = list.filter((k) => allowed.includes(k.credential_id));
    if (!list.length) return { fallback: true };
    const answer = await askInWindow({ op, site: host, rp: rpId, user: '', choices: list });
    if (!answer || !open) return { cancelled: true };
    const chosen = list[answer.choice];
    const clientData = bytesOf(JSON.stringify({ type: 'webauthn.get', challenge: options.challenge, origin, crossOrigin: false }));
    const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', clientData as BufferSource));
    const signed = engine('passkey.sign', {
      id: chosen.id,
      credential_id: chosen.credential_id,
      client_data_hash: toB64Url(hash),
      user_verified: answer.verified,
      origin_host: host,
    }).json;
    await active();
    return { clientData: toB64Url(clientData), ...signed };
  }

  const rpId = options.rp?.id ?? host;
  if (!(options.pubKeyCredParams ?? []).some((p: any) => p.alg === -7)) return { fallback: true };
  const answer = await askInWindow({ op, site: host, rp: rpId, user: options.user?.name ?? '', choices: [] });
  if (!answer || !open) return { cancelled: true };
  const made = engine('passkey.create', {
    rp_id: rpId,
    rp_name: options.rp?.name ?? rpId,
    user_handle: options.user?.id,
    user_name: options.user?.name ?? '',
    user_verified: answer.verified,
    origin_host: host,
  }).json;
  await saveAndTell();
  const clientData = bytesOf(JSON.stringify({ type: 'webauthn.create', challenge: options.challenge, origin, crossOrigin: false }));
  return { clientData: toB64Url(clientData), ...made };
}

// ---- places through the desktop app ----

// File bytes cross the link in parts: the relay may hand the browser no message over 1 MiB.
const LINK_PART = 384 * 1024;

async function linkPlace(message: any) {
  const asked = async (request: Record<string, unknown>) => {
    const reply = await ask(request);
    if (reply.error) throw new Error(reply.error === 'refused' ? 'The desktop app was told no.' : reply.error);
    return reply;
  };
  let token = 0;
  if (message.op === 'write') {
    const bytes = fromB64(message.data ?? '');
    for (let at = 0; at < bytes.length || token === 0; at += LINK_PART) {
      token = (await asked({ method: 'places.put', token, part: toB64(bytes.subarray(at, at + LINK_PART)) })).token;
    }
  }
  const reply = await asked({ method: 'places.run', op: message.op, place: message.place, name: message.name ?? '', file: message.file ?? null, token });
  let value = reply.value;
  if (message.op === 'read') {
    const parts: Uint8Array[] = [];
    let length = 0;
    for (let done = false; !done; ) {
      const got = await asked({ method: 'places.get', token: value.token, offset: length });
      const part = fromB64(got.part);
      parts.push(part);
      length += part.length;
      done = got.done;
    }
    const whole = new Uint8Array(length);
    let at = 0;
    for (const part of parts) {
      whole.set(part, at);
      at += part.length;
    }
    value = toB64(whole);
  }
  return { value, update: reply.update ?? {} };
}

/** One engine call on the desktop app's vault. Large frames, such as a long note, cross in parts. */
async function linkVaultCall(frameB64: string): Promise<{ frame?: string; locked?: boolean }> {
  if (!linkReady()) return { locked: true };
  const asked = async (request: Record<string, unknown>) => {
    const reply = await ask(request);
    if (reply.error) throw new Error(reply.error === 'refused' ? 'The desktop app was told no.' : reply.error);
    return reply;
  };
  let reply;
  const frame = fromB64(frameB64);
  if (frame.length <= LINK_PART) {
    reply = await asked({ method: 'vault.call', frame: frameB64 });
  } else {
    let token = 0;
    for (let at = 0; at < frame.length; at += LINK_PART) {
      token = (await asked({ method: 'places.put', token, part: toB64(frame.subarray(at, at + LINK_PART)) })).token;
    }
    reply = await asked({ method: 'vault.call', token });
  }
  frame.fill(0);
  if (reply.locked) return { locked: true };
  if (reply.frame) return { frame: reply.frame };
  const parts: Uint8Array[] = [];
  let length = 0;
  for (let done = false; !done; ) {
    const got = await asked({ method: 'places.get', token: reply.token, offset: length });
    const part = fromB64(got.part);
    parts.push(part);
    length += part.length;
    done = got.done;
  }
  const whole = new Uint8Array(length);
  let at = 0;
  for (const part of parts) {
    whole.set(part, at);
    part.fill(0);
    at += part.length;
  }
  const out = toB64(whole);
  whole.fill(0);
  return { frame: out };
}

// ---- messages ----

async function fromContent(message: any, sender: any) {
  const tab = sender.tab?.id;
  switch (message.type) {
    case 'cs.site': {
      const foreign = sender.frameId && looksForeign(sender.url, sender.tab?.url) ? new URL(sender.url).hostname : null;
      return siteAnswer(sender.url, foreign);
    }
    case 'cs.fill':
      return { fill: await fillFor(sender.url, message.id) };
    case 'cs.submitted':
      if (tab !== undefined) await considerOffer(tab, sender.url, String(message.username ?? ''), String(message.password ?? ''));
      return {};
    case 'cs.offer': {
      if (sender.frameId || tab === undefined) return null;
      const offer = await offerFor(tab);
      if (!offer || new URL(offer.url).hostname !== new URL(sender.url).hostname) return null;
      return { kind: offer.kind, username: offer.username };
    }
    case 'cs.offer.answer': {
      if (sender.frameId || tab === undefined) return {};
      const offer = await offerFor(tab);
      await setOffer(tab, null);
      if (offer && message.answer === 'save') await acceptOffer(offer);
      if (offer && message.answer === 'never') {
        const { neverSave = [] } = await chrome.storage.local.get('neverSave');
        await chrome.storage.local.set({ neverSave: [...neverSave, new URL(offer.url).hostname] });
      }
      return {};
    }
    case 'cs.passkey':
      return passkey(sender, message.op, message.options);
    case 'cs.open':
      await chrome.tabs.create({ url: 'vault.html' });
      return {};
    default:
      throw new Error('Not allowed.');
  }
}

async function fromPage(message: any) {
  switch (message.type) {
    case 'call': {
      await active();
      // Calls that reach a network go out from here, where the extension's host permissions
      // apply. The page only ever sees their answers.
      return { frame: toB64(await call_async(fromB64(message.frame), wasmFetch)) };
    }
    case 'attach':
      return open ? { handle: open.handle, file: open.file, name: open.name, read_only: open.read_only } : null;
    case 'opened': {
      // Only the vault in this folder comes back after a stop. A nested one locks with the worker.
      const reply = engine('vault.key_for_wrapping', {}, { handle: message.handle });
      open = { key: toB64(reply.secret!), file: message.file, handle: message.handle, name: message.name, read_only: message.read_only };
      reply.secret!.fill(0);
      await chrome.storage.session.set({ open });
      await active();
      return {};
    }
    case 'closed':
      await lockEverything();
      return {};
    case 'guard.rule':
      await chrome.storage.local.set({ lockAfter: message.after, lockOnSleep: message.onSleep });
      lastPush = 0;
      if (open) await active();
      return {};
    case 'guard.active':
      await active();
      return {};
    case 'clip.arm':
      await armClipboard(message.seconds);
      return {};
    case 'site': {
      return siteAnswer(message.url, null);
    }
    case 'fill.tab': {
      const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
      const fill = await fillFor(tab.url, message.id);
      await chrome.tabs.sendMessage(tab.id, { type: 'fill', fill }, { frameId: 0 });
      return {};
    }
    case 'link.state':
      return linkState();
    case 'link.connect':
      await connectLink();
      return linkState();
    case 'link.answer':
      answerLink(message.match);
      return linkState();
    case 'link.forget':
      await forgetLink();
      return linkState();
    case 'usernames': {
      // Checks run in the desktop app, which reaches the sites and holds the network switch.
      if (!['site_list', 'check'].includes(message.what)) throw new Error('Unknown request.');
      const reply = await ask({ method: `usernames.${message.what}`, site: message.site ?? '', name: message.name ?? '' });
      if (reply.error) throw new Error(reply.error);
      return reply;
    }
    // Backup places and security keys the browser cannot reach, for this extension's own vault,
    // through the desktop app. It asks its person once for each place, and once for the keys.
    case 'link.places.kinds': {
      if (!linkReady()) return [];
      const reply = await ask({ method: 'places.kinds' });
      return reply.error ? [] : reply.kinds;
    }
    case 'link.places':
      return linkPlace(message);
    case 'link.places.sign_in': {
      const reply = await ask({ method: 'places.sign_in', place: message.place, step: message.step ?? {} });
      if (reply.error) throw new Error(reply.error);
      return reply.answer;
    }
    case 'link.hardware': {
      const reply = await ask({ method: 'hardware.tap', kind: message.kind, factor: message.factor ?? null });
      if (reply.error) throw new Error(reply.error === 'refused' ? 'The desktop app was told no.' : reply.error);
      return { factor: reply.factor, output: reply.output };
    }
    // The vault open in the desktop app, shown in this extension's own screens. The app asks its
    // person once, and refuses anything that reaches its file, its keys or a whole copy.
    case 'link.vault.attach': {
      if (!linkReady()) return { locked: true };
      const reply = await ask({ method: 'vault.attach' });
      if (reply.error) throw new Error(reply.error === 'refused' ? 'The desktop app was told no.' : reply.error);
      return reply.locked ? { locked: true } : { handle: reply.handle, name: reply.name };
    }
    case 'link.vault.call':
      return linkVaultCall(message.frame);
    case 'link.lock':
      if (linkReady()) await ask({ method: 'lock' });
      return {};
    case 'link.fill.settings':
      return chrome.storage.local.get(['offerSave', 'passkeys']);
    case 'confirm.get': {
      const pending = asking.get(message.id);
      return pending ? { op: pending.op, site: pending.site, rp: pending.rp, user: pending.user, choices: pending.choices } : null;
    }
    case 'confirm.answer': {
      const pending = asking.get(message.id);
      if (!pending) return {};
      if (message.choice < 0) {
        pending.finish(null);
        return {};
      }
      let verified = false;
      if (message.password) {
        const bytes = fromB64(message.password);
        try {
          verified = engine('vault.verify', { factors: [] }, { secret: bytes }).json.ok === true;
        } finally {
          bytes.fill(0);
        }
        if (!verified) return { wrong: true };
      }
      pending.finish({ choice: message.choice, verified });
      return {};
    }
    default:
      throw new Error('Unknown request.');
  }
}

chrome.runtime.onMessage.addListener((message: any, sender: any, respond: (r: unknown) => void) => {
  if (sender.id !== chrome.runtime.id || typeof message?.type !== 'string') return false;
  if (message.type.startsWith('offscreen.') || message.type === 'locked' || message.type === 'changed' || message.type === 'link.changed') return false;
  const fromOurPage = typeof sender.url === 'string' && sender.url.startsWith(OWN_ORIGIN);
  const isContent = !fromOurPage && sender.tab;
  const run = isContent
    ? message.type.startsWith('cs.')
      ? fromContent(message, sender)
      : Promise.reject(new Error('Not allowed.'))
    : fromOurPage
      ? fromPage(message)
      : Promise.reject(new Error('Not allowed.'));
  ready
    .then(() => run)
    .then(
      (value) => respond({ ok: value ?? null }),
      (failure) => respond({ failed: failure?.message ?? 'That did not work.', code: failure?.code }),
    );
  return true;
});

// The web vault at wmpass.pages.dev asks whether the extension is here, and may ask it to open its
// own vault page in that tab. It learns nothing else: not whether a vault exists, nor its state.
const WEB_VAULT = 'https://wmpass.pages.dev';
chrome.runtime.onMessageExternal?.addListener((message: any, sender: any, respond: (r: unknown) => void) => {
  if (typeof sender.url !== 'string' || new URL(sender.url).origin !== WEB_VAULT || sender.tab?.id === undefined) return false;
  if (message?.type === 'web.hello') respond({ ok: true });
  else if (message?.type === 'web.open') {
    void chrome.tabs.update(sender.tab.id, { url: chrome.runtime.getURL('vault.html') });
    respond({ ok: true });
  } else respond({ failed: 'Unknown request.' });
  return false;
});

watchLink(() => broadcast({ type: 'link.changed' }));
chrome.idle.setDetectionInterval?.(60);
