// What each shell has to supply: the desktop app over Tauri, the extension over wasm.

import type { TopicId } from '$help';

export interface PickedFile {
  name: string;
  bytes: Uint8Array;
}

export interface MenuItem {
  id?: string;
  label?: string;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
  separator?: boolean;
  items?: MenuItem[];
}

export interface Host {
  kind: 'desktop' | 'extension' | 'web';
  /** `mac`, `windows`, `linux` or `other`. Decides the words for keys. */
  os: 'mac' | 'windows' | 'linux' | 'other';
  /** Runs one framed call. */
  call(frame: Uint8Array): Promise<Uint8Array>;
  /** Runs one framed call whose secret goes to the clipboard and never reaches this page. */
  callToClipboard(frame: Uint8Array, clearAfterSeconds: number): Promise<Uint8Array>;
  /** Puts plain text on the clipboard, for values already on screen. */
  copyText(text: string, sensitive: boolean, clearAfterSeconds: number): Promise<void>;
  clearClipboard(): Promise<void>;
  /** Forgets every key at once. */
  lockAll(): Promise<void>;
  /** Files in the app's folder, by path relative to it. Everything here is ciphertext. */
  listFiles(folder: string): Promise<{ name: string; size: number }[]>;
  /** Subfolders are listed too, with a size of zero. */
  readFile(path: string): Promise<Uint8Array>;
  writeFile(path: string, bytes: Uint8Array): Promise<void>;
  removeFile(path: string): Promise<void>;
  removeFolder(path: string): Promise<void>;
  /** Gives files one time, so the folder does not say which one changed. */
  coverFile(path: string): Promise<void>;
  /** Small settings that are not secret. */
  loadSettings(name: string): Promise<string | null>;
  saveSettings(name: string, text: string): Promise<void>;
  /** A file the person chose. */
  pickFile(title: string, extensions?: string[]): Promise<PickedFile | null>;
  /** Saves bytes where the person chooses. Returns whether they did. */
  saveFile(title: string, name: string, bytes: Uint8Array): Promise<boolean>;
  openUrl(url: string): Promise<void>;
  /** Asks the OS how much memory there is, in gibibytes. */
  memoryGib(): Promise<number>;
  /** Native context menu, if there is one. Resolves to the chosen id. */
  menu?(items: MenuItem[]): Promise<string | null>;
  /** Listens for app menu commands, tray items and shortcuts. */
  onCommand?(run: (command: string) => void): void;
  /** Tells the shell what is happening, for its menus and tray. */
  stateChanged?(state: { unlocked: boolean; name: string; handle?: number; file?: string; read_only?: boolean }): void;
  /** Keeps other programs from capturing the window. */
  setScreenProtection?(on: boolean): Promise<void>;
  /** Small things kept sealed by the system's keychain: logins for places, the network identity. */
  loadSealed?(name: string): Promise<string | null>;
  saveSealed?(name: string, text: string): Promise<void>;
  /** A session that outlives this page. Says what is already open. */
  attach?(): Promise<{ handle: number; file: string; name: string; read_only: boolean } | null>;
  /** The lock timer, where something other than this page keeps it. */
  lockGuard?: LockGuard;
  /** The extension's connection to the desktop app. */
  link?: DesktopLink;
  /** Opening without the password, where the shell can keep the key behind something that asks first. */
  quick?: QuickUnlock;
  /** Calls that reach a network run in this shell. */
  network?: boolean;
  /** Reads a username page in a hidden, private browser view, for sites that only answer once their scripts run. */
  checkUsernamePage?(site: string, name: string, online: boolean): Promise<string>;
  /** Username checks made by another app: the extension asks the desktop app over its link. */
  usernames?: UsernameHost;
  places?: PlacesHost;
  hardware?: HardwareHost;
  outside?: OutsideFiles;
  lan?: LanHost;
  desktop?: DesktopHost;
}

export interface UsernameHost {
  sites(): Promise<{ sites: string[]; pages: string[] }>;
  check(site: string, name: string): Promise<string>;
  links(name: string): Promise<{ site: string; url: string }[]>;
}

export interface DesktopLink {
  state(): Promise<any>;
  connect(): Promise<any>;
  answer(match: boolean): Promise<any>;
  forget(): Promise<any>;
  onChange(run: () => void): () => void;
  option(name: string, fallback: unknown): Promise<unknown>;
  setOption(name: string, value: unknown): Promise<void>;
}

export interface LockGuard {
  activity(): Promise<void>;
  setLockRule(afterSeconds: number, onSleep: boolean): Promise<void>;
}

/** A file somewhere the person keeps copies. */
export interface PlaceFile {
  id: string;
  name: string;
  size: number;
  modified_ms: number;
}

/** Places outside this device. All that ever goes there is ciphertext. */
export interface PlacesHost {
  kinds: string[];
  list(place: unknown): Promise<PlaceFile[]>;
  read(place: unknown, file: PlaceFile): Promise<Uint8Array>;
  write(place: unknown, name: string, bytes: Uint8Array): Promise<void>;
  remove(place: unknown, file: PlaceFile): Promise<void>;
  chooseFolder(): Promise<string | null>;
  /**
   * One step of signing in to an account place: through the service's page in the browser, or
   * with the place's password and then any code asked for. Answers what to keep with the place,
   * or what to ask for next and the opaque state to send back with it.
   */
  signIn?(place: unknown, step: SignInStep): Promise<SignInAnswer>;
  /** Set by the places store: where a run reports a rotated token, a first host key or certificate. */
  onUpdate?: (id: string, extra: Record<string, unknown>) => void;
}

export interface SignInStep {
  code?: string;
  mailbox_password?: string;
  pending?: string;
}

export type SignInAnswer =
  | { extra: Record<string, unknown> }
  | { needs: 'code' | 'mailbox_password' | 'approval' | 'captcha'; pending?: string; by_sms?: boolean };

/** Security keys, where the shell can reach them. */
export interface HardwareHost {
  /** The kinds this shell can reach. Both, unless it says. */
  kinds?: ('fido2' | 'yubikey')[];
  /** Asks a key for its secret. Without a factor it enrols the key first. */
  tap(kind: 'fido2' | 'yubikey', factor: unknown): Promise<{ factor: any; output: Uint8Array }>;
}

/** Quick unlock: the desktop app keeps the key in the system keyring, a browser seals it under a passkey. */
export interface QuickUnlock {
  /** The switch's title and what it costs, in this shell's words, and its help topic. */
  title: string;
  sub: string;
  help: TopicId;
  available(): Promise<boolean>;
  enrolled(file: string): Promise<boolean>;
  enroll(handle: number, file: string): Promise<void>;
  forget(file: string): Promise<void>;
  /** Opens the vault with the kept key. The reply is the same as a password's. */
  open(file: string, sealed: Uint8Array): Promise<Uint8Array>;
}

/** Files that stay where they are, such as a KeePass database. */
export interface OutsideFiles {
  choose(title: string, extensions?: string[]): Promise<{ path: string; name: string } | null>;
  read(path: string): Promise<Uint8Array>;
  write(path: string, bytes: Uint8Array): Promise<void>;
}

/** What syncing with other devices on this network needs from the shell. */
export interface LanHost {
  deviceName(): Promise<string>;
  localAddress(): Promise<string | null>;
  blobDir(folder: string): Promise<string>;
}

/** What only the desktop app can do. */
export interface DesktopHost {
  /** The lock timer runs in the app, where a suspended page cannot stop it. */
  activity(): Promise<void>;
  setLockRule(afterSeconds: number, onSleep: boolean): Promise<void>;
  installBrowserHost(): Promise<string[]>;
  removeBrowserHost(): Promise<void>;
  bridgeFingerprint(): Promise<string>;
  bridgeState(): Promise<{ running: boolean; paired: { id: string; name: string }[]; pending: string | null }>;
  bridgeSetEnabled(on: boolean, askSites: boolean): Promise<void>;
  bridgeForget(id: string): Promise<void>;
  sshState(): Promise<{ running: boolean; socket: string | null; last: string | null }>;
  sshSetEnabled(on: boolean, confirm: boolean): Promise<void>;
  autoType(id: string): Promise<void>;
  autostart(): Promise<boolean>;
  setAutostart(on: boolean): Promise<void>;
  setGlobalShortcut(chord: string): Promise<boolean>;
  setCloseToTray(on: boolean): Promise<void>;
  setShortcuts(shortcuts: Record<string, string>): Promise<void>;
  showMain(): Promise<void>;
  hidePalette(): Promise<void>;
  currentHandle(): Promise<number>;
  checkUpdate(): Promise<{ version: string; notes: string } | null>;
  installUpdate(): Promise<void>;
  version(): Promise<string>;
  /** What the Mac's widgets may show. Only the desktop app on a Mac has these. */
  widgetsFeed?(feed: WidgetFeed): Promise<void>;
  /** A wipe: what the widgets kept goes with the vault. */
  widgetsForget?(): Promise<void>;
}

/**
 * What the page tells the desktop app for its widgets. Nothing in it is secret: the app fetches
 * codes from the session itself, and drops codes, favorites and the vault's name unless the vault
 * is open and Show on widgets is on (desktop/src-tauri/src/widgets.rs).
 */
export interface WidgetFeed {
  enabled: boolean;
  state: 'none' | 'locked' | 'open';
  vault: string | null;
  health: { score: number; weak: number; reused: number; breached: number; at: number } | null;
  show_score: boolean;
  backups: { label: string; last: number | null; failed: boolean }[];
  every_days: number;
  buttons: string[];
  lock_action: string;
  access: boolean;
  codes_mode: string;
  code_ids: string[];
  favorites: { id: string; title: string; color: string }[];
  generator: string;
  recipe: unknown;
  clipboard_seconds: number;
}

let current: Host | null = null;

export function setHost(host: Host) {
  current = host;
}

export function host(): Host {
  if (!current) throw new Error('no host');
  return current;
}
