// Security keys, as far as the shell can reach them.

import { host } from '../engine/host';
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
