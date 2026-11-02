// Small words shared by every screen.

export function whenText(ms: number | null | undefined): string {
  if (!ms || ms <= 0) return 'never';
  const days = Math.floor((Date.now() - ms) / 86_400_000);
  if (days < 0) return new Date(ms).toLocaleDateString(undefined, { dateStyle: 'medium' });
  if (days === 0) return 'today';
  if (days === 1) return 'yesterday';
  if (days < 30) return `${days} days ago`;
  return new Date(ms).toLocaleDateString(undefined, { dateStyle: 'medium' });
}

export function sizeText(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
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

/** What a site matcher says, in a form fit for a row. */
export function siteLine(element: Record<string, any>): { label: string; openable: string | null } | null {
  const [kind] = Object.keys(element);
  const value = element[kind];
  switch (kind) {
    case 'Domain':
    case 'Host':
      return { label: String(value), openable: String(value) };
    case 'Url':
      return { label: String(value).replace(/^https:\/\//, '').replace(/\/$/, ''), openable: String(value) };
    case 'AndroidApp':
      return value?.package ? { label: value.package, openable: null } : null;
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

/** A chord in the words of this keyboard. */
export function chordLabel(chord: string, os: string): string {
  if (!chord) return '';
  const mac = os === 'mac';
  const words: Record<string, string> = mac
    ? { Mod: '⌘', Shift: '⇧', Alt: '⌥', Ctrl: '⌃', Comma: ',', Backslash: '\\', Space: 'Space', Period: '.', Slash: '/' }
    : { Mod: 'Ctrl', Shift: 'Shift', Alt: 'Alt', Ctrl: 'Ctrl', Comma: ',', Backslash: '\\', Space: 'Space', Period: '.', Slash: '/' };
  const parts = chord.split('+').map((p) => words[p] ?? p);
  return parts.join(mac ? '' : '+');
}

/** The chord a key press makes, or null if it is only modifiers. */
export function chordOf(event: KeyboardEvent, os: string): string | null {
  const key = event.key;
  if (['Shift', 'Control', 'Alt', 'Meta'].includes(key)) return null;
  const mod = os === 'mac' ? event.metaKey : event.ctrlKey;
  const parts: string[] = [];
  if (mod) parts.push('Mod');
  if (os === 'mac' && event.ctrlKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  const names: Record<string, string> = { ',': 'Comma', '\\': 'Backslash', ' ': 'Space', '.': 'Period', '/': 'Slash' };
  const code = event.code;
  let name: string;
  if (code.startsWith('Key')) name = code.slice(3);
  else if (code.startsWith('Digit')) name = code.slice(5);
  else name = names[key] ?? (key.length === 1 ? key.toUpperCase() : key);
  parts.push(name);
  return parts.join('+');
}
