<script lang="ts">
  // A code's real lifetime. It empties as the code runs out and turns amber at the end.
  let { period, left, size = 22 }: { period: number; left: number; size?: number } = $props();
  const stroke = 3;
  const radius = $derived((size - stroke) / 2);
  const length = $derived(2 * Math.PI * radius);
  const shown = $derived(Math.max(0, Math.min(1, left / period)));
</script>

<svg width={size} height={size} viewBox="0 0 {size} {size}" style="flex:none;transform:rotate(-90deg)" role="img" aria-label={left === 1 ? '1 second left' : `${left} seconds left`}>
  <circle cx={size / 2} cy={size / 2} r={radius} fill="none" stroke="var(--s3)" stroke-width={stroke} />
  <circle
    cx={size / 2}
    cy={size / 2}
    r={radius}
    fill="none"
    stroke={left <= 5 ? 'var(--warn)' : 'var(--primary)'}
    stroke-width={stroke}
    stroke-linecap="round"
    stroke-dasharray={length}
    stroke-dashoffset={length * (1 - shown)}
    class="ring-arc"
  />
</svg>
