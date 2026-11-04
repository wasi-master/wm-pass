// Tags across the vault: their symbols, how often each is used, and renaming or deleting one on
// every item that has it.

import type { IconName } from './icons.svelte';
import { tagGlyph } from './glyphs';
import { pass } from './pass.svelte';

class Tags {
  /** The symbol picked for each tag, kept in the vault. */
  icons = $state<Record<string, string>>({});

  async refresh() {
    try {
      this.icons = await pass.view<Record<string, string>>('tags.icons');
    } catch {
      this.icons = {};
    }
  }

  glyph(tag: string): IconName {
    return tagGlyph(tag, this.icons);
  }

  /** Every tag, the most used first. */
  mostUsed(): string[] {
    const counts = new Map<string, number>();
    for (const e of pass.entries) for (const t of e.tags) counts.set(t, (counts.get(t) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].toLowerCase().localeCompare(b[0].toLowerCase())).map(([t]) => t);
  }

  holders(tag: string): string[] {
    return pass.entries.filter((e) => e.tags.includes(tag)).map((e) => e.id);
  }

  async setIcon(tag: string, icon: string | null) {
    await pass.change('tags.set_icon', { tag, icon });
    await this.refresh();
  }

  /** Renames a tag on every item that has it, and carries its symbol over. */
  async rename(from: string, to: string) {
    await pass.change('entries.bulk', { ids: this.holders(from), op: 'tag_rename', tag: from, to });
    const icon = this.icons[from];
    if (icon) {
      await pass.change('tags.set_icon', { tag: to, icon });
      await pass.change('tags.set_icon', { tag: from, icon: null });
    }
    await this.refresh();
  }

  /** Takes a tag off every item. The items stay. */
  async remove(tag: string) {
    await pass.change('entries.bulk', { ids: this.holders(tag), op: 'tag_remove', tag });
    if (this.icons[tag]) await pass.change('tags.set_icon', { tag, icon: null });
    await this.refresh();
  }
}

export const tags = new Tags();
