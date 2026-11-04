// The web vault's shell: the session as WebAssembly in this page, files in this site's own storage,
// and the rest through what a browser page may do. It shares nothing with the extension or the
// desktop app; a vault moves between them through a backup place, as between any two devices.

import type { Host } from '$ui/engine/host';
import { browserFiles, fileAccess } from '$ui/hosts/folders';
import { passkeyQuick, webHardware } from '$ui/hosts/passkeys';
import { browserPlaces, type BrowserSignIn } from '$ui/hosts/places';
import { seal, unseal } from '$ui/hosts/sealed';
import { wasmHost } from '$ui/hosts/wasm';

const wasmUrl = new URL('../../ui/src/wasm/wmpass_wasm_bg.wasm', import.meta.url).href;

/**
 * A service's sign-in page opens in its own window and comes back to oauth.html on this site,
 * which passes the answer here over a channel only this site's pages can hear.
 */
const popupSignIn: BrowserSignIn = {
  redirect: new URL('oauth.html', location.href).href,
  open(url) {
    return new Promise((resolve, reject) => {
      const channel = new BroadcastChannel('wmpass-oauth');
      const finish = (value: string | null) => {
        clearTimeout(timer);
        channel.close();
        resolve(value);
      };
      // A service's page may cut this window off from the one that opened it, so its closing
      // cannot be watched. Ten minutes is ample for a sign-in.
      const timer = setTimeout(() => finish(null), 10 * 60_000);
      channel.onmessage = (event) => {
        if (typeof event.data?.url === 'string') finish(event.data.url);
      };
      if (!window.open(url, 'wmpass-sign-in', 'popup,width=520,height=700')) {
        clearTimeout(timer);
        channel.close();
        reject(new Error('The browser blocked the sign-in window. Allow pop-ups for this page and try again.'));
      }
    });
  },
};

export async function webVaultHost(): Promise<Host> {
  const base = await wasmHost(wasmUrl);
  return {
    ...base,
    kind: 'web',
    loadSealed: (name) => unseal(name),
    saveSealed: (name, text) => seal(name, text),
    places: browserPlaces(wasmUrl, popupSignIn),
    outside: fileAccess ? browserFiles : undefined,
    hardware: await webHardware(),
    quick: await passkeyQuick(base.call, { load: base.loadSettings, save: base.saveSettings }),
  };
}
