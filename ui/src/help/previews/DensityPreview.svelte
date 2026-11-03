<script lang="ts">
  import { prefs } from '../../engine/prefs.svelte';
  import RowPreview from './RowPreview.svelte';

  // The list at the row height shown. The same sizes the window uses, set on this box alone.
  let { value, pref }: { value?: unknown; pref?: string } = $props();
  const density = $derived(pref === 'density' && typeof value === 'string' ? value : prefs.v.density);
  const sizes: Record<string, string> = { compact: '--row:36px;--ctl:26px', comfortable: '--row:44px;--ctl:28px', spacious: '--row:52px;--ctl:32px' };
</script>

<div style={sizes[density] ?? ''}><RowPreview /></div>
