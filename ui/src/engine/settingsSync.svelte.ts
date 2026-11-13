// Settings that follow the vault. They travel inside it, so every way the vault syncs (the local
// network, a folder or server, a WM Pass server) carries them, and a copy of the file says nothing
// about them. Each value is written one key at a time and the later write wins, the same as an
// item's fields. Only the vault in this device's folder carries them: not an account held
// elsewhere, and not a vault opened from inside another.

import { pass } from './pass.svelte';
import { prefs, type PrefKey } from './prefs.svelte';

class SettingsSync {
  private running = false;
  private again = false;

  /** Hooks into the vault's reloads and the settings' writes. Call once, after both have loaded. */
  start() {
    prefs.onSet = (keys) => {
      if (keys.includes('sync_settings') && prefs.v.sync_settings) {
        void this.pull();
        return;
      }
      const values: Partial<Record<PrefKey, unknown>> = {};
      for (const key of keys) if (prefs.syncs(key)) values[key] = prefs.v[key];
      if (Object.keys(values).length) void this.push(values);
    };
    pass.afterRefresh = () => void this.pull();
  }

  private get open() {
    return pass.handle !== 0 && !!pass.file && pass.account === null && pass.child === null;
  }

  /**
   * Takes what other devices left, then leaves anything this device has set that the vault has
   * never held, so the first device to sync brings its settings with it.
   */
  async pull() {
    if (!this.open || !prefs.v.sync_settings) return;
    if (this.running) {
      this.again = true;
      return;
    }
    this.running = true;
    try {
      do {
        this.again = false;
        const held = await pass.view<Record<string, unknown>>('prefs.synced');
        prefs.applySynced(held);
        const missing: Record<string, unknown> = {};
        for (const key of prefs.syncable) {
          if (prefs.syncs(key) && !(key in held) && prefs.v[key] !== prefs.defaultOf(key)) missing[key] = prefs.v[key];
        }
        if (Object.keys(missing).length) await this.write(missing);
      } while (this.again && this.open);
    } catch {
      // A vault that cannot take them now gets them on the next change.
    } finally {
      this.running = false;
    }
  }

  private async push(values: Partial<Record<PrefKey, unknown>>) {
    if (!this.open || !prefs.v.sync_settings) return;
    try {
      await this.write(values);
    } catch {
      // Read-only, or locked meanwhile. The next pull leaves it if it still differs.
    }
  }

  private async write(values: Record<string, unknown>) {
    const reply = await pass.engine<{ changed: number }>('prefs.sync', { values }, { handle: pass.handle });
    if (reply.json.changed > 0) pass.flushSoon();
  }
}

export const settingsSync = new SettingsSync();
