// The background's end of the conversation with the desktop app. The relay program only carries
// frames: everything is sealed in wasm before it leaves here.

import { bridge_close, bridge_feed, bridge_new_keys, bridge_seal, bridge_start } from '../../ui/src/wasm/wmpass_wasm.js';
import { seal, unseal } from './sealed';
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

export function watchLink(run: () => void) {
  onChange = run;
}

function set(change: Partial<LinkState>) {
  state = { ...state, ...change };
  onChange();
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
  set({ status, code: null, problem });
}

function send(frame: Uint8Array) {
  port?.postMessage({ f: toB64(frame) });
}

async function pin() {
  if (remote) await seal(APP_KEY, remote);
  set({ status: 'ready', code: null });
  void ask({ method: 'status' }).then((r) => set({ unlocked: r.unlocked === true }));
}

function received(message: any) {
  if (message.closed) {
    close(state.status === 'ready' ? 'off' : 'absent');
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
    const reply = JSON.parse(new TextDecoder().decode(bytes));
    bytes.fill(0);
    waiting.get(reply.id)?.(reply);
    waiting.delete(reply.id);
  }
}

/** Connects, unless it already has. Quietly stays off when the app or relay is not there. */
export async function connectLink() {
  if (port) return;
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
        set({ status: 'absent', code: null, problem: chrome.runtime.lastError?.message ?? null });
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
    send(bridge_seal(text));
    text.fill(0);
  });
}
