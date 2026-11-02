// A site's mark and colour, from data/brands. Bundled, so showing one asks nobody anything.

export interface Brand {
  slug: string;
  name: string;
  color: string;
  otherNames: string[];
}

class Brands {
  /** Rises when the list or a drawing arrives, so tiles draw again. */
  ready = $state(0);
  private bySlugMap = new Map<string, Brand>();
  private byDomain = new Map<string, Brand>();
  private byName = new Map<string, Brand>();
  private chunks = new Map<string, Map<string, string>>();
  private loading = new Map<string, Promise<void>>();
  private started = false;
  base = 'brands';

  private normal(text: string) {
    return text.toLowerCase().replace(/[^\p{L}\p{N}]/gu, '');
  }

  async load() {
    if (this.started) return;
    this.started = true;
    try {
      const text = await (await fetch(`${this.base}/brands.tsv`)).text();
      for (const line of text.split('\n')) {
        const parts = line.split('\t');
        if (parts.length < 4) continue;
        const others = (parts[4] ?? '').split(',').filter(Boolean);
        const brand: Brand = { slug: parts[0], name: parts[1], color: `#${parts[2]}`, otherNames: others };
        this.bySlugMap.set(brand.slug, brand);
        // The well-known sites come first in the file and keep their claim on a name.
        const claim = (key: string) => {
          if (key && !this.byName.has(key)) this.byName.set(key, brand);
        };
        claim(this.normal(brand.name));
        claim(brand.slug);
        others.forEach((o) => claim(this.normal(o)));
        for (const domain of parts[3].split(',').filter(Boolean)) {
          if (!this.byDomain.has(domain)) this.byDomain.set(domain, brand);
        }
      }
      this.ready += 1;
    } catch {
      // No marks then. Every tile has a letter to fall back on.
    }
  }

  all() {
    return [...this.bySlugMap.values()];
  }

  bySlug(slug: string) {
    return this.bySlugMap.get(slug) ?? null;
  }

  /**
   * By the site if there is one, then by the name: an authenticator often has only a name. Failing
   * both, a site's own name often is the brand, as with amazon.co.uk or spotify.se.
   */
  find(site: string | null, title: string): Brand | null {
    const whole = site?.toLowerCase().replace(/^www\./, '');
    let host = whole;
    while (host?.includes('.')) {
      const hit = this.byDomain.get(host);
      if (hit) return hit;
      host = host.slice(host.indexOf('.') + 1);
    }
    const named = this.byName.get(this.normal(title));
    if (named) return named;
    const labels = whole?.split('.').slice(0, -1) ?? [];
    const label = [...labels].reverse().find((l) => l.length > 3) ?? (labels.length === 1 ? labels[0] : undefined);
    return label ? (this.byName.get(this.normal(label)) ?? null) : null;
  }

  /** The drawing, once its file has arrived. Asks for the file if not. */
  drawing(slug: string): string | null {
    const letter = slug[0];
    const chunk = this.chunks.get(letter);
    if (chunk) return chunk.get(slug) ?? null;
    if (!this.loading.has(letter)) {
      this.loading.set(
        letter,
        fetch(`${this.base}/brand-paths/${letter}.tsv`)
          .then((r) => r.text())
          .then((text) => {
            const map = new Map<string, string>();
            for (const line of text.split('\n')) {
              const at = line.indexOf('\t');
              if (at > 0) map.set(line.slice(0, at), line.slice(at + 1));
            }
            this.chunks.set(letter, map);
            this.ready += 1;
          })
          .catch(() => undefined),
      );
    }
    return null;
  }
}

export const brands = new Brands();
