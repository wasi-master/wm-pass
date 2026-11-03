<script lang="ts">
  import Icon from '../../parts/Icon.svelte';
  import Tile from '../../parts/Tile.svelte';
  import type { IconName } from '../../engine/icons.svelte';
  import { SAMPLE_ROWS } from './sample';

  // A small drawing of the window in whatever colours the box around it has. Clicking a part
  // opens what the guide says about it, when [onpart] is given.
  let { onpart }: { onpart?: (part: 'sidebar' | 'list' | 'item') => void } = $props();
  const tabs: [IconName, string][] = [
    ['inventory_2', 'All items'],
    ['star', 'Favorites'],
    ['pin', 'Codes'],
    ['casino', 'Generator'],
  ];
</script>

<div class="mock" role="presentation">
  <button type="button" class="mock-side" disabled={!onpart} onclick={() => onpart?.('sidebar')} aria-label="The sidebar">
    {#each tabs as [icon, label], i (label)}
      <span class="mock-tab" class:on={i === 0}><Icon name={icon} size={12} />{label}</span>
    {/each}
    <span class="grow"></span>
    <span class="mock-tab"><Icon name="lock" size={12} /><Icon name="settings" size={12} /></span>
  </button>
  <button type="button" class="mock-list" disabled={!onpart} onclick={() => onpart?.('list')} aria-label="The list">
    <span class="mock-search"><Icon name="search" size={11} />Search</span>
    {#each SAMPLE_ROWS.slice(0, 3) as row, i (row.id)}
      <span class="mock-row" class:on={i === 0}><Tile title={row.title} site={row.site} kind={row.kind} size={16} /><span class="ell">{row.title}</span></span>
    {/each}
  </button>
  <button type="button" class="mock-item" disabled={!onpart} onclick={() => onpart?.('item')} aria-label="The item page">
    <span class="gap" style="gap:6px"><Tile title="GitHub" site="github.com" kind="login" size={22} /><strong>GitHub</strong></span>
    <span class="gap" style="gap:4px"><span class="mock-btn primary">Copy password</span><span class="mock-btn">Copy username</span></span>
    <span class="mock-field"><span class="dim">Username</span>sam@example.com</span>
    <span class="mock-field"><span class="dim">Password</span>••••••••••</span>
  </button>
</div>
