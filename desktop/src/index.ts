import './freeze';
import { boot } from '$ui/boot';
import { host } from '$ui/engine/host';
import { prefs } from '$ui/engine/prefs.svelte';
import { overlays } from '$ui/parts/floating.svelte';
import { desktopHost } from './host';

void desktopHost()
  .then(boot)
  .then(async () => {
    const desktop = host().desktop!;
    await desktop.setCloseToTray(prefs.v.close_to_tray);
    await desktop.setShortcuts(prefs.shortcuts);
    if (prefs.v.global_palette) await desktop.setGlobalShortcut(prefs.v.global_palette);
    if (prefs.v.ssh_agent) await desktop.sshSetEnabled(true, prefs.v.ssh_confirm);
    if (prefs.v.update_check) void desktop.checkUpdate().then((found) => {
      if (found) overlays.toast(`WM Pass ${found.version} is available. Install it in Settings, under Updates.`);
    }).catch(() => {});
    if (!prefs.v.start_hidden && !location.search.includes('background')) await desktop.showMain();
  });

if (import.meta.env.VITE_SELFTEST) void import('./selftest').then((test) => test.run());
