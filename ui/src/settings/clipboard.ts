import { host } from '../engine/host';
import { overlays } from '../parts/floating.svelte';

export async function copyPlainLine(what: string, value: string) {
  await host().copyText(value, false, 0);
  overlays.toast(`${what} copied.`);
}
