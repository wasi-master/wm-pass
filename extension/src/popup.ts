import { mount } from 'svelte';
import '../../ui/src/app.css';
import { brands } from '../../ui/src/engine/brands.svelte';
import { setHost } from '../../ui/src/engine/host';
import { pass } from '../../ui/src/engine/pass.svelte';
import { prefs } from '../../ui/src/engine/prefs.svelte';
import { settingsSync } from '../../ui/src/engine/settingsSync.svelte';
import { accounts } from '../../ui/src/settings/accounts.svelte';
import { places } from '../../ui/src/settings/places.svelte';
import { extensionHost } from './host';
import Popup from './Popup.svelte';

pass.saveDelayMs = 0;
// The popup can close at any moment; the vault tab and the desktop app sync on their own.
places.autoSync = false;
void extensionHost().then(async (host) => {
  setHost(host);
  await prefs.load();
  settingsSync.start();
  await pass.init();
  // The door below offers these, and a KeePass file or server account means there is a vault to open.
  await Promise.all([accounts.load(), places.load()]);
  void brands.load();
  mount(Popup, { target: document.getElementById('app')! });
});
