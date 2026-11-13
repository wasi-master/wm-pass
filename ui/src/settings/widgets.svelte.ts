// What the Mac's widgets may show, sent to the desktop app whenever it changes. Nothing here is
// secret: the app fetches codes from the session itself, and it drops codes, favorites and the
// vault's name unless the vault is open and Show on widgets is on (desktop/src-tauri/src/widgets.rs).

import { hueOf, oklch } from '../engine/face';
import { host, type WidgetFeed } from '../engine/host';
import { pass } from '../engine/pass.svelte';
import { prefs } from '../engine/prefs.svelte';
import { healthNow } from '../vault/health.svelte';
import { places } from './places.svelte';
import { nothingToUnlock } from './accounts.svelte';

const MAX_CODES = 8;
const MAX_FAVORITES = 12;
/** The last health counts, kept so a locked computer still shows them. Not secret. */
const HEALTH_FILE = 'widget-health';

export const WIDGET_BUTTONS: Record<string, string> = {
  search: 'Search',
  add: 'Add',
  codes: 'Codes',
  generator: 'Generator',
  health: 'Health',
  lock: 'Lock',
};

let health = $state<WidgetFeed['health']>(null);

/** What the Generator widget copies: the generator's own settings, as its page starts them. */
function recipe(): unknown {
  switch (prefs.v.gen_mode) {
    case 'passphrase':
      return { Passphrase: { words: prefs.v.gen_words, separator: '-', capitalize: false, include_digit: true } };
    case 'pronounceable':
      return { Pronounceable: { syllables: 6, separator: '' } };
    case 'pin':
      return { Pin: { length: 6 } };
    case 'pattern':
      return { Pattern: 'Cvcv-9999' };
    default:
      return {
        Password: {
          length: Math.min(64, Math.max(8, prefs.v.gen_length)),
          lower: true,
          upper: true,
          digits: true,
          symbols: prefs.v.gen_symbols,
          exclude_ambiguous: prefs.v.gen_no_ambiguous,
        },
      };
  }
}

/** The ids the Codes widget shows: the chosen ones, or the favorites that have a code. */
export function widgetCodeIds(): string[] {
  const chosen = prefs.listOf('widget_codes');
  // Nothing on the hidden shelf: it stays out of every list until opened with the password.
  const live = pass.entries.filter((e) => e.has_otp && !e.trashed && !e.hidden);
  const ids = chosen.length ? chosen.filter((id) => live.some((e) => e.id === id)) : live.filter((e) => e.favourite).map((e) => e.id);
  return ids.slice(0, MAX_CODES);
}

function feed(): WidgetFeed {
  const kind = pass.state.kind;
  const open = kind === 'unlocked';
  const allowed = open && prefs.v.widget_access;
  const runs = places.runs;
  return {
    enabled: prefs.v.desktop_widgets,
    state: open ? 'open' : nothingToUnlock() ? 'none' : 'locked',
    vault: open && pass.state.kind === 'unlocked' ? pass.state.name : null,
    health,
    show_score: prefs.v.health_score,
    backups: places.list
      .filter((p) => p.backup)
      .map((p) => ({ label: p.label || p.kind, last: runs[p.id]?.at_ms ?? null, failed: !!runs[p.id]?.problem })),
    every_days: places.everyDays,
    buttons: prefs.listOf('widget_buttons').filter((b) => b in WIDGET_BUTTONS),
    lock_action: prefs.v.widget_lock,
    access: prefs.v.widget_access,
    codes_mode: prefs.v.widget_codes_mode,
    code_ids: allowed ? widgetCodeIds() : [],
    favorites: allowed
      ? pass.entries
          .filter((e) => e.favourite && !e.trashed && !e.hidden)
          .slice(0, MAX_FAVORITES)
          .map((e) => {
            const label = e.color != null && e.color !== '' && !Number.isNaN(Number(e.color)) ? Number(e.color) : null;
            return { id: e.id, title: e.title, color: oklch(0.58, 0.11, label ?? hueOf(e.site ?? e.title)) };
          })
      : [],
    generator: prefs.v.gen_mode === 'passphrase' ? 'passphrase' : prefs.v.gen_mode === 'pin' ? 'PIN' : 'password',
    recipe: recipe(),
    clipboard_seconds: prefs.v.clipboard_seconds,
  };
}

async function loadHealth() {
  const raw = await host().loadSettings(HEALTH_FILE);
  if (!raw) return;
  try {
    const saved = JSON.parse(raw);
    if (typeof saved?.score === 'number' && typeof saved?.at === 'number') health = saved;
  } catch {
    // A damaged file is the same as none.
  }
}

async function refreshHealth() {
  if (pass.state.kind !== 'unlocked') return;
  try {
    const report = await healthNow();
    health = {
      score: report.score,
      weak: report.weak.length,
      reused: report.reused.reduce((sum, group) => sum + group.length, 0),
      breached: report.breached.length,
      at: Date.now(),
    };
    await host().saveSettings(HEALTH_FILE, JSON.stringify(health));
  } catch {
    // The next change tries again.
  }
}

/** Keeps the desktop app's widgets fed. Only the desktop app on a Mac has widgets. */
export function startWidgetFeed() {
  const desktop = host().desktop;
  if (!desktop?.widgetsFeed || host().os !== 'mac') return;
  void loadHealth();
  pass.onWipe.push(() => {
    health = null;
    void host().removeFile(`settings/${HEALTH_FILE}.json`);
    void desktop.widgetsForget?.();
  });
  $effect.root(() => {
    $effect(() => {
      const next = feed();
      const timer = setTimeout(() => void desktop.widgetsFeed!(next), 300);
      return () => clearTimeout(timer);
    });
    // The counts follow the vault, a few seconds after it opens or changes.
    $effect(() => {
      if (pass.state.kind !== 'unlocked' || !prefs.v.desktop_widgets) return;
      void pass.entries;
      void prefs.v.health_ignored;
      void prefs.v.health_skip_local;
      const timer = setTimeout(() => void refreshHealth(), 5_000);
      return () => clearTimeout(timer);
    });
  });
}
