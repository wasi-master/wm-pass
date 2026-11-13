<script lang="ts">
  import Icon from '../parts/Icon.svelte';
  import SecretText from '../parts/SecretText.svelte';
  import Strength from '../parts/Strength.svelte';
  import { bytesOf, pass } from '../engine/pass.svelte';
  import { prefs } from '../engine/prefs.svelte';
  import type { StrengthView } from '../engine/types';
  import type { GenState } from './gen.svelte';

  let { state: gen }: { state: GenState } = $props();
  let rating = $state<StrengthView | null>(null);
  let rolls = $state(0);

  $effect(() => {
    const value = gen.value;
    const timer = setTimeout(async () => {
      // Nothing to copy, such as a pattern the engine cannot follow: no rating of the last value.
      if (!value) {
        rating = null;
        return;
      }
      const bytes = bytesOf(value);
      try {
        const rated = (await pass.engine<StrengthView>('strength.assess', { context: [] }, { secret: bytes })).json;
        // A slow rating for a value since rolled again must not stand for the new one.
        if (gen.value === value) rating = rated;
      } catch {
        if (gen.value === value) rating = null;
      } finally {
        bytes.fill(0);
      }
    }, 90);
    return () => clearTimeout(timer);
  });
</script>

<div class="gen-value">
  <!-- Said again each time it changes, so someone who cannot see it knows what Generate again made. -->
  <div class="val" aria-live="polite" aria-atomic="true"><SecretText value={gen.value || ' '} /></div>
  <!-- Only the arrow is drawn again to turn it once more: the button stays, and so does the focus
       of someone pressing it from the keyboard. -->
  <button type="button" class="icon-btn" title="Generate again" aria-label="Generate again" onclick={() => { rolls += 1; void gen.roll(); }}>
    {#key rolls}<span class:spin={rolls > 0} style="display:flex"><Icon name="refresh" size={18} /></span>{/key}
  </button>
</div>
<div style="padding-top:10px">
  <Strength score={rating?.score ?? 0} />
  <div class="gap small" style="padding-top:6px">
    <span class="grow dim">{rating ? `Time to crack: ${rating.crack_slow_hash}` : ' '}</span>
    {#if prefs.v.gen_show_bits}<span class="dimmer tnum">{Math.round(gen.bits)} bits</span>{/if}
  </div>
</div>
