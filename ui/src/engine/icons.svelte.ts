// Every glyph the app draws, in the style chosen under Icons. Only the current style is loaded.

import outlined from '../icons/outlined';

export type IconName = keyof typeof outlined;
type Table = Record<string, string>;

const loaders: Record<string, () => Promise<{ default: Table }>> = {
  rounded: () => import('../icons/rounded'),
  sharp: () => import('../icons/sharp'),
  light: () => import('../icons/light'),
  phosphor: () => import('../icons/phosphor'),
  phosphor_bold: () => import('../icons/phosphor_bold'),
  phosphor_light: () => import('../icons/phosphor_light'),
  lucide: () => import('../icons/lucide'),
};

/** The packs drawn with lines, and how wide, on the same 960-unit grid as everything else. */
const STROKES: Record<string, number> = { lucide: 80 };

/** How wide a style's lines are, or 0 for a style drawn as filled shapes. */
export const strokeOf = (style: string): number => STROKES[style] ?? 0;

class Icons {
  table = $state.raw<Table>(outlined);
  /** The style [table] is in, which is `outlined` until the chosen one has loaded. */
  style = $state('outlined');
  private cache: Record<string, Table> = { outlined };

  async setStyle(style: string) {
    const table = await this.tableOf(style);
    this.table = table;
    this.style = loaders[style] ? style : 'outlined';
  }

  async tableOf(style: string) {
    if (!this.cache[style]) {
      const load = loaders[style];
      if (!load) return outlined;
      this.cache[style] = (await load()).default;
    }
    return this.cache[style];
  }
}

export const icons = new Icons();
