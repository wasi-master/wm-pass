<script lang="ts">
  import { overlays } from './floating.svelte';
  import Dialog from './Dialog.svelte';
  import Btn from './Btn.svelte';
  import type { MenuItem } from '../engine/host';

  let menuElement = $state<HTMLElement>();
  // Where the open menu was moved to stay in the window, and which menu that was: a new menu's
  // first frame is at its own point, not where the last one ended up.
  let at = $state.raw<{ x: number; y: number; menu: unknown } | null>(null);
  const place = $derived(overlays.menu && at?.menu === overlays.menu ? at : overlays.menu);

  $effect(() => {
    const menu = overlays.menu;
    if (menu && menuElement) {
      const box = menuElement.getBoundingClientRect();
      // Kept inside the window on every side: a menu placed from a button's right edge, such as
      // an item's More menu, would otherwise start off the left of a narrow window.
      at = {
        x: Math.max(8, Math.min(menu.x, window.innerWidth - box.width - 8)),
        y: Math.max(8, Math.min(menu.y, window.innerHeight - box.height - 8)),
        menu,
      };
    }
  });

  // From the keyboard: the first choice takes the focus, the arrows move, Escape closes the menu
  // and nothing behind it, such as the search, hears it.
  $effect(() => {
    if (overlays.menu && menuElement) menuElement.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  });

  function menuKey(event: KeyboardEvent) {
    if (!overlays.menu) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      overlays.closeMenu(null);
    } else if (event.key === 'Tab') {
      // The focus would otherwise go on behind the menu while it stays open. Closed, the focus is
      // back where it was, and Tab moves on from there.
      overlays.closeMenu(null);
    } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) && menuElement) {
      event.preventDefault();
      const buttons = Array.from(menuElement.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
      const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next =
        event.key === 'Home'
          ? 0
          : event.key === 'End'
            ? buttons.length - 1
            : event.key === 'ArrowDown'
              ? (at + 1) % buttons.length
              : (at - 1 + buttons.length) % buttons.length;
      buttons[next]?.focus();
    }
  }

  // One menu is open at a time, so its headings' ids need only differ from the rest of the page.
  const menuId = `menu-${Math.random().toString(36).slice(2, 8)}`;
</script>

<svelte:window onkeydowncapture={menuKey} />

{#if overlays.question}
  {@const q = overlays.question}
  <Dialog title={q.title} onclose={() => overlays.answer(false)}>
    {#if q.body}<p class="dim">{q.body}</p>{/if}
    {#snippet foot()}
      <Btn label={q.cancel ?? 'Cancel'} onclick={() => overlays.answer(false)} />
      <Btn label={q.action} variant={q.danger ? 'danger-fill' : 'primary'} onclick={() => overlays.answer(true)} />
    {/snippet}
  </Dialog>
{/if}

{#if overlays.menu}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div style="position:fixed;inset:0;z-index:49" onclick={() => overlays.closeMenu(null)} oncontextmenu={(e) => { e.preventDefault(); overlays.closeMenu(null); }}></div>
  <div class="menu" role="menu" tabindex="-1" bind:this={menuElement} oncontextmenu={(e) => e.preventDefault()} style="left:{place?.x ?? 0}px;top:{place?.y ?? 0}px">
    {#each overlays.menu.items as item, at}
      {#if item.separator}
        <hr />
      {:else if item.items}
        <!-- A part of the menu under a heading: the heading names the group, and is no choice
             itself, so the arrows pass over it. -->
        <div role="group" aria-labelledby="{menuId}-{at}">
          <div class="menu-head" id="{menuId}-{at}" role="presentation">{item.label}</div>
          {#each item.items as inner}
            {#if inner.separator}<hr />{:else}{@render choice(inner)}{/if}
          {/each}
        </div>
      {:else}
        {@render choice(item)}
      {/if}
    {/each}
  </div>
{/if}

{#snippet choice(item: MenuItem)}
  <!-- A choice that can be ticked says whether it is, rather than only drawing the tick. -->
  <button
    type="button"
    role={item.checked !== undefined ? 'menuitemcheckbox' : 'menuitem'}
    aria-checked={item.checked !== undefined ? !!item.checked : undefined}
    class:danger={item.danger}
    disabled={item.disabled}
    onclick={() => overlays.closeMenu(item.id ?? null)}
  >
    <span class="grow ell">{item.label}</span>
    {#if item.checked}<span aria-hidden="true">✓</span>{/if}
  </button>
{/snippet}

<!-- Read out as they come. A message stays while the pointer or the keyboard is on it. -->
<div class="toasts" role="status" aria-live="polite">
  {#each overlays.toasts as toast (toast.id)}
    <div
      class="toast"
      role="presentation"
      onpointerenter={() => overlays.hold(toast.id, true)}
      onpointerleave={() => overlays.hold(toast.id, false)}
      onfocusin={() => overlays.hold(toast.id, true)}
      onfocusout={() => overlays.hold(toast.id, false)}
    >
      <span class="grow">{toast.text}</span>
      {#if toast.action}
        <button type="button" onclick={() => { toast.run?.(); overlays.dismiss(toast.id); }}>{toast.action}</button>
      {/if}
    </div>
  {/each}
</div>
