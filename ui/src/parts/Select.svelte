<script lang="ts" generics="T extends string | number">
  import Icon from './Icon.svelte';
  let {
    value,
    options,
    label,
    disabled = false,
    onchange,
  }: {
    value: T;
    options: [T, string][];
    label?: string;
    disabled?: boolean;
    onchange?: (value: T) => void;
  } = $props();

  // A value none of the options name (from another device, or a newer app) is shown as it is, at
  // the top, instead of the menu falling back to its first option while the value stays. An empty
  // value is the caller's default, such as the system's own camera.
  const known = $derived(options.some(([id]) => id === value));

  function picked(event: Event) {
    const index = (event.target as HTMLSelectElement).selectedIndex - (known ? 0 : 1);
    if (index < 0) return;
    onchange?.(options[index][0]);
  }
</script>

<span class="select">
  <select aria-label={label} {disabled} onchange={picked}>
    {#if !known}<option selected>{value === '' ? 'Default' : `Other (${value})`}</option>{/if}
    {#each options as [id, name]}
      <option selected={id === value}>{name}</option>
    {/each}
  </select>
  <span class="chev"><Icon name="unfold_more" size={14} /></span>
</span>
