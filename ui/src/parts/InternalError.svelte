<script lang="ts">
  // The message after an internal error (engine/stopped.svelte.ts). It sits over the screen
  // without closing it or making it inert, so whatever was being typed can still be selected and
  // copied. Only its button, or Escape, goes on to the door.
  import Btn from './Btn.svelte';
  import { pass } from '../engine/pass.svelte';

  let element: HTMLDialogElement;
  let leaving = $state(false);

  $effect(() => {
    // A sheet already up, such as a field being added, makes everything outside it inert: this
    // goes over it as a sheet of its own, or its button could not be reached.
    if (document.querySelector('dialog:modal')) element.showModal();
    else element.show();
    element.querySelector<HTMLButtonElement>('button')?.focus({ preventScroll: true });
    // Escape anywhere, before any screen hears it and closes what it shows.
    const key = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopImmediatePropagation();
      void go();
    };
    window.addEventListener('keydown', key, true);
    return () => {
      window.removeEventListener('keydown', key, true);
      element.close();
    };
  });

  async function go() {
    if (leaving) return;
    leaving = true;
    await pass.leaveInternal().catch(() => undefined);
    leaving = false;
  }
</script>

<dialog
  bind:this={element}
  class="sheet internal-error"
  role="alertdialog"
  aria-labelledby="internal-error-title"
  aria-describedby="internal-error-body"
  oncancel={(event) => {
    event.preventDefault();
    void go();
  }}
>
  <div class="sheet-inner">
    <div class="sheet-head" id="internal-error-title">Something went wrong</div>
    <div class="sheet-body">
      <p id="internal-error-body">
        WM Pass hit an internal error and locked the vault to keep it safe. Anything you were typing is still on this screen: copy it if you need it, then unlock again. Saved items are not affected.
      </p>
    </div>
    <div class="sheet-foot">
      <Btn label="Go to unlock" variant="primary" disabled={leaving} onclick={() => void go()} />
    </div>
  </div>
</dialog>

<style>
  /* Not modal: placed over the top of the window, the screen under it still in reach. */
  dialog.internal-error:not(:modal) {
    position: fixed;
    inset: 16px 0 auto;
    margin: 0 auto;
    z-index: 1000;
  }
</style>
