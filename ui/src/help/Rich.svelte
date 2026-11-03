<script lang="ts">
  import { blocks, type Run } from '$help/rich.ts';
  import type { Rich } from '$help/schema.ts';
  import { help } from './help.svelte';

  // The guide's text, drawn as paragraphs and lists. Links open another topic in the same sheet.
  let { text, lead = false, onlink }: { text: Rich | undefined; lead?: boolean; onlink?: (id: string) => void } = $props();
  const parts = $derived(blocks(text));

  function follow(id: string) {
    if (onlink) onlink(id);
    else void help.open(id);
  }
</script>

{#snippet line(runs: readonly Run[])}
  {#each runs as run}
    {#if run.t === 'text'}{run.s}{:else if run.t === 'strong'}<strong>{run.s}</strong>{:else if run.t === 'em'}<em>{run.s}</em>{:else if run.t === 'key'}{@const label = help.keyLabel(run.action)}{#if label}<kbd class="kbd">{label}</kbd>{:else}<span class="dim">no shortcut</span>{/if}{:else if run.t === 'link'}<button type="button" class="link" onclick={() => follow(run.id)}>{run.s ?? help.title(run.id) ?? run.id}</button>{/if}
  {/each}
{/snippet}

<div class="rich" class:lead>
  {#each parts as block}
    {#if block.t === 'p'}
      <p>{@render line(block.runs)}</p>
    {:else}
      <ul>
        {#each block.items as item}<li>{@render line(item)}</li>{/each}
      </ul>
    {/if}
  {/each}
</div>
