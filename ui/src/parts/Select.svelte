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

  function picked(event: Event) {
    const index = (event.target as HTMLSelectElement).selectedIndex;
    onchange?.(options[index][0]);
  }
</script>

<span class="select">
  <select aria-label={label} {disabled} onchange={picked}>
    {#each options as [id, name]}
      <option selected={id === value}>{name}</option>
    {/each}
  </select>
  <span class="chev"><Icon name="unfold_more" size={14} /></span>
</span>
