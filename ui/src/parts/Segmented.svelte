<script lang="ts" generics="T extends string | number">
  let {
    value,
    options,
    label,
    onchange,
  }: {
    value: T;
    options: [T, string][];
    /** What the choice is about, for a screen reader: the buttons say only the choices. */
    label?: string;
    onchange?: (value: T) => void;
  } = $props();
</script>

<span class="segmented" role="group" aria-label={label}>
  {#each options as [id, name]}
    <button type="button" aria-pressed={id === value} onclick={() => onchange?.(id)}>{name}</button>
  {/each}
</span>
