// The background's end of the conversation with the desktop app. The relay program only carries
// frames: everything is sealed in wasm before it leaves here.

import { bridge_close, bridge_feed, bridge_new_keys, bridge_seal, bridge_start } from '../../ui/src/wasm/wmpass_wasm.js';
import { seal, unseal } from '../../ui/src/hosts/sealed';
import { fromB64, fromHex, toB64, toHex, type LinkState } from './shared';

declare const chrome: any;

const RELAY = 'com.wasimaster.wmpass';
const SECRET = 'link-secret';
const APP_KEY = 'link-app';
const NAME_OF_THIS = /Firefox/.test(navigator.userAgent) ? 'Firefox' : /Edg\//.test(navigator.userAgent) ? 'Edge' : 'Chrome';

let port: any = null;
let state: LinkState = { status: 'off', code: null, fingerprint: null, unlocked: false, problem: null };
let confirmed = false;
let appAccepted = false;
let remote: string | null = null;
let nextId = 1;
const waiting = new Map<number, (reply: any) => void>();
let onChange: () => void = () => {};
const settled = new Set<() => void>();

export function watchLink(run: () => void) {
  onChange = run;
}

function set(change: Partial<LinkState>) {
  state = { ...state, ...change };
  onChange();
  if (state.status !== 'connecting') for (const run of settled) run();
}

/**
 * Waits, a short while at most, for a connection under way to finish. A worker woken by a page
 * asking for its logins starts connecting again just then, and answering at once would say the
 * app is not there.
 */
export function linkSettled(ms = 2000): Promise<void> {
  if (state.status !== 'connecting') return Promise.resolve();
  return new Promise((resolve) => {
    const done = () => {
      clearTimeout(timer);
      settled.delete(done);
      resolve();
    };
    const timer = setTimeout(done, ms);
    settled.add(done);
  });
}

export const linkState = () => state;
export const linkReady = () => state.status === 'ready';

async function secret(): Promise<Uint8Array> {
  const saved = await unseal(SECRET);
  if (saved) return fromHex(saved);
  const pair = bridge_new_keys();
  await seal(SECRET, toHex(pair.slice(0, 32)));
  return pair.slice(0, 32);
}

function close(status: LinkState['status'], problem: string | null = null) {
  const old = port;
  port = null;
  old?.disconnect();
  bridge_close();
  for (const finish of waiting.values()) finish({ error: 'The desktop app went away.' });
  waiting.clear();
  set({ status, code: null, problem, unlocked: false });
}

function send(frame: Uint8Array) {
  port?.postMessage({ f: toB64(frame) });
}

async function pin() {
  if (remote) await seal(APP_KEY, remote);
  set({ status: 'ready', code: null });
  void ask({ method: 'status' }).then((r) => set({ unlocked: r.unlocked === true }));
}

/** What the browser said when the relay would not start, in words that say what to do. */
function whyAbsent(said: string | undefined): string {
  if (said && /not found|no such native application/i.test(said)) {
    return 'This browser has not been set up. In the app, open Settings, Browser extension, choose Set up browsers, then restart the browser.';
  }
  if (said && /forbidden/i.test(said)) {
    return 'The app set up a different copy of the extension. Choose Set up browsers in the app again, then restart the browser.';
  }
  return said ? `The browser could not start the connection: ${said}` : 'The browser could not start the connection.';
}

function received(message: any) {
  if (message.closed) {
    // The relay says why when it was something else than the app going: an answer too large to pass on.
    if (typeof message.problem === 'string') close('failed', message.problem);
    else if (state.status === 'ready') close('off');
    else close('absent', 'The app is closed, or Let the extension connect is off in its Settings, Browser extension.');
    return;
  }
  let step;
  try {
    step = JSON.parse(bridge_feed(fromB64(message.f)));
  } catch (failure: any) {
    close('failed', failure?.message ?? 'The connection failed a check.');
    return;
  }
  remote = step.remote ?? remote;
  for (const frame of step.send) send(fromHex(frame));
  if (step.compare) set({ status: 'compare', code: step.compare });
  if (step.declined) close('declined');
  if (step.ready) {
    if (state.status === 'connecting') void pin();
    else {
      appAccepted = true;
      if (confirmed) void pin();
    }
  }
  if (step.message) {
    const bytes = fromHex(step.message);
    let reply: any;
    try {
      reply = JSON.parse(new TextDecoder().decode(bytes));
    } catch {
      // A reply that does not parse answers nothing, rather than throwing where nothing catches it.
      return;
    } finally {
      bytes.fill(0);
    }
    waiting.get(reply.id)?.(reply);
    waiting.delete(reply.id);
  }
}

let starting: Promise<void> | null = null;

/** Connects, unless it already has. Quietly stays off when the app or relay is not there. */
export function connectLink(): Promise<void> {
  if (port) return Promise.resolve();
  // A second call while the first still waits for its key would open a second port, and the
  // two handshakes fed into the one bridge would fail each other.
  return (starting ??= startLink().finally(() => (starting = null)));
}

async function startLink() {
  confirmed = false;
  appAccepted = false;
  remote = null;
  set({ status: 'connecting', problem: null });
  try {
    const key = await secret();
    const known = await unseal(APP_KEY);
    port = chrome.runtime.connectNative(RELAY);
    port.onMessage.addListener(received);
    port.onDisconnect.addListener(() => {
      if (port) {
        port = null;
        bridge_close();
        for (const finish of waiting.values()) finish({ error: 'The desktop app went away.' });
        waiting.clear();
        // The browser says why: most often that the app has not set this browser up yet.
        set({ status: 'absent', code: null, problem: whyAbsent(chrome.runtime.lastError?.message), unlocked: false });
      }
    });
    send(bridge_start(key, known ? fromHex(known) : new Uint8Array(), NAME_OF_THIS));
    key.fill(0);
  } catch (failure: any) {
    close('failed', failure?.message ?? 'The connection could not start.');
  }
}

/** What the person said about the code. */
export function answerLink(match: boolean) {
  if (!match) {
    close('off');
    return;
  }
  confirmed = true;
  if (appAccepted) void pin();
  else set({ status: 'waiting' });
}

export async function forgetLink() {
  close('off');
  await seal(APP_KEY, '');
}

export async function hasLink() {
  return !!(await unseal(APP_KEY));
}

export function ask(request: Record<string, unknown>): Promise<any> {
  if (state.status !== 'ready') return Promise.resolve({ error: 'Not connected to the desktop app.' });
  const id = nextId++;
  return new Promise((resolve) => {
    waiting.set(id, resolve);
    const text = new TextEncoder().encode(JSON.stringify({ id, ...request }));
    try {
      send(bridge_seal(text));
    } catch (failure: any) {
      // Nothing went out, so no reply will come to settle this.
      waiting.delete(id);
      resolve({ error: failure?.message ?? String(failure) });
    } finally {
      text.fill(0);
    }
  });
}
