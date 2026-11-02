import { mount } from 'svelte';
import '$ui/app.css';
import { setHost } from '$ui/engine/host';
import { prefs } from '$ui/engine/prefs.svelte';
import { brands } from '$ui/engine/brands.svelte';
import Palette from '$ui/palette/Palette.svelte';
import { desktopHost } from './host';

void desktopHost().then(async (host) => {
  setHost(host);
  await prefs.load();
  void brands.load();
  mount(Palette, { target: document.getElementById('app')! });
});
