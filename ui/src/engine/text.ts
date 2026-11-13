// Small words shared by every screen.

import { apps } from './apps.svelte';

/**
 * A tag's or a folder's name as typed or pasted, made one way: no invisible characters at either
 * end, such as a byte-order mark or a zero-width space a web page left on it, and accents composed,
 * so an é pasted from a Mac is the same tag as one typed elsewhere rather than a second that looks
 * the same.
 */
export function nameText(raw: string): string {
  return raw.normalize('NFC').replace(/^[\s\u200B-\u200D\u2060\uFEFF]+|[\s\u200B-\u200D\u2060\uFEFF]+$/g, '');
}

/** Names in the order a person reads them: capitals or not, accents aside, and "Item 2" before "Item 10". */
export const byName = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' }).compare;

export function whenText(ms: number | null | undefined): string {
  if (!ms || ms <= 0) return 'never';
  // By the calendar here, not by 24-hour spans: late last night is yesterday, not today.
  const midnight = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((midnight(new Date()) - midnight(new Date(ms))) / 86_400_000);
  if (days < 0) return new Date(ms).toLocaleDateString(undefined, { dateStyle: 'medium' });
  if (days === 0) return 'today';
  if (days === 1) return 'yesterday';
  if (days < 30) return `${days} days ago`;
  return new Date(ms).toLocaleDateString(undefined, { dateStyle: 'medium' });
}

export function sizeText(bytes: number): string {
  if (bytes < 1024) return bytes === 1 ? '1 byte' : `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${Math.floor(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** A code in the same groups as the big one above it. */
export function grouped(code: string): string {
  const group = code.length % 3 === 0 ? 3 : Math.floor(code.length / 2);
  if (group === 0) return code;
  const out: string[] = [];
  for (let i = 0; i < code.length; i += group) out.push(code.slice(i, i + group));
  return out.join(' ');
}

/** What a site matcher says, in a form fit for a row. An app reads by its name when it is known. */
export function siteLine(element: Record<string, any>): { label: string; openable: string | null; app?: string } | null {
  const [kind] = Object.keys(element);
  const value = element[kind];
  switch (kind) {
    case 'Domain':
    case 'Host':
      return { label: String(value), openable: String(value) };
    case 'Url':
      return { label: String(value).replace(/^https:\/\//, '').replace(/\/$/, ''), openable: String(value) };
    case 'AndroidApp':
      return value?.package ? { label: apps.nameOf(value.package) ?? value.package, openable: null, app: value.package } : null;
    case 'Regex':
      return { label: String(value), openable: null };
    default:
      return null;
  }
}

export function titleFromSite(url: string): string {
  try {
    const host = new URL(url.includes('://') ? url : `https://${url}`).hostname.replace(/^www\./, '');
    const parts = host.split('.');
    const name = parts.length >= 2 ? parts[parts.length - 2] : (parts[0] ?? '');
    return name ? name[0].toUpperCase() + name.slice(1) : '';
  } catch {
    return '';
  }
}

export function plural(count: number, one: string, many = `${one}s`) {
  return `${count} ${count === 1 ? one : many}`;
}

/**
 * Whether a password typed again is wrong, as the door judges it: once it cannot become the first
 * however it goes on, or once it has been given as it is. Not while it is still being typed.
 */
export function passwordsDiffer(again: string, first: string, tried = false): boolean {
  return again !== '' && again !== first && (tried || !first.startsWith(again));
}

/** A wait in seconds as the settings name it: "2 minutes", not "120 seconds". */
export function secondsText(seconds: number): string {
  if (seconds >= 60 && seconds % 60 === 0) return plural(seconds / 60, 'minute');
  return plural(seconds, 'second');
}

/**
 * Keys a chord names by where they sit on the keyboard, as `event.code` and the desktop's
 * shortcut parsers name them. The character such a key gives changes with Shift, AltGr and the
 * layout: `+` is its own key on a German keyboard and Shift+= on an American one.
 */
const PLACED = new Set(['Comma', 'Period', 'Slash', 'Backslash', 'Semicolon', 'Quote', 'BracketLeft', 'BracketRight', 'Minus', 'Equal', 'Backquote', 'IntlBackslash', 'IntlRo', 'IntlYen', 'Space']);
/** What those keys print on an American keyboard, for when the browser cannot say what they print on this one. */
const US_KEYS: Record<string, string> = { Comma: ',', Period: '.', Slash: '/', Backslash: '\\', Semicolon: ';', Quote: "'", BracketLeft: '[', BracketRight: ']', Minus: '-', Equal: '=', Backquote: '`', IntlBackslash: '<' };
const NUMPAD: Record<string, string> = { Add: '+', Subtract: '-', Multiply: '*', Divide: '/', Decimal: '.', Equal: '=', Comma: ',' };

let layout: Map<string, string> | null = null;

/** Learns what each key prints in this keyboard layout, where the browser can say (Chromium's can). */
export async function learnKeyboardLayout() {
  try {
    layout = (await (navigator as any).keyboard?.getLayoutMap?.()) ?? null;
  } catch {
    layout = null;
  }
}

/** A chord in the words of this keyboard. */
export function chordLabel(chord: string, os: string): string {
  if (!chord) return '';
  const mac = os === 'mac';
  const keys: Record<string, string> = { Comma: ',', Backslash: '\\', Space: 'Space', Period: '.', Slash: '/', ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→', Escape: 'Esc', Plus: '+' };
  const words: Record<string, string> = mac
    ? { ...keys, Mod: '⌘', Shift: '⇧', Alt: '⌥', Ctrl: '⌃', Enter: '↩', Backspace: '⌫', Delete: '⌦', Tab: '⇥' }
    : { ...keys, Mod: 'Ctrl', Shift: 'Shift', Alt: 'Alt', Ctrl: 'Ctrl', Delete: 'Del' };
  const parts = chord.split('+');
  // A Mac writes its modifiers in one order, ⌃⌥⇧⌘, whatever order they were pressed in.
  if (mac) {
    const order = ['Ctrl', 'Alt', 'Shift', 'Mod'];
    const key = parts.filter((p) => !order.includes(p));
    parts.splice(0, parts.length, ...order.filter((m) => parts.includes(m)), ...key);
  }
  const name = (part: string) => {
    // A key named by its place reads as what it prints here.
    if (PLACED.has(part) && part !== 'Space') return (layout?.get(part) ?? US_KEYS[part] ?? part).toUpperCase();
    if (part.startsWith('Numpad') && part.length > 6) return `Num ${NUMPAD[part.slice(6)] ?? part.slice(6)}`;
    return words[part] ?? part;
  };
  return parts.map(name).join(mac ? '' : '+');
}

function modifiers(event: KeyboardEvent, os: string) {
  const parts: string[] = [];
  if (os === 'mac' ? event.metaKey : event.ctrlKey) parts.push('Mod');
  if (os === 'mac' && event.ctrlKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  return parts;
}

/** The chord a key press makes, or null if it is only modifiers. */
export function chordOf(event: KeyboardEvent, os: string): string | null {
  const key = event.key;
  if (['Shift', 'Control', 'Alt', 'Meta', 'AltGraph'].includes(key)) return null;
  const code = event.code;
  let name: string;
  if (code.startsWith('Key')) name = code.slice(3);
  else if (code.startsWith('Digit')) name = code.slice(5);
  // By the key's place, as the menu and the global shortcut take it, so all three agree.
  else if (PLACED.has(code) || code.startsWith('Numpad')) name = code;
  else return legacyChordOf(event, os);
  return [...modifiers(event, os), name].join('+');
}

/**
 * The chord as it was saved before punctuation was named by its place: by the character the key
 * gave, with a + called Plus, or it would read as the joint between the keys of the chord.
 */
function legacyChordOf(event: KeyboardEvent, os: string): string | null {
  const key = event.key;
  if (['Shift', 'Control', 'Alt', 'Meta', 'AltGraph'].includes(key)) return null;
  const names: Record<string, string> = { ',': 'Comma', '\\': 'Backslash', ' ': 'Space', '.': 'Period', '/': 'Slash', '+': 'Plus' };
  const code = event.code;
  let name: string;
  if (code.startsWith('Key')) name = code.slice(3);
  else if (code.startsWith('Digit')) name = code.slice(5);
  else name = names[key] ?? (key.length === 1 ? key.toUpperCase() : key);
  return [...modifiers(event, os), name].join('+');
}

/** Every chord a key press may have been saved as: as it is named now, and as it was before. */
export function chordsOf(event: KeyboardEvent, os: string): string[] {
  const now = chordOf(event, os);
  const before = legacyChordOf(event, os);
  return [...new Set([now, before].filter((c): c is string => !!c))];
}
