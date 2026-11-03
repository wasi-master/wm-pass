<script lang="ts">
  import Tile from '../../parts/Tile.svelte';
  import { prefs } from '../../engine/prefs.svelte';

  let { value, pref }: { value?: unknown; pref?: string } = $props();
  const style = $derived(pref === 'site_icon_style' && typeof value === 'string' ? value : prefs.v.site_icon_style);
  const shape = $derived(pref === 'site_icon_shape' && typeof value === 'string' ? value : prefs.v.site_icon_shape);
  const sites: [string, string | null, string][] = [
    ['GitHub', 'github.com', 'login'],
    ['Spotify', 'spotify.com', 'login'],
    ['Mozilla', 'mozilla.org', 'login'],
    ['Corner shop', null, 'login'],
    ['Visa card', null, 'card'],
  ];
</script>

<div class="gap wrap" style="gap:14px">
  {#each sites as [title, site, kind] (title)}
    <div style="display:flex;flex-direction:column;align-items:center;gap:4px;width:64px">
      <Tile {title} {site} {kind} size={36} {style} {shape} />
      <span class="small dim ell" style="max-width:64px">{title}</span>
    </div>
  {/each}
</div>
<p class="preview-caption">Sites with a known mark, and items without one.</p>
