// Font files a theme can draw with, as on the phone. They are kept in the app's folder under
// fonts/, named by key as `file:<name>`; a theme that names one this device lacks uses the system
// font. They hold nothing secret, so they are stored as they are.

import { host, type PickedFile } from '../engine/host';

const FOLDER = 'fonts';
const LIMIT = 12 * 1024 * 1024;

class ThemeFonts {
  /** The keys of the files added on this device, by name. */
  saved = $state.raw<string[]>([]);
  private families = new Map<string, Promise<string | null>>();
  private using = 0;

  async refresh() {
    const files = await host()
      .listFiles(FOLDER)
      .catch(() => []);
    this.saved = files
      .filter((f) => f.size > 0)
      .map((f) => f.name)
      .sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()))
      .map((name) => `file:${name}`);
  }

  label(key: string) {
    const name = key.replace(/^file:/, '');
    const dot = name.lastIndexOf('.');
    return dot > 0 ? name.slice(0, dot) : name;
  }

  /** The CSS family a font file draws as once loaded, or null when it is missing or unreadable. */
  family(key: string): Promise<string | null> {
    if (!key.startsWith('file:')) return Promise.resolve(null);
    let found = this.families.get(key);
    if (!found) {
      found = (async () => {
        try {
          const bytes = await host().readFile(`${FOLDER}/${key.slice(5)}`);
          return await this.install(key, bytes);
        } catch {
          return null;
        }
      })();
      this.families.set(key, found);
    }
    return found;
  }

  private async install(key: string, bytes: Uint8Array) {
    const name = `WM Theme ${this.label(key).replace(/[^A-Za-z0-9 _-]/g, '')} ${this.families.size}`;
    const face = new FontFace(name, bytes.slice().buffer);
    await face.load();
    document.fonts.add(face);
    return name;
  }

  /** Has the page draw with a theme's font file, or with the system font if it has none here. */
  async use(font: string, root: HTMLElement = document.documentElement) {
    const turn = ++this.using;
    const family = await this.family(font);
    if (turn !== this.using) return;
    if (family) root.style.setProperty('--theme-font', `"${family}"`);
    else root.style.removeProperty('--theme-font');
  }

  /** Copies a chosen font file in and checks it draws. Its key, or an error to show. */
  async add(file: PickedFile): Promise<string> {
    const name = file.name
      .trim()
      .replace(/\s+/g, '-')
      .replace(/[^A-Za-z0-9._-]/g, '')
      .replace(/^\.+/, '')
      .slice(-100);
    const ext = name.slice(name.lastIndexOf('.') + 1).toLowerCase();
    if (!name.includes('.') || !['ttf', 'otf'].includes(ext)) throw new Error('Choose a .ttf or .otf font file.');
    if (file.bytes.length === 0 || file.bytes.length > LIMIT) throw new Error('That font file is too large.');
    const key = `file:${name}`;
    this.families.delete(key);
    try {
      const family = this.install(key, file.bytes);
      this.families.set(key, family);
      await family;
    } catch {
      this.families.delete(key);
      throw new Error('This app could not read that font.');
    }
    await host().writeFile(`${FOLDER}/${name}`, file.bytes);
    await this.refresh();
    return key;
  }
}

export const themeFonts = new ThemeFonts();
