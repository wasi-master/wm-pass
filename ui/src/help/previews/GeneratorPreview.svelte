<script lang="ts">
  import SecretText from '../../parts/SecretText.svelte';
  import { prefs } from '../../engine/prefs.svelte';
  import { GenState } from '../../vault/gen.svelte';

  // A real password from the generator, made with the choice being explained. Nothing is saved.
  let { value, pref }: { value?: unknown; pref?: string } = $props();
  const gen = new GenState();

  $effect(() => {
    const v = value;
    gen.mode = pref === 'gen_mode' && typeof v === 'string' ? v : prefs.v.gen_mode;
    gen.length = pref === 'gen_length' && typeof v === 'number' ? v : prefs.v.gen_length;
    gen.words = pref === 'gen_words' && typeof v === 'number' ? v : prefs.v.gen_words;
    gen.symbols = pref === 'gen_symbols' && typeof v === 'boolean' ? v : prefs.v.gen_symbols;
    gen.noAmbiguous = pref === 'gen_no_ambiguous' && typeof v === 'boolean' ? v : prefs.v.gen_no_ambiguous;
    void gen.roll();
  });
  const bits = $derived(pref === 'gen_show_bits' && typeof value === 'boolean' ? value : prefs.v.gen_show_bits);
</script>

<div class="gap" style="align-items:baseline">
  <div class="grow" style="font-size:18px;line-height:1.4;overflow-wrap:anywhere"><SecretText value={gen.value || ' '} /></div>
  {#if bits}<span class="dimmer tnum small">{Math.round(gen.bits)} bits</span>{/if}
</div>
<p class="preview-caption">Made just now by the generator, and not saved anywhere. Each choice below makes a new one.</p>
