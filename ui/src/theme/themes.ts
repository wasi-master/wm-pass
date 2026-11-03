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

function readable(color: Rgb, ground: Rgb, ratio: number): Rgb {
  if (contrast(color, ground) >= ratio) return color;
  const target: Rgb = luminance(ground) > 0.18 ? [0, 0, 0] : [1, 1, 1];
  for (let step = 0.1; step <= 1; step += 0.1) {
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
  const high = rgb(c.surfaceHighest);
  const worst = (color: string, ratio: number) => hexOf(readable(readable(rgb(color), bg, ratio), high, ratio - 0.5));
  return {
    ...c,
    text: worst(c.text, 7),
    textMid: worst(c.textMid, 4.5),
    textLow: worst(c.textLow, 3),
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
    const raw = atob(trimmed.slice(PREFIX.length).replaceAll('-', '+').replaceAll('_', '/'));
    body = new TextDecoder().decode(Uint8Array.from(raw, (c) => c.charCodeAt(0)));
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
  let on: Rgb;
  if (c.onPrimary) on = rgb(c.onPrimary);
  else if (contrast(rgb(c.background), primary) >= 4.5) on = rgb(c.background);
  else on = contrast([1, 1, 1], primary) >= contrast([0, 0, 0], primary) ? [1, 1, 1] : [0, 0, 0];
  const onDanger: Rgb = contrast([1, 1, 1], danger) >= contrast([0, 0, 0], danger) ? [1, 1, 1] : [0, 0, 0];
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
    '--primary-c': c.primaryContainer || hexOf(mix(s2, primary, isDark ? 0.28 : 0.2)),
    '--second-c': c.secondaryContainer || hexOf(mix(s2, primary, isDark ? 0.18 : 0.14)),
    '--outline': c.outline || c.textLow,
    '--hair': hexOf(mix(bg, text, isDark ? 0.14 : 0.13)),
    '--line': hexOf(mix(bg, text, isDark ? 0.26 : 0.28)),
    '--danger': c.danger,
    '--on-danger': hexOf(onDanger),
    '--danger-c': hexOf(mix(s2, danger, 0.22)),
    '--warn': c.warn,
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
