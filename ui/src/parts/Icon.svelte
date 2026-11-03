<script lang="ts">
  import { icons, type IconName } from '../engine/icons.svelte';
  import Glyph from './Glyph.svelte';
  let { name, size = 18, class: extra = '' }: { name: IconName; size?: number; class?: string } = $props();
</script>

<Glyph d={icons.table[name]} style={icons.style} {size} class={extra} />
