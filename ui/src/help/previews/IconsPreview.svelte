<script lang="ts">
  import { icons, type IconName } from '../../engine/icons.svelte';
  import { prefs } from '../../engine/prefs.svelte';

  // The app's own glyphs in the style shown. The style's table loads only when looked at.
  let { value, pref }: { value?: unknown; pref?: string } = $props();
  const style = $derived(pref === 'icon_style' && typeof value === 'string' ? value : prefs.v.icon_style);
  let table = $state.raw<Record<string, string>>(icons.table);
  $effect(() => {
    const wanted = style;
    void icons.tableOf(wanted).then((t) => {
      if (wanted === style) table = t;
    });
  });
  const names: IconName[] = ['lock', 'search', 'add', 'star', 'content_copy', 'key', 'folder', 'settings', 'delete', 'sync'];
</script>

<div class="gap wrap" style="gap:16px;color:var(--text2)">
  {#each names as name (name)}
    <svg width="24" height="24" viewBox="0 -960 960 960" fill="currentColor" aria-hidden="true"><path d={table[name]} /></svg>
  {/each}
</div>
