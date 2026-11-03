<script lang="ts">
  import { strokeOf } from '../engine/icons.svelte';
  // One glyph in a given style: a filled shape, or lines for a pack drawn with lines.
  let { d, style, size = 18, class: extra = '' }: { d: string | undefined; style: string; size?: number; class?: string } = $props();
  const stroke = $derived(strokeOf(style));
</script>

{#if stroke}
  <svg class={extra} width={size} height={size} viewBox="0 -960 960 960" fill="none" stroke="currentColor" stroke-width={stroke} stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path {d} /></svg>
{:else}
  <svg class={extra} width={size} height={size} viewBox="0 -960 960 960" fill="currentColor" aria-hidden="true"><path {d} /></svg>
{/if}
