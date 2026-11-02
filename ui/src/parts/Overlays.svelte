<script lang="ts">
  import { overlays } from './floating.svelte';
  import Dialog from './Dialog.svelte';
  import Btn from './Btn.svelte';
  import type { MenuItem } from '../engine/host';

  let menuElement = $state<HTMLElement>();
  let at = $state({ x: 0, y: 0 });

  $effect(() => {
    const menu = overlays.menu;
    if (menu && menuElement) {
      const box = menuElement.getBoundingClientRect();
      at = {
        x: Math.min(menu.x, window.innerWidth - box.width - 8),
        y: Math.min(menu.y, window.innerHeight - box.height - 8),
      };
    }
  });

  function flat(items: MenuItem[]): MenuItem[] {
    return items.flatMap((item) => (item.items ? [{ label: item.label, disabled: true }, ...item.items] : [item]));
  }
</script>

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
  <div class="menu" role="menu" bind:this={menuElement} style="left:{at.x || overlays.menu.x}px;top:{at.y || overlays.menu.y}px">
    {#each flat(overlays.menu.items) as item}
      {#if item.separator}
        <hr />
      {:else}
        <button type="button" role="menuitem" class:danger={item.danger} disabled={item.disabled} onclick={() => overlays.closeMenu(item.id ?? null)}>
          <span class="grow ell">{item.label}</span>
          {#if item.checked}<span>✓</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<div class="toasts" aria-live="polite">
  {#each overlays.toasts as toast (toast.id)}
    <div class="toast">
      <span class="grow">{toast.text}</span>
      {#if toast.action}
        <button type="button" onclick={() => { toast.run?.(); overlays.dismiss(toast.id); }}>{toast.action}</button>
      {/if}
    </div>
  {/each}
</div>
