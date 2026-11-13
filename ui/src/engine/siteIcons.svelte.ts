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
  /** Rises on forget, so a fetch asked for before it neither runs nor keeps what it brings. */
  private generation = 0;

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
    const generation = this.generation;
    this.queue.push(async () => {
      if (generation !== this.generation) return;
      try {
        const reply = await pass.engine('icon.fetch', { host: name, source: prefs.v.site_icon_from }, { online: true });
        const bytes = reply.blobs[0];
        const type = bytes?.length ? pictureType(bytes) : null;
        if (bytes?.length && type && generation === this.generation) {
          this.found.set(key, URL.createObjectURL(new Blob([bytes.slice()], { type })));
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
    // What was still waiting belongs to the vault that locked: it is not fetched after all.
    this.generation += 1;
    this.queue = [];
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

/**
 * What a site's icon is, by its first bytes, or null for anything that is not a picture. A blob
 * with no type of its own can be read as a page when opened in a tab of its own, and a site
 * chooses what it serves as its icon. An SVG is only ever drawn in an <img>, where it runs nothing.
 */
export function pictureType(bytes: Uint8Array): string | null {
  const starts = (...head: number[]) => head.every((b, i) => bytes[i] === b);
  const ascii = (from: number, text: string) => [...text].every((c, i) => bytes[from + i] === c.charCodeAt(0));
  if (starts(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)) return 'image/png';
  if (starts(0xff, 0xd8, 0xff)) return 'image/jpeg';
  if (ascii(0, 'GIF87a') || ascii(0, 'GIF89a')) return 'image/gif';
  if (ascii(0, 'RIFF') && ascii(8, 'WEBP')) return 'image/webp';
  if (starts(0x00, 0x00, 0x01, 0x00)) return 'image/x-icon';
  // Text that is an SVG document: past a byte-order mark and white space, an XML declaration, a
  // comment or a doctype may come before the <svg> element.
  const head = new TextDecoder().decode(bytes.subarray(0, 1024)).replace(/^\uFEFF/, '').trimStart();
  if (/^</.test(head) && /^(?:<\?xml[^>]*>\s*|<!--[\s\S]*?-->\s*|<!DOCTYPE svg[^>]*>\s*)*<svg[\s>]/i.test(head)) return 'image/svg+xml';
  return null;
}

/** A router's or NAS's page is nobody else's business. The engine refuses these too. */
function local(name: string) {
  if (!name.includes('.') || /^[\d.]+$/.test(name) || name.includes(':')) return true;
  return /\.(local|lan|home|internal|localhost|home\.arpa)$/.test(name);
}

export const siteIcons = new SiteIcons();

pass.onWipe.push(() => siteIcons.forget());
