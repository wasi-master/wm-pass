<script lang="ts">
  import Icon from '../../parts/Icon.svelte';
  import type { IconName } from '../../engine/icons.svelte';
  import { prefs } from '../../engine/prefs.svelte';
  import { ROUTE_COLOURS } from '../../settings/nav.svelte';

  let { value, pref }: { value?: unknown; pref?: string } = $props();
  const style = $derived(pref === 'settings_icons' && typeof value === 'string' ? value : prefs.v.settings_icons);
  const rows: [string, string, IconName][] = [
    ['security', 'Security', 'shield'],
    ['backup', 'Backups', 'backup'],
    ['look', 'Appearance', 'palette'],
  ];
</script>

<div class="set-group" style="margin:0">
  <div class="body">
    {#each rows as [id, title, icon] (id)}
      <div class="set-row">
        {#if style === 'coloured'}
          <span class="tile-glyph" style="background:{ROUTE_COLOURS[id]}"><Icon name={icon} size={16} /></span>
        {:else if style === 'mono'}
          <span class="tile-glyph mono"><Icon name={icon} size={16} /></span>
        {:else if style === 'bare'}
          <span class="tile-glyph bare" style="color:{ROUTE_COLOURS[id]}"><Icon name={icon} size={18} /></span>
        {/if}
        <span class="text">{title}</span>
      </div>
    {/each}
  </div>
</div>
