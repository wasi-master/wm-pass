// Which settings pages this window has: some are only on the desktop, in the extension or on a Mac,
// and a few need a network the shell may not have.

import { host } from '../engine/host';
import { PAGES, type PageInfo } from './pages';

export function available(info: PageInfo): boolean {
  if (info.desktop && host().kind !== 'desktop') return false;
  if (info.extension && host().kind !== 'extension') return false;
  if (info.mac && host().os !== 'mac') return false;
  if (info.needsNetwork && !host().network && !(info.viaDesktop && host().usernames)) return false;
  return true;
}

/** The pages a hub lists, in their sections and in order. */
export function hubSections(hub: string): [string, PageInfo[]][] {
  const sections = new Map<string, PageInfo[]>();
  for (const info of PAGES) {
    if (info.parent !== hub || !available(info)) continue;
    const name = info.section ?? '';
    sections.set(name, [...(sections.get(name) ?? []), info]);
  }
  return [...sections];
}
