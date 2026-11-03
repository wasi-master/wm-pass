<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { AnchorId, TopicId } from '$help';
  import HelpButton from '../help/HelpButton.svelte';

  // A titled run of settings rows. A heading the guide explains carries a "?", and a tour can
  // point at the whole group.
  let { title, help, anchor, children }: { title?: string; help?: TopicId | null; anchor?: AnchorId; children?: Snippet } = $props();
</script>

<section class="set-group" data-help-anchor={anchor}>
  {#if title}
    <h2 class="title">
      {title}
      {#if help}<HelpButton id={help} {title} />{/if}
    </h2>
  {/if}
  <div class="body">{@render children?.()}</div>
</section>
