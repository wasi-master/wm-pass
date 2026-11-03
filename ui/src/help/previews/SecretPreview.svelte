<script lang="ts">
  import SecretText from '../../parts/SecretText.svelte';
  import { prefs } from '../../engine/prefs.svelte';

  let { value, pref }: { value?: unknown; pref?: string } = $props();
  const colour = $derived(pref === 'colour_secrets' && typeof value === 'boolean' ? value : prefs.v.colour_secrets);
</script>

<div style="font-size:20px;line-height:1.4"><SecretText value="Tr0ub4dor&3-Ox8!lI" {colour} /></div>
<p class="preview-caption">A sample password. Colored digits and symbols are easier to read aloud and to type on another device.</p>
