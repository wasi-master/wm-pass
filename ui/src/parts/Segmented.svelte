<script lang="ts" generics="T extends string | number">
  let {
    value,
    options,
    onchange,
  }: {
    value: T;
    options: [T, string][];
    onchange?: (value: T) => void;
  } = $props();
</script>

<span class="segmented" role="group">
  {#each options as [id, name]}
    <button type="button" aria-pressed={id === value} onclick={() => onchange?.(id)}>{name}</button>
  {/each}
</span>
