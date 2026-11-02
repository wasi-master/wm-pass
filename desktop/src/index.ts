import { boot } from '$ui/boot';
import { host } from '$ui/engine/host';
import { prefs } from '$ui/engine/prefs.svelte';
import { desktopHost } from './host';

void desktopHost()
  .then(boot)
  .then(async () => {
    const desktop = host().desktop!;
    await desktop.setCloseToTray(prefs.v.close_to_tray);
    await desktop.setShortcuts(prefs.shortcuts);
    if (prefs.v.global_palette) await desktop.setGlobalShortcut(prefs.v.global_palette);
    if (prefs.v.ssh_agent) await desktop.sshSetEnabled(true, prefs.v.ssh_confirm);
    if (prefs.v.bridge_enabled) await desktop.bridgeSetEnabled(true);
    if (!prefs.v.start_hidden && !location.search.includes('background')) await desktop.showMain();
  });

if (import.meta.env.VITE_SELFTEST) void import('./selftest').then((test) => test.run());
