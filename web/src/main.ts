// The web vault at wmpass.pages.dev/vault. Where the extension is installed it offers to hand
// over to it; otherwise the whole app runs here, on the same engine, with its files kept in this
// site's storage in this browser.

import { mount, unmount } from 'svelte';
import '$ui/app.css';
import { boot } from '$ui/boot';
import { findExtension, openInExtension } from './extension';
import Handoff from './Handoff.svelte';
import { webVaultHost } from './host';

const ALWAYS = 'wmpass.web.always_extension';
const HERE = 'wmpass.web.here';

function stored(storage: Storage, key: string) {
  try {
    return storage.getItem(key);
  } catch {
    return null;
  }
}

async function start() {
  if (import.meta.env.PROD && 'serviceWorker' in navigator) {
    // An offline copy. The page itself is fetched fresh whenever the network is there.
    void navigator.serviceWorker.register('./sw.js', { scope: './', updateViaCache: 'none' }).catch(() => {});
  }
  // Asks the browser not to clear this site's storage when space runs low, since the vault lives there.
  void navigator.storage?.persist?.().catch(() => false);

  const extension = await findExtension();
  if (extension && stored(localStorage, ALWAYS) === '1') {
    await openInExtension(extension);
    return;
  }
  if (extension && stored(sessionStorage, HERE) !== '1') {
    const target = document.getElementById('app')!;
    const handoff = mount(Handoff, {
      target,
      props: {
        onextension: (always: boolean) => {
          try {
            if (always) localStorage.setItem(ALWAYS, '1');
          } catch {}
          void openInExtension(extension);
        },
        onhere: () => {
          try {
            sessionStorage.setItem(HERE, '1');
          } catch {}
          void unmount(handoff);
          void webVaultHost().then(boot);
        },
      },
    });
    return;
  }
  await boot(await webVaultHost());
}

void start();
