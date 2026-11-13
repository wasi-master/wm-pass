// The web vault at wmpass.pages.dev/vault. Where the extension is installed it offers to hand
// over to it; otherwise the whole app runs here, on the same engine, with its files kept in this
// site's storage in this browser.

import { mount, unmount } from 'svelte';
import '$ui/app.css';
import { boot } from '$ui/boot';
import { pass } from '$ui/engine/pass.svelte';
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

/**
 * What a page that cannot start says, instead of staying blank: a browser keeping no storage for
 * this site (some private windows), or the engine failing to load while offline the first time.
 */
function failed(failure: unknown) {
  const target = document.getElementById('app')!;
  const reason = failure instanceof Error && failure.message ? failure.message : String(failure ?? '');
  const storage = /indexeddb|database|storage|quota/i.test(reason) || (failure as DOMException)?.name === 'InvalidStateError';
  const box = document.createElement('main');
  box.className = 'handoff';
  box.style.cssText = 'max-width:440px;margin:12vh auto 0;padding:0 16px;display:grid;gap:12px;text-align:center';
  const title = document.createElement('h1');
  title.textContent = 'WM Pass could not start here';
  const body = document.createElement('p');
  body.className = 'dim';
  body.textContent = storage
    ? 'This browser keeps no storage for this page, as some private windows do, so there is nowhere to keep a vault. Open it in a regular window.'
    : 'Check the connection and reload the page.';
  const detail = document.createElement('p');
  detail.className = 'small dimmer';
  detail.textContent = reason;
  box.append(title, body, ...(reason ? [detail] : []));
  target.replaceChildren(box);
}

/**
 * Asks the browser not to clear this site's storage when space runs low, since the vault lives
 * there. Asked once there is a vault to keep, on its first save, rather than as the page opens:
 * Firefox puts the question to the person, and one on every visit before anything is in it says
 * nothing they could judge.
 */
function keepStorage() {
  const storage = navigator.storage;
  if (!storage?.persist) return;
  void storage
    .persisted()
    .then((kept) => {
      if (kept) return;
      const ask = () => {
        pass.onSaved = pass.onSaved.filter((run) => run !== ask);
        void storage.persist().catch(() => false);
      };
      pass.onSaved.push(ask);
    })
    .catch(() => undefined);
}

async function start() {
  if (import.meta.env.PROD && 'serviceWorker' in navigator) {
    // An offline copy. The page itself is fetched fresh whenever the network is there.
    void navigator.serviceWorker.register('./sw.js', { scope: './', updateViaCache: 'none' }).catch(() => {});
  }
  keepStorage();

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
          void webVaultHost().then(boot).catch(failed);
        },
      },
    });
    return;
  }
  await boot(await webVaultHost());
}

void start().catch(failed);
