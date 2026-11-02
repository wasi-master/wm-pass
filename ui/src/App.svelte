<script lang="ts">
  import Overlays from './parts/Overlays.svelte';
  import Door from './door/Door.svelte';
  import Main from './vault/Main.svelte';
  import { pass } from './engine/pass.svelte';
  import { prefs } from './engine/prefs.svelte';
  import { host } from './engine/host';
  import { icons } from './engine/icons.svelte';
  import { applyTheme } from './theme/themes';
  import { ui } from './vault/ui.svelte';
  import { onMount, untrack } from 'svelte';

  let main = $state<Main>();
  let systemDark = $state(window.matchMedia('(prefers-color-scheme: dark)').matches);

  onMount(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = (e: MediaQueryListEvent) => (systemDark = e.matches);
    media.addEventListener('change', onChange);
    const resize = () => (ui.width = window.innerWidth);
    resize();
    window.addEventListener('resize', resize);
    host().onCommand?.((name) => main?.command(name));
    return () => {
      media.removeEventListener('change', onChange);
      window.removeEventListener('resize', resize);
    };
  });

  $effect(() => {
    const look = prefs.v.look;
    const dark = look === 'SYSTEM' ? systemDark : look !== 'LIGHT';
    applyTheme(prefs.theme, dark, look === 'OBSIDIAN');
    untrack(() => prefs.themeApplied++);
  });

  $effect(() => {
    const root = document.documentElement;
    root.dataset.motion = prefs.v.rich_motion ? 'on' : 'off';
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
      void host().desktop?.activity();
    }
  }

  $effect(() => {
    void host().desktop?.setLockRule(prefs.v.lock_after, prefs.v.lock_on_sleep);
  });

  $effect(() => {
    void host().desktop?.setShortcuts(prefs.shortcuts);
  });

  $effect(() => {
    void host().desktop?.bridgeSetEnabled(prefs.v.bridge_enabled, prefs.v.bridge_ask_every_site);
  });

  $effect(() => {
    if (pass.state.kind !== 'unlocked') return;
    let last = Date.now();
    const timer = setInterval(() => {
      const now = Date.now();
      // A timer that was held up for a long time means the machine was asleep.
      const slept = now - last > 30_000;
      last = now;
      const after = prefs.v.lock_after;
      if ((slept && prefs.v.lock_on_sleep) || (after >= 0 && now - lastActive > Math.max(after, 5) * 1000)) void pass.lock();
    }, 2000);
    const hidden = () => {
      if (document.hidden && prefs.v.lock_on_hide) void pass.lock();
    };
    document.addEventListener('visibilitychange', hidden);
    return () => {
      clearInterval(timer);
      document.removeEventListener('visibilitychange', hidden);
    };
  });

  // Leaving the page must not lose a pending save.
  $effect(() => {
    const flush = () => {
      if (pass.handle) void pass.flush();
    };
    window.addEventListener('pagehide', flush);
    return () => window.removeEventListener('pagehide', flush);
  });
</script>

<svelte:window onpointerdown={active} onkeydown={active} onwheel={active} />

{#if pass.state.kind === 'unlocked'}
  <Main bind:this={main} />
{:else}
  {#key pass.state.kind}
    <Door />
  {/key}
{/if}
<Overlays />
