import { host } from '../engine/host';
import { overlays } from '../parts/floating.svelte';

export async function copyPlainLine(what: string, value: string) {
  try {
    await host().copyText(value, false, 0);
    overlays.toast(`${what} copied.`);
  } catch {
    // A browser refuses the clipboard to a page without focus or permission.
    overlays.toast(`${what} could not be copied.`);
  }
}
