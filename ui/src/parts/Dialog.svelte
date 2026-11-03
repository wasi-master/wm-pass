<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    wide = false,
    onclose,
    children,
    head,
    foot,
    class: extra = '',
    onkeydown,
  }: {
    title?: string;
    wide?: boolean;
    onclose?: () => void;
    children?: Snippet;
    /** A head of its own, in place of the plain title. */
    head?: Snippet;
    foot?: Snippet;
    class?: string;
    onkeydown?: (event: KeyboardEvent) => void;
  } = $props();
  const id = `dialog-${Math.random().toString(36).slice(2, 8)}`;

  let element: HTMLDialogElement;

  $effect(() => {
    element.showModal();
    return () => element.close();
  });

  function clicked(event: MouseEvent) {
    // A click on the dialog itself, not its contents, landed on the backdrop.
    if (event.target === element) onclose?.();
  }

  function cancelled(event: Event) {
    event.preventDefault();
    onclose?.();
  }
</script>

<dialog bind:this={element} class="sheet {extra}" class:wide aria-labelledby={title || head ? id : undefined} onclick={clicked} oncancel={cancelled} {onkeydown}>
  <div class="sheet-inner">
    {#if head}<div class="sheet-head" {id}>{@render head()}</div>{:else if title}<div class="sheet-head" {id}>{title}</div>{/if}
    <div class="sheet-body">{@render children?.()}</div>
    {#if foot}<div class="sheet-foot">{@render foot()}</div>{/if}
  </div>
</dialog>
