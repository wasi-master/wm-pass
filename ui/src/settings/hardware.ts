// Security keys, as far as the shell can reach them.

import { host } from '../engine/host';
import { pass } from '../engine/pass.svelte';
import type { NeededFactor } from '../engine/types';

export function hardwareAvailable() {
  const hardware = host().hardware;
  return !!hardware && (hardware.kinds?.length ?? 2) > 0;
}

/** The key's answer for a factor the door was asked for. */
export async function hardwareTap(factor: NeededFactor): Promise<Uint8Array> {
  const hardware = host().hardware;
  if (!hardware) throw new Error('This app cannot reach a security key here.');
  const kind = factor.kind === 'yubikey' ? 'yubikey' : 'fido2';
  return (await hardware.tap(kind, factor)).output;
}

/** Whether a YubiKey's challenge-response slot can be reached here. */
export function yubikeyAvailable() {
  const hardware = host().hardware;
  return !!hardware && (hardware.kinds?.includes('yubikey') ?? true);
}

/** A YubiKey's answer for a KeePass database it is part of the key to, from the slot KeePassXC set up. */
export async function keepassAnswer(data: Uint8Array, slot: 1 | 2): Promise<Uint8Array> {
  const { challenge } = (await pass.engine<{ challenge: string }>('kdbx.challenge', {}, { blobs: [data] })).json;
  return hardwareTap({ kind: 'yubikey', challenge, slot });
}
