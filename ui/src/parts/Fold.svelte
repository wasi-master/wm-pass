<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';

  let {
    title,
    summary = null,
    open = $bindable(false),
    ontoggle,
    children,
  }: {
    title: string;
    summary?: string | null;
    open?: boolean;
    ontoggle?: (open: boolean) => void;
    children?: Snippet;
  } = $props();
</script>

<div class="fold" class:open>
  <button
    type="button"
    class="head"
    aria-expanded={open}
    onclick={() => {
      open = !open;
      ontoggle?.(open);
    }}
  >
    <span class="grow">
      <span style="font-weight:500">{title}</span>
      {#if !open && summary}<span class="dim small ell" style="display:block">{summary}</span>{/if}
    </span>
    <span class="chev"><Icon name="expand_more" size={18} /></span>
  </button>
  {#if open}<div class="inner">{@render children?.()}</div>{/if}
</div>
