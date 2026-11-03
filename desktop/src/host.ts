// The desktop shell. Everything goes through the app's own commands, and nothing here holds state.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Host, MenuItem, PlaceFile, PlacesHost } from '$ui/engine/host';
import { prefs } from '$ui/engine/prefs.svelte';

const encoder = new TextEncoder();

/** A JSON head and bytes in one body: `head_len(u32 LE) head bytes`. */
function packed(head: unknown, bytes: Uint8Array = new Uint8Array()) {
  const text = encoder.encode(JSON.stringify(head));
  const out = new Uint8Array(4 + text.length + bytes.length);
  new DataView(out.buffer).setUint32(0, text.length, true);
  out.set(text, 4);
  out.set(bytes, 4 + text.length);
  return out;
}

function asBytes(value: unknown): Uint8Array {
  return value instanceof ArrayBuffer ? new Uint8Array(value) : new Uint8Array(value as number[]);
}

async function rawCall(command: string, args?: Record<string, unknown> | Uint8Array) {
  return asBytes(await invoke(command, args));
}

let pendingMenu: ((id: string | null) => void) | null = null;

export async function desktopHost(): Promise<Host> {
  const os = await invoke<Host['os']>('os_name');
  void listen<string>('menu-choice', (event) => {
    pendingMenu?.(event.payload);
    pendingMenu = null;
  });

  return {
    kind: 'desktop',
    os,
    network: true,
    call: (frame) => rawCall('call', frame),
    callToClipboard(frame, clearAfterSeconds) {
      const body = new Uint8Array(4 + frame.length);
      new DataView(body.buffer).setUint32(0, clearAfterSeconds, true);
      body.set(frame, 4);
      return rawCall('call_to_clipboard', body).finally(() => body.fill(0));
    },
    copyText: (text, sensitive, clearAfterSeconds) => invoke('copy_text', { text, sensitive, clearAfterSeconds }),
    clearClipboard: () => invoke('clear_clipboard'),
    lockAll: () => invoke('lock_all'),
    listFiles: (folder) => invoke('list_files', { folder }),
    readFile: (path) => rawCall('read_file', { path }),
    writeFile: (path, bytes) => invoke('write_file', packed({ path }, bytes)),
    removeFile: (path) => invoke('remove_file', { path }),
    removeFolder: (path) => invoke('remove_folder', { path }),
    coverFile: (path) => invoke('cover_file', { path }),
    loadSettings: (name) => invoke('load_settings', { name }),
    saveSettings: (name, text) => invoke('save_settings', { name, text }),
    loadSealed: (name) => invoke('load_sealed', { name }),
    saveSealed: (name, text) => invoke('save_sealed', { name, text }),
    async pickFile(title, extensions) {
      const raw = await rawCall('pick_file', { title, extensions: extensions ?? [] });
      if (raw.length < 4) return null;
      const length = new DataView(raw.buffer, raw.byteOffset).getUint32(0, true);
      return { name: new TextDecoder().decode(raw.subarray(4, 4 + length)), bytes: raw.slice(4 + length) };
    },
    saveFile: (title, name, bytes) => invoke('save_file', packed({ title, name }, bytes)),
    openUrl: (url) => invoke('open_url', { url }),
    checkUsernamePage: (site, name, online) => invoke('check_username_page', { site, name, online }),
    memoryGib: () => invoke('memory_gib'),
    menu(items: MenuItem[]) {
      pendingMenu?.(null);
      return new Promise((resolve) => {
        pendingMenu = resolve;
        void invoke('popup_menu', { items });
      });
    },
    onCommand(run) {
      void listen<string>('command', (event) => run(event.payload));
    },
    stateChanged({ unlocked, handle }) {
      void invoke('state_changed', { unlocked, handle: handle ?? 0 });
    },
    setScreenProtection: (on) => invoke('set_screen_protection', { on }),
    places,
    hardware: {
      async tap(kind, factor) {
        const { decodeReply } = await import('$ui/engine/frame');
        const reply = decodeReply(await rawCall('hardware_tap', { kind, factor: factor ?? null }));
        return { factor: reply.json, output: reply.secret! };
      },
    },
    outside: {
      choose: (title, extensions) => invoke('outside_choose', { title, extensions: extensions ?? [] }),
      read: (path) => rawCall('outside_read', { path }),
      write: (path, bytes) => invoke('outside_write', packed({ path }, bytes)),
    },
    lan: {
      deviceName: () => invoke('device_name'),
      localAddress: () => invoke('local_address'),
      blobDir: (folder) => invoke('blob_dir', { folder }),
    },
    desktop: {
      activity: () => invoke('activity'),
      setLockRule: (afterSeconds, onSleep) => invoke('set_lock_rule', { afterSeconds, onSleep }),
      quickAvailable: () => invoke('quick_available'),
      quickEnrolled: (file) => invoke('quick_enrolled', { file }),
      quickEnroll: (handle, file) => invoke('quick_enroll', { handle, file }),
      quickForget: (file) => invoke('quick_forget', { file }),
      quickOpen: (file, sealed) => rawCall('quick_open', packed({ file }, sealed)),
      installBrowserHost: () => invoke('install_browser_host'),
      removeBrowserHost: () => invoke('remove_browser_host'),
      bridgeFingerprint: () => invoke('bridge_fingerprint'),
      bridgeState: () => invoke('bridge_state'),
      bridgeSetEnabled: (on, askSites) => invoke('bridge_set_enabled', { on, askSites }),
      bridgeForget: (id) => invoke('bridge_forget', { id }),
      sshState: () => invoke('ssh_state'),
      sshSetEnabled: (on, confirm) => invoke('ssh_set_enabled', { on, confirm }),
      autoType: (id) => invoke('auto_type', { id }),
      autostart: () => invoke('autostart'),
      setAutostart: (on) => invoke('set_autostart', { on }),
      setGlobalShortcut: (chord) => invoke('set_global_shortcut', { chord }),
      setCloseToTray: (on) => invoke('set_close_to_tray', { on }),
      setShortcuts: (shortcuts) => invoke('set_shortcuts', { shortcuts }),
      showMain: () => invoke('show_main'),
      hidePalette: () => invoke('hide_palette'),
      currentHandle: () => invoke('current_handle'),
      checkUpdate: () => invoke('check_update'),
      installUpdate: () => invoke('install_update'),
      version: () => invoke('app_version'),
    },
  };
}

// Places outside this computer. A run can change a place (a rotated token, a first host key);
// the shell holds those changes until they are collected here and saved with the place.
async function collected<T>(run: Promise<T>): Promise<T> {
  try {
    return await run;
  } finally {
    const updates: [string, Record<string, unknown>][] = await invoke('places_updates');
    for (const [id, extra] of updates) places.onUpdate?.(id, extra);
  }
}

const places: PlacesHost = {
  kinds: ['folder', 'webdav', 's3'],
  list: (place) => collected(invoke('places_list', { place, online: prefs.v.network_allowed })),
  read: (place, file: PlaceFile) => collected(rawCall('places_read', { place, online: prefs.v.network_allowed, file })),
  write: (place, name, bytes) => collected(invoke('places_write', packed({ place, online: prefs.v.network_allowed, name }, bytes))),
  remove: (place, file) => collected(invoke('places_remove', { place, online: prefs.v.network_allowed, file })),
  chooseFolder: () => invoke('places_choose_folder'),
  signIn: (place, step) => invoke('places_sign_in', { place, step, online: prefs.v.network_allowed }),
};

void invoke<string[]>('places_kinds').then((kinds) => (places.kinds = kinds));
