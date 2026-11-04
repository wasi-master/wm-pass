// Places outside the browser, as the extension reaches them: its own copy of the WebAssembly, and
// account sign-ins through the browser's identity window.

import { browserPlaces } from '../../ui/src/hosts/places';

declare const chrome: any;

export const extensionPlaces = browserPlaces(chrome.runtime.getURL('wmpass_wasm_bg.wasm'), {
  redirect: chrome.identity.getRedirectURL('oauth'),
  open: (url) => chrome.identity.launchWebAuthFlow({ url, interactive: true }),
});
