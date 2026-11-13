<script lang="ts">
  import { icons, type IconName } from '../../engine/icons.svelte';
  import { prefs } from '../../engine/prefs.svelte';
  import Glyph from '../../parts/Glyph.svelte';

  // The app's own glyphs in the style shown. The style's table loads only when looked at.
  let { value, pref }: { value?: unknown; pref?: string } = $props();
  const style = $derived(pref === 'icon_style' && typeof value === 'string' ? value : prefs.v.icon_style);
  let table = $state.raw<Record<string, string>>(icons.table);
  let shown = $state(icons.style);
  $effect(() => {
    const wanted = style;
    void icons.load(wanted).then((got) => {
      if (wanted !== style) return;
      table = got.table;
      shown = got.style;
    });
  });
  const names: IconName[] = ['lock', 'search', 'add', 'star', 'content_copy', 'key', 'folder', 'settings', 'delete', 'sync'];
</script>

<div class="gap wrap" style="gap:16px;color:var(--text2)">
  {#each names as name (name)}
    <Glyph d={table[name]} style={shown} size={24} />
  {/each}
</div>
