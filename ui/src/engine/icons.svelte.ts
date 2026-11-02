// Every glyph the app draws, in the style chosen under Icons. Only the current style is loaded.

import outlined from '../icons/outlined';

export type IconName = keyof typeof outlined;
type Table = Record<string, string>;

const loaders: Record<string, () => Promise<{ default: Table }>> = {
  rounded: () => import('../icons/rounded'),
  sharp: () => import('../icons/sharp'),
  light: () => import('../icons/light'),
};

class Icons {
  table = $state.raw<Table>(outlined);
  private cache: Record<string, Table> = { outlined };

  async setStyle(style: string) {
    this.table = await this.tableOf(style);
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
