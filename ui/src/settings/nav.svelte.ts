// Which settings page is showing, the colour each one wears, and the row to point at.

import { ui } from '../vault/ui.svelte';

/** Each settings page's colour, the same as the phone's. Its rows wear it, and so does the row leading to it. */
export const ROUTE_COLOURS: Record<string, string> = {
  security: '#10B981',
  shelf: '#7C4DFF',
  breaches: '#F4511E',
  folders: '#FFB300',
  trash: '#E53935',
  import: '#1E88E5',
  export: '#00ACC1',
  backup: '#3949AB',
  sync: '#00897B',
  vaults: '#5C6BC0',
  hardware: '#43A047',
  receive: '#26A69A',
  aliases: '#8E24AA',
  inbox: '#039BE5',
  usernames: '#5E35B1',
  compare: '#8D6E63',
  tools: '#8E24AA',
  appearance: '#D81B60',
  editing: '#7CB342',
  look: '#D81B60',
  themes: '#D81B60',
  icons: '#FB8C00',
  list: '#43A047',
  navigation: '#3F51B5',
  add: '#00BFA5',
  editor: '#7CB342',
  entry: '#26A69A',
  codes: '#5C6BC0',
  generator: '#AB47BC',
  health: '#EC407A',
  shortcuts: '#78909C',
  file: '#8D6E63',
  about: '#90A4AE',
  guide: '#0097A7',
  browser: '#0288D1',
  ssh: '#546E7A',
  updates: '#00838F',
  desktop: '#6D4C41',
};

export function settingsAccent(): string | undefined {
  const page = ui.section.split('/')[1];
  return page ? ROUTE_COLOURS[page] : undefined;
}

class Highlight {
  title = $state<string | null>(null);
  private timer: ReturnType<typeof setTimeout> | undefined;
  point(title: string) {
    this.title = title;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => (this.title = null), 1400);
  }
}

export const highlight = new Highlight();
