// Every page in Settings, and everything its search can find. The words match the phone's.

import type { IconName } from '../engine/icons.svelte';

export interface PageInfo {
  id: string;
  title: string;
  group: string;
  icon: IconName;
  /** Reached from another page rather than the list: a hub, or the page that links to it. */
  parent?: string;
  /** On a hub, the heading it is listed under. */
  section?: string;
  /** On a hub, the line under its title. */
  sub?: string;
  /** A page of pages. */
  hub?: boolean;
  desktop?: boolean;
  /** Only in the browser extension. */
  extension?: boolean;
  /** Only on a Mac, for what only macOS has. */
  mac?: boolean;
  needsNetwork?: boolean;
  /** Shown without a network of its own where the shell has the desktop app make the calls. */
  viaDesktop?: boolean;
}

// The phone's groups. Its Appearance, Adding and editing and Tools are hubs here as there, so the
// list names what a thing is about rather than every page; every page keeps its route.
export const GROUPS = ['Protection', 'Your vault', 'Backup and sync', 'Tools and customizing', 'This computer', 'This app'];

export const PAGES: PageInfo[] = [
  { id: 'security', title: 'Security', group: 'Protection', icon: 'shield' },
  { id: 'shelf', title: 'Hidden shelf', group: 'Protection', icon: 'visibility_lock' },
  { id: 'breaches', title: 'Breaches', group: 'Protection', icon: 'local_fire_department' },
  { id: 'health', title: 'Health checks', group: 'Protection', icon: 'ecg_heart' },
  { id: 'hardware', title: 'Hardware key', group: 'Protection', icon: 'security_key', parent: 'security' },
  { id: 'folders', title: 'Folders', group: 'Your vault', icon: 'folder' },
  { id: 'trash', title: 'Trash', group: 'Your vault', icon: 'delete' },
  { id: 'import', title: 'Import', group: 'Your vault', icon: 'download' },
  { id: 'export', title: 'Export', group: 'Your vault', icon: 'upload' },
  { id: 'backup', title: 'Backups', group: 'Backup and sync', icon: 'backup' },
  { id: 'sync', title: 'Sync and accounts', group: 'Backup and sync', icon: 'sync' },
  { id: 'vaults', title: 'Nested and shared vaults', group: 'Backup and sync', icon: 'family_restroom', parent: 'sync' },
  { id: 'receive', title: 'Receive an item', group: 'Backup and sync', icon: 'qr_code_scanner', parent: 'sync' },
  { id: 'tools', title: 'Tools', group: 'Tools and customizing', icon: 'widgets', hub: true, sub: 'Email aliases, a throwaway inbox, names and comparing' },
  { id: 'aliases', title: 'Email aliases', group: 'Tools and customizing', icon: 'alternate_email', parent: 'tools', section: 'Addresses and names', sub: 'SimpleLogin, addy.io, Firefox Relay, DuckDuckGo' },
  { id: 'inbox', title: 'Disposable inbox', group: 'Tools and customizing', icon: 'inbox', parent: 'tools', section: 'Addresses and names', sub: 'A throwaway address for one sign-up', needsNetwork: true },
  { id: 'usernames', title: 'Username checker', group: 'Tools and customizing', icon: 'person_search', parent: 'tools', section: 'Addresses and names', sub: 'See which sites have a name free', needsNetwork: true, viaDesktop: true },
  { id: 'compare', title: 'Compare two passwords', group: 'Tools and customizing', icon: 'balance', parent: 'tools', section: 'Passwords' },
  { id: 'appearance', title: 'Appearance', group: 'Tools and customizing', icon: 'palette', hub: true, sub: 'Theme, icons, the list and the item page' },
  { id: 'look', title: 'Style and theme', group: 'Tools and customizing', icon: 'palette', parent: 'appearance', section: 'How it looks', sub: 'Theme, dark mode, sizes and motion' },
  { id: 'themes', title: 'Theme', group: 'Tools and customizing', icon: 'palette', parent: 'look' },
  { id: 'icons', title: 'Icons', group: 'Tools and customizing', icon: 'image', parent: 'appearance', section: 'How it looks', sub: 'App icons, site icons and these ones' },
  { id: 'list', title: 'Vault list', group: 'Tools and customizing', icon: 'view_agenda', parent: 'appearance', section: 'What it shows', sub: 'Layout, sorting, rows and gestures' },
  { id: 'navigation', title: 'Sidebar', group: 'Tools and customizing', icon: 'dashboard', parent: 'appearance', section: 'What it shows', sub: 'Pages, what else it lists and the lock button' },
  { id: 'entry', title: 'Item page', group: 'Tools and customizing', icon: 'description', parent: 'appearance', section: 'What it shows', sub: 'Buttons, fields and history' },
  { id: 'codes', title: 'Codes', group: 'Tools and customizing', icon: 'pin', parent: 'appearance', section: 'What it shows', sub: 'Size, grouping and privacy' },
  { id: 'widgets', title: 'Widgets', group: 'Tools and customizing', icon: 'widgets', parent: 'appearance', section: 'What it shows', sub: 'The quick bar, the lock button and codes', desktop: true, mac: true },
  { id: 'editing', title: 'Adding and editing', group: 'Tools and customizing', icon: 'edit', hub: true, sub: 'The add menu, the editor and the generator' },
  { id: 'add', title: 'Add menu', group: 'Tools and customizing', icon: 'add', parent: 'editing', section: 'New items', sub: 'Which types are offered, and in what order' },
  { id: 'generator', title: 'Generator', group: 'Tools and customizing', icon: 'casino', parent: 'editing', section: 'New items', sub: 'What a new password looks like' },
  { id: 'editor', title: 'Editor', group: 'Tools and customizing', icon: 'edit', parent: 'editing', section: 'Changing them', sub: 'What starts open or folded' },
  { id: 'shortcuts', title: 'Keyboard shortcuts', group: 'Tools and customizing', icon: 'keyboard' },
  { id: 'desktop-link', title: 'Desktop app', group: 'This computer', icon: 'computer', extension: true },
  { id: 'desktop', title: 'Desktop', group: 'This computer', icon: 'computer', desktop: true },
  { id: 'browser', title: 'Browser extension', group: 'This computer', icon: 'extension', desktop: true },
  { id: 'ssh', title: 'SSH agent', group: 'This computer', icon: 'terminal', desktop: true },
  { id: 'updates', title: 'Updates', group: 'This computer', icon: 'update', desktop: true },
  { id: 'file', title: 'Settings file', group: 'This app', icon: 'import_export' },
  { id: 'guide', title: 'Guide', group: 'This app', icon: 'help' },
  { id: 'about', title: 'About', group: 'This app', icon: 'info' },
];

export function pageById(id: string): PageInfo | undefined {
  return PAGES.find((p) => p.id === id);
}

/** The pages above this one, outermost first: Appearance, Style and theme for Theme. */
export function ancestors(id: string): PageInfo[] {
  const out: PageInfo[] = [];
  for (let p = pageById(id)?.parent; p; p = pageById(p)?.parent) {
    const info = pageById(p);
    if (!info) break;
    out.unshift(info);
  }
  return out;
}

/** The page on the settings list this one is under, itself when it is listed there. */
export function rootOf(id: string): string {
  return ancestors(id)[0]?.id ?? id;
}

/** Where a page lives, as the settings search writes it: "Settings, Appearance, Style and theme". */
export function trail(id: string): string {
  return ['Settings', ...ancestors(id).map((p) => p.title), pageById(id)?.title].filter(Boolean).join(', ');
}

/**
 * Where a search result lives, inside Settings: the page it is on with the hubs above it, such as
 * "Appearance, Style and theme". A page's own result names the pages above it, or Settings.
 */
export function whereOf(found: Found): string {
  const above = ancestors(found.route).map((p) => p.title);
  const page = pageById(found.route);
  const path = page && page.title !== found.title ? [...above, page.title] : above;
  return path.length ? path.join(', ') : 'Settings';
}

export interface Found {
  title: string;
  route: string;
  words?: string;
  desktop?: boolean;
  /** Only on a Mac. */
  mac?: boolean;
}

export const SEARCH_INDEX: Found[] = [
  { title: 'Appearance', route: 'appearance', words: 'look theme icons list customise customize' },
  { title: 'Adding and editing', route: 'editing', words: 'new item add menu editor generator customise customize' },
  { title: 'Tools', route: 'tools', words: 'aliases inbox username compare utilities' },
  { title: 'Theme', route: 'look', words: 'dracula catppuccin nord solarized gruvbox colours colors dark colors' },
  { title: 'Dark mode', route: 'look', words: 'light black amoled system' },
  { title: 'Row height', route: 'look', words: 'density compact spacious' },
  { title: 'Text size', route: 'look', words: 'font zoom scale bigger' },
  { title: 'Animations', route: 'look', words: 'motion reduce' },
  { title: 'App icons', route: 'icons', words: 'material symbols rounded sharp light phosphor lucide glyphs' },
  { title: 'Brand colors', route: 'icons', words: 'site icons logo simple icons favicon letters' },
  { title: 'Shape', route: 'icons', words: 'circle square rounded' },
  { title: 'Style', route: 'icons', words: 'settings icons tile coloured background colored' },
  { title: 'Layout', route: 'list', words: 'grid compact list' },
  { title: 'Grouping', route: 'list', words: 'folder site' },
  { title: 'Sort by', route: 'list', words: 'order name changed type' },
  { title: 'Pinned items first', route: 'list' },
  { title: 'Fold logins on one site', route: 'list', words: 'multiple accounts' },
  { title: 'Section headings', route: 'list' },
  { title: 'Second line', route: 'list', words: 'username subtitle' },
  { title: 'Small marks', route: 'list', words: 'badges star' },
  { title: 'Button on the row', route: 'list', words: 'copy' },
  { title: 'Click', route: 'list', words: 'gesture tap' },
  { title: 'Double-click', route: 'list', words: 'gesture' },
  { title: 'Sidebar', route: 'navigation', words: 'tabs order navigation pages' },
  { title: 'Open on', route: 'navigation', words: 'start page last' },
  { title: 'Lock button', route: 'navigation', words: 'padlock' },
  { title: 'Add menu', route: 'add', words: 'types kinds new item order hide' },
  { title: 'Sections open at the start', route: 'editor', words: 'fold collapse minimise minimize' },
  { title: 'Remember what I open', route: 'editor' },
  { title: 'Fold less used fields', route: 'editor', words: 'details' },
  { title: 'Start logins with a password', route: 'editor', words: 'generate' },
  { title: 'Click a field to copy it', route: 'entry' },
  { title: 'Hide shown secrets after', route: 'entry', words: 'reveal timeout' },
  { title: 'Color digits and symbols', route: 'entry', words: 'password colors' },
  { title: 'Old passwords to keep', route: 'entry', words: 'history' },
  { title: 'Size', route: 'codes', words: 'totp digits big' },
  { title: 'Split in two groups', route: 'codes', words: 'totp' },
  { title: 'Show the next code', route: 'codes', words: 'totp' },
  { title: 'Time ring', route: 'codes', words: 'totp timer' },
  { title: 'Hide codes until clicked', route: 'codes', words: 'privacy totp' },
  { title: 'Type', route: 'generator', words: 'password words pin passphrase default' },
  { title: 'Length', route: 'generator', words: 'default' },
  { title: 'Words', route: 'generator', words: 'passphrase diceware default' },
  { title: 'Symbols', route: 'generator', words: 'special characters punctuation' },
  { title: 'Order', route: 'codes', words: 'totp sort name' },
  { title: 'Leave out local addresses', route: 'health', words: 'router nas home lan' },
  { title: 'Tag new items', route: 'editor', words: 'auto tags labels' },
  { title: 'Sync settings with the vault', route: 'sync', words: 'customizations follow devices' },
  { title: 'Settings that stay on this device', route: 'sync', words: 'skip local not synced' },
  { title: 'Add a password', route: 'security', words: 'unlock method second slot' },
  { title: 'Add a key file', route: 'security', words: 'keyfile two factor unlock' },
  { title: 'Lockout password', route: 'security', words: 'duress refuse hour' },
  { title: 'Lock and refuse passwords now', route: 'security', words: 'panic lockout hour' },
  { title: 'Avoid look-alikes', route: 'generator', words: 'ambiguous' },
  { title: 'Show strength in bits', route: 'generator', words: 'entropy' },
  { title: 'Problems to list', route: 'health', words: 'weak reused breach second factor duplicates' },
  { title: 'Show the score', route: 'health' },
  { title: 'Lock after no activity', route: 'security', words: 'auto-lock timeout' },
  { title: 'Lock when the computer sleeps', route: 'security' },
  { title: 'Lock when the window is hidden', route: 'security', words: 'tray' },
  { title: 'Block screenshots', route: 'security', words: 'capture screen sharing', desktop: true },
  { title: 'Clear the clipboard after', route: 'security', words: 'copy timeout' },
  { title: 'Change it', route: 'security', words: 'master password rekey' },
  { title: 'Add a security key', route: 'security', words: 'yubikey fido hardware' },
  { title: 'Wipe password', route: 'security', words: 'duress panic' },
  { title: 'Decoy vault', route: 'security', words: 'duress hidden' },
  { title: 'Emergency kit', route: 'security', words: 'shamir trusted' },
  { title: 'Allow network features', route: 'security', words: 'offline internet' },
  { title: 'Backups', route: 'backup', words: 'webdav s3 restore schedule' },
  { title: 'Sync and accounts', route: 'sync', words: 'bitwarden keepass lan server pair' },
  { title: 'Nested and shared vaults', route: 'vaults', words: 'family share vault inside' },
  { title: 'Keyboard shortcuts', route: 'shortcuts', words: 'hotkey chord' },
  { title: 'Settings file', route: 'file', words: 'export import reset backup' },
  { title: 'Guide', route: 'guide', words: 'help manual tour tutorial explain how what is' },
  { title: 'Help buttons', route: 'guide', words: 'question mark explain' },
  { title: 'Tips on new pages', route: 'guide', words: 'hints first time' },
  { title: 'Show the welcome again', route: 'guide', words: 'tour start onboarding' },
  { title: 'Email aliases', route: 'aliases', words: 'simplelogin addy relay duckduckgo' },
  { title: 'Breaches', route: 'breaches', words: 'hibp leak' },
  { title: 'Import', route: 'import', words: '1password bitwarden chrome keepass csv' },
  { title: 'Export', route: 'export', words: 'csv json kdbx' },
  { title: 'Keep deleted items for', route: 'trash', words: 'retention' },
  { title: 'Keep running in the tray', route: 'desktop', words: 'close minimise background minimize', desktop: true },
  { title: 'Start hidden', route: 'desktop', desktop: true },
  { title: 'Open when you sign in', route: 'desktop', words: 'autostart login startup', desktop: true },
  { title: 'Quick search', route: 'desktop', words: 'global shortcut palette hotkey', desktop: true },
  { title: 'Auto-type', route: 'desktop', words: 'type fill', desktop: true },
  { title: 'Offer widgets', route: 'widgets', mac: true, words: 'widget gallery notification centre center mac', desktop: true },
  { title: 'Quick bar', route: 'widgets', mac: true, words: 'widget buttons order', desktop: true },
  { title: 'Lock button', route: 'widgets', mac: true, words: 'widget panic lock', desktop: true },
  { title: 'Codes on widgets', route: 'widgets', mac: true, words: 'widget totp pick choose', desktop: true },
  { title: 'Codes ahead', route: 'widgets', mac: true, words: 'widget totp future refresh', desktop: true },
  { title: 'Show on widgets', route: 'security', mac: true, words: 'widget codes favorites totp', desktop: true },
  { title: 'Let the extension connect', route: 'browser', words: 'chrome firefox bridge native messaging', desktop: true },
  { title: 'Run the SSH agent', route: 'ssh', words: 'ssh-add git keys', desktop: true },
  { title: 'Check when WM Pass starts', route: 'updates', words: 'update version', desktop: true },
];
