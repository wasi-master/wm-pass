<script lang="ts">
  import IconBtn from '../parts/IconBtn.svelte';
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
      if (!value) return;
      const bytes = bytesOf(value);
      rating = await pass.engine<StrengthView>('strength.assess', { context: [] }, { secret: bytes }).then((r) => r.json);
      bytes.fill(0);
    }, 90);
    return () => clearTimeout(timer);
  });
</script>

<div class="gen-value">
  <div class="val"><SecretText value={gen.value || ' '} /></div>
  {#key rolls}
    <IconBtn icon="refresh" label="Generate again" class={rolls ? 'spin' : ''} onclick={() => { rolls += 1; void gen.roll(); }} />
  {/key}
</div>
<div style="padding-top:10px">
  <Strength score={rating?.score ?? 0} />
  <div class="gap small" style="padding-top:6px">
    <span class="grow dim">{rating ? `Time to crack: ${rating.crack_slow_hash}` : ' '}</span>
    {#if prefs.v.gen_show_bits}<span class="dimmer tnum">{Math.round(gen.bits)} bits</span>{/if}
  </div>
</div>
