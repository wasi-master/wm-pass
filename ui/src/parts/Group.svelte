<script lang="ts">
  import type { Snippet } from 'svelte';
  let { title, children }: { title?: string; children?: Snippet } = $props();
</script>

<section class="set-group">
  {#if title}<h2 class="title">{title}</h2>{/if}
  <div class="body">{@render children?.()}</div>
</section>
