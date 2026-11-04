// What a page of the extension runs on. Files are shared IndexedDB, and the session is in the
// background, so it survives the popup closing.

import type { HardwareHost, Host, PlacesHost, UsernameSite } from '../../ui/src/engine/host';
import { decodeReply } from '../../ui/src/engine/frame';
import { browserFiles, fileAccess } from '../../ui/src/hosts/folders';
import { passkeyQuick, webHardware } from '../../ui/src/hosts/passkeys';
import { webHost } from '../../ui/src/hosts/web';
import { extensionPlaces } from './places';
import { seal, unseal } from '../../ui/src/hosts/sealed';
import { fromB64, toB64 } from './shared';

declare const chrome: any;

export async function send<T = any>(message: Record<string, unknown>): Promise<T> {
  const reply = await chrome.runtime.sendMessage(message);
  if (reply?.failed) throw Object.assign(new Error(reply.failed), { code: reply.code });
  return reply?.ok as T;
}

/**
 * The extension's own places, and while the desktop app is connected, the kinds only it can reach:
 * SFTP, FTP, SMB, NFS, IMAP, iCloud. Those run in the app, which asks its person once for each.
 */
function linkedPlaces(own: PlacesHost) {
  let desktop: string[] = [];
  const through = (place: any) => !own.kinds.includes(place?.kind) && desktop.includes(place?.kind);
  const run = async (op: string, place: any, extra: Record<string, unknown> = {}) => {
    const { value, update } = await send<{ value: any; update: Record<string, unknown> }>({ type: 'link.places', op, place, ...extra });
    if (place?.id && Object.keys(update).length) places.onUpdate?.(place.id, update);
    return value;
  };
  const places: PlacesHost = {
    get kinds() {
      return [...own.kinds, ...desktop.filter((kind) => kind !== 'folder' && !own.kinds.includes(kind))];
    },
    list: (place) => (through(place) ? run('list', place) : own.list(place)),
    read: async (place, file) => (through(place) ? fromB64(await run('read', place, { file })) : own.read(place, file)),
    write: async (place, name, bytes) => void (through(place) ? await run('write', place, { name, data: toB64(bytes) }) : await own.write(place, name, bytes)),
    remove: async (place, file) => void (through(place) ? await run('delete', place, { file }) : await own.remove(place, file)),
    chooseFolder: () => own.chooseFolder(),
    signIn: (place, step) => (through(place) ? send({ type: 'link.places.sign_in', place, step }) : own.signIn!(place, step)),
  };
  // The places store listens on this object; the extension's own runs report through it too.
  Object.defineProperty(own, 'onUpdate', { get: () => places.onUpdate, configurable: true });
  const refresh = () =>
    void send<string[]>({ type: 'link.places.kinds' }).then(
      (kinds) => (desktop = kinds ?? []),
      () => (desktop = []),
    );
  refresh();
  return { places, refresh };
}

/**
 * Security keys: through the browser's WebAuthn where it can, and through the desktop app, while
 * it is connected, for a YubiKey's challenge-response slot and keys set up there before browsers
 * could reach them.
 */
function linkedHardware(own: HardwareHost | undefined) {
  let linked = false;
  const hardware: HardwareHost = {
    get kinds() {
      return [...(own || linked ? (['fido2'] as const) : []), ...(linked ? (['yubikey'] as const) : [])];
    },
    async tap(kind, factor: any) {
      const older = kind === 'fido2' && factor && factor.kind !== 'webauthn';
      if (own && kind === 'fido2' && !older) return own.tap(kind, factor);
      if (!linked) throw new Error('This key needs the desktop app. Connect it under Settings, Desktop app.');
      const reply = await send<{ factor: any; output: string }>({ type: 'link.hardware', kind, factor: factor ?? null });
      return { factor: reply.factor, output: fromB64(reply.output) };
    },
  };
  const refresh = () =>
    void send<any>({ type: 'link.state' }).then(
      (state) => (linked = state?.status === 'ready'),
      () => (linked = false),
    );
  refresh();
  return { hardware, refresh };
}

/** The vault handle a request frame names, read from its head. */
function handleOf(frame: Uint8Array): number {
  const length = new DataView(frame.buffer, frame.byteOffset, frame.byteLength).getUint32(0, true);
  try {
    return Number(JSON.parse(new TextDecoder().decode(frame.subarray(4, 4 + length))).handle ?? 0);
  } catch {
    return 0;
  }
}

/** A reply frame that carries only an error, as the engine writes one. */
function errorFrame(code: string, message: string): Uint8Array {
  const text = new TextEncoder().encode(JSON.stringify({ error: { code, message } }));
  const out = new Uint8Array(4 + text.length + 1 + 4);
  new DataView(out.buffer).setUint32(0, text.length, true);
  out.set(text, 4);
  return out;
}

export async function extensionHost(): Promise<Host> {
  const base = await webHost({
    call: async (frame) => fromB64((await send<{ frame: string }>({ type: 'call', frame: toB64(frame) })).frame),
    lockAll: () => send({ type: 'closed' }),
  });
  const linkPlaces = linkedPlaces(extensionPlaces);
  const linkKeys = linkedHardware(await webHardware());
  let onCommand: (command: string) => void = () => {};
  /** The desktop app's handle while these screens show its vault, else zero. */
  let desktopHandle = 0;
  async function call(frame: Uint8Array): Promise<Uint8Array> {
    if (!desktopHandle || handleOf(frame) !== desktopHandle) return base.call(frame);
    let reply: { frame?: string; locked?: boolean };
    try {
      reply = await send({ type: 'link.vault.call', frame: toB64(frame) });
    } catch (failure: any) {
      return errorFrame('desktop', failure?.message ?? 'The desktop app did not answer.');
    }
    if (reply.locked || !reply.frame) {
      // Locked or gone there: these screens go back to the door.
      desktopHandle = 0;
      onCommand('lock');
      return errorFrame('locked', 'The vault in the desktop app is locked.');
    }
    return fromB64(reply.frame);
  }
  chrome.runtime.onMessage.addListener((message: any) => {
    if (message?.type === 'locked') onCommand('lock');
    if (message?.type === 'changed') onCommand('reload');
    if (message?.type === 'link.changed') {
      linkPlaces.refresh();
      linkKeys.refresh();
    }
  });
  const host: Host = {
    ...base,
    kind: 'extension',
    call,
    network: true,
    onCommand: (run) => {
      onCommand = run;
    },
    async callToClipboard(frame, clearAfterSeconds) {
      const reply = decodeReply(await call(frame));
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
    places: linkPlaces.places,
    outside: fileAccess ? browserFiles : undefined,
    hardware: linkKeys.hardware,
    quick: await passkeyQuick(base.call, { load: base.loadSettings, save: base.saveSettings }),
    usernames: {
      siteList: async () => (await send<{ sites: UsernameSite[] }>({ type: 'usernames', what: 'site_list' })).sites,
      check: async (site, name) => (await send<{ answer: string }>({ type: 'usernames', what: 'check', site, name })).answer,
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
      async attachVault() {
        const reply = await send<{ handle?: number; name?: string; locked?: boolean }>({ type: 'link.vault.attach' });
        if (reply.locked || !reply.handle) return null;
        desktopHandle = reply.handle;
        return { handle: reply.handle, name: reply.name ?? '' };
      },
      detachVault() {
        desktopHandle = 0;
      },
      lockDesktop: () => send({ type: 'link.lock' }),
    },
  };
  return host;
}
