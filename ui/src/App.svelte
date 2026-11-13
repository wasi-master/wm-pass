<script lang="ts">
  import Overlays from './parts/Overlays.svelte';
  import InternalError from './parts/InternalError.svelte';
  import { stopped } from './engine/stopped.svelte';
  import HelpSheet from './help/HelpSheet.svelte';
  import Coach from './help/Coach.svelte';
  import Welcome from './help/Welcome.svelte';
  import { help } from './help/help.svelte';
  import { overlays } from './parts/floating.svelte';
  import { chordsOf } from './engine/text';
  import Door from './door/Door.svelte';
  import Main from './vault/Main.svelte';
  import { pass } from './engine/pass.svelte';
  import { prefs } from './engine/prefs.svelte';
  import { host } from './engine/host';
  import { icons } from './engine/icons.svelte';
  import { applyTheme } from './theme/themes';
  import { themeFonts } from './theme/fonts.svelte';
  import { startWidgetFeed } from './settings/widgets.svelte';
  import { lan } from './settings/lan.svelte';
  import { ui } from './vault/ui.svelte';
  import { onMount, untrack } from 'svelte';

  let main = $state<Main>();
  const WAITING = new Set(['search', 'add', 'codes', 'generator', 'health', 'backups']);
  let pendingRoute = $state<string | null>(null);

  $effect(() => {
    if (!main || !pendingRoute) return;
    const route = pendingRoute;
    pendingRoute = null;
    untrack(() => main?.command(route));
  });
  let systemDark = $state(window.matchMedia('(prefers-color-scheme: dark)').matches);

  onMount(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = (e: MediaQueryListEvent) => (systemDark = e.matches);
    media.addEventListener('change', onChange);
    const resize = () => (ui.width = window.innerWidth);
    resize();
    window.addEventListener('resize', resize);
    host().onCommand?.((name) => {
      // A widget's panic lock: the wait is set before the door opens, whatever is on screen.
      if (name === 'panic') void pass.panic();
      // A bug in the session, met by something other than this page: the shell has locked it.
      else if (name === 'internal') pass.internalError();
      // At the door there is no window to explain but the door itself.
      else if (!main && (name === 'help' || name === 'guide')) void help.open('screen.door');
      // A page asked for from a widget waits for the vault to open, as on the phone.
      else if (!main && (WAITING.has(name) || name.startsWith('item:'))) pendingRoute = name;
      else main?.command(name);
    });
    startWidgetFeed();
    return () => {
      media.removeEventListener('change', onChange);
      window.removeEventListener('resize', resize);
    };
  });

  $effect(() => {
    const look = prefs.v.look;
    const dark = look === 'SYSTEM' ? systemDark : look !== 'LIGHT';
    applyTheme(prefs.theme, dark, look === 'OBSIDIAN');
    void themeFonts.use(prefs.theme.font);
    untrack(() => prefs.themeApplied++);
  });

  $effect(() => {
    const root = document.documentElement;
    root.dataset.motion = prefs.v.rich_motion ? 'on' : 'off';
    root.dataset.help = prefs.v.help_buttons;
    root.dataset.density = prefs.v.density;
    root.style.setProperty('font-size', `${prefs.v.text_scale}%`);
    root.style.setProperty('--fs', `${(13 * prefs.v.text_scale) / 100}px`);
    root.style.setProperty('--fs-s', `${(12 * prefs.v.text_scale) / 100}px`);
    root.style.setProperty('--fs-xs', `${(11 * prefs.v.text_scale) / 100}px`);
    root.style.setProperty('--fs-l', `${(15 * prefs.v.text_scale) / 100}px`);
    root.style.setProperty('--fs-xl', `${(20 * prefs.v.text_scale) / 100}px`);
  });

  $effect(() => {
    void icons.setStyle(prefs.v.icon_style);
  });

  $effect(() => {
    void host().setScreenProtection?.(prefs.v.screen_protection);
  });

  // ---- locking on its own ----

  let lastActive = Date.now();
  let lastTold = 0;
  function active() {
    lastActive = Date.now();
    if (lastActive - lastTold > 5000) {
      lastTold = lastActive;
      void (host().desktop ?? host().lockGuard)?.activity();
    }
  }

  $effect(() => {
    void (host().desktop ?? host().lockGuard)?.setLockRule(prefs.v.lock_after, prefs.v.lock_on_sleep);
  });

  $effect(() => {
    void host().desktop?.setShortcuts(prefs.shortcuts);
  });

  $effect(() => {
    void host().desktop?.bridgeSetEnabled(prefs.v.bridge_enabled, prefs.v.bridge_ask_every_site);
  });

  $effect(() => {
    if (pass.state.kind !== 'unlocked') return;
    // The idle clock starts when the vault opens, however it was opened: a vault opened with no
    // key or click in this window, such as one the desktop app lends, must not lock at once.
    lastActive = Date.now();
    let last = Date.now();
    let hiddenBefore = document.hidden;
    // A lock that fails still leaves the vault, and the next tick tries again: nothing to report here.
    const lock = () => void pass.lock().catch(() => undefined);
    const timer = setInterval(() => {
      const now = Date.now();
      // A timer that was held up for a long time means the machine was asleep. A browser holds up
      // a hidden tab's timers by itself, to once a minute, which is not sleep: a wait that began
      // or ended hidden has to be longer to count.
      const wasHidden = document.hidden || hiddenBefore;
      hiddenBefore = document.hidden;
      const slept = now - last > (wasHidden ? 150_000 : 30_000);
      last = now;
      const after = prefs.v.lock_after;
      if ((slept && prefs.v.lock_on_sleep) || (after >= 0 && now - lastActive > Math.max(after, 5) * 1000)) lock();
    }, 2000);
    const hidden = () => {
      if (document.hidden && prefs.v.lock_on_hide) lock();
    };
    document.addEventListener('visibilitychange', hidden);
    return () => {
      clearInterval(timer);
      document.removeEventListener('visibilitychange', hidden);
    };
  });

  // ---- devices on this network ----

  // While the vault in this folder is open, the computer waits for its paired devices by itself,
  // so a phone only has to choose Sync now.
  $effect(() => {
    if (pass.state.kind === 'unlocked' && host().lan) void lan.load();
  });
  const listenForDevices = $derived(
    pass.state.kind === 'unlocked' &&
      !pass.state.readOnly &&
      !!host().lan &&
      prefs.v.network_allowed &&
      lan.peers.length > 0 &&
      !!pass.file &&
      pass.account === null &&
      pass.child === null,
  );
  $effect(() => {
    if (!listenForDevices) return;
    untrack(() => lan.listen());
    return () => void lan.stopListening();
  });

  // A question or menu asked about the open vault goes with it: answered after a lock, it would
  // act on nothing, and it would sit over the door. So does a message's Undo.
  $effect(() => {
    if (pass.state.kind === 'unlocked') return;
    untrack(() => {
      if (overlays.question) overlays.answer(false);
      if (overlays.menu) overlays.closeMenu(null);
      overlays.forgetVault();
    });
  });

  // What the vault has to tell its owner: a password changed on two devices at once, a clock that
  // looks wrong. Said on every unlock, and again when a sync brings something it had not said.
  let noticesSaid = '';
  pass.onNotices = (notices, opened) => {
    const said = [notices.password_conflict?.message, notices.clock_warning?.message].filter(Boolean).join('\n');
    if (!said || (!opened && said === noticesSaid)) {
      noticesSaid = said;
      return;
    }
    noticesSaid = said;
    const conflict = notices.password_conflict;
    if (conflict) {
      overlays.toast(conflict.message, {
        action: 'Go to Unlock methods',
        run: () => {
          ui.go('settings/security');
          // Only the later change stayed: there is nothing to remove, and having looked is enough.
          if (!conflict.both_open) void pass.passwordLossSeen().catch(() => undefined);
        },
        ms: 30000,
      });
    }
    if (notices.clock_warning) overlays.toast(notices.clock_warning.message, { ms: 20000 });
  };

  // A save that fails would otherwise lose the change without a word.
  pass.onSaveError = (message) => overlays.toast(`Your changes could not be saved: ${message}`, { ms: 10000 });
  // Another window or program rekeyed the vault: this one locks rather than write the old key back.
  pass.onKeyChanged = (lost) => {
    const unsaved = lost || (ui.editing !== null && ui.editorDirty);
    overlays.toast(
      `The master password was changed in another window, so this one locked. Unlock with the new password.${unsaved ? ' Changes made here since its last save could not be kept: make them again after unlocking.' : ''}`,
      { ms: 20000 },
    );
  };

  // ---- the guide ----

  // A new vault, or the first one opened on this install, gets the welcome once. Someone who used
  // the app before the guide existed gets one pointer to it instead.
  $effect(() => {
    if (pass.state.kind !== 'unlocked') return;
    untrack(() => {
      if (!prefs.v.help_welcome_seen) help.welcome = true;
      else if (prefs.upgraded && !help.seen('guide-pointer')) {
        help.markSeen('guide-pointer');
        overlays.toast('The Guide explains every setting and screen.', { action: 'Open', run: () => main?.command('guide'), ms: 8000 });
      }
    });
  });

  function doorKey(event: KeyboardEvent) {
    active();
    if (pass.state.kind === 'unlocked') return;
    const helpChord = prefs.shortcuts.help;
    if (helpChord && chordsOf(event, host().os).includes(helpChord)) {
      event.preventDefault();
      void help.open('screen.door');
    }
  }

  // Leaving the page must not lose a pending save.
  $effect(() => {
    const flush = () => {
      if (pass.handle) void pass.flush();
    };
    window.addEventListener('pagehide', flush);
    return () => window.removeEventListener('pagehide', flush);
  });
</script>

<svelte:window onpointerdown={active} onkeydown={doorKey} onwheel={active} />

{#if pass.state.kind === 'unlocked'}
  <Main bind:this={main} />
{:else}
  {#key pass.state.kind}
    <Door />
  {/key}
{/if}
<HelpSheet />
{#if pass.state.kind === 'unlocked'}
  <Coach />
  {#if help.welcome}<Welcome />{/if}
{/if}
<Overlays />
{#if stopped.on}<InternalError />{/if}
