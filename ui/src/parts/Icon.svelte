<script lang="ts">
  import { icons, type IconName } from '../engine/icons.svelte';
  let { name, size = 18, class: extra = '' }: { name: IconName; size?: number; class?: string } = $props();
</script>

<svg class={extra} width={size} height={size} viewBox="0 -960 960 960" fill="currentColor" aria-hidden="true"><path d={icons.table[name]} /></svg>
