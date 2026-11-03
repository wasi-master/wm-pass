import { mount } from 'svelte';
import '../../ui/src/app.css';
import { brands } from '../../ui/src/engine/brands.svelte';
import { setHost } from '../../ui/src/engine/host';
import { pass } from '../../ui/src/engine/pass.svelte';
import { prefs } from '../../ui/src/engine/prefs.svelte';
import { settingsSync } from '../../ui/src/engine/settingsSync.svelte';
import { extensionHost } from './host';
import Popup from './Popup.svelte';

pass.saveDelayMs = 0;
void extensionHost().then(async (host) => {
  setHost(host);
  await prefs.load();
  settingsSync.start();
  await pass.init();
  void brands.load();
  mount(Popup, { target: document.getElementById('app')! });
});
