// What a page of the extension runs on. Files are shared IndexedDB, and the session is in the
// background, so it survives the popup closing.

import type { Host } from '../../ui/src/engine/host';
import { decodeReply } from '../../ui/src/engine/frame';
import { webHost } from '../../ui/src/hosts/web';
import { extensionPlaces } from './places';
import { seal, unseal } from './sealed';
import { fromB64, toB64 } from './shared';

declare const chrome: any;

export async function send<T = any>(message: Record<string, unknown>): Promise<T> {
  const reply = await chrome.runtime.sendMessage(message);
  if (reply?.failed) throw Object.assign(new Error(reply.failed), { code: reply.code });
  return reply?.ok as T;
}

export async function extensionHost(): Promise<Host> {
  const base = await webHost({
    call: async (frame) => fromB64((await send<{ frame: string }>({ type: 'call', frame: toB64(frame) })).frame),
    lockAll: () => send({ type: 'closed' }),
  });
  let onCommand: (command: string) => void = () => {};
  chrome.runtime.onMessage.addListener((message: any) => {
    if (message?.type === 'locked') onCommand('lock');
    if (message?.type === 'changed') onCommand('reload');
  });
  const host: Host = {
    ...base,
    kind: 'extension',
    onCommand: (run) => {
      onCommand = run;
    },
    async callToClipboard(frame, clearAfterSeconds) {
      const reply = decodeReply(await base.call(frame));
      const text = new TextDecoder().decode(reply.secret ?? new Uint8Array());
      reply.secret?.fill(0);
      await host.copyText(text, true, clearAfterSeconds);
      return new Uint8Array([2, 0, 0, 0, 0x7b, 0x7d, 0, 0, 0, 0, 0]);
    },
    async copyText(text, sensitive, clearAfterSeconds) {
      await navigator.clipboard.writeText(text);
      await send({ type: 'clip.arm', seconds: sensitive ? clearAfterSeconds : 0 });
    },
    async clearClipboard() {
      await navigator.clipboard.writeText('');
      await send({ type: 'clip.arm', seconds: 0 });
    },
    stateChanged(state) {
      void (state.unlocked && state.handle ? send({ type: 'opened', ...state }) : send({ type: 'closed' }));
    },
    attach: () => send({ type: 'attach' }),
    lockGuard: {
      activity: () => send({ type: 'guard.active' }),
      setLockRule: (after, onSleep) => send({ type: 'guard.rule', after, onSleep }),
    },
    loadSealed: (name) => unseal(name),
    saveSealed: (name, text) => seal(name, text),
    places: extensionPlaces,
    usernames: {
      sites: () => send({ type: 'usernames', what: 'sites' }),
      check: async (site, name) => (await send<{ answer: string }>({ type: 'usernames', what: 'check', site, name })).answer,
      links: async (name) => (await send<{ links: { site: string; url: string }[] }>({ type: 'usernames', what: 'links', name })).links,
    },
    link: {
      state: () => send({ type: 'link.state' }),
      connect: () => send({ type: 'link.connect' }),
      answer: (match) => send({ type: 'link.answer', match }),
      forget: () => send({ type: 'link.forget' }),
      onChange: (run) => {
        const listen = (message: any) => {
          if (message?.type === 'link.changed') run();
        };
        chrome.runtime.onMessage.addListener(listen);
        return () => chrome.runtime.onMessage.removeListener(listen);
      },
      option: async (name, fallback) => (await chrome.storage.local.get(name))[name] ?? fallback,
      setOption: (name, value) => chrome.storage.local.set({ [name]: value }),
    },
  };
  return host;
}
