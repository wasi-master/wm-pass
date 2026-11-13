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

  private asked = 0;

  async setStyle(style: string) {
    // A style chosen while another is still loading wins, however the two loads finish.
    const turn = ++this.asked;
    const got = await this.load(style);
    if (turn !== this.asked) return;
    this.table = got.table;
    this.style = got.style;
  }

  /** A style's glyphs, or the outlined ones when it is unknown or its file would not load. */
  async tableOf(style: string) {
    return (await this.load(style)).table;
  }

  /**
   * A style's glyphs with the style they really are: outlined when the one asked for is unknown or
   * would not load, since each style is drawn differently and glyphs must go with their own.
   */
  async load(style: string): Promise<{ style: string; table: Table }> {
    if (!this.cache[style]) {
      const load = loaders[style];
      if (!load) return { style: 'outlined', table: outlined };
      try {
        this.cache[style] = (await load()).default;
      } catch (failure) {
        console.warn(`The ${style} icons could not be loaded`, failure);
        return { style: 'outlined', table: outlined };
      }
    }
    return { style, table: this.cache[style] };
  }
}

export const icons = new Icons();
