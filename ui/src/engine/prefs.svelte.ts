// Preferences that are not secret. They live outside the vault so the door can be drawn in the
// right colours before it opens. Nothing here names a site or an account. Keys match the phone's
// where the meaning matches, so a settings file travels between them.

import { host } from './host';
import { BUILT_IN, DEFAULT_THEME, readTheme, themeToText, type ThemeFile } from '../theme/themes';

type Value = boolean | number | string;

interface Def {
  value: Value;
  /** Settings that weaken protection never come from a file someone else made. */
  portable?: boolean;
  allowed?: readonly string[];
}

const SETTINGS = 'prefs';
const IMPORT_LIMIT = 200_000;
export const DEFAULT_TABS = ['all', 'favourites', 'codes', 'generator', 'health', 'trash'];
export const ALL_TABS = ['all', 'favourites', 'codes', 'generator', 'health', 'trash', 'shelf'];
export const DEFAULT_ADD = ['login', 'otp', 'note', 'card', 'identity', 'recovery', 'wifi', 'ssh', 'api', 'database', 'licence', 'document'];

export const DEFAULT_SHORTCUTS: Record<string, string> = {
  search: 'Mod+F',
  add: 'Mod+N',
  lock: 'Mod+L',
  edit: 'Mod+E',
  generator: 'Mod+G',
  codes: 'Mod+Shift+C',
  health: 'Mod+Shift+H',
  settings: 'Mod+Comma',
  copy_password: 'Mod+Shift+P',
  copy_username: 'Mod+Shift+U',
  copy_code: 'Mod+Shift+O',
  open_site: 'Mod+Shift+L',
  toggle_sidebar: 'Mod+Backslash',
  palette: 'Mod+K',
  help: 'F1',
};

const DEFS = {
  // ---- look ----
  look: { value: 'SYSTEM', allowed: ['SYSTEM', 'DARK', 'LIGHT', 'OBSIDIAN'] },
  theme_id: { value: DEFAULT_THEME },
  custom_themes: { value: '' },
  rich_motion: { value: true },
  colour_secrets: { value: true },
  text_scale: { value: 100 },
  density: { value: 'comfortable', allowed: ['compact', 'comfortable', 'spacious'] },

  // ---- icons ----
  icon_style: { value: 'rounded', allowed: ['outlined', 'rounded', 'sharp', 'light', 'phosphor', 'phosphor_bold', 'phosphor_light', 'lucide'] },
  site_icon_style: { value: 'brand', allowed: ['brand', 'tinted', 'uniform', 'letters'] },
  site_icon_shape: { value: 'circle', allowed: ['circle', 'rounded', 'square', 'none'] },
  settings_icons: { value: 'coloured', allowed: ['coloured', 'mono', 'bare', 'off'] },

  // ---- vault list ----
  layout: { value: 'list', allowed: ['list', 'compact', 'grid'] },
  grouping: { value: 'smart', allowed: ['smart', 'folder', 'types'] },
  sort: { value: 'name', allowed: ['name', 'changed', 'kind', 'site'] },
  group_sites: { value: true },
  pinned_on_top: { value: true },
  list_headings: { value: true },
  row_icons: { value: true },
  row_subtitle: { value: true },
  row_badges: { value: true },
  row_button: { value: 'password', allowed: ['password', 'username', 'code', 'none'] },
  tap_action: { value: 'open', allowed: ['open', 'password', 'username', 'code'] },
  double_click: { value: 'password', allowed: ['password', 'username', 'edit', 'open_site', 'none'] },

  // ---- window ----
  tabs: { value: DEFAULT_TABS.join(',') },
  sidebar_kinds: { value: true },
  sidebar_folders: { value: true },
  sidebar_tags: { value: true },
  sidebar_counts: { value: true },
  sidebar_open: { value: true },
  sidebar_width: { value: 216 },
  list_width: { value: 320 },
  start_tab: { value: 'first', allowed: ['first', 'last'] },
  last_tab: { value: '' },
  show_lock: { value: true },

  // ---- adding and editing ----
  add_kinds: { value: DEFAULT_ADD.join(',') },
  editor_open: { value: 'sites' },
  editor_remember: { value: true },
  editor_fold_details: { value: true },
  editor_generate: { value: false },

  // ---- entry page ----
  entry_actions: { value: true },
  entry_action_username: { value: true },
  entry_action_open: { value: true },
  tap_copies: { value: true },
  reveal_seconds: { value: 45 },

  // ---- codes ----
  code_groups: { value: true },
  code_size: { value: 'normal', allowed: ['small', 'normal', 'large'] },
  code_next: { value: true },
  code_ring: { value: true },
  code_hidden: { value: false },
  code_sort: { value: 'default', allowed: ['default', 'name'] },

  // ---- generator ----
  gen_mode: { value: 'password', allowed: ['password', 'passphrase', 'pronounceable', 'pin', 'pattern'] },
  gen_length: { value: 20 },
  gen_words: { value: 4 },
  gen_symbols: { value: true },
  gen_no_ambiguous: { value: false },
  gen_show_bits: { value: true },

  // ---- health ----
  health_hidden: { value: '' },
  health_score: { value: true },

  // ---- widgets on a Mac: what they show stays on this computer ----
  widget_buttons: { value: 'search,add,codes,lock' },
  // A settings file from someone else must not turn the widget's lock into an hour's lockout.
  widget_lock: { value: 'lock', allowed: ['lock', 'panic'], portable: false },
  widget_access: { value: false, portable: false },
  widget_codes: { value: '', portable: false },
  widget_codes_mode: { value: 'upcoming', allowed: ['upcoming', 'current'], portable: false },
  desktop_widgets: { value: true, portable: false },

  // ---- settings pages ----
  folds_open: { value: '' },
  shortcuts_desktop: { value: '' },

  // ---- the guide ----
  help_buttons: { value: 'hover', allowed: ['always', 'hover', 'off'] },
  help_hints: { value: true },
  // What this install has already been shown. Not a customisation, so not in a settings file.
  help_welcome_seen: { value: false, portable: false },
  help_seen: { value: '', portable: false },
  help_tours_done: { value: '', portable: false },

  // ---- settings sync: each device decides for itself ----
  sync_settings: { value: true, portable: false },
  sync_skip: { value: '', portable: false },

  // ---- protection: never taken from a settings file, never reset with the look ----
  lock_after: { value: 300, portable: false },
  lock_on_sleep: { value: true, portable: false },
  lock_on_hide: { value: false, portable: false },
  clipboard_seconds: { value: 60, portable: false },
  screen_protection: { value: true, portable: false },
  network_allowed: { value: false, portable: false },
  close_to_tray: { value: true, portable: false },
  start_hidden: { value: false, portable: false },
  bridge_enabled: { value: false, portable: false },
  bridge_ask_every_site: { value: true, portable: false },
  ssh_agent: { value: false, portable: false },
  ssh_confirm: { value: true, portable: false },
  auto_type_enabled: { value: false, portable: false },
  update_check: { value: false, portable: false },
  global_palette: { value: 'Mod+Shift+Space', portable: false },
} satisfies Record<string, Def>;

export type PrefKey = keyof typeof DEFS;

/**
 * Customisations that describe this window rather than the person, or mean something different
 * on the phone under the same name. They never travel with the vault.
 */
const DEVICE_ONLY: ReadonlySet<string> = new Set(['tabs', 'last_tab', 'folds_open', 'editor_open', 'sidebar_open', 'sidebar_width', 'list_width', 'widget_buttons']);
type Widen<T> = T extends boolean ? boolean : T extends number ? number : string;
type PrefValues = { [K in PrefKey]: Widen<(typeof DEFS)[K]['value']> };

function defaults(): PrefValues {
  const out: Record<string, Value> = {};
  for (const [key, def] of Object.entries(DEFS)) out[key] = def.value;
  return out as PrefValues;
}

class Prefs {
  v = $state<PrefValues>(defaults());
  /** Rises after a theme reaches the page, for what reads its colours back. */
  themeApplied = $state(0);
  /** True when this install had settings from before the guide existed. */
  readonly upgraded: boolean = false;
  private loaded = false;

  async load() {
    const raw = await host().loadSettings(SETTINGS);
    if (raw) {
      try {
        const parsed = JSON.parse(raw);
        for (const [key, def] of Object.entries(DEFS)) {
          const value = parsed[key];
          if (this.fits(def, value)) (this.v as any)[key] = value;
        }
        // Someone who has used the app for months does not need a welcome. They get one pointer
        // to the Guide instead.
        if (!('help_welcome_seen' in parsed)) {
          this.v.help_welcome_seen = true;
          (this as { upgraded: boolean }).upgraded = true;
        }
      } catch {
        // Defaults stand in for a damaged file.
      }
    }
    this.loaded = true;
  }

  /** The value a setting has until someone changes it. */
  defaultOf(key: PrefKey): Value {
    return DEFS[key].value;
  }

  has(key: string): key is PrefKey {
    return key in DEFS;
  }

  private fits(def: Def, value: unknown): value is Value {
    if (typeof value !== typeof def.value) return false;
    if (def.allowed && !def.allowed.includes(value as string)) return false;
    if (typeof value === 'number' && !Number.isFinite(value)) return false;
    return true;
  }

  private persist() {
    if (!this.loaded) return;
    void host().saveSettings(SETTINGS, JSON.stringify(this.v));
  }

  /** Told about every setting someone changes, for the vault to carry it to other devices. */
  onSet: ((key: PrefKey) => void) | null = null;

  set<K extends PrefKey>(key: K, value: PrefValues[K]) {
    (this.v as any)[key] = value;
    this.persist();
    this.onSet?.(key);
  }

  // ---- settings that follow the vault ----

  /** Every setting that can follow the vault, whether or not this device lets it. */
  get syncable(): PrefKey[] {
    return (Object.entries(DEFS) as [PrefKey, Def][]).filter(([key, def]) => def.portable !== false && !DEVICE_ONLY.has(key)).map(([key]) => key);
  }

  /** Whether this setting goes to and comes from the vault on this device. */
  syncs(key: string): key is PrefKey {
    if (!this.v.sync_settings || !this.has(key)) return false;
    return this.syncable.includes(key) && !this.listOf('sync_skip').includes(key);
  }

  /**
   * Takes settings another device left in the vault. Each one gets the checks a settings file
   * gets, and own themes keep their ids so the theme chosen with them is still found. Returns how
   * many changed. Nothing here is sent back.
   */
  applySynced(values: Record<string, unknown>) {
    let changed = 0;
    for (const [key, raw] of Object.entries(values)) {
      if (!this.syncs(key)) continue;
      let value = raw;
      if (key === 'custom_themes') value = this.cleanThemes(raw);
      if (key === 'theme_id' && raw === 'system') continue;
      if (value === undefined || !this.fits(DEFS[key], value) || value === this.v[key]) continue;
      (this.v as any)[key] = value;
      changed++;
    }
    if (changed) this.persist();
    return changed;
  }

  private cleanThemes(raw: unknown): string | undefined {
    if (typeof raw !== 'string') return undefined;
    if (!raw) return '';
    let list: ThemeFile[];
    try {
      list = JSON.parse(raw);
    } catch {
      return undefined;
    }
    if (!Array.isArray(list)) return undefined;
    const clean: ThemeFile[] = [];
    for (const theme of list) {
      try {
        const checked = readTheme(themeToText(theme));
        clean.push(typeof theme.id === 'string' && /^custom_[\w-]{1,40}$/.test(theme.id) ? { ...checked, id: theme.id } : checked);
      } catch {
        // A theme that fails the checks stays behind.
      }
    }
    return JSON.stringify(clean);
  }

  // ---- lists kept as text ----

  listOf(key: PrefKey): string[] {
    return String(this.v[key])
      .split(',')
      .filter((s) => s.length > 0);
  }

  setList(key: PrefKey, list: string[]) {
    this.set(key, list.join(',') as never);
  }

  toggleInList(key: PrefKey, item: string, on: boolean) {
    const list = this.listOf(key).filter((s) => s !== item);
    if (on) list.push(item);
    this.setList(key, list);
  }

  // ---- themes ----

  get customThemes(): ThemeFile[] {
    if (!this.v.custom_themes) return [];
    try {
      return JSON.parse(this.v.custom_themes);
    } catch {
      return [];
    }
  }

  get allThemes(): ThemeFile[] {
    return [...BUILT_IN, ...this.customThemes];
  }

  get theme(): ThemeFile {
    return this.allThemes.find((t) => t.id === this.v.theme_id) ?? BUILT_IN[0];
  }

  saveTheme(theme: ThemeFile) {
    const others = this.customThemes.filter((t) => t.id !== theme.id);
    this.set('custom_themes', JSON.stringify([...others, theme]));
  }

  deleteTheme(id: string) {
    this.set('custom_themes', JSON.stringify(this.customThemes.filter((t) => t.id !== id)));
    if (this.v.theme_id === id) this.set('theme_id', DEFAULT_THEME);
  }

  // ---- shortcuts ----

  get shortcuts(): Record<string, string> {
    const out = { ...DEFAULT_SHORTCUTS };
    for (const line of this.v.shortcuts_desktop.split('\n')) {
      const at = line.indexOf('=');
      if (at > 0) out[line.slice(0, at)] = line.slice(at + 1);
    }
    return out;
  }

  /** One chord does one thing, so setting it takes it away from whatever had it. */
  setShortcut(action: string, chord: string) {
    const all = this.shortcuts;
    for (const key of Object.keys(all)) if (chord && all[key] === chord) all[key] = '';
    all[action] = chord;
    this.set('shortcuts_desktop', Object.entries(all).map(([k, v]) => `${k}=${v}`).join('\n'));
  }

  // ---- settings files ----

  /** Every customisation as a file. Protection settings are left out on purpose. */
  exportText() {
    const out: Record<string, unknown> = { format: 'wmpass-settings', version: 1 };
    for (const [key, def] of Object.entries(DEFS) as [PrefKey, Def][]) {
      if (def.portable !== false) out[key] = this.v[key];
    }
    return JSON.stringify(out, null, 2);
  }

  /**
   * Applies a settings file, which may have come from anyone. Unknown keys and values this app
   * does not understand are skipped. Returns how many settings changed.
   */
  importText(text: string) {
    if (text.length > IMPORT_LIMIT) throw new Error('That file is too long to be settings.');
    let json: Record<string, any>;
    try {
      json = JSON.parse(text);
    } catch {
      throw new Error('That is not a WM Pass settings file.');
    }
    if (json?.format !== 'wmpass-settings') throw new Error('That is not a WM Pass settings file.');
    const renamed: Record<string, string> = {};
    if (typeof json.custom_themes === 'string' && json.custom_themes) {
      // Themes in the file get the same checks as a theme imported on its own, including a new
      // id, so a theme the file chooses has to be found under that new id.
      let list: ThemeFile[] = [];
      try {
        list = JSON.parse(json.custom_themes);
      } catch {
        list = [];
      }
      const incoming: ThemeFile[] = [];
      for (const theme of list) {
        try {
          const clean = readTheme(themeToText(theme));
          renamed[theme.id] = clean.id;
          incoming.push(clean);
        } catch {
          // A bad theme is skipped like any other bad value.
        }
      }
      json.custom_themes = JSON.stringify([...this.customThemes.filter((m) => !incoming.some((t) => t.name === m.name)), ...incoming]);
    }
    if (renamed[json.theme_id]) json.theme_id = renamed[json.theme_id];
    if (json.theme_id === 'system') delete json.theme_id;
    let changed = 0;
    for (const [key, def] of Object.entries(DEFS) as [PrefKey, Def][]) {
      if (def.portable === false || !(key in json)) continue;
      if (this.fits(def, json[key]) && json[key] !== this.v[key]) {
        (this.v as any)[key] = json[key];
        changed++;
      }
    }
    this.persist();
    return changed;
  }

  /** Puts every customisation back. Your own themes and protection settings stay. */
  resetCustomisations() {
    for (const [key, def] of Object.entries(DEFS) as [PrefKey, Def][]) {
      if (def.portable !== false && key !== 'custom_themes') (this.v as any)[key] = def.value;
    }
    this.persist();
  }
}

export const prefs = new Prefs();
