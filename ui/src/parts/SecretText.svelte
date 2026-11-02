<script lang="ts">
  import { prefs } from '../engine/prefs.svelte';
  // A secret in a face people can read aloud, with digits and symbols in their own colours.
  let { value, class: extra = '' }: { value: string; class?: string } = $props();
  const parts = $derived.by(() => {
    if (!prefs.v.colour_secrets) return [{ text: value, cls: '' }];
    const out: { text: string; cls: string }[] = [];
    for (const ch of value) {
      const cls = /\d/.test(ch) ? 'd' : /[\p{L}\s]/u.test(ch) ? '' : 's';
      const last = out[out.length - 1];
      if (last && last.cls === cls) last.text += ch;
      else out.push({ text: ch, cls });
    }
    return out;
  });
</script>

<span class="mono secret selectable {extra}">{#each parts as part}<span class={part.cls}>{part.text}</span>{/each}</span>
