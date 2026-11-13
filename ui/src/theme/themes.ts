// Themes, in the same file format as the phone's. A theme shared from one opens on the other.

import { BUILT_IN } from './builtin';

export interface ThemeColors {
  background: string;
  surface: string;
  surfaceHigh: string;
  surfaceHighest: string;
  text: string;
  textMid: string;
  textLow: string;
  primary: string;
  danger: string;
  warn: string;
  info: string;
  onPrimary?: string;
  primaryContainer?: string;
  secondaryContainer?: string;
  outline?: string;
  digit?: string;
  symbol?: string;
}

export interface ThemeFile {
  id: string;
  name: string;
  light: ThemeColors | null;
  dark: ThemeColors | null;
  corners: number;
  font: string;
  format: string;
  version: number;
}

export { BUILT_IN };
export const DEFAULT_THEME = 'wmpass';
const PREFIX = 'wmpass-theme:';
const LIMIT = 8 * 1024;
const HEX = /^#[0-9a-fA-F]{6}$/;

type Rgb = [number, number, number];

export function rgb(hex: string): Rgb {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

export function hexOf([r, g, b]: Rgb): string {
  const part = (v: number) =>
    Math.round(Math.min(1, Math.max(0, v)) * 255)
      .toString(16)
      .padStart(2, '0');
  return `#${part(r)}${part(g)}${part(b)}`;
}

const linear = (v: number) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);

export function luminance(c: Rgb) {
  return 0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2]);
}

export function contrast(a: Rgb, b: Rgb) {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

export function mix(a: Rgb, b: Rgb, amount: number): Rgb {
  return [a[0] + (b[0] - a[0]) * amount, a[1] + (b[1] - a[1]) * amount, a[2] + (b[2] - a[2]) * amount];
}

function readable(color: Rgb, ground: Rgb, ratio: number, by = 0.1): Rgb {
  if (contrast(color, ground) >= ratio) return color;
  const target: Rgb = luminance(ground) > 0.18 ? [0, 0, 0] : [1, 1, 1];
  for (let step = by; step <= 1; step += by) {
    const tried = mix(color, target, step);
    if (contrast(tried, ground) >= ratio) return tried;
  }
  return target;
}

/**
 * A theme someone made may be careless, or unkind: text the colour of its background would make a
 * warning unreadable. So every side passes through this, built-in or not.
 */
export function readableOn(c: ThemeColors): ThemeColors {
  const bg = rgb(c.background);
  const low = rgb(c.surface);
  const high = rgb(c.surfaceHighest);
  // On the background and on the first surface, where the sidebars and grouped settings sit, in
  // full; on the highest surface a little less.
  const worst = (color: string, ratio: number, by = 0.1) =>
    hexOf(readable(readable(readable(rgb(color), bg, ratio, by), low, ratio, by), high, ratio - 0.5, by));
  return {
    ...c,
    text: worst(c.text, 7),
    textMid: worst(c.textMid, 4.5),
    // The faintest text still says things, such as when an item was changed, a count, or that an
    // export holds passwords in plain text, so it is held to the bar for words, 4.5:1.
    // In fine steps, so it goes only as far as it must and stays fainter than the middle text.
    textLow: worst(c.textLow, 4.5, 0.02),
    primary: worst(c.primary, 3),
    danger: worst(c.danger, 3),
    warn: worst(c.warn, 3),
    info: worst(c.info, 3),
  };
}

function checked(colors: ThemeColors | null | undefined): ThemeColors | null {
  if (!colors) return null;
  const required = [
    colors.background,
    colors.surface,
    colors.surfaceHigh,
    colors.surfaceHighest,
    colors.text,
    colors.textMid,
    colors.textLow,
    colors.primary,
    colors.danger,
    colors.warn,
    colors.info,
  ];
  if (!required.every((v) => typeof v === 'string' && HEX.test(v))) throw new Error('That theme has a color that is not valid.');
  const optional = [colors.onPrimary, colors.primaryContainer, colors.secondaryContainer, colors.outline, colors.digit, colors.symbol];
  if (!optional.every((v) => !v || (typeof v === 'string' && HEX.test(v)))) throw new Error('That theme has a color that is not valid.');
  return readableOn({
    background: colors.background,
    surface: colors.surface,
    surfaceHigh: colors.surfaceHigh,
    surfaceHighest: colors.surfaceHighest,
    text: colors.text,
    textMid: colors.textMid,
    textLow: colors.textLow,
    primary: colors.primary,
    danger: colors.danger,
    warn: colors.warn,
    info: colors.info,
    onPrimary: colors.onPrimary ?? '',
    primaryContainer: colors.primaryContainer ?? '',
    secondaryContainer: colors.secondaryContainer ?? '',
    outline: colors.outline ?? '',
    digit: colors.digit ?? '',
    symbol: colors.symbol ?? '',
  });
}

export function themeToText(theme: ThemeFile) {
  return JSON.stringify(theme, null, 2);
}

export function themeToCode(theme: ThemeFile) {
  const raw = new TextEncoder().encode(JSON.stringify(theme));
  let binary = '';
  for (const byte of raw) binary += String.fromCharCode(byte);
  return PREFIX + btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
}

/**
 * Reads a theme file or code from anywhere. It comes from a stranger, so it gets a new id, a name
 * with nothing odd in it, numbers within range, and readable text.
 */
export function readTheme(text: string): ThemeFile {
  const trimmed = text.trim();
  if (trimmed.length > LIMIT * 2) throw new Error('That is too long to be a theme.');
  let body = trimmed;
  if (trimmed.startsWith(PREFIX)) {
    // A code cut short when copied fails here, and the browser's own words for that say nothing useful.
    try {
      const raw = atob(trimmed.slice(PREFIX.length).replaceAll('-', '+').replaceAll('_', '/'));
      body = new TextDecoder().decode(Uint8Array.from(raw, (c) => c.charCodeAt(0)));
    } catch {
      throw new Error('That theme code is incomplete. Copy all of it and try again.');
    }
  }
  if (body.length > LIMIT) throw new Error('That is too long to be a theme.');
  let parsed: any;
  try {
    parsed = JSON.parse(body);
  } catch {
    throw new Error('That is not a WM Pass theme.');
  }
  if (parsed?.format !== 'wmpass-theme') throw new Error('That is not a WM Pass theme.');
  if ((parsed.version ?? 1) > 1) throw new Error('This theme needs a newer version of WM Pass.');
  if (!parsed.light && !parsed.dark) throw new Error('That theme has no colors.');
  const name = String(parsed.name ?? '')
    .replace(/\p{Cc}/gu, '')
    .trim()
    .slice(0, 40);
  return {
    id: `custom_${crypto.randomUUID().slice(0, 8)}`,
    name: name || 'Shared theme',
    light: checked(parsed.light),
    dark: checked(parsed.dark),
    corners: Math.min(28, Math.max(0, Number(parsed.corners) || 0)),
    font: ['system', 'inter', 'serif', 'mono'].includes(parsed.font) ? parsed.font : 'system',
    format: 'wmpass-theme',
    version: 1,
  };
}

/**
 * Ink for labels on the accent. The theme's own onPrimary wins only when it reads at 4.5:1: a theme
 * with a pale accent and white on it was unreadable. A theme that names none gets its background
 * where that reads. Otherwise whichever of white or black reads best.
 */
export function onBrand(c: ThemeColors, primary: Rgb = rgb(c.primary)): Rgb {
  const own = c.onPrimary ? rgb(c.onPrimary) : null;
  if (own && contrast(own, primary) >= 4.5) return own;
  const ground = rgb(c.background);
  if (!own && contrast(ground, primary) >= 4.5) return ground;
  const inks: Rgb[] = [[1, 1, 1], [0, 0, 0]];
  if (own) inks.unshift(own);
  return inks.reduce((best, ink) => (contrast(ink, primary) > contrast(best, primary) ? ink : best));
}

/** Puts a theme on the page as custom properties. */
export function applyTheme(theme: ThemeFile, dark: boolean, black: boolean, root: HTMLElement = document.documentElement) {
  const chosen = dark ? (theme.dark ?? theme.light) : (theme.light ?? theme.dark);
  if (!chosen) return;
  // A theme with one side, asked for the other, stays what it is.
  const isDark = chosen === theme.dark;
  const c = readableOn(chosen);
  const black_ = black && isDark;
  const bg = black_ ? ([0, 0, 0] as Rgb) : rgb(c.background);
  const s1 = black_ ? rgb('#0B0B0B') : rgb(c.surface);
  const s2 = black_ ? rgb('#161616') : rgb(c.surfaceHigh);
  const s3 = black_ ? rgb('#222222') : rgb(c.surfaceHighest);
  const text = rgb(c.text);
  const primary = rgb(c.primary);
  const danger = rgb(c.danger);
  const on = onBrand(c, primary);
  const onDanger: Rgb = contrast([1, 1, 1], danger) >= contrast([0, 0, 0], danger) ? [1, 1, 1] : [0, 0, 0];
  // Words in an accent colour, such as an error, a warning or a link, are held to 4.5:1 on the
  // background and the raised surfaces they sit on. Fills, rings and icons keep the theme's own.
  const words = (color: Rgb) => hexOf(readable(readable(color, bg, 4.5), s2, 4.5));
  // The tint behind a chosen row or a page open in a list. A row's second line is in the middle
  // text, so the accent goes in only as far as that still reads at 4.5:1 on it.
  const textMid = rgb(c.textMid);
  const tint = (amount: number) => {
    let at = amount;
    let fill = mix(s2, primary, at);
    while (at > 0.04 && contrast(textMid, fill) < 4.5) {
      at -= 0.02;
      fill = mix(s2, primary, at);
    }
    return hexOf(fill);
  };
  const primaryC = c.primaryContainer || tint(isDark ? 0.28 : 0.2);
  const onTint = rgb(primaryC);
  // Marks and words that sit on that tint, in a chosen row: a star or a timer at 3:1, as any
  // picture that says something, and a link at 4.5:1, as words.
  // A little over the bar, so rounding to a hex colour does not land just under it.
  const tinted = (color: Rgb, ratio: number) => hexOf(readable(color, onTint, ratio + 0.05, 0.02));
  // The edge of a box, a button, a chip or a switch: what shows where a control is, so it is held to
  // 3:1 on the background and on the first surface, the two they sit on. Lines that only divide
  // keep the softer --line.
  const controlBorder = (() => {
    let at = isDark ? 0.26 : 0.28;
    let edge = mix(bg, text, at);
    while (at < 1 && (contrast(edge, bg) < 3.05 || contrast(edge, s1) < 3.05)) {
      at = Math.min(1, at + 0.02);
      edge = mix(bg, text, at);
    }
    return hexOf(edge);
  })();
  const vars: Record<string, string> = {
    '--bg': hexOf(bg),
    '--s1': hexOf(s1),
    '--s2': hexOf(s2),
    '--s3': hexOf(s3),
    '--text': c.text,
    '--text2': c.textMid,
    '--text3': c.textLow,
    '--primary': c.primary,
    '--on-primary': hexOf(on),
    '--primary-c': primaryC,
    '--warn-on-tint': tinted(rgb(c.warn), 3),
    '--danger-on-tint': tinted(danger, 3),
    '--link-on-tint': tinted(primary, 4.5),
    '--second-c': c.secondaryContainer || tint(isDark ? 0.18 : 0.14),
    '--outline': c.outline || c.textLow,
    '--hair': hexOf(mix(bg, text, isDark ? 0.14 : 0.13)),
    '--line': hexOf(mix(bg, text, isDark ? 0.26 : 0.28)),
    '--control-border': controlBorder,
    '--danger': c.danger,
    '--on-danger': hexOf(onDanger),
    '--danger-c': hexOf(mix(s2, danger, 0.22)),
    '--warn': c.warn,
    '--danger-text': words(danger),
    '--warn-text': words(rgb(c.warn)),
    '--link': words(primary),
    '--info': c.info,
    '--digit': c.digit || c.info,
    '--symbol': c.symbol || c.warn,
    '--hover': hexOf(mix(bg, text, isDark ? 0.07 : 0.05)),
    '--press': hexOf(mix(bg, text, isDark ? 0.12 : 0.09)),
    '--r-s': `${Math.round(theme.corners / 3)}px`,
    '--r-m': `${Math.round(theme.corners / 2)}px`,
    '--r-l': `${Math.round(theme.corners * 0.85)}px`,
  };
  for (const [key, value] of Object.entries(vars)) root.style.setProperty(key, value);
  root.style.setProperty('color-scheme', isDark ? 'dark' : 'light');
  root.dataset.dark = isDark ? '1' : '0';
  root.dataset.font = theme.font;
}
