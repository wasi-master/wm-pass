<script lang="ts">
  import type { Snippet } from 'svelte';
  import { overlays } from './floating.svelte';

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
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    element.showModal();
    return () => {
      element.close();
      // The sheet is out of the page by the time this runs, and the browser only hands the focus
      // back from a dialog still in it: without this, the keyboard is left at the top of the window.
      const now = document.activeElement;
      if (before?.isConnected && (!now || now === document.body || element.contains(now))) before.focus({ preventScroll: true });
    };
  });

  // Where the press began. Selecting text in a field and letting go past the sheet's edge also
  // ends as a click on the dialog itself, and must not close it with what was typed.
  let pressedBackdrop = false;
  function pressed(event: PointerEvent) {
    pressedBackdrop = event.target === element;
  }

  function clicked(event: MouseEvent) {
    // A click on the dialog itself, not its contents, landed on the backdrop.
    if (event.target === element && pressedBackdrop) onclose?.();
    pressedBackdrop = false;
  }

  // A button that goes with what it did, such as a step's buttons once the next step shows, leaves
  // the keyboard on nothing. It goes back to the first thing in the sheet that takes it.
  function lost(event: FocusEvent) {
    if (event.relatedTarget) return;
    const gone = event.target as Element;
    requestAnimationFrame(() => {
      if (gone.isConnected || !element?.isConnected || !element.open) return;
      const now = document.activeElement;
      if (now && now !== document.body && now !== element) return;
      element
        .querySelector<HTMLElement>(
          '.sheet-body :is(button, input, select, textarea, [tabindex="0"]):not(:disabled), .sheet-foot button:not(:disabled), .sheet-head button:not(:disabled)',
        )
        ?.focus();
    });
  }

  function cancelled(event: Event) {
    event.preventDefault();
    onclose?.();
  }
</script>

<dialog bind:this={element} class="sheet {extra}" class:wide aria-labelledby={title || head ? id : undefined} onpointerdown={pressed} onclick={clicked} oncancel={cancelled} onfocusout={lost} {onkeydown}>
  <div class="sheet-inner">
    {#if head}<div class="sheet-head" {id}>{@render head()}</div>{:else if title}<div class="sheet-head" {id}>{title}</div>{/if}
    <div class="sheet-body">{@render children?.()}</div>
    {#if foot}<div class="sheet-foot">{@render foot()}</div>{/if}
  </div>
  <!-- An open sheet makes the rest of the window inert, the messages along the bottom included, so a
       screen reader would not hear "Copied" or an error said while it is up. They are said here too. -->
  <div class="sr-only" role="status">{#each overlays.toasts as toast (toast.id)}<p>{toast.text}</p>{/each}</div>
</dialog>
