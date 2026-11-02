// Small things both the background and the pages need.

export function toB64(bytes: Uint8Array): string {
  let text = '';
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
}

export function fromB64(text: string): Uint8Array {
  const raw = atob(text);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}

export function toB64Url(bytes: Uint8Array): string {
  return toB64(bytes).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function fromB64Url(text: string): Uint8Array {
  return fromB64(text.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (text.length % 4)) % 4));
}

export function fromHex(text: string): Uint8Array {
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(text.slice(i * 2, i * 2 + 2), 16);
  return out;
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/** A site row in the popup or the inline menu. Never holds a secret. */
export interface SiteRow {
  id: string;
  title: string;
  subtitle: string;
  site?: string | null;
  icon?: string | null;
  has_password: boolean;
  has_otp: boolean;
}

export type LinkStatus = 'off' | 'absent' | 'connecting' | 'compare' | 'waiting' | 'ready' | 'declined' | 'failed';

export interface LinkState {
  status: LinkStatus;
  code: string | null;
  fingerprint: string | null;
  unlocked: boolean;
  problem: string | null;
}

/** What the inline menu and popup ask for. */
export interface SiteAnswer {
  source: 'vault' | 'desktop' | 'none';
  locked: boolean;
  rows: SiteRow[];
  /** The asking frame sits inside a page from somewhere else. */
  foreign: string | null;
}
