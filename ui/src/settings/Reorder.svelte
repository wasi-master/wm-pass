<script lang="ts">
  import { tick } from 'svelte';
  import IconBtn from '../parts/IconBtn.svelte';
  import Icon from '../parts/Icon.svelte';

  // A list to put in order: drag a row, or use the buttons. Everything can be reached by keyboard.
  let {
    items,
    label,
    keepLast = true,
    locked = [],
    onchange,
  }: {
    items: string[];
    label: (item: string) => string;
    keepLast?: boolean;
    locked?: string[];
    onchange: (items: string[]) => void;
  } = $props();

  let dragging = $state<number | null>(null);

  function move(from: number, to: number) {
    if (to < 0 || to >= items.length || from === to) return;
    const next = [...items];
    const [item] = next.splice(from, 1);
    next.splice(to, 0, item);
    onchange(next);
  }

  let list = $state<HTMLElement>();
  /**
   * Moved with its button, the row is put back in the page in its new place, which takes the
   * focus off the button. It goes back to the same button, or to the other one at the end of the
   * list where that one is now off, so the keyboard can keep moving it.
   */
  async function step(from: number, by: -1 | 1) {
    const item = items[from];
    move(from, from + by);
    await tick();
    const row = [...(list?.querySelectorAll<HTMLElement>('.item') ?? [])].find((r) => r.dataset.item === item);
    const buttons = row ? [...row.querySelectorAll<HTMLButtonElement>('button')] : [];
    const [up, down] = buttons;
    const want = by < 0 ? up : down;
    (want && !want.disabled ? want : by < 0 ? down : up)?.focus();
  }
</script>

<div class="reorder" role="list" bind:this={list}>
  {#each items as item, index (item)}
    <div
      class="item"
      data-item={item}
      draggable="true"
      role="listitem"
      style={dragging === index ? 'background:var(--hover)' : ''}
      ondragstart={(event) => {
        dragging = index;
        event.dataTransfer!.effectAllowed = 'move';
      }}
      ondragover={(event) => {
        event.preventDefault();
        if (dragging !== null && dragging !== index) {
          move(dragging, index);
          dragging = index;
        }
      }}
      ondragend={() => (dragging = null)}
    >
      <span class="dimmer" style="display:flex;cursor:grab"><Icon name="drag_handle" size={16} /></span>
      <span class="grow ell">{label(item)}</span>
      <!-- The glyph set has no upward chevron, so the downward one is turned over. -->
      <button type="button" class="icon-btn" title="Move {label(item)} up" aria-label="Move {label(item)} up" disabled={index === 0} onclick={() => step(index, -1)}>
        <span style="display:flex;transform:rotate(180deg)"><Icon name="expand_more" size={18} /></span>
      </button>
      <IconBtn icon="expand_more" label="Move {label(item)} down" disabled={index === items.length - 1} onclick={() => step(index, 1)} />
      {#if !locked.includes(item)}
        <IconBtn icon="remove" label="Remove {label(item)}" disabled={keepLast && items.length === 1} onclick={() => onchange(items.filter((i) => i !== item))} />
      {/if}
    </div>
  {/each}
</div>
