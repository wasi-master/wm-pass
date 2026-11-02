<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    wide = false,
    onclose,
    children,
    foot,
  }: {
    title?: string;
    wide?: boolean;
    onclose?: () => void;
    children?: Snippet;
    foot?: Snippet;
  } = $props();

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

<dialog bind:this={element} class="sheet" class:wide onclick={clicked} oncancel={cancelled}>
  <div class="sheet-inner">
    {#if title}<div class="sheet-head">{title}</div>{/if}
    <div class="sheet-body">{@render children?.()}</div>
    {#if foot}<div class="sheet-foot">{@render foot()}</div>{/if}
  </div>
</dialog>
