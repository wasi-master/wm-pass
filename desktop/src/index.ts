import './freeze';
import { invoke } from '@tauri-apps/api/core';
import { boot } from '$ui/boot';
import { host } from '$ui/engine/host';
import { prefs } from '$ui/engine/prefs.svelte';
import { overlays } from '$ui/parts/floating.svelte';
import { desktopHost } from './host';

let booted = false;

void desktopHost()
  .then(boot)
  .then(async () => {
    booted = true;
    const desktop = host().desktop!;
    // Each step stands alone: one that fails, such as the tray, must not leave the shortcuts or the
    // SSH agent unset with it.
    const step = (what: string, run: () => Promise<unknown>) => run().catch((failure) => console.warn(`${what} could not be set up`, failure));
    try {
      await step('Closing to the tray', () => desktop.setCloseToTray(prefs.v.close_to_tray));
      await step('The shortcuts', () => desktop.setShortcuts(prefs.shortcuts));
      await step('The quick search shortcut', async () => {
        if (prefs.v.global_palette && !(await desktop.setGlobalShortcut(prefs.v.global_palette))) {
          overlays.toast('The quick search shortcut is taken by something else on this computer. Choose another in Settings, under Desktop.');
        }
      });
      if (prefs.v.ssh_agent) {
        await desktop.sshSetEnabled(true, prefs.v.ssh_confirm).catch((failure: any) => {
          overlays.toast(`The SSH agent could not start: ${failure?.message ?? failure}`);
        });
      }
      // Only with network features allowed: off means nothing leaves this computer, the check included.
      if (prefs.v.update_check && prefs.v.network_allowed) void desktop.checkUpdate().then((found) => {
        if (found) overlays.toast(`WM Pass ${found.version} is available. Install it in Settings, under Updates.`);
      }).catch(() => {});
    } finally {
      // Start hidden is what keeps it in the tray, at sign-in or not, as the help says. The window
      // comes even when a step above failed, rather than an app running with no window at all.
      if (!prefs.v.start_hidden) await desktop.showMain();
    }
  })
  .catch((failure) => {
    console.error('WM Pass did not finish starting', failure);
    // The window starts hidden, and a page that failed before it was up would never show it: the
    // app would run with no window at all. Showing it lets the failure be seen.
    if (!booted) void invoke('show_main').catch(() => {});
  });

if (import.meta.env.VITE_SELFTEST) void import('./selftest').then((test) => test.run());
