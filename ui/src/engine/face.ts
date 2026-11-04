// What an entry looks like. The same arithmetic as the phone, so a site keeps its colour everywhere.

import type { IconName } from './icons.svelte';
import { contrast, hexOf, mix, rgb, type ThemeColors } from '../theme/themes';
import { brands } from './brands.svelte';
import { GLYPH_CHOICES } from './glyphs';

export function kindIcon(kind: string): IconName {
  const table: Record<string, IconName> = {
    login: 'key',
    card: 'credit_card',
    identity: 'badge',
    note: 'sticky_note_2',
    document: 'description',
    ssh: 'terminal',
    api: 'api',
    database: 'database',
    licence: 'license',
    recovery: 'emergency_home',
    wifi: 'wifi',
    otp: 'pin',
  };
  return table[kind] ?? 'key';
}

export function kindName(kind: string): string {
  const table: Record<string, string> = {
    login: 'Login',
    card: 'Card',
    identity: 'Identity',
    note: 'Note',
    document: 'Document',
    ssh: 'SSH key',
    api: 'API key',
    database: 'Database',
    licence: 'License',
    recovery: 'Recovery',
    wifi: 'Wi-Fi',
    otp: 'Code',
  };
  return table[kind] ?? kind;
}

export function kindPlural(kind: string): string {
  const table: Record<string, string> = {
    login: 'Logins',
    card: 'Cards',
    identity: 'Identities',
    note: 'Notes',
    document: 'Documents',
    ssh: 'SSH keys',
    api: 'API keys',
    database: 'Databases',
    licence: 'Licenses',
    recovery: 'Recovery',
    wifi: 'Wi-Fi',
    otp: 'Codes',
  };
  return table[kind] ?? kind;
}

export { GLYPH_CHOICES };

/** An OKLCH colour as sRGB, clamped. Equal lightness and chroma look equally bright at any hue. */
export function oklch(l: number, c: number, hueDegrees: number): string {
  const h = (hueDegrees * Math.PI) / 180;
  const a = c * Math.cos(h);
  const b = c * Math.sin(h);
  const lp = l + 0.3963377774 * a + 0.2158037573 * b;
  const mp = l - 0.1055613458 * a - 0.0638541728 * b;
  const sp = l - 0.0894841775 * a - 1.291485548 * b;
  const ll = lp ** 3;
  const mm = mp ** 3;
  const ss = sp ** 3;
  const gamma = (v: number) => {
    const clipped = Math.min(1, Math.max(0, v));
    return clipped <= 0.0031308 ? 12.92 * clipped : 1.055 * clipped ** (1 / 2.4) - 0.055;
  };
  return hexOf([
    gamma(4.0767416621 * ll - 3.3077115913 * mm + 0.2309699292 * ss),
    gamma(-1.2684380046 * ll + 2.6097574011 * mm - 0.3413193965 * ss),
    gamma(-0.0041960863 * ll - 0.7034186147 * mm + 1.707614701 * ss),
  ]);
}

export interface Face {
  fill: string;
  ink: string;
  /** A brand's drawing, a named glyph, or a letter. */
  brand: string | null;
  glyph: IconName | null;
  letter: string;
}

function roughLuminance(hex: string) {
  const [r, g, b] = rgb(hex);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * The standard tile. By default a flat circle, as in Contacts, where a known brand keeps its
 * colour. A label's hue always wins, because someone chose it for that one entry.
 */
export function faceOf(
  title: string,
  site: string | null,
  kind: string,
  label: string | null,
  icon: string | null,
  style: string,
  shape: string,
  colors: { ground: string; neutral: string; second: string; text: string; text2: string },
): Face {
  const hue = label != null && label !== '' && !Number.isNaN(Number(label)) ? Number(label) : null;
  const chosenBrand = icon?.startsWith('brand:') ? brands.bySlug(icon.slice(6)) : null;
  const chosenGlyph = icon?.startsWith('glyph:') ? (GLYPH_CHOICES[icon.slice(6)] ?? null) : null;
  const brand = chosenBrand ?? (chosenGlyph ? null : brands.find(site, title));
  const letterSource = site ?? title;
  const letter = [...letterSource].find((c) => /[\p{L}\p{N}]/u.test(c))?.toUpperCase() ?? '';
  const bare = shape === 'none';

  let glyph: IconName | null = chosenGlyph;
  let drawn: string | null = null;
  if (!chosenGlyph) {
    if (brand && style !== 'letters') drawn = brand.slug;
    else if (!brand && kind !== 'login' && !site) glyph = kindIcon(kind);
    else if (!letter) glyph = kindIcon(kind);
  }

  const brandColour = brand ? (roughLuminance(brand.color) < 0.16 ? '#3C4552' : brand.color) : null;
  const brandShown = !!brand && !!drawn && hue === null && style !== 'uniform' && style !== 'letters';
  const groundRgb = rgb(colors.ground);
  const dark = roughLuminance(colors.ground) < 0.5;
  // With brand colours on, a site without a mark still gets a colour of its own, soft as in
  // Contacts, rather than every letter on the same grey.
  const ownHue = hue === null && !brand && !chosenGlyph && style === 'brand' ? hueOf(site ?? (title || kind)) : null;
  let fill: string;
  if (bare) fill = 'transparent';
  else if (hue !== null) fill = oklch(0.58, 0.11, hue);
  else if (ownHue !== null) fill = oklch(dark ? 0.4 : 0.86, dark ? 0.07 : 0.06, ownHue);
  else if (brandShown && style === 'brand' && contrast(rgb(brandColour!), groundRgb) >= 1.3) fill = brandColour!;
  else if (brandShown && style === 'tinted') fill = colors.neutral;
  else fill = colors.second;

  let ink: string;
  if (hue !== null) ink = bare ? oklch(0.62, 0.14, hue) : '#FFFFFF';
  else if (ownHue !== null) ink = oklch(dark ? 0.9 : 0.36, 0.09, ownHue);
  else if (brandShown && !bare && fill === brandColour) {
    ink = roughLuminance(hexOf(mix(rgb(brandColour!), [0, 0, 0], 0.16))) > 0.62 ? '#16181C' : '#FFFFFF';
  } else if (brandShown && contrast(rgb(brandColour!), bare ? groundRgb : rgb(colors.neutral)) >= 2.2) ink = brandColour!;
  else if (bare) ink = colors.text2;
  else ink = colors.text;
  return { fill, ink, brand: drawn, glyph, letter };
}

/** A site or title's own hue, always the same for the same name. */
export function hueOf(seed: string) {
  let hash = 0;
  for (const ch of seed.toLowerCase()) hash = (Math.imul(31, hash) + ch.charCodeAt(0)) | 0;
  const raw = Math.abs(hash) % 360;
  if (raw >= 70 && raw < 100) return raw - 30;
  if (raw >= 100 && raw <= 135) return raw + 35;
  return raw;
}

export type { ThemeColors };
