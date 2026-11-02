<script lang="ts">
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
</script>

<div class="reorder">
  {#each items as item, index (item)}
    <div
      class="item"
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
      <IconBtn icon="expand_more" label="Move {label(item)} down" disabled={index === items.length - 1} onclick={() => move(index, index + 1)} />
      {#if !locked.includes(item)}
        <IconBtn icon="remove" label="Remove {label(item)}" disabled={keepLast && items.length === 1} onclick={() => onchange(items.filter((i) => i !== item))} />
      {/if}
    </div>
  {/each}
</div>
