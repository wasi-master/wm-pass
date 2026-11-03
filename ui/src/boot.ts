// Starts the app in a page, once a shell has said what it can do.

import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { brands } from './engine/brands.svelte';
import { setHost, type Host } from './engine/host';
import { pass } from './engine/pass.svelte';
import { prefs } from './engine/prefs.svelte';
import { settingsSync } from './engine/settingsSync.svelte';
import { accounts } from './settings/accounts.svelte';
import { places } from './settings/places.svelte';

export async function boot(host: Host) {
  setHost(host);
  await prefs.load();
  settingsSync.start();
  await pass.init();
  await Promise.all([accounts.load(), places.load()]);
  void brands.load();
  if (host.os === 'mac' && host.kind === 'desktop') document.documentElement.dataset.titlebar = 'overlay';
  return mount(App, { target: document.getElementById('app')! });
}
