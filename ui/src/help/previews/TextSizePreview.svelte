<script lang="ts">
  import { prefs } from '../../engine/prefs.svelte';

  let { value, pref }: { value?: unknown; pref?: string } = $props();
  const scale = $derived(pref === 'text_scale' && typeof value === 'number' ? value : prefs.v.text_scale);
  const px = (base: number) => `${(base * scale) / 100}px`;
</script>

<div style="font-size:{px(13)}">
  <div style="font-size:{px(20)};font-weight:600">GitHub</div>
  <div class="dim" style="font-size:{px(12)}">sam@example.com</div>
  <p style="padding-top:6px">Text in lists, pages and settings, at {scale}%.</p>
</div>
