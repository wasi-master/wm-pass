import { mount } from 'svelte';
import '../../ui/src/app.css';
import { setHost } from '../../ui/src/engine/host';
import { prefs } from '../../ui/src/engine/prefs.svelte';
import { extensionHost } from './host';
import Confirm from './Confirm.svelte';

void extensionHost().then(async (host) => {
  setHost(host);
  await prefs.load();
  mount(Confirm, { target: document.getElementById('app')! });
});
