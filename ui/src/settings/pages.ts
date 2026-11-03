// Every page in Settings, and everything its search can find. The words match the phone's.

import type { IconName } from '../engine/icons.svelte';

export interface PageInfo {
  id: string;
  title: string;
  group: string;
  icon: IconName;
  /** Reached from another page rather than the list. */
  parent?: string;
  desktop?: boolean;
  /** Only in the browser extension. */
  extension?: boolean;
  needsNetwork?: boolean;
  /** Shown without a network of its own where the shell has the desktop app make the calls. */
  viaDesktop?: boolean;
}

export const GROUPS = ['Protection', 'Your vault', 'Backup and sync', 'Tools', 'Customize', 'This computer', 'This app'];

export const PAGES: PageInfo[] = [
  { id: 'security', title: 'Security', group: 'Protection', icon: 'shield' },
  { id: 'shelf', title: 'Hidden shelf', group: 'Protection', icon: 'visibility_lock' },
  { id: 'breaches', title: 'Breaches', group: 'Protection', icon: 'local_fire_department' },
  { id: 'hardware', title: 'Hardware key', group: 'Protection', icon: 'security_key', parent: 'security' },
  { id: 'folders', title: 'Folders', group: 'Your vault', icon: 'folder' },
  { id: 'trash', title: 'Trash', group: 'Your vault', icon: 'delete' },
  { id: 'import', title: 'Import', group: 'Your vault', icon: 'download' },
  { id: 'export', title: 'Export', group: 'Your vault', icon: 'upload' },
  { id: 'backup', title: 'Backups', group: 'Backup and sync', icon: 'backup' },
  { id: 'sync', title: 'Sync and accounts', group: 'Backup and sync', icon: 'sync' },
  { id: 'vaults', title: 'Nested and shared vaults', group: 'Backup and sync', icon: 'family_restroom', parent: 'sync' },
  { id: 'receive', title: 'Receive an item', group: 'Backup and sync', icon: 'qr_code_scanner', parent: 'sync' },
  { id: 'aliases', title: 'Email aliases', group: 'Tools', icon: 'alternate_email' },
  { id: 'inbox', title: 'Disposable inbox', group: 'Tools', icon: 'inbox', needsNetwork: true },
  { id: 'usernames', title: 'Username checker', group: 'Tools', icon: 'person_search', needsNetwork: true, viaDesktop: true },
  { id: 'compare', title: 'Compare two passwords', group: 'Tools', icon: 'balance' },
  { id: 'look', title: 'Appearance', group: 'Customize', icon: 'palette' },
  { id: 'themes', title: 'Theme', group: 'Customize', icon: 'palette', parent: 'look' },
  { id: 'icons', title: 'Icons', group: 'Customize', icon: 'image' },
  { id: 'list', title: 'Vault list', group: 'Customize', icon: 'view_agenda' },
  { id: 'navigation', title: 'Sidebar', group: 'Customize', icon: 'dashboard' },
  { id: 'add', title: 'Add menu', group: 'Customize', icon: 'add' },
  { id: 'editor', title: 'Editor', group: 'Customize', icon: 'edit' },
  { id: 'entry', title: 'Item page', group: 'Customize', icon: 'description' },
  { id: 'codes', title: 'Codes', group: 'Customize', icon: 'pin' },
  { id: 'generator', title: 'Generator', group: 'Customize', icon: 'casino' },
  { id: 'health', title: 'Health checks', group: 'Customize', icon: 'ecg_heart' },
  { id: 'shortcuts', title: 'Keyboard shortcuts', group: 'Customize', icon: 'keyboard' },
  { id: 'desktop-link', title: 'Desktop app', group: 'This computer', icon: 'computer', extension: true },
  { id: 'desktop', title: 'Desktop', group: 'This computer', icon: 'computer', desktop: true },
  { id: 'browser', title: 'Browser extension', group: 'This computer', icon: 'extension', desktop: true },
  { id: 'ssh', title: 'SSH agent', group: 'This computer', icon: 'terminal', desktop: true },
  { id: 'updates', title: 'Updates', group: 'This computer', icon: 'update', desktop: true },
  { id: 'file', title: 'Settings file', group: 'This app', icon: 'import_export' },
  { id: 'guide', title: 'Guide', group: 'This app', icon: 'help' },
  { id: 'about', title: 'About', group: 'This app', icon: 'info' },
];

export interface Found {
  title: string;
  where: string;
  route: string;
  words?: string;
  desktop?: boolean;
}

export const SEARCH_INDEX: Found[] = [
  { title: 'Theme', where: 'Appearance', route: 'look', words: 'dracula catppuccin nord solarized gruvbox colours colors dark colors' },
  { title: 'Dark mode', where: 'Appearance', route: 'look', words: 'light black amoled system' },
  { title: 'Row height', where: 'Appearance', route: 'look', words: 'density compact spacious' },
  { title: 'Text size', where: 'Appearance', route: 'look', words: 'font zoom scale bigger' },
  { title: 'Animations', where: 'Appearance', route: 'look', words: 'motion reduce' },
  { title: 'App icons', where: 'Icons', route: 'icons', words: 'material symbols rounded sharp light phosphor lucide glyphs' },
  { title: 'Brand colors', where: 'Icons', route: 'icons', words: 'site icons logo simple icons favicon letters' },
  { title: 'Shape', where: 'Icons', route: 'icons', words: 'circle square rounded' },
  { title: 'Style', where: 'Icons', route: 'icons', words: 'settings icons tile coloured background colored' },
  { title: 'Layout', where: 'Vault list', route: 'list', words: 'grid compact list' },
  { title: 'Grouping', where: 'Vault list', route: 'list', words: 'folder site' },
  { title: 'Sort by', where: 'Vault list', route: 'list', words: 'order name changed type' },
  { title: 'Pinned items first', where: 'Vault list', route: 'list' },
  { title: 'Fold logins on one site', where: 'Vault list', route: 'list', words: 'multiple accounts' },
  { title: 'Section headings', where: 'Vault list', route: 'list' },
  { title: 'Second line', where: 'Vault list', route: 'list', words: 'username subtitle' },
  { title: 'Small marks', where: 'Vault list', route: 'list', words: 'badges star' },
  { title: 'Button on the row', where: 'Vault list', route: 'list', words: 'copy' },
  { title: 'Click', where: 'Vault list', route: 'list', words: 'gesture tap' },
  { title: 'Double-click', where: 'Vault list', route: 'list', words: 'gesture' },
  { title: 'Sidebar', where: 'Sidebar', route: 'navigation', words: 'tabs order navigation pages' },
  { title: 'Open on', where: 'Sidebar', route: 'navigation', words: 'start page last' },
  { title: 'Lock button', where: 'Sidebar', route: 'navigation', words: 'padlock' },
  { title: 'Add menu', where: 'Add menu', route: 'add', words: 'types kinds new item order hide' },
  { title: 'Sections open at the start', where: 'Editor', route: 'editor', words: 'fold collapse minimise minimize' },
  { title: 'Remember what I open', where: 'Editor', route: 'editor' },
  { title: 'Fold less used fields', where: 'Editor', route: 'editor', words: 'details' },
  { title: 'Start logins with a password', where: 'Editor', route: 'editor', words: 'generate' },
  { title: 'Click a field to copy it', where: 'Item page', route: 'entry' },
  { title: 'Hide shown secrets after', where: 'Item page', route: 'entry', words: 'reveal timeout' },
  { title: 'Color digits and symbols', where: 'Item page', route: 'entry', words: 'password colors' },
  { title: 'Old passwords to keep', where: 'Item page', route: 'entry', words: 'history' },
  { title: 'Size', where: 'Codes', route: 'codes', words: 'totp digits big' },
  { title: 'Split in two groups', where: 'Codes', route: 'codes', words: 'totp' },
  { title: 'Show the next code', where: 'Codes', route: 'codes', words: 'totp' },
  { title: 'Time ring', where: 'Codes', route: 'codes', words: 'totp timer' },
  { title: 'Hide codes until clicked', where: 'Codes', route: 'codes', words: 'privacy totp' },
  { title: 'Type', where: 'Generator', route: 'generator', words: 'password words pin passphrase default' },
  { title: 'Length', where: 'Generator', route: 'generator', words: 'default' },
  { title: 'Avoid look-alikes', where: 'Generator', route: 'generator', words: 'ambiguous' },
  { title: 'Show strength in bits', where: 'Generator', route: 'generator', words: 'entropy' },
  { title: 'Problems to list', where: 'Health checks', route: 'health', words: 'weak reused breach second factor duplicates' },
  { title: 'Show the score', where: 'Health checks', route: 'health' },
  { title: 'Lock after no activity', where: 'Security', route: 'security', words: 'auto-lock timeout' },
  { title: 'Lock when the computer sleeps', where: 'Security', route: 'security' },
  { title: 'Lock when the window is hidden', where: 'Security', route: 'security', words: 'tray' },
  { title: 'Block screenshots', where: 'Security', route: 'security', words: 'capture screen sharing' },
  { title: 'Clear the clipboard after', where: 'Security', route: 'security', words: 'copy timeout' },
  { title: 'Change it', where: 'Security', route: 'security', words: 'master password rekey' },
  { title: 'Add a security key', where: 'Security', route: 'security', words: 'yubikey fido hardware' },
  { title: 'Wipe password', where: 'Security', route: 'security', words: 'duress panic' },
  { title: 'Decoy vault', where: 'Security', route: 'security', words: 'duress hidden' },
  { title: 'Emergency kit', where: 'Security', route: 'security', words: 'shamir trusted' },
  { title: 'Allow network features', where: 'Security', route: 'security', words: 'offline internet' },
  { title: 'Backups', where: 'Backups', route: 'backup', words: 'webdav s3 restore schedule' },
  { title: 'Sync and accounts', where: 'Sync', route: 'sync', words: 'bitwarden keepass lan server pair' },
  { title: 'Nested and shared vaults', where: 'Sync', route: 'vaults', words: 'family share vault inside' },
  { title: 'Keyboard shortcuts', where: 'Shortcuts', route: 'shortcuts', words: 'hotkey chord' },
  { title: 'Settings file', where: 'Settings file', route: 'file', words: 'export import reset backup' },
  { title: 'Guide', where: 'Guide', route: 'guide', words: 'help manual tour tutorial explain how what is' },
  { title: 'Help buttons', where: 'Guide', route: 'guide', words: 'question mark explain' },
  { title: 'Tips on new pages', where: 'Guide', route: 'guide', words: 'hints first time' },
  { title: 'Show the welcome again', where: 'Guide', route: 'guide', words: 'tour start onboarding' },
  { title: 'Email aliases', where: 'Tools', route: 'aliases', words: 'simplelogin addy relay duckduckgo' },
  { title: 'Breaches', where: 'Tools', route: 'breaches', words: 'hibp leak' },
  { title: 'Import', where: 'Your vault', route: 'import', words: '1password bitwarden chrome keepass csv' },
  { title: 'Export', where: 'Your vault', route: 'export', words: 'csv json kdbx' },
  { title: 'Keep deleted items for', where: 'Trash', route: 'trash', words: 'retention' },
  { title: 'Keep running in the tray', where: 'Desktop', route: 'desktop', words: 'close minimise background minimize', desktop: true },
  { title: 'Start hidden', where: 'Desktop', route: 'desktop', desktop: true },
  { title: 'Open when you sign in', where: 'Desktop', route: 'desktop', words: 'autostart login startup', desktop: true },
  { title: 'Quick search', where: 'Desktop', route: 'desktop', words: 'global shortcut palette hotkey', desktop: true },
  { title: 'Auto-type', where: 'Desktop', route: 'desktop', words: 'type fill', desktop: true },
  { title: 'Let the extension connect', where: 'Browser extension', route: 'browser', words: 'chrome firefox bridge native messaging', desktop: true },
  { title: 'Run the SSH agent', where: 'SSH agent', route: 'ssh', words: 'ssh-add git keys', desktop: true },
  { title: 'Check when WM Pass starts', where: 'Updates', route: 'updates', words: 'update version', desktop: true },
];
