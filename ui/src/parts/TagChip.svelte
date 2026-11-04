<script lang="ts">
  import Icon from './Icon.svelte';
  import { tags } from '../engine/tags.svelte';

  // A tag as a chip, with the symbol it wears. Chosen, it carries a cross that takes it off; a
  // right-click asks to rename or delete it on every item.
  let {
    tag,
    selected = false,
    onclick,
    onremove,
    onmanage,
  }: { tag: string; selected?: boolean; onclick?: () => void; onremove?: () => void; onmanage?: () => void } = $props();

  function menu(event: MouseEvent) {
    if (!onmanage) return;
    event.preventDefault();
    onmanage();
  }
</script>

<span class="chip tag-chip" class:on={selected} oncontextmenu={menu} role="presentation">
  <button type="button" class="main" aria-pressed={selected} {onclick} title={onmanage ? 'Right-click to rename or delete' : undefined}>
    <Icon name={tags.glyph(tag)} size={14} />{tag}
  </button>
  {#if onremove}
    <button type="button" class="cross" aria-label="Take off {tag}" title="Take off {tag}" onclick={onremove}><Icon name="close" size={12} /></button>
  {/if}
</span>

<style>
  .tag-chip { padding: 0; gap: 0; }
  .tag-chip.on { background: var(--second-c); border-color: transparent; color: var(--text); }
  .main {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 100%;
    padding: 0 9px;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    border-radius: 12px;
  }
  .cross {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    margin: 0 2px 0 -5px;
    border: 0;
    border-radius: 50%;
    background: none;
    color: inherit;
    padding: 0;
  }
  .cross:hover { background: var(--hover); }
</style>
