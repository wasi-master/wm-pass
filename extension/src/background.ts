// Holds the session for every page of the extension and answers the content scripts. The browser
// can stop this worker at any quiet moment, so what it needs to come back lives in storage.session,
// which stays in memory, is gone when the browser closes, and is out of content scripts' reach.
//
// A message's sender is set by the browser. Content scripts run next to hostile pages, so they get
// only the `cs.` calls here, and those use the sender's address, never one the message claims.

import init, { call, call_async, lock_all, start } from '../../ui/src/wasm/wmpass_wasm.js';
import { bytesOf, decodeReply, encodeRequest, EngineError } from '../../ui/src/engine/frame';
import { internalFrame, isInternal, stoppedError } from '../../ui/src/engine/internal';
import { wasmFetch } from '../../ui/src/hosts/fetch';
import { committed, database, request as idb } from '../../ui/src/hosts/web';
import { answerLink, ask, connectLink, forgetLink, hasLink, linkReady, linkSettled, linkState, watchLink } from './bridge';
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

// ---- an internal error ----
//
// A panic in the session traps, as stable wasm32 cannot unwind, and the module cannot be used
// again. A call that traps at once throws a `RuntimeError`; one that traps after its first wait,
// inside the module's own queue of tasks, never answers, and shows only as an error here. Either
// way the module's whole memory, every key it held among it, is filled with zeros, the vault is
// noted as locked, and the pages are told. Nothing reaches the module after that; a page's Go to
// unlock starts the extension afresh ('restart'), and so does the browser stopping this worker.

let memory: WebAssembly.Memory | null = null;
let dead = false;
const waiting = new Set<() => void>();

function stopForGood() {
  if (dead) return;
  dead = true;
  if (memory) new Uint8Array(memory.buffer).fill(0);
  for (const answer of [...waiting]) answer();
  waiting.clear();
  open = null;
  void chrome.storage.session.remove(['open', 'offers', 'typed']).catch(() => {});
  void chrome.alarms.clear('lock').catch(() => {});
  for (const pending of [...asking.values()]) pending.withdraw();
  broadcast({ type: 'internal' });
}

const trapped = (failure: unknown) => {
  if (failure instanceof WebAssembly.RuntimeError) stopForGood();
};
self.addEventListener('error', (event: any) => trapped(event.error));
self.addEventListener('unhandledrejection', (event: any) => trapped(event.reason));

/** Whether a reply frame is an internal error, read from its head alone. */
function saysInternal(frame: Uint8Array) {
  try {
    const length = new DataView(frame.buffer, frame.byteOffset, frame.byteLength).getUint32(0, true);
    const code = JSON.parse(new TextDecoder().decode(frame.subarray(4, 4 + length)))?.error?.code;
    return isInternal(new EngineError(String(code), ''));
  } catch {
    return false;
  }
}

/** A page's call, answered with an internal error once the module has trapped. */
function pageCall(frame: Uint8Array): Promise<Uint8Array> {
  if (dead) return Promise.resolve(internalFrame());
  return new Promise((resolve, reject) => {
    const answer = () => resolve(internalFrame());
    waiting.add(answer);
    const failed = (failure: unknown) => {
      waiting.delete(answer);
      if (failure instanceof WebAssembly.RuntimeError) {
        stopForGood();
        resolve(internalFrame());
      } else reject(failure);
    };
    try {
      call_async(frame, wasmFetch).then((reply) => {
        waiting.delete(answer);
        resolve(reply);
      }, failed);
    } catch (failure) {
      failed(failure);
    }
  });
}

const ready = (async () => {
  memory = (await init({ module_or_path: chrome.runtime.getURL('wmpass_wasm_bg.wasm') })).memory;
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
  // A pairing that no longer opens costs the link, not every answer this worker gives.
  if (await hasLink().catch(() => false)) void connectLink();
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
  if (dead) throw stoppedError();
  const frame = encodeRequest({ method, handle: options.handle ?? open?.handle ?? 0, args, now_ms: Date.now() }, options.secret, options.blobs ?? []);
  try {
    return decodeReply(call(frame));
  } catch (failure) {
    if (!isInternal(failure)) throw failure;
    stopForGood();
    throw stoppedError();
  } finally {
    frame.fill(0);
  }
}

/**
 * Whether the open vault can be saved from here: a vault file can, while an account or a nested
 * vault is kept by the page that opened it, and only while that page is open.
 */
async function canSave() {
  if (!open) return false;
  if (open.file) return true;
  return (await chrome.runtime.sendMessage({ type: 'save.holder' }).catch(() => false)) === true;
}

const NO_WRITER = 'Open the WM Pass page this vault was opened in, then try again. Only it can save to this vault.';

/** After the background changed the vault, puts it on disk and tells any open page. */
async function saveAndTell() {
  if (!open) return;
  if (!open.file) {
    // The page that keeps it saves, and its lists reload with it.
    const held = await chrome.runtime.sendMessage({ type: 'save.request' }).catch(() => false);
    if (held !== true) throw new Error(NO_WRITER);
    return;
  }
  const reply = engine('vault.save', {});
  // Done once it is on disk: a full disk shows only as the write commits, and must not pass as saved.
  const store = await files();
  store.put(reply.blobs[0], open.file);
  await committed(store.transaction);
  // The extension's pages hear of it here, and reload; they keep no file sizes that this write
  // could leave wrong (see webHost's `shared`, off in the extension).
  broadcast({ type: 'changed' });
}

function broadcast(message: unknown) {
  chrome.runtime.sendMessage(message).catch(() => {});
}

// ---- locking ----

/** The last change to the sign-ins kept per tab in session storage, which go one at a time. */
let sessionWritten: Promise<unknown> = Promise.resolve();

async function lockEverything() {
  if (!dead) lock_all();
  open = null;
  // A passkey window still up would ask for a master password that now opens nothing.
  for (const pending of [...asking.values()]) pending.withdraw();
  await chrome.storage.session.remove('open');
  // After any write already on its way, which would otherwise put a sign-in back.
  await (sessionWritten = sessionWritten.then(() => chrome.storage.session.remove(['offers', 'typed'])).catch(() => {}));
  await chrome.alarms.clear('lock');
  // The next unlock sets the timer afresh, even within moments of the last activity before this
  // lock, or the vault opened then would have no timer at all until some later activity.
  lastPush = 0;
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
    if (alarm.name === 'clip') {
      if (clipTimer) clearTimeout(clipTimer);
      clipTimer = null;
      await wipeClipboard();
    }
  });
});

// Firefox for Android has no idle API: without this check the whole worker would stop here.
chrome.idle?.onStateChanged.addListener((idle: string) => {
  void ready.then(async () => {
    const { lockOnSleep = true } = await chrome.storage.local.get('lockOnSleep');
    if (idle === 'locked' && lockOnSleep) await lockEverything();
  });
});

// ---- clipboard ----

let clipTimer: ReturnType<typeof setTimeout> | null = null;

async function armClipboard(seconds: number) {
  await chrome.alarms.clear('clip');
  if (clipTimer) clearTimeout(clipTimer);
  clipTimer = null;
  if (seconds <= 0) return;
  // Chrome fires no alarm sooner than thirty seconds, so a shorter wait runs on a timer here. The
  // alarm stays as the fallback for a worker stopped before the timer fires; whichever comes
  // first clears the other, so nothing copied after is wiped by the second.
  await chrome.alarms.create('clip', { delayInMinutes: Math.max(seconds / 60, 0.5) });
  if (seconds < 30) {
    clipTimer = setTimeout(() => {
      clipTimer = null;
      void chrome.alarms.clear('clip').then(() => wipeClipboard());
    }, seconds * 1000);
  }
}

/**
 * Clears the clipboard, one clearing at a time: two at once would both make the hidden page
 * Chrome allows only one of, and the second would fail. A failure is let go, as nothing waits
 * on it but a timer.
 */
let wiping: Promise<void> = Promise.resolve();
function wipeClipboard(): Promise<void> {
  wiping = wiping.then(clearNow).catch(() => {});
  return wiping;
}

/** A worker has no clipboard. Chrome lends a hidden page for it; Firefox's background is a page. */
async function clearNow() {
  if (chrome.offscreen) {
    if (!(await chrome.offscreen.hasDocument())) {
      await chrome.offscreen.createDocument({ url: 'offscreen.html', reasons: ['CLIPBOARD'], justification: 'Clears a copied password when its time is up.' });
    }
    await chrome.runtime.sendMessage({ type: 'offscreen.clear' });
    await chrome.offscreen.closeDocument();
    return;
  }
  // As the hidden page does: the copy's text is replaced with nothing, not a space to paste.
  const replace = (event: ClipboardEvent) => {
    event.clipboardData?.setData('text/plain', '');
    event.preventDefault();
  };
  document.addEventListener('copy', replace, { once: true });
  const box = document.createElement('textarea');
  box.value = ' ';
  document.body.append(box);
  box.select();
  document.execCommand('copy');
  box.remove();
  document.removeEventListener('copy', replace);
}

// ---- sites ----

function looksForeign(frameUrl: string, tabUrl: string | undefined) {
  if (!tabUrl) return false;
  const frame = new URL(frameUrl).hostname;
  const top = new URL(tabUrl).hostname;
  // Two names of one site, such as login.example.com framed in www.example.com, are no warning.
  return !(frame === top || frame.endsWith(`.${top}`) || top.endsWith(`.${frame}`) || sameSite(frame, top));
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
  await linkSettled();
  if (linkReady()) {
    const reply = await ask({ method: 'match', url });
    if (reply.locked || reply.error) return { source: 'desktop', locked: true, rows: [], foreign };
    return { source: 'desktop', locked: false, rows: (reply.entries ?? []).map((r: any) => rowOf(r.entry)), foreign };
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
  await linkSettled();
  if (linkReady()) {
    const reply = await ask({ method: 'fill', url, entry: id });
    if (reply.error) throw new Error(reply.error);
    if (reply.locked || !reply.fill) throw new Error('The WM Pass app is locked. Unlock it there and try again.');
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
  /** The tab's own page when the form was in a frame of it, such as a sign-in box from another host. */
  top?: string;
  /** Put to the person once already, so the pages after it do not ask again. */
  shown?: boolean;
}

/** One site by the public suffix list, so accounts.example.com hands over to www.example.com. */
function sameSite(a: string, b: string) {
  if (a === b) return true;
  if (!open) return false;
  try {
    return engine('match.same_site', { a, b }).json.same === true;
  } catch {
    return false;
  }
}

/** Sign-ins still being weighed for an offer, per tab. */
const considering = new Map<number, Promise<void>>();

/**
 * Asks the tab's top page to look for the offer again. A single-page app moves on without loading
 * a page, and a form in a frame has no page load of its own on top, so neither would ask by itself.
 */
function nudgeOffer(tab: number) {
  for (const after of [1500, 4000]) {
    setTimeout(() => void chrome.tabs.sendMessage(tab, { type: 'offer.check' }, { frameId: 0 }).catch(() => {}), after);
  }
}

async function offerFor(tab: number) {
  const { offers = {} } = await chrome.storage.session.get('offers');
  const offer = offers[tab] as Offer | undefined;
  if (!offer || Date.now() - offer.at > OFFER_TTL_MS) return null;
  return offer;
}

// One change at a time: two tabs signing in together would otherwise each write over the other.
function setPerTab(key: 'offers' | 'typed', tab: number, value: unknown): Promise<void> {
  const write = sessionWritten.then(async () => {
    // A lock since this was asked for forgot every sign-in, and this one goes with them.
    if (value && !open) return;
    const { [key]: all = {} } = await chrome.storage.session.get(key);
    if (value) all[tab] = value;
    else delete all[tab];
    await chrome.storage.session.set({ [key]: all });
  });
  sessionWritten = write.catch(() => {});
  return write;
}

const setOffer = (tab: number, offer: Offer | null) => setPerTab('offers', tab, offer);

/**
 * The username sent on the first step of a two-step sign-in, for the password step after it,
 * which has none of its own. Only for the tab and the address it was typed at, for a few minutes.
 * Kept in session storage beside the offers, in memory and out of content scripts' reach: the
 * worker often stops while the password is being typed, and would forget it.
 */
const TYPED_TTL_MS = 180_000;
type Typed = { origin: string; username: string; at: number };

async function typedFor(tab: number, url: string) {
  const { typed = {} } = await chrome.storage.session.get('typed');
  const got = typed[tab] as Typed | undefined;
  if (!got || Date.now() - got.at > TYPED_TTL_MS || got.origin !== new URL(url).origin) return '';
  return got.username;
}

// A closed tab's sign-in is no use to anyone, and its number may come back for another tab.
chrome.tabs.onRemoved.addListener((tab: number) => {
  void setPerTab('typed', tab, null).catch(() => {});
  void setOffer(tab, null).catch(() => {});
});

/** The password saved in a login, or an empty one where it has none. */
function storedPassword(id: string): string {
  try {
    const reply = engine('entry.reveal', { id, field: 'password' });
    const stored = new TextDecoder().decode(reply.secret ?? new Uint8Array());
    reply.secret?.fill(0);
    return stored;
  } catch {
    return '';
  }
}

async function considerOffer(tab: number, url: string, username: string, password: string, top?: string, borrowed = false) {
  // A vault opened read-only could keep nothing the bar offered, and would say so only after Save.
  if (!open || open.read_only || !password) return;
  const site = new URL(url).hostname;
  const { offerSave = true, neverSave = [] } = await chrome.storage.local.get(['offerSave', 'neverSave']);
  if (!offerSave || neverSave.includes(site)) return;
  const rows = engine('match.web', { url }).json as any[];
  const logins = rows.map((r) => r.entry).filter((e) => e.kind === 'login');
  // The password step of a two-step sign-in has no username on it, or only the one borrowed from
  // the step before, which may not be spelled as saved. A login here that already has this
  // password is the one just used, not a new one to save.
  if (!username || borrowed) {
    for (const e of logins) {
      if (e.has_password !== false && storedPassword(e.id) === password) return;
    }
  }
  // Sites take an email in any case, and some keep a space typed after it. With no username on
  // the form, such as a change of password, the one login saved here is the one it is for, rather
  // than a new one with no username beside it.
  const wanted = username.trim().toLowerCase();
  const same = logins.find((e) => (e.subtitle ?? '').trim().toLowerCase() === wanted) ?? (!wanted && logins.length === 1 ? logins[0] : undefined);
  if (same) {
    // A login saved with no password yet gets this one as an update, not a second login.
    if (storedPassword(same.id) === password) return;
    await setOffer(tab, { url, username: username || (same.subtitle ?? ''), password, kind: 'update', id: same.id, at: Date.now(), top });
    nudgeOffer(tab);
    return;
  }
  await setOffer(tab, { url, username, password, kind: 'new', id: null, at: Date.now(), top });
  nudgeOffer(tab);
}

async function acceptOffer(offer: Offer) {
  // Asked first, so nothing is changed in a vault that would then go unsaved.
  if (!(await canSave())) throw new Error(NO_WRITER);
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
  /** The site takes only a verified answer, so the master password is asked for, not offered. */
  verify: boolean;
  /** A passkey the site listed as already made is in the vault: the site is told so, not given another. */
  exists?: boolean;
  /** The tab and frame that asked, so a request the page gives up on puts its window away. */
  from: string;
  finish: (answer: { choice: number; verified: boolean } | null) => void;
  /** Puts the window away unanswered. */
  withdraw: () => void;
}

const asking = new Map<string, Pending>();

function askInWindow(pending: Omit<Pending, 'finish' | 'withdraw'>): Promise<{ choice: number; verified: boolean } | null> {
  return new Promise((resolve) => {
    const id = crypto.randomUUID();
    // The worker must not be stopped while a person is deciding.
    const keepAwake = setInterval(() => chrome.runtime.getPlatformInfo(() => {}), 20_000);
    let windowId: number | undefined;
    let done = false;
    const finish = (answer: { choice: number; verified: boolean } | null) => {
      if (done) return;
      done = true;
      clearInterval(keepAwake);
      chrome.windows.onRemoved.removeListener(gone);
      asking.delete(id);
      resolve(answer);
    };
    const gone = (closed: number) => {
      if (closed === windowId) finish(null);
    };
    const withdraw = () => {
      finish(null);
      if (windowId !== undefined) void chrome.windows.remove(windowId).catch(() => {});
    };
    asking.set(id, { ...pending, finish, withdraw });
    chrome.windows.onRemoved.addListener(gone);
    // A window that never opens would leave the page waiting and this worker kept awake for good.
    chrome.windows.create({ url: `confirm.html?id=${id}`, type: 'popup', width: 400, height: 500 }).then(
      (w: any) => {
        windowId = w.id;
        // Withdrawn while the window was still opening.
        if (done) void chrome.windows.remove(w.id).catch(() => {});
      },
      () => finish(null),
    );
  });
}

async function passkey(sender: any, op: 'get' | 'create', options: any) {
  // Firefox for Android opens no windows, so the browser's own prompt answers there.
  if (!open || !chrome.windows) return { fallback: true };
  const url = new URL(sender.url);
  const origin = url.origin;
  const host = url.hostname;
  const from = `${sender.tab?.id}:${sender.frameId ?? 0}`;
  if (url.protocol !== 'https:') return { fallback: true };
  // A frame from another origin may use passkeys only where its page allows it, which the
  // browser checks and this does not, and its answer would claim it was no frame at all. The
  // browser's own prompt answers there.
  if (sender.frameId && (!sender.tab?.url || new URL(sender.tab.url).origin !== origin)) return { fallback: true };

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
    const answer = await askInWindow({ op, site: host, rp: rpId, user: '', choices: list, verify: options.userVerification === 'required', from });
    if (!answer || !open) return { cancelled: true };
    const chosen = list[answer.choice];
    if (!chosen) return { cancelled: true };
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
  // One the site already has for this account, and that is kept here, is not made twice. The
  // person still says yes first, so a page cannot learn without asking which passkeys are here.
  const excludedIds: string[] = (options.excludeCredentials ?? []).map((c: any) => c?.id).filter((id: unknown) => typeof id === 'string');
  let exists = false;
  if (excludedIds.length) {
    try {
      exists = (engine('passkey.list', { rp_id: rpId, origin_host: host }).json as any[]).some((k) => excludedIds.includes(k.credential_id));
    } catch {
      return { fallback: true };
    }
  }
  // A new passkey has to be kept, so a vault nothing here can save leaves it to the browser
  // before the person is asked, not after.
  if (!exists && (open.read_only || !(await canSave()))) return { fallback: true };
  const answer = await askInWindow({ op, site: host, rp: rpId, user: options.user?.name ?? '', choices: [], verify: !exists && options.userVerification === 'required', exists, from });
  if (!answer || !open) return { cancelled: true };
  if (exists) return { excluded: true };
  // Into the login already saved here for this account, rather than a second entry beside it.
  const wanted = String(options.user?.name ?? '').trim().toLowerCase();
  let into: string | undefined;
  if (wanted) {
    try {
      const rows = engine('match.web', { url: sender.url }).json as any[];
      into = rows.map((r) => r.entry).find((e) => e.kind === 'login' && String(e.subtitle ?? '').trim().toLowerCase() === wanted)?.id;
    } catch {
      into = undefined;
    }
  }
  const made = engine('passkey.create', {
    id: into,
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
      if (tab !== undefined) {
        const own = String(message.username ?? '');
        const borrowed = own ? '' : await typedFor(tab, sender.url);
        const weighing = considerOffer(tab, sender.url, own || borrowed, String(message.password ?? ''), sender.frameId ? sender.tab?.url : undefined, !!borrowed);
        const settled = weighing.catch(() => {});
        considering.set(tab, settled);
        try {
          await weighing;
        } finally {
          if (considering.get(tab) === settled) considering.delete(tab);
        }
      }
      return {};
    case 'cs.username':
      if (tab !== undefined && open) {
        const entry: Typed = { origin: new URL(sender.url).origin, username: String(message.username ?? '').slice(0, 256), at: Date.now() };
        await setPerTab('typed', tab, entry);
      }
      return {};
    case 'cs.offer': {
      if (sender.frameId || tab === undefined) return null;
      // The page after a sign-in can ask before the sign-in it follows has been weighed.
      await considering.get(tab);
      const offer = await offerFor(tab);
      if (!offer || offer.shown) return null;
      const there = new URL(offer.url);
      const here = new URL(sender.url);
      const onSite = sameSite(there.hostname, here.hostname) || (!!offer.top && sameSite(new URL(offer.top).hostname, here.hostname));
      if (!onSite) return null;
      if (message.asksPassword) {
        // The same page asking for a password again is the form back after a failed sign-in, often
        // with a note such as ?error=1 added to its address. Another page of the site asking, as
        // some show the form again at the address it posts to, keeps the offer for a page that
        // has moved on.
        if (there.origin === here.origin && there.pathname === here.pathname) await setOffer(tab, null);
        return null;
      }
      await setOffer(tab, { ...offer, shown: true });
      // The bar goes when the offer does, so a late click never answers nothing.
      // Its time made goes with it, so the bar's answer is for this offer and no later one.
      return { kind: offer.kind, username: offer.username, left: OFFER_TTL_MS - (Date.now() - offer.at), at: offer.at };
    }
    case 'cs.offer.answer': {
      if (sender.frameId || tab === undefined) return {};
      const latest = await offerFor(tab);
      // Another sign-in in the same tab since the bar came up, as a single-page app allows, made
      // an offer of its own. Save on the bar keeps only the login the bar named, never that one.
      const later = !!latest && message.answer === 'save' && typeof message.at === 'number' && latest.at !== message.at;
      const offer = later ? null : latest;
      if (!later) await setOffer(tab, null);
      if (later) throw new Error('Another sign-in on this page came after this one, so this one was not kept. Add it in WM Pass instead.');
      // Locking forgets every sign-in waiting here, so a locked vault is said as that, not age.
      if (!offer && message.answer === 'save') throw new Error(open ? 'That sign-in is too old to save now. Add it in WM Pass instead.' : 'WM Pass locked, so this sign-in was not kept. Unlock it and add the login there.');
      if (offer && message.answer === 'save') await acceptOffer(offer);
      if (offer && message.answer === 'never') {
        const { neverSave = [] } = await chrome.storage.local.get('neverSave');
        const site = new URL(offer.url).hostname;
        if (!neverSave.includes(site)) await chrome.storage.local.set({ neverSave: [...neverSave, site] });
      }
      return {};
    }
    case 'cs.passkey':
      // Any script on the page can post the request. Anything but a sign-in would otherwise go
      // down the path that makes a new passkey, behind a window that says "Sign in".
      if (message.op !== 'get' && message.op !== 'create') return { fallback: true };
      return passkey(sender, message.op, message.options ?? {});
    case 'cs.passkey.cancel': {
      // Only the asks of the frame that sent this: a page cannot put away another's window.
      const from = `${sender.tab?.id}:${sender.frameId ?? 0}`;
      for (const pending of [...asking.values()]) if (pending.from === from) pending.withdraw();
      return {};
    }
    case 'cs.open':
      await showVault();
      return {};
    default:
      throw new Error('Not allowed.');
  }
}

/** Brings the vault tab forward, or opens one: a press of the menu each time would pile up tabs. */
async function showVault() {
  const page = chrome.runtime.getURL('vault.html');
  const tabs: any[] = await chrome.tabs.query({}).catch(() => []);
  const there = tabs.find((t) => typeof t.url === 'string' && t.url.startsWith(page));
  if (there?.id !== undefined) {
    await chrome.tabs.update(there.id, { active: true });
    // Firefox for Android has tabs but no windows.
    if (chrome.windows && there.windowId !== undefined) await chrome.windows.update(there.windowId, { focused: true }).catch(() => {});
    return;
  }
  await chrome.tabs.create({ url: page });
}

// Run in every frame of a tab, to find the one with the sign-in form. Self-contained: it is sent
// to the page as source.
function probeFrame() {
  const shown = (el: HTMLInputElement) => {
    const rect = el.getBoundingClientRect();
    // Not one pushed off the page's edge, as boxes kept for bots to fill are.
    if (rect.right + scrollX <= 0 || rect.bottom + scrollY <= 0) return false;
    return rect.width >= 8 && rect.height >= 8 && getComputedStyle(el).visibility !== 'hidden';
  };
  const signIn = (el: HTMLInputElement) =>
    el.type === 'password' || el.type === 'email' || /user|login|email|account|identifier/i.test(`${el.name} ${el.id} ${el.autocomplete}`);
  // In open shadow roots too, where sites built from web components keep their sign-in boxes.
  const all: HTMLInputElement[] = [];
  const walk = (node: ParentNode) => {
    for (const el of Array.from(node.querySelectorAll('*'))) {
      if (el instanceof HTMLInputElement) all.push(el);
      if (el.shadowRoot) walk(el.shadowRoot);
    }
  };
  walk(document);
  const inputs = all.filter(shown);
  const form = inputs.some(signIn);
  // The popup holds the window's focus while this runs, so hasFocus() is false everywhere. The
  // field last used keeps its place as the document's active element, or its shadow root's.
  const password = inputs.some((el) => el.type === 'password');
  let active = document.activeElement;
  while (active?.shadowRoot?.activeElement) active = active.shadowRoot.activeElement;
  // Only a sign-in box counts as the one in use: a search box focused on the page above a framed
  // sign-in form would otherwise put the page's newsletter box ahead of it.
  return { url: location.href, form, password, focused: active instanceof HTMLInputElement && signIn(active) };
}

/**
 * Fills a login from the popup into the frame of the tab that holds the sign-in form, which on
 * some sites is a frame of its own. Each frame gets the login only if it is saved for that frame's
 * own address.
 */
async function fillTab(tab: any, id: string) {
  type Probe = { frameId: number; documentId?: string; result?: { url: string; form: boolean; password?: boolean; focused: boolean } };
  let probes: Probe[] = [];
  try {
    probes = await chrome.scripting.executeScript({ target: { tabId: tab.id, allFrames: true }, func: probeFrame });
  } catch {
    // A page scripts cannot reach, such as the browser's own: the top frame is tried as before.
  }
  // The field in use first, then a frame with a password field over one with only an email box,
  // such as a newsletter's.
  const rank = (p: Probe) => (p.result?.focused ? 0 : p.result?.password ? 1 : p.frameId === 0 ? 2 : 3);
  const frames = probes.filter((p) => p.result?.form).sort((a, b) => rank(a) - rank(b));
  if (frames.length === 0) frames.push({ frameId: 0, result: { url: tab.url, form: true, focused: false } });
  let refusedHost: string | null = null;
  for (const frame of frames) {
    let fill: any;
    try {
      fill = await fillFor(frame.result!.url, id);
    } catch (failure: any) {
      // Only "not saved for this address" moves on to the next frame. Anything else is said.
      if (!/does not belong to this page/.test(failure?.message ?? '')) throw failure;
      refusedHost ??= new URL(frame.result!.url).hostname;
      continue;
    }
    // Pinned to the document that was probed, where the browser says which: a frame that moved
    // on to another address while the login was fetched gets nothing.
    const target = frame.documentId ? { frameId: frame.frameId, documentId: frame.documentId } : { frameId: frame.frameId };
    // The frame's origin goes too, so a frame that has gone elsewhere since refuses it, where the
    // browser cannot pin the message to one document.
    const send = () => chrome.tabs.sendMessage(tab.id, { type: 'fill', fill, origin: new URL(frame.result!.url).origin }, target);
    let answer: any;
    try {
      answer = await send();
    } catch (failure: any) {
      // A tab open from before the extension was installed or updated has no content script yet.
      if (!/Receiving end does not exist|Could not establish connection/i.test(failure?.message ?? '')) throw new Error('This page cannot be filled. Reload it and try again.');
      try {
        const into = frame.documentId ? { tabId: tab.id, documentIds: [frame.documentId] } : { tabId: tab.id, frameIds: [frame.frameId] };
        await chrome.scripting.executeScript({ target: into, files: ['content.js'] });
        answer = await send();
      } catch {
        throw new Error('This page cannot be filled. Reload it and try again.');
      }
    }
    if (answer?.filled !== false) return;
  }
  throw new Error(refusedHost ? `The sign-in form here is on ${refusedHost}, and this login is not saved for it.` : 'No sign-in form was found on this page.');
}

async function fromPage(message: any) {
  switch (message.type) {
    case 'call': {
      await active();
      // Calls that reach a network go out from here, where the extension's host permissions
      // apply. The page only ever sees their answers.
      const reply = await pageCall(fromB64(message.frame));
      // A reply the module wrote after a trap this worker did not see.
      if (!dead && saysInternal(reply)) stopForGood();
      return { frame: toB64(reply) };
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
    case 'internal':
      // A page saw an internal error first.
      stopForGood();
      return {};
    case 'restart':
      // Only a fresh start loads the session's module again. Answered first, so the page hears.
      stopForGood();
      setTimeout(() => chrome.runtime.reload(), 100);
      return {};
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
      // The tab the popup was opened over, which it names: from here, the current window is
      // whichever was focused last, not always the popup's.
      const [tab] = typeof message.tab === 'number' ? [await chrome.tabs.get(message.tab).catch(() => null)] : await chrome.tabs.query({ active: true, currentWindow: true });
      if (!tab) throw new Error('No sign-in form was found on this page.');
      await fillTab(tab, message.id);
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
    case 'vault.show':
      await showVault();
      return {};
    case 'link.fill.settings':
      return chrome.storage.local.get(['offerSave', 'passkeys']);
    case 'confirm.get': {
      const pending = asking.get(message.id);
      return pending ? { op: pending.op, site: pending.site, rp: pending.rp, user: pending.user, choices: pending.choices, verify: pending.verify, exists: pending.exists === true } : null;
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
      // A site that requires it would turn down an unverified answer after the window had gone.
      if (pending.verify && !verified) return { needed: true };
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
  // A message that wakes a stopped worker runs only once the open vault is back from storage.
  // Run before, it would find no vault and answer as if locked.
  const run = () =>
    isContent
      ? message.type.startsWith('cs.')
        ? fromContent(message, sender)
        : Promise.reject(new Error('Not allowed.'))
      : fromOurPage
        ? fromPage(message)
        : Promise.reject(new Error('Not allowed.'));
  ready
    .then(run)
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
    void chrome.tabs.update(sender.tab.id, { url: chrome.runtime.getURL('vault.html') }).catch(() => {});
    respond({ ok: true });
  } else respond({ failed: 'Unknown request.' });
  return false;
});

// Tabs open before the extension was installed or updated have no working content script: Chrome
// puts one only into pages loaded after, and an update leaves the old one cut off. Without this,
// every tab already open would show no key until it was reloaded. Firefox puts them in itself.
chrome.runtime.onInstalled.addListener((details: any) => {
  if (!chrome.offscreen || (details?.reason !== 'install' && details?.reason !== 'update')) return;
  void chrome.tabs.query({ url: ['http://*/*', 'https://*/*'] }).then((tabs: any[]) => {
    for (const tab of tabs) {
      if (tab.id === undefined || tab.discarded) continue;
      void chrome.scripting.executeScript({ target: { tabId: tab.id, allFrames: true }, files: ['content.js'] }).catch(() => {});
    }
  }, () => {});
});

watchLink(() => broadcast({ type: 'link.changed' }));
chrome.idle?.setDetectionInterval?.(60);
