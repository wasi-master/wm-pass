<script lang="ts">
  let { score = 0, active = true }: { score?: number; active?: boolean } = $props();
  const color = $derived(score >= 4 ? 'var(--primary)' : score === 3 ? 'var(--info)' : score === 2 ? 'var(--warn)' : 'var(--danger)');
</script>

<div class="strength" role="img" aria-label="Strength {score} out of 4">
  {#each [0, 1, 2, 3] as i}
    <i style={active && i < Math.max(1, score) ? `background:${color}` : ''}></i>
  {/each}
</div>
