// Websites' own icons, for tiles when Settings, Icons puts website icons in the order. They come
// through the engine, so the network switch and the activity log cover them, and are never asked
// for addresses on the local network. They stay in memory until the app quits.

import { host } from './host';
import { pass } from './pass.svelte';
import { prefs } from './prefs.svelte';

const MOST_AT_ONCE = 4;

class SiteIcons {
  /** Rises when an icon arrives, so tiles draw again. */
  ready = $state(0);
  private found = new Map<string, string | null>();
  private queue: (() => Promise<void>)[] = [];
  private running = 0;

  /** Whether this shell may fetch them at all. */
  get possible() {
    return host().network === true;
  }

  /** The picture for a host, once fetched. Asks for it if not, when allowed. */
  picture(site: string): string | null {
    const name = site.trim().toLowerCase().replace(/\.$/, '');
    const key = `${prefs.v.site_icon_from}:${name}`;
    if (this.found.has(key)) return this.found.get(key) ?? null;
    if (!this.possible || !prefs.v.network_allowed || local(name)) return null;
    this.found.set(key, null);
    this.queue.push(async () => {
      try {
        const reply = await pass.engine('icon.fetch', { host: name, source: prefs.v.site_icon_from }, { online: true });
        const bytes = reply.blobs[0];
        if (bytes?.length) {
          this.found.set(key, URL.createObjectURL(new Blob([bytes.slice()])));
          this.ready += 1;
        }
      } catch {
        // No icon then; the tile keeps its mark or letter.
      }
    });
    this.next();
    return null;
  }

  /** Forgets every icon, as when website icons are taken out of the order. */
  forget() {
    for (const url of this.found.values()) if (url) URL.revokeObjectURL(url);
    this.found.clear();
    this.ready += 1;
  }

  private next() {
    while (this.running < MOST_AT_ONCE && this.queue.length) {
      const work = this.queue.shift()!;
      this.running += 1;
      void work().finally(() => {
        this.running -= 1;
        this.next();
      });
    }
  }
}

/** A router's or NAS's page is nobody else's business. The engine refuses these too. */
function local(name: string) {
  if (!name.includes('.') || /^[\d.]+$/.test(name) || name.includes(':')) return true;
  return /\.(local|lan|home|internal|localhost|home\.arpa)$/.test(name);
}

export const siteIcons = new SiteIcons();

pass.onWipe.push(() => siteIcons.forget());
