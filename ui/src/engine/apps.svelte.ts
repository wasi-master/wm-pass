// Names of well-known Android apps, from the engine's table, so a login's apps read as apps.

import { pass } from './pass.svelte';

class Apps {
  private names = $state<Record<string, string>>({});
  private started = false;

  /** The app's name, or null when the table does not know it. Asks for the table once. */
  nameOf(pkg: string): string | null {
    if (!this.started) {
      this.started = true;
      pass
        .engine<{ package: string; name: string }[]>('related.apps')
        .then((reply) => {
          this.names = Object.fromEntries(reply.json.map((app) => [app.package, app.name]));
        })
        .catch(() => undefined);
    }
    return this.names[pkg] ?? null;
  }
}

export const apps = new Apps();
