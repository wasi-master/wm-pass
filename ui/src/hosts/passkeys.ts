// Security keys and quick unlock for the browser builds, through WebAuthn's PRF extension. PRF is
// hmac-secret with its input hashed first, and the desktop and phone apps ask a key exactly the
// same way, so a key set up here opens the vault in every app, and one set up there opens it here.
//
// Every credential belongs to the web vault's domain. The extension may claim it too, since it
// may reach every site, so a key works in both. A copy of the page served from anywhere else
// cannot ask for these credentials at all: the browser checks the domain.

import { decodeReply, encodeRequest } from '../engine/frame';
import type { HardwareHost, QuickUnlock } from '../engine/host';

/** The relying party every WM Pass credential belongs to. The same as `ctap::WEB_RP_ID`. */
export const WEB_RP_ID = 'wmpass.pages.dev';

function rpId() {
  // A page on this computer, such as the design preview, cannot claim the real domain.
  const here = location.hostname;
  return location.protocol === 'chrome-extension:' || here === WEB_RP_ID || here.endsWith(`.${WEB_RP_ID}`) ? WEB_RP_ID : here;
}

const random = (length: number) => crypto.getRandomValues(new Uint8Array(length));
const hex = (bytes: ArrayBuffer | Uint8Array) => Array.from(new Uint8Array(bytes), (b) => b.toString(16).padStart(2, '0')).join('');
const unhex = (text: string) => new Uint8Array((text.match(/../g) ?? []).map((pair) => parseInt(pair, 16)));

/** Whether this browser has WebAuthn with PRF, as far as it will say. */
async function capabilities(): Promise<Record<string, boolean>> {
  const credential = (globalThis as any).PublicKeyCredential;
  if (!credential) return {};
  try {
    return (await credential.getClientCapabilities?.()) ?? { 'extension:prf': true };
  } catch {
    return {};
  }
}

function friendly(failure: any): Error {
  if (failure?.name === 'NotAllowedError' && /focus/i.test(failure.message)) return new Error('The browser asks only while this window is in front. Click into it and try again.');
  if (failure?.name === 'NotAllowedError') return new Error('The browser’s request was cancelled or timed out.');
  if (failure?.name === 'InvalidStateError') return new Error('That key is already set up here.');
  if (failure?.name === 'SecurityError') return new Error('This page is not allowed to ask for WM Pass keys.');
  return failure instanceof Error ? failure : new Error(String(failure));
}

/** Registers a credential that can answer PRF. Answers its id and how the browser reaches it. */
async function register(attachment: 'platform' | 'cross-platform', name: string): Promise<{ id: Uint8Array; transports: string[] }> {
  const made: any = await navigator.credentials
    .create({
      publicKey: {
        rp: { id: rpId(), name: 'WM Pass' },
        user: { id: random(32), name, displayName: name },
        challenge: random(32),
        pubKeyCredParams: [
          { type: 'public-key', alg: -7 },
          { type: 'public-key', alg: -257 },
        ],
        authenticatorSelection: {
          authenticatorAttachment: attachment,
          residentKey: 'discouraged',
          // A security key is asked without its PIN, as the desktop app asks it, since the answer
          // differs with and without. A passkey on this device is quick unlock, so it checks who you are.
          userVerification: attachment === 'platform' ? 'required' : 'discouraged',
        },
        hints: attachment === 'platform' ? ['client-device'] : ['security-key'],
        extensions: { prf: {} },
      } as any,
    })
    .catch((failure) => {
      throw friendly(failure);
    });
  if (made?.getClientExtensionResults?.().prf?.enabled === false) {
    throw new Error('That key cannot give WM Pass a secret. It needs hmac-secret, which most FIDO2 keys have.');
  }
  return { id: new Uint8Array(made.rawId), transports: made.response?.getTransports?.() ?? [] };
}

/**
 * Asks a credential for its answer to `input`. The transports say where to look: without them a
 * browser may ask a plugged-in key for a passkey that lives on this device, and give up.
 */
async function answer(credential: Uint8Array, input: Uint8Array, verify: boolean, transports: string[] = []): Promise<Uint8Array> {
  const got: any = await navigator.credentials
    .get({
      publicKey: {
        rpId: rpId(),
        challenge: random(32),
        allowCredentials: [{ type: 'public-key', id: credential, ...(transports.length ? { transports } : {}) }],
        userVerification: verify ? 'required' : 'discouraged',
        extensions: { prf: { eval: { first: input } } },
      } as any,
    })
    .catch((failure) => {
      throw friendly(failure);
    });
  const first = got?.getClientExtensionResults?.().prf?.results?.first;
  if (!first) throw new Error('The key answered without its secret. It needs hmac-secret, which most FIDO2 keys have.');
  return new Uint8Array(first);
}

/** Security keys as a second factor. A YubiKey's challenge-response slot is not reachable from a page. */
export async function webHardware(): Promise<HardwareHost | undefined> {
  if (!(await capabilities())['extension:prf']) return undefined;
  return {
    kinds: ['fido2'],
    async tap(kind, factor: any) {
      if (kind !== 'fido2') throw new Error('A browser cannot reach a YubiKey’s challenge-response slot. Use the desktop app or the phone for this key.');
      if (factor && factor.kind !== 'webauthn') {
        throw new Error('This key was set up by an older version of the app, in a way a browser cannot ask. Open the vault in the desktop app or the phone, and add the key again there or here.');
      }
      const chosen = factor ?? { kind: 'webauthn', credential_id: hex((await register('cross-platform', 'WM Pass vault')).id), input: hex(random(32)) };
      return { factor: chosen, output: await answer(unhex(chosen.credential_id), unhex(chosen.input), false) };
    },
  };
}

interface Kept {
  credential: string;
  transports: string[];
  input: string;
  sealed: string;
}

type Settings = { load(name: string): Promise<string | null>; save(name: string, text: string): Promise<void> };
type Call = (frame: Uint8Array) => Promise<Uint8Array>;

/**
 * Opening with a passkey on this device: Touch ID, Windows Hello, or the browser's own. The vault
 * key is sealed, inside the session, under what the passkey answers, and only that sealed copy is
 * kept here. Without the passkey it opens nothing.
 */
export async function passkeyQuick(call: Call, settings: Settings): Promise<QuickUnlock | undefined> {
  const caps = await capabilities();
  if (!caps['extension:prf']) return undefined;
  const name = (file: string) => `quick.${file}`;
  const read = async (file: string): Promise<Kept | null> => {
    const text = await settings.load(name(file));
    return text ? (JSON.parse(text) as Kept) : null;
  };
  return {
    title: 'Unlock with a passkey',
    sub: 'Touch ID, Windows Hello or your phone. Only a sealed copy of the key stays in this browser.',
    help: 'action.passkey_unlock',
    async available() {
      const credential = (globalThis as any).PublicKeyCredential;
      return caps.userVerifyingPlatformAuthenticator ?? caps.passkeyPlatformAuthenticator ?? !!(await credential?.isUserVerifyingPlatformAuthenticatorAvailable?.());
    },
    async enrolled(file) {
      return !!(await read(file));
    },
    async enroll(handle, file) {
      const credential = await register('platform', `WM Pass: ${file.replace(/\.wmp$/, '')}`);
      const transports = credential.transports.length ? credential.transports : ['internal', 'hybrid'];
      const input = random(32);
      const secret = await answer(credential.id, input, true, transports);
      const frame = encodeRequest({ method: 'vault.quick_seal', handle, args: {}, now_ms: Date.now() }, secret, []);
      secret.fill(0);
      try {
        const reply = decodeReply(await call(frame));
        if (reply.json?.error) throw new Error(reply.json.error.message);
        const kept: Kept = { credential: hex(credential.id), transports, input: hex(input), sealed: hex(reply.blobs[0]) };
        await settings.save(name(file), JSON.stringify(kept));
      } finally {
        frame.fill(0);
      }
    },
    async forget(file) {
      const kept = await read(file);
      await settings.save(name(file), '');
      // Lets the browser's password manager drop the passkey too, where it can.
      if (kept) void (globalThis as any).PublicKeyCredential?.signalUnknownCredential?.({ rpId: rpId(), credentialId: btoa(String.fromCharCode(...unhex(kept.credential))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '') }).catch(() => {});
    },
    async open(file, sealedFile) {
      const kept = await read(file);
      if (!kept) throw new Error('Quick unlock is not set up for this vault here.');
      const secret = await answer(unhex(kept.credential), unhex(kept.input), true, kept.transports ?? []);
      const frame = encodeRequest({ method: 'vault.quick_open', handle: 0, args: {}, now_ms: Date.now() }, secret, [unhex(kept.sealed), sealedFile]);
      secret.fill(0);
      try {
        return await call(frame);
      } finally {
        frame.fill(0);
      }
    },
  };
}
