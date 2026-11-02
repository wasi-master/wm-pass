<script lang="ts">
  let {
    checked = false,
    disabled = false,
    label,
    onchange,
  }: { checked?: boolean; disabled?: boolean; label?: string; onchange?: (value: boolean) => void } = $props();
</script>

<button type="button" class="switch" role="switch" aria-checked={checked} aria-label={label} {disabled} onclick={() => onchange?.(!checked)}></button>
