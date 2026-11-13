// Places outside the browser, as the extension reaches them: its own copy of the WebAssembly, and
// account sign-ins through the browser's identity window.

import { browserPlaces } from '../../ui/src/hosts/places';

declare const chrome: any;

// Firefox for Android has no identity window. Asked for at load, its absence would stop every page.
export const extensionPlaces = browserPlaces(chrome.runtime.getURL('wmpass_wasm_bg.wasm'), {
  redirect: chrome.identity?.getRedirectURL('oauth') ?? '',
  open: async (url) => {
    if (!chrome.identity) throw new Error('This browser cannot open the sign-in for this place.');
    return chrome.identity.launchWebAuthFlow({ url, interactive: true });
  },
});
