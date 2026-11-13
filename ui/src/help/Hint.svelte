<script lang="ts">
  import type { TourId } from '$help';
  import IconBtn from '../parts/IconBtn.svelte';
  import Icon from '../parts/Icon.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import { help } from './help.svelte';

  // One line at the top of a page the first time it opens: what the page is for, and where to read
  // more. Closing it, or following it, means it is not shown again.
  let { id, topic, tour }: { id: string; topic: string; tour?: TourId } = $props();

  const shown = $derived(prefs.v.help_hints && !help.seen(`hint:${id}`));
  $effect(() => {
    if (shown) void help.load().catch(() => undefined);
  });
  const words = $derived.by(() => {
    const t = help.topic(topic);
    const content = help.content;
    if (!t || !content) return '';
    return content.plain(t.what, (other) => help.title(other), (a) => help.keyLabel(a));
  });

  function dismiss() {
    help.markSeen(`hint:${id}`);
  }
</script>

{#if shown && words}
  <div class="hint" role="note">
    <span class="dim" style="display:flex"><Icon name="info" size={18} /></span>
    <span class="grow small">{words}</span>
    <button type="button" class="btn quiet" onclick={() => { dismiss(); void help.open(topic); }}>More</button>
    {#if tour}<button type="button" class="btn quiet" onclick={() => { dismiss(); void help.startTour(tour); }}>Tour</button>{/if}
    <IconBtn icon="close" label="Do not show this again" size={16} class="tiny" onclick={dismiss} />
  </div>
{/if}
