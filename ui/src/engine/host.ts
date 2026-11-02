// What each shell has to supply: the desktop app over Tauri, the extension over wasm.

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
  stateChanged?(state: { unlocked: boolean; name: string }): void;
  /** Keeps other programs from capturing the window. */
  setScreenProtection?(on: boolean): Promise<void>;
  /** Small things kept sealed by the system's keychain: logins for places, the network identity. */
  loadSealed?(name: string): Promise<string | null>;
  saveSealed?(name: string, text: string): Promise<void>;
  /** Calls that reach a network run in this shell. */
  network?: boolean;
  places?: PlacesHost;
  hardware?: HardwareHost;
  outside?: OutsideFiles;
  lan?: LanHost;
  desktop?: DesktopHost;
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
}

/** Security keys, where the shell can reach them. */
export interface HardwareHost {
  /** Asks a key for its secret. Without a factor it enrols the key first. */
  tap(kind: 'fido2' | 'yubikey', factor: unknown): Promise<{ factor: any; output: Uint8Array }>;
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
  quickAvailable(): Promise<boolean>;
  quickEnrolled(file: string): Promise<boolean>;
  quickEnroll(handle: number, file: string): Promise<void>;
  quickForget(file: string): Promise<void>;
  /** Opens the vault with the key kept in the system's store. The reply is the same as a password's. */
  quickOpen(file: string, sealed: Uint8Array): Promise<Uint8Array>;
  installBrowserHost(): Promise<string[]>;
  removeBrowserHost(): Promise<void>;
  bridgeFingerprint(): Promise<string>;
  bridgeState(): Promise<{ running: boolean; paired: { id: string; name: string }[]; pending: string | null }>;
  bridgeSetEnabled(on: boolean): Promise<void>;
  bridgeForget(id: string): Promise<void>;
  bridgeAnswer(accepted: boolean): Promise<void>;
  sshState(): Promise<{ running: boolean; socket: string | null; last: string | null }>;
  sshSetEnabled(on: boolean, confirm: boolean): Promise<void>;
  autoType(text: string, secret: boolean): Promise<void>;
  autostart(): Promise<boolean>;
  setAutostart(on: boolean): Promise<void>;
  setGlobalShortcut(chord: string): Promise<boolean>;
  setCloseToTray(on: boolean): Promise<void>;
  checkUpdate(): Promise<{ version: string; notes: string } | null>;
  installUpdate(): Promise<void>;
  version(): Promise<string>;
}

let current: Host | null = null;

export function setHost(host: Host) {
  current = host;
}

export function host(): Host {
  if (!current) throw new Error('no host');
  return current;
}
