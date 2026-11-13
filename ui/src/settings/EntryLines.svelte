<script lang="ts">
  import type { Snippet } from 'svelte';
  import Tile from '../parts/Tile.svelte';
  import type { Summary } from '../engine/types';

  let { items, onpick, note }: { items: Summary[]; onpick: (entry: Summary) => void; note?: Snippet<[Summary]> } = $props();
</script>

<div class="set-group" style="margin-bottom:0">
  <div class="body">
    {#each items as entry (entry.id)}
      <button type="button" class="set-row" onclick={() => onpick(entry)}>
        <Tile title={entry.title} site={entry.site} kind={entry.kind} label={entry.color} icon={entry.icon} size={28} />
        <span class="text">
          <span class="ell" style="display:block;font-weight:500">{entry.title || 'Untitled'}</span>
          {#if note}{@render note(entry)}{:else if entry.subtitle}<span class="sub ell" style="display:block">{entry.subtitle}</span>{/if}
        </span>
      </button>
    {/each}
  </div>
</div>
