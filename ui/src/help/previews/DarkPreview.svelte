<script lang="ts">
  import { prefs } from '../../engine/prefs.svelte';
  import { applyTheme } from '../../theme/themes';
  import WindowMock from './WindowMock.svelte';

  // The window in your theme, light or dark as shown, without changing the real window.
  let { value, pref }: { value?: unknown; pref?: string } = $props();
  const look = $derived(pref === 'look' && typeof value === 'string' ? value : prefs.v.look);
  let box = $state<HTMLElement>();
  $effect(() => {
    if (!box) return;
    const dark = look === 'SYSTEM' ? matchMedia('(prefers-color-scheme: dark)').matches : look !== 'LIGHT';
    applyTheme(prefs.theme, dark, look === 'OBSIDIAN', box);
  });
</script>

<div bind:this={box} class="mock-frame"><WindowMock /></div>
<p class="preview-caption">{prefs.theme.name}, {look === 'SYSTEM' ? 'following the system' : look === 'OBSIDIAN' ? 'with black behind it' : look === 'DARK' ? 'dark' : 'light'}.</p>
